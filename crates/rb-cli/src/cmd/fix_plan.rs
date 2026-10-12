//! `rulebearing fix --plan <rule>`: for each violation of a rule, the cheapest refactor of each
//! of three kinds that clears it, verified before it is printed.
//!
//! - Source: [design § The agentic engineering hat](../../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two)
//!   ("the three cheapest refactors that clear it, as a machine-readable plan (move file to a
//!   legal directory, introduce a port in package X, invert the edge), each with the number of
//!   edges it touches")
//! - Contract: [plan 0004 § 1.6](../../../../docs/plans/pending/0004-wave-4-reach.md#16-interfaces-and-contracts-this-wave-freezes)
//!   (the `fix --plan` output), [§ 1.7](../../../../docs/plans/pending/0004-wave-4-reach.md#17-decisions-applied-and-decisions-this-wave-must-make)
//!   (exactly the three templates)
//! - Plan: [plan 0004, Step 9](../../../../docs/plans/pending/0004-wave-4-reach.md#23-steps-for-sub-wave-4c-fix---plan-and-the-eval-harness-funded-only-by-the--12-rule)
//! - Decisions: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md) (the agent
//!   surface is the command line), [ADR-0015](../../../../docs/adr/0015-stable-violation-id.md)
//!   (violations are named by their stable id),
//!   [ADR-0076](../../../../docs/adr/0076-fix-plan-candidates-are-verified-and-the-eval-harness-scores-fix-text.md)
//!   (how a candidate is chosen, costed and verified)
//! - Requirement: [FR-REACH-03](../../../../docs/prd.md#fr-reach-03)
//!
//! A violation is planned on one edge: its own `from -> to` for a dependency, cycle or
//! instability finding, and the first step of the path for a reachability finding. A finding on a
//! module alone (an orphan, a `required` rule, a dependents count), on a folder, a project or a
//! code element has no edge to change, so its `plans` is empty and its `reason` says why. The
//! three templates, and what each changes in a copy of the graph:
//!
//! | Kind | The change | `edgesTouched`: the import statements the agent edits |
//! | --- | --- | --- |
//! | `move` | `from` moves to `target`, keeping its imports and its importers | its imports plus the imports of it |
//! | `port` | a new module in `package`; `from` imports it in place of `to`, and `to` imports it | 2 |
//! | `invert` | `from` no longer imports `to`; `to` imports `from` | 2, plus one per other dependent of `to` (`dependentsToChange`), which imported what moves to `from`'s side |
//!
//! Each template's candidates are the folders of the module's language that `place` lists, except
//! the repository's root, tried nearest first (the fewest folders away from the module's own
//! folder for `move`, from both ends' folders for `port`; then the deeper; then by name), and the
//! first one that is legal is the template's plan. Legal means both checks pass
//! on the changed graph: every edge the change adds is allowed by `can-import`
//! ([`can_import::blocking_rules`]), and the whole rule set, evaluated as `cruise` evaluates it,
//! no longer reports the violation and reports no error-severity violation, and no violation of
//! the same rule, that the graph as it is does not (a moved module's findings are compared under
//! its old path). A candidate that fails either check is not printed. The plans are sorted by
//! `edgesTouched`, then `move`, `port`, `invert`, so at most three are printed, one per
//! template; with none, `reason` names what ruled each template out. The tool never edits a file:
//! the agent executes the plan.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use clap::Args;
use rb_config::Config;
use rb_model::{GraphDocument, Language, Module, Severity, Violation, ViolationType};
use rb_rules::{EngineError, EvalOptions, evaluate};
use serde_json::{Map, Value, json};

use crate::cli::ConfigArgs;
use crate::cmd::{can_import, place};
use crate::context::Context;
use crate::{Outcome, RunExit, cache, configure};

/// `fix`.
#[derive(Debug, Clone, Default, Args)]
pub struct FixArgs {
    /// Print, as JSON, the cheapest verified refactor of each kind (move, port, invert) for each
    /// violation of RULE; nothing is edited
    #[arg(long, value_name = "RULE", required = true)]
    pub plan: String,
    /// A graph document to answer from instead of the cache
    #[arg(long, value_name = "FILE")]
    pub graph: Option<String>,
    /// Extract afresh, neither reading nor writing the cache
    #[arg(long)]
    pub no_cache: bool,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
}

/// What every printed candidate was verified with.
pub const LEGAL_BY: &str = "can-import";

/// A candidate refactor's template, with what it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanKind {
    /// Move the importing module to `target`, a folder (`""` for the root).
    Move {
        /// The folder.
        target: String,
    },
    /// Introduce a port in `package`, a folder, that both ends import.
    Port {
        /// The folder.
        package: String,
    },
    /// Reverse the edge; the other modules that import the target change with it.
    Invert {
        /// The other dependents of the target, sorted.
        dependents: Vec<String>,
    },
}

impl PlanKind {
    /// The kind as the output names it.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Move { .. } => "move",
            Self::Port { .. } => "port",
            Self::Invert { .. } => "invert",
        }
    }

    /// The order among plans of the same cost.
    fn rank(&self) -> u8 {
        match self {
            Self::Move { .. } => 0,
            Self::Port { .. } => 1,
            Self::Invert { .. } => 2,
        }
    }
}

