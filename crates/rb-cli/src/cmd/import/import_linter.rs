//! `rulebearing import import-linter`: import-linter's contracts as native rules.
//!
//! - Source: [design § import-linter contracts](../../../../../docs/artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them),
//!   [§ The developer relations hat](../../../../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo)
//! - Plan: [Wave 2, Step 11](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f)
//! - Decisions: [ADR-0013](../../../../../docs/adr/0013-ruff-parser-for-python.md) (the import-linter
//!   mapping), [ADR-0034](../../../../../docs/adr/0034-slices-group-types-or-modules-and-segments.md) (`segments`)
//! - Requirements: [FR-CLI-04](../../../../../docs/prd.md#fr-cli-04), [FR-RULE-07](../../../../../docs/prd.md#fr-rule-07)
//!
//! The settings come from `.importlinter`, `setup.cfg` (`[importlinter]` or
//! `[tool:importlinter]`) or `pyproject.toml` (`[tool.importlinter]`). A module is written as the
//! path of its file or package folder under the root that holds its top-level package, found by
//! reading the repository tree beside the settings file, which is also where `acyclic_siblings`
//! finds its packages and `exhaustive` its modules.
//!
//! | Contract | Written as |
//! | --- | --- |
//! | `forbidden` | one `forbidden` rule; `reachable: true` unless `allow_indirect_imports` |
//! | `layers` | one `forbidden` rule per lower-to-higher module pair, and per pair of `\|` siblings, each `reachable: true`; one set per container |
//! | `independence` | one `reachable` `forbidden` rule per ordered pair; with `allow_indirect_imports`, or a wildcard, the `independence` shorthand |
//! | `protected` | an `allowed` rule naming the importers, beside one rule allowing every import of an unprotected module |
//! | `acyclic_siblings` | one slice rule per package at or below each ancestor, `segments: 1`, `beFreeOfCycles` |
//! | `ignore_imports` | `knownViolations` entries with `reason: ignore_imports` |
//!
//! `layers` is written expanded rather than as the `layers` shorthand because the shorthand has
//! neither `reachable` nor sibling layers, and import-linter checks chains; the expansion is the
//! shorthand's own, with `reachable: true`, which is also the hand translation
//! `testbeds/oracles/configs/seddonym__import-linter.yaml` holds. `protected` is an `allowed` rule
//! as the design table says; because dependency-cruiser's `allowed` rules are one allow-list for
//! the whole file, the import also writes the rule that allows everything the protected
//! contracts do not name.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::ImportError;
use super::pattern;
use super::yaml::{Document, Item, Node};

/// Where the settings were found and which dialect they are written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `.importlinter` or `setup.cfg`: INI.
    Ini,
    /// `pyproject.toml`: TOML.
    Toml,
}

/// One option's value: text (INI) or a list (TOML arrays; INI lines split on newlines).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Field {
    Text(String),
    List(Vec<String>),
}

impl Field {
    fn list(&self) -> Vec<String> {
        match self {
            Self::Text(text) => text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect(),
            Self::List(items) => items.clone(),
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Text(text) => text.trim().to_owned(),
            Self::List(items) => items.join("\n"),
        }
    }

    fn flag(&self) -> Option<bool> {
        match self.text().to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Some(true),
            "false" | "no" | "off" | "0" => Some(false),
            _ => None,
        }
    }
}

/// One contract as written.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Contract {
    id: String,
    name: String,
    kind: String,
    options: BTreeMap<String, Field>,
}

impl Contract {
    fn list(&self, key: &str) -> Vec<String> {
        self.options.get(key).map(Field::list).unwrap_or_default()
    }

    fn flag(&self, key: &str, default: bool) -> bool {
        self.options
            .get(key)
            .and_then(Field::flag)
            .unwrap_or(default)
    }

    /// The options as `key = value` lines, for the comment above the rules.
    fn describe(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "import-linter contract `{}` ({}): {}",
            self.id, self.kind, self.name
        )];
        for (key, value) in &self.options {
            if matches!(key.as_str(), "name" | "type" | "id") {
                continue;
            }
            let items = value.list();
            if items.len() <= 1 {
                lines.push(format!("  {key} = {}", value.text()));
            } else {
                lines.push(format!("  {key} = {}", items.join(", ")));
            }
        }
        lines
    }
}

/// The settings: the root packages and the contracts.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Settings {
    roots: Vec<String>,
    contracts: Vec<Contract>,
}

