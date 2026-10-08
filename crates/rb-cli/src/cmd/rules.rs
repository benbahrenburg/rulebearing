//! `rulebearing rules [--json]`: every rule with what it matched, for an ADR guard or an agent.
//!
//! - Source: [design § The subcommands a guard reaches for](../../../../docs/artifacts/design.md#the-subcommands-a-guard-reaches-for)
//!   (the field list)
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Plan: [Wave 1, Step 14](../../../../docs/plans/implemented/0001-wave-1-typescript-parity.md#step-14-rules---json-explain-explain---plain-test-can-import-1e)
//! - Requirement: [FR-CLI-01](../../../../docs/prd.md#fr-cli-01)
//!
//! `fromMatches`, `toMatches` and `violations` need a graph: `--graph`, the saved
//! `.graph/cruise.json`, or a fresh extraction. Each rule also carries its lifecycle fields,
//! `since`, `deprecated` and `replacedBy` ([Wave 3, Step 12](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#23-steps-for-sub-wave-3c-presets-lifecycle-fields-snapshot-and-changelog)).
//!
//! `rules --unused [--releases N]` ([FR-CLI-07](../../../../docs/prd.md#fr-cli-07)) reads the
//! snapshots under `.graph/snapshots/` ([`crate::cmd::snapshot`]) instead of a graph: it takes
//! the last `N` (default 3) in version order and lists the dependency rules of the current
//! configuration that each of them records with zero `fromMatches` and zero `toMatches`. A rule a
//! snapshot does not record (it did not exist at that release) is not listed, since it has not
//! been unused for `N` releases. With fewer than `N` snapshots the command prints
//! `insufficient history` and exits 0 rather than guess ([Wave 3 plan § 1.6](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#16-decisions-applied-and-decisions-this-wave-must-make)).
//! Listing a rule changes nothing about it: an unused rule still fails `cruise` as vacuous
//! unless it has `allowEmpty` ([ADR-0007](../../../../docs/adr/0007-vacuous-rules-fail-by-default.md)).
//! `--graph` and paths are refused with `--unused`, which reads no graph.

use std::fmt::Write as _;

use clap::Args;
use rb_config::{Config, Family, Rule};
use rb_rules::RuleStats;
use serde_json::{Value, json};

use crate::cli::{ConfigArgs, GraphArgs};
use crate::cmd::snapshot::{self, RuleCounts, Snapshot};
use crate::context::Context;
use crate::pipeline::{self, RunOptions};
use crate::progress::Progress;
use crate::{Outcome, RunExit, configure};

/// `rules`.
#[derive(Debug, Clone, Default, Args)]
pub struct RulesArgs {
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
    /// The graph
    #[command(flatten)]
    pub graph: GraphArgs,
    /// Print JSON
    #[arg(long)]
    pub json: bool,
    /// List the rules that matched nothing on either side in each of the last --releases
    /// snapshots under .graph/snapshots
    #[arg(long)]
    pub unused: bool,
    /// With --unused: how many of the latest snapshots to read
    #[arg(long, value_name = "N", default_value_t = 3, requires = "unused", value_parser = clap::value_parser!(u32).range(1..))]
    pub releases: u32,
}

/// Every dependency rule with its list, in evaluation order.
pub fn listed(config: &Config) -> Vec<(Family, &Rule)> {
    config.rules.all_dependency_rules().collect()
}

/// A rule as `rules --json` prints it.
pub fn rule_json(family: Family, rule: &Rule, stats: Option<&RuleStats>) -> Value {
    let severity = if family == Family::Allowed {
        Value::Null
    } else {
        json!(rule.severity())
    };
    json!({
        "name": stats.map_or_else(|| rule.name().to_owned(), |s| s.name.clone()),
        "family": family.as_str(),
        "severity": severity,
        "comment": rule.meta.comment,
        "fix": rule.meta.fix,
        "since": rule.meta.since,
        "deprecated": rule.meta.deprecated,
        "replacedBy": rule.meta.replaced_by,
        "from": serde_json::to_value(&rule.from).unwrap_or(Value::Null),
        "to": serde_json::to_value(&rule.to).unwrap_or(Value::Null),
        "module": rule.module.as_ref().and_then(|m| serde_json::to_value(m).ok()),
        "select": Value::Null,
        "fromMatches": stats.map(|s| s.from_matches),
        "toMatches": stats.map(|s| s.to_matches),
        "violations": stats.map(|s| s.violations),
    })
}

/// Evaluates the configuration over the query graph, for its per-rule statistics.
///
/// # Errors
/// An [`Outcome`] to return when the graph cannot be read or evaluated.
pub fn statistics(
    ctx: &Context<'_>,
    config: &Config,
    graph: &GraphArgs,
) -> Result<rb_rules::Evaluation, Outcome> {
    let document = pipeline::query_graph(ctx, config, graph.graph.as_deref(), &graph.paths)
        .map_err(|m| Outcome::failed(RunExit::Untrustworthy, format!("rulebearing: {m}\n")))?;
    let options = RunOptions {
        liveness: false,
        options_used: serde_json::Map::new(),
        paths: graph.paths.clone(),
        affected: None,
    };
    pipeline::evaluate_document(ctx, config, document, &options, &mut Progress::new(None))
        .map(pipeline::Run::into_evaluation)
        .map_err(|e| Outcome::failed(RunExit::Untrustworthy, format!("rulebearing: {e}\n")))
}

/// The rules of `config` that every one of `snapshots` records with nothing matched on either
/// side, in evaluation order and each listed once.
pub fn unused<'a>(config: &'a Config, snapshots: &[Snapshot]) -> Vec<(Family, &'a Rule)> {
    let mut seen = std::collections::BTreeSet::new();
    listed(config)
        .into_iter()
        .filter(|(_, rule)| {
            let name = rule.name();
            !snapshots.is_empty()
                && snapshots
                    .iter()
                    .all(|s| s.rules.get(name).is_some_and(RuleCounts::unused))
                && seen.insert(name)
        })
        .collect()
}