/// One verified candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The template and what it names.
    pub kind: PlanKind,
    /// The import statements the agent adds, removes or rewrites.
    pub edges_touched: usize,
    /// What verified it: always [`LEGAL_BY`].
    pub legal_by: &'static str,
}

impl Plan {
    /// The plan as the output writes it, keys in the contract's order.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("kind".into(), json!(self.kind.name()));
        match &self.kind {
            PlanKind::Move { target } => {
                out.insert("target".into(), json!(folder_shown(target)));
            }
            PlanKind::Port { package } => {
                out.insert("package".into(), json!(package_shown(package)));
            }
            PlanKind::Invert { .. } => {}
        }
        out.insert("edgesTouched".into(), json!(self.edges_touched));
        if let PlanKind::Invert { dependents } = &self.kind {
            out.insert("dependentsToChange".into(), json!(dependents));
        }
        out.insert("legalBy".into(), json!(self.legal_by));
        Value::Object(out)
    }
}

/// A violation's plans, and when there is none, why.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Planned {
    /// At most one per template, cheapest first.
    pub plans: Vec<Plan>,
    /// Why `plans` is empty.
    pub reason: Option<String>,
}

impl Planned {
    fn none(reason: String) -> Self {
        Self {
            plans: Vec::new(),
            reason: Some(reason),
        }
    }

    /// Adds `plans` and, when it is empty, `reason` to a finding object.
    pub fn insert_into(&self, finding: &mut Map<String, Value>) {
        finding.insert(
            "plans".into(),
            Value::Array(self.plans.iter().map(Plan::to_json).collect()),
        );
        if let Some(reason) = &self.reason {
            finding.insert("reason".into(), json!(reason));
        }
    }
}

/// A folder as `move` prints it: with a trailing `/`, `./` for the root.
pub fn folder_shown(folder: &str) -> String {
    if folder.is_empty() {
        "./".to_owned()
    } else {
        format!("{folder}/")
    }
}

/// A folder as `port` prints it: as it is, `.` for the root.
pub fn package_shown(folder: &str) -> String {
    if folder.is_empty() {
        ".".to_owned()
    } else {
        folder.to_owned()
    }
}

/// The folder of a path, `""` at the root.
pub fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// A path in `folder`.
fn joined(folder: &str, file: &str) -> String {
    if folder.is_empty() {
        file.to_owned()
    } else {
        format!("{folder}/{file}")
    }
}

/// How many leading folder names two folders share.
pub fn shared_depth(a: &str, b: &str) -> usize {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    a.split('/')
        .zip(b.split('/'))
        .take_while(|(x, y)| x == y)
        .count()
}

/// How many folder names a folder has, 0 for the root.
pub fn depth(folder: &str) -> usize {
    if folder.is_empty() {
        0
    } else {
        folder.split('/').count()
    }
}

/// The number of folders between two folders: up from `a` to the folder they share, then down
/// to `b`.
pub fn distance(a: &str, b: &str) -> usize {
    let shared = shared_depth(a, b);
    depth(a) - shared + depth(b) - shared
}

/// `folders` in the order their candidates are tried: the fewest folders away from `anchors`
/// (summed) first, then the deeper, then by name.
pub fn nearest(folders: impl IntoIterator<Item = String>, anchors: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = folders.into_iter().collect();
    let away = |f: &str| anchors.iter().map(|a| distance(f, a)).sum::<usize>();
    out.sort_by(|a, b| {
        away(a)
            .cmp(&away(b))
            .then_with(|| depth(b).cmp(&depth(a)))
            .then_with(|| a.cmp(b))
    });
    out
}

/// A violation as two runs are compared on: rule, type, and ends.
type Key = (String, String, String, String);

fn key_of(v: &Violation, renamed: &BTreeMap<String, String>) -> Key {
    let back = |path: &str| {
        renamed
            .get(path)
            .cloned()
            .unwrap_or_else(|| path.to_owned())
    };
    (
        v.rule.name.clone(),
        v.violation_type
            .map(ViolationType::as_str)
            .unwrap_or_default()
            .to_owned(),
        back(&v.from),
        back(&v.to),
    )
}

/// A module the agent can edit: in the graph, not a package, a built-in or an unresolved name,
/// and of a known language.
fn local(document: &GraphDocument, path: &str) -> Option<Language> {
    let module = document.modules.iter().find(|m| m.source == path)?;
    let foreign = module.core_module == Some(true)
        || module.could_not_resolve == Some(true)
        || module.source.starts_with("node_modules/")
        || module.source.contains("/node_modules/");
    if foreign { None } else { module.language }
}

/// The plans for one rule's violations over one graph: the graph's own evaluation is made once.
pub struct Planner<'a> {
    document: GraphDocument,
    config: &'a Config,
    options: EvalOptions,
    base: BTreeSet<Key>,
}