/// INI as `configparser` reads it: sections, `key = value` or `key: value`, indented
/// continuation lines, `#` and `;` comments, keys lower-cased.
fn parse_ini(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut section: Option<String> = None;
    let mut key: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        let indented = line.len() != trimmed.len();
        if indented && let (Some(s), Some(k)) = (&section, &key) {
            if let Some(value) = sections.get_mut(s).and_then(|m| m.get_mut(k)) {
                value.push('\n');
                value.push_str(trimmed);
            }
            continue;
        }
        if let Some(name) = trimmed.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            section = Some(name.trim().to_owned());
            sections.entry(name.trim().to_owned()).or_default();
            key = None;
            continue;
        }
        let split = trimmed.find(['=', ':']);
        if let (Some(s), Some(at)) = (&section, split) {
            let k = trimmed[..at].trim().to_ascii_lowercase();
            let v = trimmed[at + 1..].trim().to_owned();
            sections.entry(s.clone()).or_default().insert(k.clone(), v);
            key = Some(k);
        }
    }
    sections
}

fn settings_from_ini(text: &str, file: &str) -> Result<Settings, ImportError> {
    let sections = parse_ini(text);
    let (prefix, top) = ["importlinter", "tool:importlinter"]
        .into_iter()
        .find_map(|name| sections.get(name).map(|s| (name, s)))
        .ok_or_else(|| {
            ImportError::Invalid(format!(
                "{file} has no [importlinter] or [tool:importlinter] section"
            ))
        })?;
    let mut roots = Field::Text(
        top.get("root_packages")
            .or_else(|| top.get("root_package"))
            .cloned()
            .unwrap_or_default(),
    )
    .list();
    roots.sort();
    roots.dedup();
    let contract_prefix = format!("{prefix}:contract:");
    let mut contracts = Vec::new();
    for (name, options) in &sections {
        let Some(id) = name.strip_prefix(&contract_prefix) else {
            continue;
        };
        contracts.push(Contract {
            id: id.to_owned(),
            name: options
                .get("name")
                .cloned()
                .unwrap_or_else(|| id.to_owned()),
            kind: options.get("type").cloned().unwrap_or_default(),
            options: options
                .iter()
                .map(|(k, v)| (k.clone(), Field::Text(v.clone())))
                .collect(),
        });
    }
    Ok(Settings { roots, contracts })
}

fn toml_field(value: &toml::Value) -> Field {
    match value {
        toml::Value::Array(items) => Field::List(
            items
                .iter()
                .map(|v| match v {
                    toml::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect(),
        ),
        toml::Value::String(s) => Field::Text(s.clone()),
        other => Field::Text(other.to_string()),
    }
}

/// A contract id from its name when the TOML table gives none.
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

fn settings_from_toml(text: &str, file: &str) -> Result<Settings, ImportError> {
    let value: toml::Table = toml::from_str(text).map_err(|e| ImportError::Parse {
        file: file.to_owned(),
        reason: e.to_string(),
    })?;
    let table = value
        .get("tool")
        .and_then(|t| t.get("importlinter"))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| ImportError::Invalid(format!("{file} has no [tool.importlinter] table")))?;
    let mut roots = table
        .get("root_packages")
        .or_else(|| table.get("root_package"))
        .map(|v| toml_field(v).list())
        .unwrap_or_default();
    roots.sort();
    roots.dedup();
    let mut contracts = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, entry) in table
        .get("contracts")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let Some(options) = entry.as_table() else {
            continue;
        };
        let name = options
            .get("name")
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let mut id = options
            .get("id")
            .and_then(toml::Value::as_str)
            .map_or_else(|| slug(&name), str::to_owned);
        if id.is_empty() || !seen.insert(id.clone()) {
            id = format!("contract-{}", index + 1);
            seen.insert(id.clone());
        }
        contracts.push(Contract {
            id,
            name,
            kind: options
                .get("type")
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            options: options
                .iter()
                .map(|(k, v)| (k.clone(), toml_field(v)))
                .collect(),
        });
    }
    Ok(Settings { roots, contracts })
}

/// The file names searched for, in order, when `--from` is not given.
pub const DEFAULT_FILES: &[&str] = &[".importlinter", "setup.cfg", "pyproject.toml"];

/// Whether `file` holds import-linter settings.
pub fn holds_settings(file: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(file) else {
        return false;
    };
    match dialect(file) {
        Dialect::Toml => settings_from_toml(&text, "").is_ok(),
        Dialect::Ini => settings_from_ini(&text, "").is_ok(),
    }
}

fn dialect(file: &Path) -> Dialect {
    if file.extension().is_some_and(|e| e == "toml") {
        Dialect::Toml
    } else {
        Dialect::Ini
    }
}