/// `rules --unused`.
fn run_unused(ctx: &Context<'_>, config: &Config, args: &RulesArgs) -> Outcome {
    if args.graph.graph.is_some() || !args.graph.paths.is_empty() {
        return Outcome::failed(
            RunExit::InvalidConfig,
            "rulebearing rules: --unused reads the snapshots under .graph/snapshots, not a graph; drop --graph and the paths\n",
        );
    }
    let mut snapshots = match snapshot::read_all(&ctx.resolve(snapshot::SNAPSHOTS)) {
        Ok(s) => s,
        Err(message) => {
            return Outcome::failed(
                RunExit::Untrustworthy,
                format!("rulebearing rules: {message}\n"),
            );
        }
    };
    let wanted = args.releases as usize;
    let sufficient = snapshots.len() >= wanted;
    let window = snapshots.split_off(snapshots.len().saturating_sub(wanted));
    let releases: Vec<&str> = window.iter().map(|s| s.version.as_str()).collect();
    let found = if sufficient {
        unused(config, &window)
    } else {
        Vec::new()
    };
    if args.json {
        let rules: Vec<Value> = found
            .iter()
            .map(|(family, rule)| rule_json(*family, rule, None))
            .collect();
        let mut text = serde_json::to_string_pretty(&json!({
            "releases": releases,
            "insufficientHistory": !sufficient,
            "unused": rules,
        }))
        .unwrap_or_default();
        text.push('\n');
        return Outcome::printed(text);
    }
    if !sufficient {
        return Outcome::printed(format!(
            "insufficient history: {} snapshot(s) under {}, and --releases asks for {wanted}; write one per release with `rulebearing snapshot`\n",
            releases.len(),
            snapshot::SNAPSHOTS
        ));
    }
    let span = releases.join(", ");
    if found.is_empty() {
        return Outcome::printed(format!("no rule was unused in each of {span}\n"));
    }
    let mut out = format!("unused in each of {span}:\n");
    for (_, rule) in &found {
        let mut line = format!("  {}", rule.name());
        if let Some(deprecated) = &rule.meta.deprecated {
            let _ = write!(line, " (deprecated {deprecated})");
        }
        if let Some(next) = &rule.meta.replaced_by {
            let _ = write!(line, " (replaced by {next})");
        }
        out.push_str(&line);
        out.push('\n');
    }
    let _ = writeln!(
        out,
        "{} rule(s); each still fails cruise as vacuous unless it has allowEmpty: delete it, or mark it deprecated with replacedBy",
        found.len()
    );
    Outcome::printed(out)
}