impl<'a> Planner<'a> {
    /// A planner over `document`, which is reset first, so an evaluated graph plans as a fresh one.
    ///
    /// # Errors
    /// The engine's error when the graph cannot be evaluated.
    pub fn new(
        mut document: GraphDocument,
        config: &'a Config,
        today: chrono::NaiveDate,
    ) -> Result<Self, EngineError> {
        rb_rules::reset(&mut document);
        let options = EvalOptions {
            liveness: false,
            today,
            ..EvalOptions::default()
        };
        let evaluation = evaluate(document.clone(), config, &options)?;
        let base = evaluation
            .violations()
            .iter()
            .filter(|v| v.rule.severity != Severity::Ignore)
            .map(|v| key_of(v, &BTreeMap::new()))
            .collect();
        Ok(Self {
            document,
            config,
            options,
            base,
        })
    }

    /// The graph's violations of `rule`, in the engine's order.
    ///
    /// # Errors
    /// The engine's error when the graph cannot be evaluated.
    pub fn violations(&self, rule: &str) -> Result<Vec<Violation>, EngineError> {
        let evaluation = evaluate(self.document.clone(), self.config, &self.options)?;
        Ok(evaluation
            .violations()
            .iter()
            .filter(|v| v.rule.name == rule && v.rule.severity != Severity::Ignore)
            .cloned()
            .collect())
    }

    /// The edge a violation is planned on, or why it has none.
    fn edge_of(&self, v: &Violation) -> Result<(String, String), String> {
        let kind = v.violation_type.unwrap_or(ViolationType::Dependency);
        let to = match kind {
            ViolationType::Folder | ViolationType::Project | ViolationType::Element => {
                return Err(format!(
                    "a {} finding is not an edge between two modules; move, port and invert change a module's imports",
                    kind.as_str()
                ));
            }
            ViolationType::Module => None,
            ViolationType::Reachability => v
                .via
                .iter()
                .flatten()
                .map(|step| step.name.clone())
                .find(|name| *name != v.from),
            _ => Some(v.to.clone()),
        };
        let Some(to) = to.filter(|to| *to != v.from) else {
            return Err(format!(
                "`{}` finds the module {} itself, not one of its imports, so there is no edge to move, port or invert",
                v.rule.name, v.from
            ));
        };
        let present = self
            .document
            .modules
            .iter()
            .find(|m| m.source == v.from)
            .is_some_and(|m| m.dependencies.iter().any(|d| d.resolved == to));
        if present {
            Ok((v.from.clone(), to))
        } else {
            Err(format!("the edge {} -> {to} is not in the graph", v.from))
        }
    }