/// Directories never searched for packages.
const SKIPPED: &[&str] = &[
    ".git",
    ".venv",
    "venv",
    "node_modules",
    "__pycache__",
    ".tox",
    ".nox",
    "build",
    "dist",
    "site-packages",
];

/// The repository as the importer reads it: where each root package lives.
#[derive(Debug, Clone, Default)]
struct Layout {
    repo: PathBuf,
    /// Root package to the folder, relative to the repository, that holds it (`src`, or an empty
    /// string for the repository itself).
    homes: BTreeMap<String, String>,
    /// Root packages whose folder was not found.
    missing: Vec<String>,
}

impl Layout {
    fn find(repo: &Path, roots: &[String]) -> Self {
        let mut layout = Self {
            repo: repo.to_path_buf(),
            ..Self::default()
        };
        for root in roots {
            match find_package(repo, root) {
                Some(home) => {
                    layout.homes.insert(root.clone(), home);
                }
                None => layout.missing.push(root.clone()),
            }
        }
        layout
    }

    /// The folder holding the top-level package of `module`, if it is one of the roots.
    fn home(&self, module: &str) -> Option<String> {
        let top = module.split('.').next().unwrap_or(module);
        if let Some(home) = self.homes.get(top) {
            return Some(home.clone());
        }
        self.missing.iter().any(|m| m == top).then(String::new)
    }

    fn is_local(&self, module: &str) -> bool {
        self.home(module).is_some()
    }

    /// The path pattern of a module expression.
    fn pattern(&self, module: &str, as_packages: bool) -> String {
        match self.home(module) {
            Some(home) => pattern::module_path(&home, module, as_packages),
            None => pattern::external_module(module),
        }
    }

    /// The folder of a package, when it exists.
    fn package_dir(&self, module: &str) -> Option<PathBuf> {
        let home = self.home(module)?;
        let dir = self
            .repo
            .join(&home)
            .join(module.replace('.', std::path::MAIN_SEPARATOR_STR));
        dir.is_dir().then_some(dir)
    }

    /// The repository-relative file of a module: `pkg/a.py` or `pkg/a/__init__.py`.
    fn module_file(&self, module: &str) -> Option<String> {
        let home = self.home(module)?;
        let base = pattern::join(&home, &module.replace('.', "/"));
        let package = format!("{base}/__init__.py");
        let file = format!("{base}.py");
        if self.repo.join(&package).is_file() {
            Some(package)
        } else if self.repo.join(&file).is_file() {
            Some(file)
        } else {
            None
        }
    }

    /// The modules directly inside a package: its sub-packages and `.py` files.
    fn children(&self, module: &str) -> Vec<String> {
        let Some(dir) = self.package_dir(module) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in read_dir_sorted(&dir) {
            let name = entry
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if entry.is_dir() {
                if is_package(&entry) {
                    out.push(name);
                }
            } else if let Some(stem) = name.strip_suffix(".py")
                && stem != "__init__"
            {
                out.push(stem.to_owned());
            }
        }
        out
    }

    /// Every package at or below `module`, sorted by dotted name.
    fn packages_below(&self, module: &str) -> Vec<String> {
        let mut out = vec![module.to_owned()];
        let Some(dir) = self.package_dir(module) else {
            return out;
        };
        let mut stack = vec![(dir, module.to_owned())];
        while let Some((dir, dotted)) = stack.pop() {
            for entry in read_dir_sorted(&dir) {
                if entry.is_dir() && is_package(&entry) {
                    let name = entry
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let child = format!("{dotted}.{name}");
                    out.push(child.clone());
                    stack.push((entry, child));
                }
            }
        }
        out.sort();
        out
    }

    /// Every module the expression names, from the tree (wildcards expanded).
    fn expand(&self, expression: &str) -> Vec<String> {
        let segments: Vec<&str> = expression.split('.').collect();
        let Some((top, rest)) = segments.split_first() else {
            return Vec::new();
        };
        if top.contains('*') || !self.is_local(top) {
            return Vec::new();
        }
        let mut current = vec![(*top).to_owned()];
        for segment in rest {
            let mut next = Vec::new();
            for module in &current {
                match *segment {
                    "*" => next.extend(
                        self.children(module)
                            .into_iter()
                            .map(|c| format!("{module}.{c}")),
                    ),
                    "**" => {
                        let mut stack: Vec<String> = self
                            .children(module)
                            .into_iter()
                            .map(|c| format!("{module}.{c}"))
                            .collect();
                        while let Some(found) = stack.pop() {
                            stack.extend(
                                self.children(&found)
                                    .into_iter()
                                    .map(|c| format!("{found}.{c}")),
                            );
                            next.push(found);
                        }
                    }
                    literal => next.push(format!("{module}.{literal}")),
                }
            }
            current = next;
        }
        current.retain(|m| self.module_file(m).is_some());
        current.sort();
        current.dedup();
        current
    }
}