/// Runs `rules`.
pub fn run(ctx: &mut Context<'_>, args: &RulesArgs) -> Outcome {
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    if args.unused {
        return run_unused(ctx, &config, args);
    }
    let evaluation = match statistics(ctx, &config, &args.graph) {
        Ok(e) => e,
        Err(o) => return o,
    };
    let rules: Vec<Value> = listed(&config)
        .into_iter()
        .zip(evaluation.rule_stats.iter())
        .map(|((family, rule), stats)| rule_json(family, rule, Some(stats)))
        .collect();
    if args.json {
        let mut text = serde_json::to_string_pretty(&json!({ "rules": rules })).unwrap_or_default();
        text.push('\n');
        return Outcome::printed(text);
    }
    let mut out = format!(
        "{:<40} {:<10} {:<9} {:>5} {:>5} {:>10}\n",
        "rule", "family", "severity", "from", "to", "violations"
    );
    for r in &rules {
        let _ = writeln!(
            out,
            "{:<40} {:<10} {:<9} {:>5} {:>5} {:>10}",
            r["name"].as_str().unwrap_or_default(),
            r["family"].as_str().unwrap_or_default(),
            r["severity"].as_str().unwrap_or("-"),
            r["fromMatches"].to_string(),
            r["toMatches"].to_string(),
            r["violations"].to_string(),
        );
    }
    Outcome::printed(out)
}

#[cfg(test)]
mod tests {
    use rb_config::RuleMeta;

    use super::*;

    fn rule(name: &str) -> Rule {
        Rule {
            meta: RuleMeta {
                name: Some(name.into()),
                ..RuleMeta::default()
            },
            ..Rule::default()
        }
    }

    fn snapshot(version: &str, rules: &[(&str, u64, u64)]) -> Snapshot {
        Snapshot {
            version: version.into(),
            rules: rules
                .iter()
                .map(|(name, from, to)| {
                    (
                        (*name).to_owned(),
                        RuleCounts {
                            from_matches: *from,
                            to_matches: *to,
                            violations: 0,
                        },
                    )
                })
                .collect(),
            ..Snapshot::default()
        }
    }

    #[test]
    fn a_rule_is_unused_only_when_every_snapshot_records_it_matching_nothing() {
        let mut config = Config::default();
        for name in ["idle", "busy-from", "busy-to", "new", "idle"] {
            config.rules.dependencies.forbidden.push(rule(name));
        }
        let snapshots = [
            snapshot(
                "1",
                &[("idle", 0, 0), ("busy-from", 0, 0), ("busy-to", 0, 0)],
            ),
            snapshot(
                "2",
                &[
                    ("idle", 0, 0),
                    ("busy-from", 1, 0),
                    ("busy-to", 0, 0),
                    ("new", 0, 0),
                ],
            ),
            snapshot(
                "3",
                &[
                    ("idle", 0, 0),
                    ("busy-from", 0, 0),
                    ("busy-to", 0, 2),
                    ("new", 0, 0),
                ],
            ),
        ];
        let names: Vec<&str> = unused(&config, &snapshots)
            .into_iter()
            .map(|(_, r)| r.name())
            .collect();
        assert_eq!(
            names,
            ["idle"],
            "listed once, though two rules share the name"
        );
        assert!(
            unused(&config, &[]).is_empty(),
            "no history, nothing unused"
        );
    }

    #[test]
    fn rules_json_carries_the_lifecycle_fields() {
        let mut with = rule("old");
        with.meta.since = Some("1.2.0".into());
        with.meta.deprecated = Some("2.0.0".into());
        with.meta.replaced_by = Some("new".into());
        let value = rule_json(Family::Forbidden, &with, None);
        assert_eq!(value["since"], "1.2.0");
        assert_eq!(value["deprecated"], "2.0.0");
        assert_eq!(value["replacedBy"], "new");
        let plain = rule_json(Family::Forbidden, &rule("plain"), None);
        for key in ["since", "deprecated", "replacedBy"] {
            assert_eq!(plain[key], Value::Null, "{key} is null when absent");
        }
    }
}