    /// Whether `trial` is legal: the edges it adds pass `can-import`, and its evaluation no longer
    /// has `target` and adds no error, nor any finding of `rule`, to the graph's own.
    fn legal(
        &self,
        trial: GraphDocument,
        added: &[(&str, &str)],
        renamed: &BTreeMap<String, String>,
        target: &Key,
    ) -> Result<bool, EngineError> {
        match can_import::blocking_rules(self.config, self.options.today, &trial, added) {
            Some(rules) if rules.is_empty() => {}
            _ => return Ok(false),
        }
        let evaluation = evaluate(trial, self.config, &self.options)?;
        for v in evaluation.violations() {
            if v.rule.severity == Severity::Ignore {
                continue;
            }
            let key = key_of(v, renamed);
            if key == *target {
                return Ok(false);
            }
            if !self.base.contains(&key)
                && (v.rule.severity == Severity::Error || v.rule.name == target.0)
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The plans for one violation.
    ///
    /// # Errors
    /// The engine's error when a changed graph cannot be evaluated.
    pub fn plan_for(&self, v: &Violation) -> Result<Planned, EngineError> {
        let (from, to) = match self.edge_of(v) {
            Ok(edge) => edge,
            Err(reason) => return Ok(Planned::none(reason)),
        };
        let target = key_of(v, &BTreeMap::new());
        let mut plans = Vec::new();
        let mut why = Vec::new();
        for (name, found) in [
            ("move", self.plan_move(&from, &target)?),
            ("port", self.plan_port(&from, &to, &target)?),
            ("invert", self.plan_invert(&from, &to, &target)?),
        ] {
            match found {
                Ok(plan) => plans.push(plan),
                Err(reason) => why.push(format!("{name}: {reason}")),
            }
        }
        plans.sort_by(|a, b| {
            a.edges_touched
                .cmp(&b.edges_touched)
                .then_with(|| a.kind.rank().cmp(&b.kind.rank()))
        });
        let reason = plans.is_empty().then(|| {
            let mut text = String::from("no candidate is legal; ");
            text.push_str(&why.join("; "));
            text
        });
        Ok(Planned { plans, reason })
    }

    /// The modules other than `except` that import `path`, sorted.
    fn dependents(&self, path: &str, except: &str) -> Vec<String> {
        let found: BTreeSet<String> = self
            .document
            .modules
            .iter()
            .filter(|m| m.source != except && m.source != path)
            .filter(|m| m.dependencies.iter().any(|d| d.resolved == path))
            .map(|m| m.source.clone())
            .collect();
        found.into_iter().collect()
    }

    /// `from` moved to the nearest folder where the change is legal.
    fn plan_move(&self, from: &str, target: &Key) -> Result<Result<Plan, String>, EngineError> {
        let Some(language) = local(&self.document, from) else {
            return Ok(Err(format!("{from} is not a module of the repository")));
        };
        let Some(module) = self.document.modules.iter().find(|m| m.source == from) else {
            return Ok(Err(format!("{from} is not a module of the repository")));
        };
        let file = from.rsplit('/').next().unwrap_or(from);
        let home = folder_of(from);
        let importers = self.dependents(from, "");
        let edges_touched = module.dependencies.len() + importers.len();
        // The repository's root is never a candidate: a module moved there leaves every package.
        let folders = nearest(
            place::candidates(&self.document, language)
                .into_iter()
                .filter(|f| f != home && !f.is_empty()),
            &[home],
        );
        for folder in folders {
            let moved = joined(&folder, file);
            if self.document.modules.iter().any(|m| m.source == moved) {
                continue;
            }
            let mut trial = self.document.clone();
            for m in &mut trial.modules {
                if m.source == from {
                    m.source.clone_from(&moved);
                }
                for d in &mut m.dependencies {
                    if d.resolved == from {
                        d.resolved.clone_from(&moved);
                        d.module.clone_from(&moved);
                    }
                }
            }
            let mut added: Vec<(&str, &str)> = module
                .dependencies
                .iter()
                .map(|d| (moved.as_str(), d.resolved.as_str()))
                .collect();
            added.extend(importers.iter().map(|i| (i.as_str(), moved.as_str())));
            let renamed = BTreeMap::from([(moved.clone(), from.to_owned())]);
            if self.legal(trial, &added, &renamed, target)? {
                return Ok(Ok(Plan {
                    kind: PlanKind::Move { target: folder },
                    edges_touched,
                    legal_by: LEGAL_BY,
                }));
            }
        }
        Ok(Err(format!(
            "no folder of the {} modules takes {from} without a new error-severity finding",
            language.as_str()
        )))
    }

    /// A port both ends import, in the folder nearest both where the change is legal.
    fn plan_port(
        &self,
        from: &str,
        to: &str,
        target: &Key,
    ) -> Result<Result<Plan, String>, EngineError> {
        let (Some(from_language), Some(to_language)) =
            (local(&self.document, from), local(&self.document, to))
        else {
            return Ok(Err(format!(
                "{to} is not a module of the repository, so it cannot import a port"
            )));
        };
        if from_language != to_language {
            return Ok(Err(format!(
                "{from} and {to} are in different languages, and a port is a module of one (ADR-0014)"
            )));
        }
        let file = place::default_name(from_language);
        let (from_home, to_home) = (folder_of(from), folder_of(to));
        // The repository's root is no package, so a port is never put there.
        let folders = nearest(
            place::candidates(&self.document, from_language)
                .into_iter()
                .filter(|f| !f.is_empty()),
            &[from_home, to_home],
        );
        for folder in folders {
            let port = joined(&folder, file);
            if self.document.modules.iter().any(|m| m.source == port) {
                continue;
            }
            let mut trial = self.document.clone();
            let mut module = Module::new(port.clone());
            module.language = Some(from_language);
            trial.modules.push(module);
            for m in &mut trial.modules {
                if m.source == from {
                    m.dependencies.retain(|d| d.resolved != to);
                    m.dependencies
                        .push(place::new_edge(from_language, &port, Vec::new()));
                } else if m.source == to {
                    m.dependencies
                        .push(place::new_edge(to_language, &port, Vec::new()));
                }
            }
            let added = [(from, port.as_str()), (to, port.as_str())];
            if self.legal(trial, &added, &BTreeMap::new(), target)? {
                return Ok(Ok(Plan {
                    kind: PlanKind::Port { package: folder },
                    edges_touched: 2,
                    legal_by: LEGAL_BY,
                }));
            }
        }
        Ok(Err(format!(
            "no folder of the {} modules holds a port that both {from} and {to} may import",
            from_language.as_str()
        )))
    }

    /// The edge reversed, when that is legal.
    fn plan_invert(
        &self,
        from: &str,
        to: &str,
        target: &Key,
    ) -> Result<Result<Plan, String>, EngineError> {
        let (Some(_), Some(to_language)) = (local(&self.document, from), local(&self.document, to))
        else {
            return Ok(Err(format!(
                "{to} is not a module of the repository, so it cannot import {from}"
            )));
        };
        // When `to` already imports `from` (a cycle of two), removing the edge is the whole change.
        let reversed = !self
            .document
            .modules
            .iter()
            .any(|m| m.source == to && m.dependencies.iter().any(|d| d.resolved == from));
        let mut trial = self.document.clone();
        for m in &mut trial.modules {
            if m.source == from {
                m.dependencies.retain(|d| d.resolved != to);
            } else if m.source == to && reversed {
                m.dependencies
                    .push(place::new_edge(to_language, from, Vec::new()));
            }
        }
        if !self.legal(trial, &[(to, from)], &BTreeMap::new(), target)? {
            return Ok(Err(format!(
                "the reversed edge {to} -> {from} is not legal"
            )));
        }
        let dependents = self.dependents(to, from);
        Ok(Ok(Plan {
            edges_touched: 1 + usize::from(reversed) + dependents.len(),
            kind: PlanKind::Invert { dependents },
            legal_by: LEGAL_BY,
        }))
    }
}

/// The `fix --plan` document: the rule and each violation with its plans.
///
/// # Errors
/// The engine's error when the graph cannot be evaluated.
pub fn plan_document(planner: &Planner<'_>, rule: &str) -> Result<Value, EngineError> {
    let mut violations = Vec::new();
    for v in planner.violations(rule)? {
        let found = planner.plan_for(&v)?;
        let mut entry = Map::new();
        entry.insert("id".into(), json!(v.id.clone().unwrap_or_default()));
        entry.insert("from".into(), json!(v.from));
        entry.insert("to".into(), json!(v.to));
        found.insert_into(&mut entry);
        violations.push(Value::Object(entry));
    }
    Ok(json!({ "rule": rule, "violations": violations }))
}

/// The `agent` report with `plans` (and `reason` when there is none) added to each violation it
/// shows: `cruise --output-type agent --with-plans`. Violations are matched by id, else by rule
/// and ends.
///
/// # Errors
/// A message when the report is not the `agent` reporter's JSON or the graph cannot be evaluated.
pub fn with_plans(
    report: &str,
    document: &GraphDocument,
    config: &Config,
    today: chrono::NaiveDate,
) -> Result<String, String> {
    let mut value: Value =
        serde_json::from_str(report).map_err(|e| format!("the agent report is not JSON: {e}"))?;
    let planner = Planner::new(document.clone(), config, today).map_err(|e| e.to_string())?;
    let violations = &document.summary.violations;
    let Some(rules) = value.get_mut("rules").and_then(Value::as_array_mut) else {
        return Err("the agent report has no `rules`".to_owned());
    };
    for rule in rules {
        let name = rule
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let Some(findings) = rule.get_mut("violations").and_then(Value::as_array_mut) else {
            continue;
        };
        for finding in findings {
            let id = finding.get("id").and_then(Value::as_str).map(str::to_owned);
            let from = finding
                .get("from")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let to = finding
                .get("to")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let found = violations.iter().find(|v| match &id {
                Some(id) => v.id.as_deref() == Some(id.as_str()),
                None => v.rule.name == name && v.from == from && v.to == to,
            });
            let these = match found {
                Some(v) => planner.plan_for(v).map_err(|e| e.to_string())?,
                None => Planned::none("the finding is not in the graph's violations".to_owned()),
            };
            if let Some(object) = finding.as_object_mut() {
                these.insert_into(object);
            }
        }
    }
    let mut out = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    out.push('\n');
    Ok(out)
}

/// Runs `fix --plan`.
pub fn run(ctx: &mut Context<'_>, args: &FixArgs) -> Outcome {
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    if let Err(no_rule) = rb_rules::explain::find(&config, &args.plan) {
        return Outcome::failed(
            RunExit::InvalidConfig,
            format!("rulebearing fix: {no_rule}\n"),
        );
    }
    let document = match cache::document(ctx, &config, args.graph.as_deref(), args.no_cache) {
        Ok(d) => d,
        Err(m) => {
            return Outcome::failed(RunExit::Untrustworthy, format!("rulebearing fix: {m}\n"));
        }
    };
    let planned = Planner::new(document, &config, ctx.today)
        .and_then(|planner| plan_document(&planner, &args.plan));
    match planned {
        Ok(value) => {
            let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
            text.push('\n');
            Outcome::printed(text)
        }
        Err(e) => {
            let mut message = String::new();
            let _ = writeln!(message, "rulebearing fix: {e}");
            Outcome::failed(RunExit::Untrustworthy, message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rb_model::{MiniDependency, RuleSummary};

    const CONFIG: &str = r#"
rules:
  dependencies:
    forbidden:
      - name: no-cross-app-imports
        severity: error
        from: { path: "^apps/([^/]+)/" }
        to: { path: "^apps/", pathNot: "^apps/$1/" }
      - name: domain-not-to-web
        severity: error
        from: { path: "^src/domain/" }
        to: { path: "^src/web/" }
      - name: no-builtins
        severity: error
        from: {}
        to: { dependencyTypes: [core] }
"#;

    fn config() -> Result<Config, rb_config::ConfigError> {
        rb_config::load_text(
            CONFIG,
            rb_config::read::Syntax::Yaml,
            std::path::Path::new("."),
            &rb_config::LoadOptions::default(),
        )
    }

    fn module(source: &str, imports: &[&str]) -> Module {
        let mut m = Module::new(source.to_owned());
        m.language = Some(Language::Typescript);
        m.dependencies = imports
            .iter()
            .map(|to| place::new_edge(Language::Typescript, to, Vec::new()))
            .collect();
        m
    }

    /// The contract's example and a layered pair: web reaches into worker; the domain imports a
    /// view that a page imports too; a tool imports a built-in.
    fn document() -> GraphDocument {
        let mut fs = Module::new("fs".to_owned());
        fs.core_module = Some(true);
        let mut tool = module("tools/read.ts", &[]);
        let mut builtin = place::new_edge(Language::Typescript, "fs", Vec::new());
        builtin.dependency_types = vec![rb_model::DependencyType::Core];
        builtin.core_module = true;
        tool.dependencies.push(builtin);
        GraphDocument {
            modules: vec![
                module("apps/web/src/main.ts", &["apps/web/src/x.ts"]),
                module("apps/web/src/x.ts", &["apps/worker/src/y.ts"]),
                module("apps/worker/src/y.ts", &[]),
                module("apps/worker/src/z.ts", &["apps/worker/src/y.ts"]),
                module("packages/shared/src/index.ts", &[]),
                module("src/domain/model.ts", &["src/web/view.ts"]),
                module("src/web/view.ts", &[]),
                module(
                    "src/web/page.ts",
                    &["src/web/view.ts", "src/domain/model.ts"],
                ),
                tool,
                fs,
            ],
            ..GraphDocument::default()
        }
    }

    fn today() -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2026, 10, 11).unwrap_or_default()
    }

    fn only(subject: &Planner<'_>, rule: &str) -> Result<Violation, Box<dyn std::error::Error>> {
        let mut found = subject.violations(rule)?;
        assert_eq!(found.len(), 1, "{rule}");
        found.pop().ok_or_else(|| "no violation".into())
    }

    #[test]
    fn the_kinds_are_the_three_the_design_names() {
        let table = [
            (
                PlanKind::Move {
                    target: String::new(),
                },
                "move",
                0,
            ),
            (
                PlanKind::Port {
                    package: String::new(),
                },
                "port",
                1,
            ),
            (
                PlanKind::Invert {
                    dependents: Vec::new(),
                },
                "invert",
                2,
            ),
        ];
        for (kind, name, rank) in table {
            assert_eq!(kind.name(), name);
            assert_eq!(kind.rank(), rank, "{name}");
        }
        assert_eq!(LEGAL_BY, "can-import");
    }

    #[test]
    fn each_plan_serialises_in_the_contract_order() {
        let plan = |kind| Plan {
            kind,
            edges_touched: 2,
            legal_by: LEGAL_BY,
        };
        let text = |p: &Plan| serde_json::to_string(&p.to_json()).unwrap_or_default();
        assert_eq!(
            text(&plan(PlanKind::Move {
                target: "packages/shared/src".into()
            })),
            r#"{"kind":"move","target":"packages/shared/src/","edgesTouched":2,"legalBy":"can-import"}"#
        );
        assert_eq!(
            text(&plan(PlanKind::Port {
                package: "packages/contracts".into()
            })),
            r#"{"kind":"port","package":"packages/contracts","edgesTouched":2,"legalBy":"can-import"}"#
        );
        assert_eq!(
            text(&plan(PlanKind::Invert {
                dependents: vec!["apps/worker/src/z.ts".into()]
            })),
            r#"{"kind":"invert","edgesTouched":2,"dependentsToChange":["apps/worker/src/z.ts"],"legalBy":"can-import"}"#
        );
        let mut finding = Map::new();
        Planned::none("why".into()).insert_into(&mut finding);
        assert_eq!(
            serde_json::to_string(&finding).unwrap_or_default(),
            r#"{"plans":[],"reason":"why"}"#
        );
        let mut finding = Map::new();
        Planned {
            plans: vec![plan(PlanKind::Invert {
                dependents: Vec::new(),
            })],
            reason: None,
        }
        .insert_into(&mut finding);
        assert!(finding.get("reason").is_none());
    }

    #[test]
    fn folders_paths_and_preference() {
        assert_eq!(folder_shown(""), "./");
        assert_eq!(folder_shown("a/b"), "a/b/");
        assert_eq!(package_shown(""), ".");
        assert_eq!(package_shown("a/b"), "a/b");
        assert_eq!(folder_of("a/b/c.ts"), "a/b");
        assert_eq!(folder_of("c.ts"), "");
        assert_eq!(joined("", "c.ts"), "c.ts");
        assert_eq!(joined("a", "c.ts"), "a/c.ts");
        assert_eq!(shared_depth("a/b/c", "a/b/d"), 2);
        assert_eq!(shared_depth("a", ""), 0);
        assert_eq!(shared_depth("", "a"), 0);
        assert_eq!(shared_depth("ab", "a"), 0);
        assert_eq!(distance("a/b/c", "a/d"), 3);
        assert_eq!(distance("a", "a"), 0);
        assert_eq!(distance("", "a/b"), 2);
        assert_eq!(depth(""), 0);
        assert_eq!(depth("a/b"), 2);
        assert_eq!(
            nearest(
                ["", "a", "a/b", "a/c", "a/c/d", "e"].map(str::to_owned),
                &["a/b/x"]
            ),
            ["a/b", "a", "a/c", "", "a/c/d", "e"]
        );
        // Both ends count: each end's own folder ties with the folder they share, and the deeper wins.
        assert_eq!(
            nearest(
                ["m/f/a", "m/f/b", "m/f", "m", "m/o"].map(str::to_owned),
                &["m/f/a", "m/f/b"]
            ),
            ["m/f/a", "m/f/b", "m/f", "m", "m/o"]
        );
    }

    proptest! {
        #[test]
        fn shared_depth_and_distance_are_symmetric_and_bounded(
            a in proptest::collection::vec("[ab]{1,2}", 0..4),
            b in proptest::collection::vec("[ab]{1,2}", 0..4),
        ) {
            let (a, b) = (a.join("/"), b.join("/"));
            prop_assert_eq!(shared_depth(&a, &b), shared_depth(&b, &a));
            prop_assert!(shared_depth(&a, &b) <= depth(&a).min(depth(&b)));
            prop_assert_eq!(shared_depth(&a, &a), depth(&a));
            prop_assert_eq!(distance(&a, &b), distance(&b, &a));
            prop_assert_eq!(distance(&a, &a), 0);
            prop_assert!(distance(&a, &b) <= depth(&a) + depth(&b));
            let file = joined(&a, "f.ts");
            prop_assert_eq!(folder_of(&file), a.as_str());
        }

        #[test]
        fn the_order_is_total_and_nearest_first(
            folders in proptest::collection::btree_set("[ab]{1,2}(/[ab]{1,2}){0,2}", 0..8),
            anchor in "[ab]{1,2}(/[ab]{1,2}){0,2}",
        ) {
            let given: Vec<String> = folders.iter().cloned().collect();
            let once = nearest(given.clone(), &[anchor.as_str()]);
            let reversed = nearest(given.into_iter().rev(), &[anchor.as_str()]);
            prop_assert_eq!(&once, &reversed);
            let mut sorted = once.clone();
            sorted.sort();
            prop_assert_eq!(sorted, folders.into_iter().collect::<Vec<_>>());
            for pair in once.windows(2) {
                prop_assert!(distance(&pair[0], &anchor) <= distance(&pair[1], &anchor));
            }
        }
    }

    #[test]
    fn a_cross_app_import_gets_a_move_and_a_port_but_no_invert()
    -> Result<(), Box<dyn std::error::Error>> {
        let config = config()?;
        let subject = Planner::new(document(), &config, today())?;
        let v = only(&subject, "no-cross-app-imports")?;
        assert_eq!(
            (v.from.as_str(), v.to.as_str()),
            ("apps/web/src/x.ts", "apps/worker/src/y.ts")
        );
        let planned = subject.plan_for(&v)?;
        assert_eq!(planned.reason, None);
        assert_eq!(
            planned.plans,
            [
                // Its one import and its one importer; anywhere under apps/ the importer breaks the rule.
                Plan {
                    kind: PlanKind::Move {
                        target: "packages".into()
                    },
                    edges_touched: 2,
                    legal_by: LEGAL_BY
                },
                Plan {
                    kind: PlanKind::Port {
                        package: "packages".into()
                    },
                    edges_touched: 2,
                    legal_by: LEGAL_BY
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn a_layer_edge_inverts_with_the_dependents_that_change()
    -> Result<(), Box<dyn std::error::Error>> {
        let config = config()?;
        let subject = Planner::new(document(), &config, today())?;
        let v = only(&subject, "domain-not-to-web")?;
        let planned = subject.plan_for(&v)?;
        let invert = planned.plans.iter().find(|p| p.kind.name() == "invert");
        assert_eq!(
            invert,
            Some(&Plan {
                kind: PlanKind::Invert {
                    dependents: vec!["src/web/page.ts".into()]
                },
                edges_touched: 3,
                legal_by: LEGAL_BY,
            })
        );
        let port = planned.plans.iter().find(|p| p.kind.name() == "port");
        assert_eq!(
            port.map(|p| p.kind.clone()),
            Some(PlanKind::Port {
                package: "src/domain".into()
            })
        );
        let costs: Vec<usize> = planned.plans.iter().map(|p| p.edges_touched).collect();
        let mut sorted = costs.clone();
        sorted.sort_unstable();
        assert_eq!(costs, sorted, "cheapest first");
        Ok(())
    }

    #[test]
    fn a_rule_every_folder_breaks_has_no_plan_and_says_why()
    -> Result<(), Box<dyn std::error::Error>> {
        let config = config()?;
        let subject = Planner::new(document(), &config, today())?;
        let v = only(&subject, "no-builtins")?;
        let planned = subject.plan_for(&v)?;
        assert!(planned.plans.is_empty());
        let reason = planned.reason.unwrap_or_default();
        assert!(reason.starts_with("no candidate is legal; move: no folder of the typescript modules takes tools/read.ts"), "{reason}");
        assert!(
            reason.contains("port: fs is not a module of the repository"),
            "{reason}"
        );
        assert!(
            reason.contains("invert: fs is not a module of the repository"),
            "{reason}"
        );
        Ok(())
    }

    fn violation(kind: ViolationType, from: &str, to: &str, via: Option<&[&str]>) -> Violation {
        Violation {
            from: from.into(),
            to: to.into(),
            unresolved_to: None,
            dependency_types: None,
            violation_type: Some(kind),
            rule: RuleSummary {
                name: "r".into(),
                severity: Severity::Error,
            },
            cycle: None,
            via: via.map(|steps| {
                steps
                    .iter()
                    .map(|s| MiniDependency {
                        name: (*s).into(),
                        dependency_types: Vec::new(),
                    })
                    .collect()
            }),
            metrics: None,
            comment: None,
            id: None,
            fix: None,
            decision: None,
        }
    }

    #[test]
    fn the_edge_a_finding_is_planned_on() -> Result<(), Box<dyn std::error::Error>> {
        let config = config()?;
        let subject = Planner::new(document(), &config, today())?;
        let edge = |v: &Violation| subject.edge_of(v);
        assert_eq!(
            edge(&violation(
                ViolationType::Reachability,
                "apps/web/src/main.ts",
                "apps/worker/src/y.ts",
                Some(&["apps/web/src/x.ts", "apps/worker/src/y.ts"])
            )),
            Ok((
                "apps/web/src/main.ts".to_owned(),
                "apps/web/src/x.ts".to_owned()
            ))
        );
        assert_eq!(
            edge(&violation(
                ViolationType::Cycle,
                "src/domain/model.ts",
                "src/web/view.ts",
                None
            )),
            Ok((
                "src/domain/model.ts".to_owned(),
                "src/web/view.ts".to_owned()
            ))
        );
        for kind in [
            ViolationType::Folder,
            ViolationType::Project,
            ViolationType::Element,
        ] {
            let why = edge(&violation(kind, "a", "b", None))
                .err()
                .unwrap_or_default();
            assert!(
                why.starts_with(&format!("a {} finding is not an edge", kind.as_str())),
                "{why}"
            );
        }
        let module = edge(&violation(
            ViolationType::Module,
            "src/web/view.ts",
            "src/web/view.ts",
            None,
        ));
        assert!(
            module
                .err()
                .unwrap_or_default()
                .contains("finds the module src/web/view.ts itself")
        );
        let missing = edge(&violation(
            ViolationType::Dependency,
            "src/web/view.ts",
            "src/domain/model.ts",
            None,
        ));
        assert_eq!(
            missing,
            Err("the edge src/web/view.ts -> src/domain/model.ts is not in the graph".to_owned())
        );
        let planned = subject.plan_for(&violation(ViolationType::Folder, "a", "b", None))?;
        assert!(planned.plans.is_empty() && planned.reason.is_some());
        Ok(())
    }

    #[test]
    fn a_cycle_of_two_inverts_by_removing_one_edge() -> Result<(), Box<dyn std::error::Error>> {
        let config = rb_config::load_text(
            "rules:\n  dependencies:\n    forbidden:\n      - { name: no-cycles, severity: error, from: {}, to: { circular: true } }\n",
            rb_config::read::Syntax::Yaml,
            std::path::Path::new("."),
            &rb_config::LoadOptions::default(),
        )?;
        let document = GraphDocument {
            modules: vec![
                module("src/a.ts", &["src/b.ts"]),
                module("src/b.ts", &["src/a.ts"]),
            ],
            ..GraphDocument::default()
        };
        let subject = Planner::new(document, &config, today())?;
        let found = subject.violations("no-cycles")?;
        assert!(!found.is_empty());
        let first = found
            .iter()
            .find(|v| v.from == "src/a.ts")
            .ok_or("no a -> b")?;
        let planned = subject.plan_for(first)?;
        let invert = planned
            .plans
            .iter()
            .find(|p| p.kind.name() == "invert")
            .ok_or("no invert")?;
        assert_eq!(invert.edges_touched, 1, "b already imports a");
        Ok(())
    }

    #[test]
    fn plans_reach_the_agent_report_by_id() -> Result<(), Box<dyn std::error::Error>> {
        let config = config()?;
        let options = EvalOptions {
            liveness: false,
            today: today(),
            ..EvalOptions::default()
        };
        let evaluated = evaluate(document(), &config, &options)?;
        let v = evaluated
            .violations()
            .iter()
            .find(|v| v.rule.name == "no-builtins")
            .ok_or("no violation")?;
        let report = json!({ "rules": [
            { "name": "no-builtins", "violations": [{ "id": v.id, "from": v.from, "to": v.to }] },
            { "name": "gone", "violations": [{ "id": null, "from": "x", "to": "y" }] },
            { "name": "no-list" }
        ] });
        let out = with_plans(&report.to_string(), &evaluated.document, &config, today())?;
        let value: Value = serde_json::from_str(&out)?;
        assert_eq!(value["rules"][0]["violations"][0]["plans"], json!([]));
        assert!(
            value["rules"][0]["violations"][0]["reason"]
                .as_str()
                .is_some_and(|r| r.starts_with("no candidate is legal"))
        );
        assert_eq!(
            value["rules"][1]["violations"][0]["reason"],
            "the finding is not in the graph's violations"
        );
        assert!(out.ends_with("}\n"));
        assert!(with_plans("not json", &evaluated.document, &config, today()).is_err());
        assert!(with_plans("{}", &evaluated.document, &config, today()).is_err());
        Ok(())
    }
}