fn read_dir_sorted(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|it| it.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    entries.sort();
    entries
}

fn is_package(dir: &Path) -> bool {
    dir.join("__init__.py").is_file()
}

/// Finds the folder holding package `name`: the repository itself, `src/`, or the shallowest
/// folder (three levels at most) holding a directory of that name with an `__init__.py`.
fn find_package(repo: &Path, name: &str) -> Option<String> {
    for home in ["", "src"] {
        if repo.join(home).join(name).join("__init__.py").is_file() {
            return Some(home.to_owned());
        }
    }
    let mut level = vec![(repo.to_path_buf(), String::new())];
    for _ in 0..3 {
        let mut next = Vec::new();
        for (dir, rel) in &level {
            for entry in read_dir_sorted(dir) {
                let file = entry
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if !entry.is_dir() || file.starts_with('.') || SKIPPED.contains(&file.as_str()) {
                    continue;
                }
                if file == name && is_package(&entry) {
                    return Some(rel.clone());
                }
                next.push((entry.clone(), pattern::join(rel, &file)));
            }
        }
        level = next;
    }
    None
}

/// What one contract contributed.
#[derive(Debug, Default)]
struct Output {
    forbidden: Vec<Item>,
    allowed: Vec<Item>,
    independence: Vec<Item>,
    slices: Vec<Item>,
    known: Vec<Item>,
    /// Comment lines for the knownViolations block.
    notes: Vec<String>,
    catch_all: BTreeSet<String>,
}

fn path_value(patterns: &[String]) -> Node {
    if patterns.len() == 1 {
        Node::str(patterns[0].clone())
    } else {
        Node::strs(patterns)
    }
}

/// A `forbidden` rule.
fn forbidden(
    name: String,
    contract: &Contract,
    from: &[String],
    to: &[String],
    reachable: bool,
    allow_empty: bool,
) -> Node {
    let mut to_node = vec![("path", path_value(to))];
    if reachable {
        to_node.push(("reachable", Node::Bool(true)));
    }
    let mut pairs = vec![
        ("name", Node::Str(name)),
        (
            "comment",
            Node::str(format!("import-linter contract: {}", contract.name)),
        ),
        ("severity", Node::str("error")),
        ("from", Node::map(vec![("path", path_value(from))])),
        ("to", Node::map(to_node)),
    ];
    if allow_empty {
        pairs.push(("allowEmpty", Node::Bool(true)));
    }
    Node::map(pairs)
}

fn first_with_comments(items: &mut [Item], comments: Vec<String>) {
    if let Some(first) = items.first_mut() {
        first.comments = comments;
    }
}

/// A layer line: its members, whether they are independent (`|`), and which are optional.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Layer {
    members: Vec<(String, bool)>,
    independent: bool,
}

fn parse_layer(line: &str) -> Layer {
    let independent = line.contains('|');
    let members = line
        .split(['|', ':'])
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(|m| {
            m.strip_prefix('(')
                .and_then(|r| r.strip_suffix(')'))
                .map_or((m.to_owned(), false), |inner| {
                    (inner.trim().to_owned(), true)
                })
        })
        .collect();
    Layer {
        members,
        independent,
    }
}

/// One rule the contract produced, with its from and to patterns, for matching `ignore_imports`.
struct Produced {
    name: String,
    from: Vec<String>,
    to: Vec<String>,
}

fn layers(contract: &Contract, layout: &Layout, out: &mut Output, produced: &mut Vec<Produced>) {
    let parsed: Vec<Layer> = contract
        .list("layers")
        .iter()
        .map(|l| parse_layer(l))
        .collect();
    let containers = contract.list("containers");
    let passes: Vec<Option<String>> = if containers.is_empty() {
        vec![None]
    } else {
        containers.iter().cloned().map(Some).collect()
    };
    let mut comments = contract.describe();
    let mut items = Vec::new();
    for container in &passes {
        let full = |member: &str| {
            container
                .as_ref()
                .map_or_else(|| member.to_owned(), |c| format!("{c}.{member}"))
        };
        let prefix = match container {
            Some(c) if passes.len() > 1 => format!("{c}:"),
            _ => String::new(),
        };
        let mut push = |low: &(String, bool), high: &(String, bool), items: &mut Vec<Item>| {
            let name = format!("{}:{prefix}{}-to-{}", contract.id, low.0, high.0);
            let from = vec![layout.pattern(&full(&low.0), true)];
            let to = vec![layout.pattern(&full(&high.0), true)];
            items.push(Item::plain(forbidden(
                name.clone(),
                contract,
                &from,
                &to,
                true,
                low.1 || high.1,
            )));
            produced.push(Produced { name, from, to });
        };
        for (lower, layer) in parsed.iter().enumerate() {
            if layer.independent {
                for a in &layer.members {
                    for b in layer.members.iter().filter(|b| b.0 != a.0) {
                        push(a, b, &mut items);
                    }
                }
            }
            for higher in &parsed[..lower] {
                for low in &layer.members {
                    for high in &higher.members {
                        push(low, high, &mut items);
                    }
                }
            }
        }
        if contract.flag("exhaustive", false) {
            let container_name = container.clone().unwrap_or_default();
            let named: BTreeSet<String> = parsed
                .iter()
                .flat_map(|l| l.members.iter().map(|m| m.0.clone()))
                .chain(contract.list("exhaustive_ignores"))
                .collect();
            if layout.package_dir(&container_name).is_none() {
                comments.push(format!(
                    "exhaustive: the package `{container_name}` was not found, so which modules sit in no layer was not checked"
                ));
            } else {
                let unnamed: Vec<String> = layout
                    .children(&container_name)
                    .into_iter()
                    .filter(|c| !named.contains(c))
                    .map(|c| format!("{container_name}.{c}"))
                    .collect();
                if unnamed.is_empty() {
                    comments.push(format!(
                        "exhaustive: every module directly under `{container_name}` is a layer (checked when imported; no rule checks it on later runs)"
                    ));
                } else {
                    comments.push(format!(
                        "exhaustive: {} sit in no layer, which import-linter reports as a broken contract; no rule checks it",
                        unnamed.join(", ")
                    ));
                }
            }
        }
    }
    if items.is_empty() {
        comments.push("fewer than two layers: nothing to forbid".into());
        out.forbidden.push(Item {
            comments,
            node: forbidden(contract.id.clone(), contract, &[], &[], true, true),
            disabled: true,
        });
        return;
    }
    first_with_comments(&mut items, comments);
    out.forbidden.extend(items);
}

fn forbidden_contract(
    contract: &Contract,
    layout: &Layout,
    out: &mut Output,
    produced: &mut Vec<Produced>,
) {
    let as_packages = contract.flag("as_packages", true);
    let from: Vec<String> = contract
        .list("source_modules")
        .iter()
        .map(|m| layout.pattern(m, as_packages))
        .collect();
    let to: Vec<String> = contract
        .list("forbidden_modules")
        .iter()
        .map(|m| layout.pattern(m, as_packages))
        .collect();
    let reachable = !contract.flag("allow_indirect_imports", false);
    let node = forbidden(contract.id.clone(), contract, &from, &to, reachable, false);
    let mut comments = contract.describe();
    let disabled = from.is_empty() || to.is_empty();
    if disabled {
        comments.push("source_modules or forbidden_modules is empty: nothing to forbid".into());
    }
    out.forbidden.push(Item {
        comments,
        node,
        disabled,
    });
    if !disabled {
        produced.push(Produced {
            name: contract.id.clone(),
            from,
            to,
        });
    }
}

fn independence(
    contract: &Contract,
    layout: &Layout,
    out: &mut Output,
    produced: &mut Vec<Produced>,
) {
    let modules = contract.list("modules");
    let direct_only = contract.flag("allow_indirect_imports", false);
    let wildcard = modules.iter().any(|m| m.contains('*'));
    let mut comments = contract.describe();
    if direct_only || wildcard {
        let bodies: Vec<String> = modules
            .iter()
            .map(|m| {
                let full = layout.pattern(m, true);
                full.trim_start_matches('^')
                    .trim_end_matches("(/|\\.py$)")
                    .to_owned()
            })
            .collect();
        let pattern = format!("^({})(?:/|\\.py$)", bodies.join("|"));
        if wildcard && !direct_only {
            comments.push(
                "a wildcard names the modules, so they cannot be listed pair by pair: the shorthand checks direct imports only, where import-linter also checks chains"
                    .into(),
            );
        }
        let node = Node::map(vec![
            ("name", Node::str(contract.id.clone())),
            (
                "comment",
                Node::str(format!("import-linter contract: {}", contract.name)),
            ),
            ("severity", Node::str("error")),
            ("pattern", Node::str(pattern.clone())),
        ]);
        out.independence.push(Item {
            comments,
            node,
            disabled: false,
        });
        produced.push(Produced {
            name: contract.id.clone(),
            from: vec![pattern.clone()],
            to: vec![pattern],
        });
        return;
    }
    let mut items = Vec::new();
    for a in &modules {
        for b in modules.iter().filter(|b| *b != a) {
            let name = format!("{}:{a}-to-{b}", contract.id);
            let from = vec![layout.pattern(a, true)];
            let to = vec![layout.pattern(b, true)];
            items.push(Item::plain(forbidden(
                name.clone(),
                contract,
                &from,
                &to,
                true,
                false,
            )));
            produced.push(Produced { name, from, to });
        }
    }
    if items.is_empty() {
        comments.push("fewer than two modules: nothing to forbid".into());
        out.forbidden.push(Item {
            comments,
            node: forbidden(contract.id.clone(), contract, &[], &[], true, false),
            disabled: true,
        });
        return;
    }
    first_with_comments(&mut items, comments);
    out.forbidden.extend(items);
}

fn protected(contract: &Contract, layout: &Layout, out: &mut Output, produced: &mut Vec<Produced>) {
    let as_packages = contract.flag("as_packages", true);
    let protected: Vec<String> = contract
        .list("protected_modules")
        .iter()
        .map(|m| layout.pattern(m, as_packages))
        .collect();
    let importers: Vec<String> = contract
        .list("allowed_importers")
        .iter()
        .map(|m| layout.pattern(m, as_packages))
        .chain(protected.iter().cloned())
        .collect();
    out.catch_all.extend(protected.iter().cloned());
    let node = Node::map(vec![
        ("name", Node::str(contract.id.clone())),
        (
            "comment",
            Node::str(format!("import-linter contract: {}", contract.name)),
        ),
        ("from", Node::map(vec![("path", path_value(&importers))])),
        ("to", Node::map(vec![("path", path_value(&protected))])),
        ("allowEmpty", Node::Bool(true)),
    ]);
    let disabled = protected.is_empty();
    let mut comments = contract.describe();
    if disabled {
        comments.push("protected_modules is empty: nothing to protect".into());
    }
    out.allowed.push(Item {
        comments,
        node,
        disabled,
    });
    if !disabled {
        // An import of a protected module from anywhere else is `not-in-allowed`.
        produced.push(Produced {
            name: "not-in-allowed".into(),
            from: vec!["^".into()],
            to: protected,
        });
    }
}

fn acyclic_siblings(contract: &Contract, layout: &Layout, out: &mut Output) {
    let mut comments = contract.describe();
    let skip = contract.flag("skip_descendants", false);
    let mut packages = Vec::new();
    for ancestor in contract.list("ancestors") {
        if layout.package_dir(&ancestor).is_none() {
            comments.push(format!(
                "the package `{ancestor}` was not found under the repository, so only it is sliced, not the packages below it"
            ));
        }
        if skip {
            packages.push(ancestor);
        } else {
            packages.extend(layout.packages_below(&ancestor));
        }
    }
    packages.sort();
    packages.dedup();
    let mut items: Vec<Item> = packages
        .iter()
        .map(|package| {
            Item::plain(Node::map(vec![
                ("name", Node::str(format!("{}:{package}", contract.id))),
                (
                    "comment",
                    Node::str(format!("import-linter contract: {}", contract.name)),
                ),
                ("severity", Node::str("error")),
                ("matching", Node::str(format!("{package}.(*)"))),
                ("segments", Node::Int(1)),
                ("should", Node::str("beFreeOfCycles")),
                ("allowEmpty", Node::Bool(true)),
            ]))
        })
        .collect();
    if !contract.list("ignore_imports").is_empty() {
        comments.push(
            "ignore_imports: a slice rule has no knownViolations entry, so these imports still count toward a cycle"
                .into(),
        );
    }
    if items.is_empty() {
        comments.push("no ancestors: nothing to slice".into());
        out.slices.push(Item {
            comments,
            node: Node::map(vec![("name", Node::str(contract.id.clone()))]),
            disabled: true,
        });
        return;
    }
    first_with_comments(&mut items, comments);
    out.slices.extend(items);
}

/// `ignore_imports` as `knownViolations`: each `importer -> imported` whose modules the tree
/// holds, once per rule of the contract whose `from` and `to` match it.
fn ignore_imports(contract: &Contract, layout: &Layout, produced: &[Produced], out: &mut Output) {
    for line in contract.list("ignore_imports") {
        let Some((source, target)) = line.split_once("->") else {
            out.notes.push(format!(
                "`{}`: `{line}` is not `importer -> imported`",
                contract.id
            ));
            continue;
        };
        let (source, target) = (source.trim(), target.trim());
        let froms: Vec<String> = layout
            .expand(source)
            .iter()
            .filter_map(|m| layout.module_file(m))
            .collect();
        let tos: Vec<String> = if layout.is_local(target) {
            layout
                .expand(target)
                .iter()
                .filter_map(|m| layout.module_file(m))
                .collect()
        } else if target.contains('*') {
            Vec::new()
        } else {
            vec![target.to_owned()]
        };
        if froms.is_empty() || tos.is_empty() {
            out.notes.push(format!(
                "`{}`: `{line}` names no module file under the repository, so it has no entry",
                contract.id
            ));
            continue;
        }
        let mut matched = false;
        for from in &froms {
            for to in &tos {
                for rule in produced {
                    let hit = |patterns: &[String], text: &str| {
                        patterns
                            .iter()
                            .any(|p| regex::Regex::new(p).is_ok_and(|re| re.is_match(text)))
                    };
                    if hit(&rule.from, from) && hit(&rule.to, to) {
                        matched = true;
                        out.known.push(Item::plain(Node::map(vec![
                            ("from", Node::str(from.clone())),
                            ("to", Node::str(to.clone())),
                            (
                                "rule",
                                Node::map(vec![
                                    ("name", Node::str(rule.name.clone())),
                                    ("severity", Node::str("error")),
                                ]),
                            ),
                            ("reason", Node::str("ignore_imports")),
                        ])));
                    }
                }
            }
        }
        if !matched {
            out.notes.push(format!(
                "`{}`: `{line}` matches no rule of the contract, which import-linter reports as an unmatched ignore",
                contract.id
            ));
        }
    }
}

/// Imports the settings in `file` (displayed as `display`), reading the tree beside it.
///
/// # Errors
/// [`ImportError`] when the file cannot be read or holds no import-linter settings.
pub fn import(file: &Path, display: &str) -> Result<Document, ImportError> {
    let text = std::fs::read_to_string(file).map_err(|e| ImportError::Read {
        file: display.to_owned(),
        reason: e.to_string(),
    })?;
    let settings = match dialect(file) {
        Dialect::Toml => settings_from_toml(&text, display)?,
        Dialect::Ini => settings_from_ini(&text, display)?,
    };
    let repo = file.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    let layout = Layout::find(&repo, &settings.roots);
    Ok(document(&settings, &layout, display))
}

/// `rules`: the dependency rules, with the catch-all `allowed` rule when a contract is
/// protected, then the shorthands and the slices.
fn rules(
    forbidden: Vec<Item>,
    allowed: Vec<Item>,
    catch_all: &BTreeSet<String>,
    independence: Vec<Item>,
    slices: Vec<Item>,
) -> Node {
    let mut rules = Vec::new();
    let mut dependencies = Vec::new();
    if !forbidden.is_empty() {
        dependencies.push(("forbidden", Node::List(forbidden)));
    }
    if !allowed.is_empty() {
        let everything: Vec<String> = catch_all.iter().cloned().collect();
        let mut allowed = allowed;
        allowed.push(Item {
            comments: vec![
                "dependency-cruiser's allowed rules are one allow-list: this one allows every import of a module no protected contract names.".into(),
            ],
            node: Node::map(vec![
                ("name", Node::str("import-linter:unprotected")),
                (
                    "comment",
                    Node::str("import-linter: every import of an unprotected module"),
                ),
                ("from", Node::Map(Vec::new())),
                ("to", Node::map(vec![("pathNot", path_value(&everything))])),
                ("allowEmpty", Node::Bool(true)),
            ]),
            disabled: false,
        });
        dependencies.push(("allowed", Node::List(allowed)));
    }
    if !dependencies.is_empty() {
        rules.push(("dependencies", Node::map(dependencies)));
    }
    if !independence.is_empty() {
        rules.push(("independence", Node::List(independence)));
    }
    if !slices.is_empty() {
        rules.push(("slices", Node::List(slices)));
    }
    Node::map(rules)
}

fn document(settings: &Settings, layout: &Layout, display: &str) -> Document {
    let mut out = Output::default();
    let mut unsupported = Vec::new();
    for contract in &settings.contracts {
        let mut produced = Vec::new();
        match contract.kind.as_str() {
            "forbidden" => forbidden_contract(contract, layout, &mut out, &mut produced),
            "layers" => layers(contract, layout, &mut out, &mut produced),
            "independence" => independence(contract, layout, &mut out, &mut produced),
            "protected" => protected(contract, layout, &mut out, &mut produced),
            "acyclic_siblings" => acyclic_siblings(contract, layout, &mut out),
            other => {
                let mut lines = contract.describe();
                lines.push(format!(
                    "stays in import-linter: `{other}` is a custom contract type, Python code the importer cannot translate"
                ));
                unsupported.push(lines);
                continue;
            }
        }
        ignore_imports(contract, layout, &produced, &mut out);
    }
    let mut header = vec![
        format!("Imported from {display} by `rulebearing import import-linter`."),
        "Each rule's comment names its import-linter contract; the contract's options are written above its first rule.".into(),
    ];
    if !layout.missing.is_empty() {
        header.push(format!(
            "The package folder of {} was not found beside {display}; its modules are written as if it sat at the repository root.",
            layout.missing.join(", ")
        ));
    }
    for lines in unsupported {
        header.push(String::new());
        header.extend(lines);
    }
    let mut body: Vec<(String, Node)> = vec![("$schema".into(), Node::str(super::SCHEMA))];
    let mut homes: Vec<String> = settings
        .roots
        .iter()
        .map(|r| layout.home(r).unwrap_or_default())
        .map(|h| if h.is_empty() { ".".to_owned() } else { h })
        .collect();
    homes.sort();
    homes.dedup();
    body.push((
        "languages".into(),
        Node::map(vec![(
            "python",
            Node::map(vec![("roots", Node::strs(&homes))]),
        )]),
    ));
    let mut key_comments = Vec::new();
    if !out.known.is_empty() || !out.notes.is_empty() {
        let mut notes = vec![
            "ignore_imports: `rulebearing baseline --baseline-mode shrink-only` fails when an entry no longer occurs, which is import-linter's unmatched-ignore alerting.".to_owned(),
            "An entry excuses the violation from its importer to its imported module; a chain through the ignored import from another module is still reported.".to_owned(),
        ];
        notes.extend(out.notes.iter().cloned());
        key_comments.push(("options".to_owned(), notes));
        body.push((
            "options".into(),
            Node::map(vec![("knownViolations", Node::List(out.known))]),
        ));
    }
    let rules = rules(
        out.forbidden,
        out.allowed,
        &out.catch_all,
        out.independence,
        out.slices,
    );
    body.push(("rules".into(), rules));
    Document {
        header,
        body,
        key_comments,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_reads_as_configparser_does() {
        let sections = parse_ini(
            "# c\n[importlinter]\nroot_package = pkg\n\n[importlinter:contract:one]\nName: One\nlayers=\n    a\n    ; skipped\n    b | c\n",
        );
        assert_eq!(sections["importlinter"]["root_package"], "pkg");
        assert_eq!(sections["importlinter:contract:one"]["name"], "One");
        assert_eq!(
            sections["importlinter:contract:one"]["layers"],
            "\na\nb | c"
        );
    }

    #[test]
    fn toml_contracts_take_an_id_or_a_slug() -> Result<(), ImportError> {
        let settings = settings_from_toml(
            "[tool.importlinter]\nroot_packages = [\"b\", \"a\"]\n[[tool.importlinter.contracts]]\nname = \"My Layers!\"\ntype = \"layers\"\nlayers = [\"x\", \"y\"]\n[[tool.importlinter.contracts]]\nid = \"f\"\nname = \"F\"\ntype = \"forbidden\"\n[[tool.importlinter.contracts]]\nname = \"My Layers!\"\ntype = \"layers\"\n",
            "pyproject.toml",
        )?;
        assert_eq!(settings.roots, ["a", "b"]);
        let ids: Vec<&str> = settings.contracts.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["my-layers", "f", "contract-3"]);
        assert_eq!(settings.contracts[0].list("layers"), ["x", "y"]);
        assert!(settings_from_toml("[tool.other]\n", "p").is_err());
        assert!(settings_from_toml("not toml [", "p").is_err());
        assert!(settings_from_ini("[other]\n", "s").is_err());
        Ok(())
    }

    #[test]
    fn layer_lines_carry_siblings_and_optional_members() {
        assert_eq!(
            parse_layer("a | (b)"),
            Layer {
                members: vec![("a".into(), false), ("b".into(), true)],
                independent: true
            }
        );
        let open = parse_layer("a : b");
        assert!(!open.independent);
        assert_eq!(open.members.len(), 2);
    }

    #[test]
    fn fields_split_and_flag() {
        assert_eq!(Field::Text("\n a\n b ".into()).list(), ["a", "b"]);
        assert_eq!(Field::Text("True".into()).flag(), Some(true));
        assert_eq!(Field::Text("off".into()).flag(), Some(false));
        assert_eq!(Field::Text("maybe".into()).flag(), None);
        assert_eq!(Field::List(vec!["a".into(), "b".into()]).text(), "a\nb");
        assert_eq!(slug("A  b--C"), "a-b-c");
    }
}
