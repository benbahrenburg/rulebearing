//! `plantuml`: the graph as a `PlantUML` diagram, generated as `ArchUnitNET`'s
//! `PlantUmlDefinition.ComponentDiagram()` generates one, so a diagram can be written once and
//! then enforced with an `adhereTo` diagram rule.
//!
//! - Source: [design § Diagram rules](../../../docs/artifacts/design.md#diagram-rules),
//!   [design § Reporters](../../../docs/artifacts/design.md#reporters)
//! - Coverage: [`ArchUnitNET` § `PlantUML`](../../../docs/artifacts/archunitnet-0.13.4-coverage.md#plantuml)
//!   (`WithDependenciesFromSlices` / `FromTypes` / `FromNamespaces`, `GenerationOptions`)
//! - Plan: [Wave 3, Step 9](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//!   and the options in [§ 1.5](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//! - Decisions: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md),
//!   [ADR-0034](../../../docs/adr/0034-slices-group-types-or-modules-and-segments.md)
//! - Requirements: [FR-OUT-02](../../../docs/prd.md#fr-out-02), [FR-RULE-05](../../../docs/prd.md#fr-rule-05)
//!
//! `reporterOptions.plantuml` takes `ArchUnitNET`'s names: `from` (`slices`, `types`,
//! `namespaces` or `folders`; `--from` on the command line wins), `Matching` or
//! `MatchingWithPackages` (the slice pattern, the argument of the `SliceRuleDefinition` method
//! of that name), and the `GenerationOptions` `LimitDependencies`, `C4Style`, `FocusOn`,
//! `IncludeDependenciesToOther` and `DependencyFilters`. The nodes and the arrows are the ones
//! `ArchUnitNET`'s `PlantUmlFileBuilder` selects ([`rb_rules::plantuml_export`], proven against
//! `ArchUnitNET`'s own output by conformance gate 2). How they are written depends on the form:
//!
//! | Form | Written as |
//! | --- | --- |
//! | `from: types`; `from: slices` with `MatchingWithPackages` | `ArchUnitNET`'s text, byte for byte: classes and `--\|>` arrows, or packages (C4 boundaries with `C4Style`). A picture, not an `adhereTo` diagram: a stereotype matches a namespace, so types cannot be components, and the C4 `!include` is refused by the parser |
//! | `from: slices` with `Matching`, `from: namespaces`, `from: folders` | an `adhereTo` diagram: `hide stereotype` in place of the C4 include, one `[Name] <<pattern>>` component per node ([`stereotype`]), one `[A] --> [B]` line per arrow, a circle as two lines (`ArchUnitNET`'s `<-[#red]>` and `--\|>` are not arrows its parser reads) |
//!
//! A dependency target is placed in the node holding its namespace (folder), loaded or only
//! referenced, since `adhereTo` places it that way; a type in the global namespace lies in the
//! component [`GLOBAL`].
//!
//! `FocusOn` and `DependencyFilters` filter dependencies before they are drawn, as
//! `GenerationOptions.DependencyFilter` does: `FocusOn` is a regular expression over full names
//! (a type's, or a module's path) and keeps a dependency when exactly one end matches
//! (`DependencyFilters.FocusOn`); each `DependencyFilters` entry is `IgnoreDependenciesToParents`,
//! `IgnoreDependenciesToChildren`, `IgnoreDependenciesToChildrenAndParents` (the upstream filters)
//! or a regular expression whose matching targets are left out. `IncludeDependenciesToOther`
//! draws, in the component forms, a node for each namespace (folder) outside the grouping that is
//! depended on. An option `ArchUnitNET` ignores for the chosen form (`LimitDependencies` and
//! `C4Style` from types, `IncludeDependenciesToOther` with packages, `C4Style` without them) is
//! refused rather than ignored. `from: folders` is Rulebearing's, for TypeScript and Python
//! modules: a node per folder of the local modules.

use std::collections::{BTreeMap, BTreeSet};

use rb_model::GraphDocument;
use rb_rules::elements::Architecture;
use rb_rules::plantuml_export::{
    Builder, Dependency, DependencyType, ExportError, ExportSlice, GenerationOptions, SliceNode,
    export_slices, export_types, ignore_dependencies_to_children,
    ignore_dependencies_to_children_and_parents, ignore_dependencies_to_parents,
    remove_pattern_inappropriate,
};
use serde_json::Value;

use crate::Rendered;

/// The header of a diagram an `adhereTo` rule reads: no `!include`, which the parser refuses.
pub const ADHERE_HEADER: &str = "@startuml\n\nhide stereotype\n\n";

/// The component a type in the global namespace lies in (a name cannot be empty).
pub const GLOBAL: &str = "(global)";

/// What the nodes of a diagram are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum From {
    /// Slices of a pattern (`WithDependenciesFromSlices`).
    Slices,
    /// Types (`WithDependenciesFromTypes`).
    Types,
    /// Namespaces (`WithDependenciesFrom(Architecture.Namespaces)`).
    Namespaces,
    /// Folders of modules (Rulebearing's).
    Folders,
}

impl From {
    /// The value of `from` or `--from`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "slices" => Some(Self::Slices),
            "types" => Some(Self::Types),
            "namespaces" => Some(Self::Namespaces),
            "folders" => Some(Self::Folders),
            _ => None,
        }
    }
}

/// Why a diagram cannot be written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlantUmlError {
    /// An option that is unknown, of the wrong type, or not applicable to the form.
    #[error("reporterOptions.plantuml: {0}")]
    Options(String),
    /// The result is not a graph document.
    #[error("plantuml: the result is not a graph document: {0}")]
    Document(String),
    /// A slice pattern `ArchUnitNET` refuses, or a type it cannot place.
    #[error("plantuml: {0}")]
    Slicing(String),
    /// A name or structure `ArchUnitNET`'s generator refuses.
    #[error("plantuml: {0}")]
    Export(#[from] ExportError),
}

/// `reporterOptions.plantuml`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlantUmlOptions {
    /// `from`; when absent, `slices` with a slice pattern, else `namespaces` when the graph has
    /// .NET types, else `folders`.
    pub from: Option<From>,
    /// `Matching`: the slice pattern.
    pub matching: Option<String>,
    /// `MatchingWithPackages`: the slice pattern, each name keeping its prefix as its package.
    pub matching_with_packages: Option<String>,
    /// `LimitDependencies`.
    pub limit_dependencies: bool,
    /// `C4Style`.
    pub c4_style: bool,
    /// `FocusOn`: a regular expression over full names.
    pub focus_on: Option<String>,
    /// `IncludeDependenciesToOther`.
    pub include_dependencies_to_other: bool,
    /// `DependencyFilters`: upstream filter names or regular expressions over targets.
    pub dependency_filters: Vec<String>,
}

const KEYS: &[&str] = &[
    "from",
    "Matching",
    "MatchingWithPackages",
    "LimitDependencies",
    "C4Style",
    "FocusOn",
    "IncludeDependenciesToOther",
    "DependencyFilters",
];

const NAMED_FILTERS: &[&str] = &[
    "IgnoreDependenciesToParents",
    "IgnoreDependenciesToChildren",
    "IgnoreDependenciesToChildrenAndParents",
];

fn invalid<T>(message: impl Into<String>) -> Result<T, PlantUmlError> {
    Err(PlantUmlError::Options(message.into()))
}

fn pattern(key: &str, text: &str) -> Result<(), PlantUmlError> {
    if rb_rules::patterns::get(text).is_none() {
        return invalid(format!("{key} `{text}` is not a regular expression"));
    }
    Ok(())
}

impl PlantUmlOptions {
    /// Reads `reporterOptions.plantuml`, with `--from` (`from_flag`) winning over `from`.
    ///
    /// # Errors
    /// [`PlantUmlError::Options`] for an unknown key, a value of the wrong type, an unknown
    /// `from`, or a pattern that does not compile.
    pub fn from_reporter_options(
        section: Option<&Value>,
        from_flag: Option<&str>,
    ) -> Result<Self, PlantUmlError> {
        let empty = serde_json::Map::new();
        let map = match section {
            None | Some(Value::Null) => &empty,
            Some(Value::Object(map)) => map,
            Some(_) => return invalid("must be an object"),
        };
        if let Some(key) = map.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return invalid(format!(
                "unknown key `{key}`; the keys are {}",
                KEYS.join(", ")
            ));
        }
        let text = |key: &str| -> Result<Option<String>, PlantUmlError> {
            match map.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.clone())),
                Some(_) => invalid(format!("{key} must be a string")),
            }
        };
        let flag = |key: &str| -> Result<bool, PlantUmlError> {
            match map.get(key) {
                None | Some(Value::Null) => Ok(false),
                Some(Value::Bool(b)) => Ok(*b),
                Some(_) => invalid(format!("{key} must be true or false")),
            }
        };
        let from_text = from_flag.map(str::to_owned).or(text("from")?);
        let from = match from_text {
            None => None,
            Some(name) => match From::parse(&name) {
                Some(from) => Some(from),
                None => {
                    return invalid(format!(
                        "from `{name}`: use slices, types, namespaces or folders"
                    ));
                }
            },
        };
        let dependency_filters = match map.get("DependencyFilters") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|v| {
                    v.as_str().map(str::to_owned).ok_or_else(|| {
                        PlantUmlError::Options("DependencyFilters must be a list of strings".into())
                    })
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return invalid("DependencyFilters must be a list of strings"),
        };
        for filter in &dependency_filters {
            if !NAMED_FILTERS.contains(&filter.as_str()) {
                pattern("DependencyFilters", filter)?;
            }
        }
        let options = Self {
            from,
            matching: text("Matching")?,
            matching_with_packages: text("MatchingWithPackages")?,
            limit_dependencies: flag("LimitDependencies")?,
            c4_style: flag("C4Style")?,
            focus_on: text("FocusOn")?,
            include_dependencies_to_other: flag("IncludeDependenciesToOther")?,
            dependency_filters,
        };
        if let Some(focus) = &options.focus_on {
            pattern("FocusOn", focus)?;
        }
        if options.matching.is_some() && options.matching_with_packages.is_some() {
            return invalid("give Matching or MatchingWithPackages, not both");
        }
        Ok(options)
    }

    /// Whether the dependency `origin -> target` is drawn: `FocusOn` and every filter keep it.
    #[must_use]
    pub fn keeps(&self, origin: &str, target: &str) -> bool {
        let focused = self.focus_on.as_deref().is_none_or(|focus| {
            rb_rules::patterns::test(focus, origin) != rb_rules::patterns::test(focus, target)
        });
        focused
            && self
                .dependency_filters
                .iter()
                .all(|filter| match filter.as_str() {
                    "IgnoreDependenciesToParents" => ignore_dependencies_to_parents(origin, target),
                    "IgnoreDependenciesToChildren" => {
                        ignore_dependencies_to_children(origin, target)
                    }
                    "IgnoreDependenciesToChildrenAndParents" => {
                        ignore_dependencies_to_children_and_parents(origin, target)
                    }
                    pattern => !rb_rules::patterns::test(pattern, target),
                })
    }
}

/// Renders the diagram. The reporter exits 0, as every drawing reporter does.
///
/// # Errors
/// [`PlantUmlError`] for options that do not apply, a result that is not a graph document, or
/// a name or pattern `ArchUnitNET` refuses.
pub fn render(result: &Value, options: &PlantUmlOptions) -> Result<Rendered, PlantUmlError> {
    let document: GraphDocument = serde_json::from_value(result.clone())
        .map_err(|e| PlantUmlError::Document(e.to_string()))?;
    let architecture = Architecture::new(&document);
    let filter = |origin: &str, target: &str| options.keeps(origin, target);
    let generation = GenerationOptions {
        dependency_filter: Some(&filter),
        include_dependencies_to_other: options.include_dependencies_to_other,
        limit_dependencies: options.limit_dependencies,
        c4_style: options.c4_style,
    };
    let has_types = architecture
        .types
        .values()
        .any(|t| t.referenced != Some(true));
    let from = options.from.unwrap_or(
        if options.matching.is_some() || options.matching_with_packages.is_some() {
            From::Slices
        } else if has_types {
            From::Namespaces
        } else {
            From::Folders
        },
    );
    if options.c4_style && !(from == From::Slices && options.matching_with_packages.is_some()) {
        return invalid(
            "C4Style draws each slice inside the package MatchingWithPackages keeps; use from: slices with MatchingWithPackages",
        );
    }
    let output = match from {
        From::Types => {
            if options.limit_dependencies {
                return invalid(
                    "LimitDependencies applies to slices, namespaces and folders, not to types",
                );
            }
            Builder::new()
                .with_types(&export_types(&architecture), &generation)?
                .render()?
        }
        From::Slices => {
            if let Some(pattern) = &options.matching_with_packages {
                if options.include_dependencies_to_other {
                    return invalid(
                        "IncludeDependenciesToOther applies to types and to the component forms, not to MatchingWithPackages",
                    );
                }
                let slicing = slicing(&architecture, pattern, true)?;
                Builder::new()
                    .with_slices(&export_slices(&slicing), &generation)?
                    .render()?
            } else if let Some(pattern) = &options.matching {
                let slicing = slicing(&architecture, pattern, false)?;
                let mut nodes = export_slices(&slicing);
                remove_pattern_inappropriate(&mut nodes);
                let nodes = nodes
                    .into_iter()
                    .map(|slice| {
                        let texts = slice
                            .types
                            .iter()
                            .filter_map(|member| slicing.texts.get(member).cloned())
                            .collect();
                        Node { slice, texts }
                    })
                    .collect();
                components(&architecture, nodes, &generation, '.', options)?
            } else {
                return invalid(
                    "from: slices needs the slice pattern: Matching (for example \"RiverBooks.(*)\") or MatchingWithPackages",
                );
            }
        }
        From::Namespaces => components(
            &architecture,
            namespaces(&architecture),
            &generation,
            '.',
            options,
        )?,
        From::Folders => components(&architecture, folders(&document), &generation, '/', options)?,
    };
    Ok(Rendered {
        output,
        exit_code: 0,
    })
}

fn slicing(
    architecture: &Architecture<'_>,
    pattern: &str,
    packages: bool,
) -> Result<rb_rules::slices::Slicing, PlantUmlError> {
    rb_rules::slices::slicing(architecture, pattern, packages, "reporterOptions.plantuml")
        .map_err(|e| PlantUmlError::Slicing(e.to_string()))
}

/// A component to draw: the slice the builder reads, and the namespaces (folders) it holds.
struct Node {
    slice: ExportSlice,
    texts: BTreeSet<String>,
}

fn node(name: &str, text: &str) -> Node {
    Node {
        slice: ExportSlice {
            description: name.to_owned(),
            namespace: None,
            asterisks: None,
            is_namespace: false,
            types: BTreeSet::new(),
            dependencies: Vec::new(),
        },
        texts: BTreeSet::from([text.to_owned()]),
    }
}

/// A namespace's component name: the namespace, or [`GLOBAL`].
fn namespace_name(namespace: &str) -> &str {
    if namespace.is_empty() {
        GLOBAL
    } else {
        namespace
    }
}

/// One node per namespace of a loaded type.
fn namespaces(architecture: &Architecture<'_>) -> Vec<Node> {
    let mut by_namespace: BTreeMap<&str, Node> = BTreeMap::new();
    for ty in architecture
        .types
        .values()
        .filter(|t| t.referenced != Some(true))
    {
        let namespace = ty.namespace.as_deref().unwrap_or_default();
        let entry = by_namespace
            .entry(namespace)
            .or_insert_with(|| node(namespace_name(namespace), namespace));
        entry.slice.types.insert(ty.full_name.clone());
        entry.slice.dependencies.extend(
            ty.dependencies
                .iter()
                .map(|d| (ty.full_name.clone(), d.target.clone())),
        );
    }
    by_namespace.into_values().collect()
}

/// The folder a module lies in: its path up to the last `/`, or `.` at the root.
fn folder_of(source: &str) -> &str {
    source.rsplit_once('/').map_or(".", |(folder, _)| folder)
}

/// One node per folder of a local module.
fn folders(document: &GraphDocument) -> Vec<Node> {
    let mut by_folder: BTreeMap<&str, Node> = BTreeMap::new();
    for module in document.modules.iter().filter(|m| {
        m.core_module != Some(true)
            && m.could_not_resolve != Some(true)
            && m.followable != Some(false)
    }) {
        let folder = folder_of(&module.source);
        let entry = by_folder
            .entry(folder)
            .or_insert_with(|| node(folder, folder));
        entry.slice.types.insert(module.source.clone());
        entry.slice.dependencies.extend(
            module
                .dependencies
                .iter()
                .map(|d| (module.source.clone(), d.resolved.clone())),
        );
    }
    by_folder.into_values().collect()
}

/// A character of a stereotype, escaped for both the `regex` crate and JavaScript: `.` and the
/// other metacharacters with a backslash (in a class, every ASCII punctuation character as
/// `\xHH`), `<` and `>` as `\x3C` and `\x3E` so a stereotype never closes early.
fn escaped(c: char, in_class: bool) -> String {
    match c {
        '<' | '>' => format!("\\x{:02X}", u32::from(c)),
        c if in_class && c.is_ascii_punctuation() => format!("\\x{:02X}", u32::from(c)),
        '\\' | '^' | '$' | '.' | '|' | '?' | '*' | '+' | '(' | ')' | '[' | ']' | '{' | '}' => {
            format!("\\{c}")
        }
        c => c.to_string(),
    }
}

/// A regular expression matching one segment (no `separator`) that is none of `excluded`: the
/// complement of a finite set, built over the set's trie, since the linear-time engine has no
/// lookahead.
fn segment_not_in(excluded: &BTreeSet<&str>, separator: char) -> String {
    fn node(suffixes: &[&str], root: bool, separator: char) -> String {
        let firsts: BTreeSet<char> = suffixes.iter().filter_map(|s| s.chars().next()).collect();
        let mut alternatives = Vec::new();
        if !root && !suffixes.contains(&"") {
            alternatives.push(String::new());
        }
        let class: String = std::iter::once(separator)
            .chain(firsts.iter().copied())
            .map(|c| escaped(c, true))
            .collect();
        alternatives.push(format!("[^{class}][^{}]*", escaped(separator, true)));
        for first in &firsts {
            let rest: Vec<&str> = suffixes
                .iter()
                .filter_map(|s| s.strip_prefix(*first))
                .collect();
            alternatives.push(format!(
                "{}{}",
                escaped(*first, false),
                node(&rest, false, separator)
            ));
        }
        format!("(?:{})", alternatives.join("|"))
    }
    let suffixes: Vec<&str> = excluded.iter().copied().collect();
    node(&suffixes, true, separator)
}

/// The stereotype of a component holding the namespaces (folders) `texts`: a regular expression
/// that matches each text, and each full name one segment below it (the name of a type or module
/// it holds), but no namespace of `known` below it. So the components never intersect, and a
/// namespace the diagram leaves out lies in no component. `adhereTo` matches a stereotype against
/// a type's namespace to place it, and against a dependency's full name to allow it.
#[must_use]
pub fn stereotype(texts: &BTreeSet<String>, known: &BTreeSet<String>, separator: char) -> String {
    let patterns: Vec<String> = texts
        .iter()
        .map(|text| {
            let children: BTreeSet<&str> = known
                .iter()
                .filter_map(|k| {
                    let below = if text.is_empty() {
                        Some(k.as_str())
                    } else {
                        k.strip_prefix(text.as_str())?.strip_prefix(separator)
                    };
                    below
                        .and_then(|b| b.split(separator).next())
                        .filter(|s| !s.is_empty())
                })
                .collect();
            let segment = segment_not_in(&children, separator);
            if text.is_empty() {
                format!("{segment}?")
            } else {
                let literal: String = text.chars().map(|c| escaped(c, false)).collect();
                format!("{literal}(?:{}{segment})?", escaped(separator, false))
            }
        })
        .collect();
    match &patterns[..] {
        [one] => format!("^{one}$"),
        many => format!("^(?:{})$", many.join("|")),
    }
}

/// The component forms: each dependency target is placed in the node holding its namespace
/// (folder), or, with `IncludeDependenciesToOther`, in a node of its own namespace; then the
/// builder selects the arrows and they are written as an `adhereTo` diagram.
fn components(
    architecture: &Architecture<'_>,
    mut nodes: Vec<Node>,
    generation: &GenerationOptions<'_>,
    separator: char,
    options: &PlantUmlOptions,
) -> Result<String, PlantUmlError> {
    let text_of = |target: &str| -> String {
        if separator == '/' {
            // A module outside the folders with no path (a core module, a bare package) is a
            // node of its own rather than one of the root folder.
            if target.contains('/') {
                folder_of(target).to_owned()
            } else {
                target.to_owned()
            }
        } else {
            rb_rules::plantuml::namespace_of(architecture, target)
        }
    };
    let mut others: BTreeMap<String, Node> = BTreeMap::new();
    let members: BTreeSet<String> = nodes
        .iter()
        .flat_map(|n| n.slice.types.iter().cloned())
        .collect();
    let targets: BTreeSet<String> = nodes
        .iter()
        .flat_map(|n| n.slice.dependencies.iter())
        .filter(|(origin, target)| options.keeps(origin, target))
        .map(|(_, target)| target.clone())
        .filter(|target| !members.contains(target))
        .collect();
    for target in targets {
        let text = text_of(&target);
        if let Some(home) = nodes.iter_mut().find(|n| n.texts.contains(&text)) {
            home.slice.types.insert(target);
        } else if options.include_dependencies_to_other {
            others
                .entry(text.clone())
                .or_insert_with(|| node(namespace_name(&text), &text))
                .slice
                .types
                .insert(target);
        }
    }
    nodes.extend(others.into_values());
    // Every namespace (folder) the graph knows, with its ancestors: what a stereotype excludes.
    let mut known: BTreeSet<String> = BTreeSet::new();
    let mut know = |text: &str| {
        let mut current = text;
        while !current.is_empty() && known.insert(current.to_owned()) {
            match current.rfind(separator) {
                Some(at) => current = &current[..at],
                None => break,
            }
        }
    };
    for node in &nodes {
        for text in &node.texts {
            know(text);
        }
        for (_, target) in &node.slice.dependencies {
            know(&text_of(target));
        }
    }
    if separator == '.' {
        for name in architecture.types.keys() {
            know(&text_of(name));
        }
    }
    nodes.sort_by(|a, b| a.slice.description.cmp(&b.slice.description));
    let slices: Vec<ExportSlice> = nodes.iter().map(|n| n.slice.clone()).collect();
    let builder = Builder::new().with_slices(&slices, generation)?;
    let depth = |name: &str| name.matches(separator).count();
    let mut lines: Vec<String> = Vec::new();
    for node in &nodes {
        // The builder's check on the name, then the stereotype after it.
        SliceNode::new(&node.slice.description, None, None)?;
        lines.push(format!(
            "[{}] <<{}>>",
            node.slice.description,
            stereotype(&node.texts, &known, separator)
        ));
    }
    for Dependency {
        origin,
        target,
        kind,
    } in builder.dependencies()
    {
        match kind {
            DependencyType::Circle => {
                lines.push(format!("[{origin}] --> [{target}]"));
                lines.push(format!("[{target}] --> [{origin}]"));
            }
            DependencyType::OneToOneCompact if depth(origin) != depth(target) => {}
            // Without packages the builder draws only these three kinds.
            _ => lines.push(format!("[{origin}] --> [{target}]")),
        }
    }
    let mut body = lines.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    Ok(format!(
        "{ADHERE_HEADER}{body}{}",
        rb_rules::plantuml_export::FOOTER
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_model::{
        Dependency as Edge, ElementDependency, Language, Location, Module, ModuleSystem,
        TypeElement,
    };
    use serde_json::json;

    fn ty(full: &str, namespace: Option<&str>, kind: &str, deps: &[&str]) -> TypeElement {
        let name = full.rsplit('.').next().unwrap_or(full);
        let mut t = TypeElement::new(full, name, kind, Location::in_file(Language::Dotnet, None));
        t.namespace = namespace.map(str::to_owned);
        t.dependencies = deps
            .iter()
            .map(|d| ElementDependency {
                target: (*d).to_owned(),
                kind: "body".into(),
                member: None,
                line: None,
                form: None,
            })
            .collect();
        t
    }

    /// Orders and Billing depend on each other, Billing on Shared, a helper one namespace below
    /// Orders on Orders, a type in the global namespace on Orders, and Orders on `System.String`.
    fn shop() -> Value {
        let mut string = ty("System.String", Some("System"), "class", &[]);
        string.referenced = Some(true);
        let document = GraphDocument {
            code: Some(rb_model::CodeLayer {
                types: vec![
                    ty(
                        "A.Orders.Order",
                        Some("A.Orders"),
                        "class",
                        &["A.Billing.Invoice", "A.Orders.Order", "System.String"],
                    ),
                    ty(
                        "A.Orders.Internal.Helper",
                        Some("A.Orders.Internal"),
                        "class",
                        &["A.Orders.Order"],
                    ),
                    ty(
                        "A.Billing.Invoice",
                        Some("A.Billing"),
                        "class",
                        &["A.Orders.Order", "A.Shared.Money"],
                    ),
                    ty("A.Shared.Money", Some("A.Shared"), "interface", &[]),
                    ty("Program", None, "class", &["A.Orders.Order"]),
                    string,
                ],
                ..rb_model::CodeLayer::default()
            }),
            ..GraphDocument::default()
        };
        serde_json::to_value(&document).unwrap_or(Value::Null)
    }

    fn options(section: &Value) -> Result<PlantUmlOptions, PlantUmlError> {
        PlantUmlOptions::from_reporter_options(Some(section), None)
    }

    fn draw(section: &Value) -> Result<String, PlantUmlError> {
        Ok(render(&shop(), &options(section)?)?.output)
    }

    fn arrows(text: &str) -> Vec<&str> {
        text.lines().filter(|l| l.contains(" --> ")).collect()
    }

    #[test]
    fn options_take_archunitnets_names() -> Result<(), PlantUmlError> {
        assert_eq!(
            PlantUmlOptions::from_reporter_options(None, None)?,
            PlantUmlOptions::default()
        );
        let all = options(&json!({
            "from": "slices", "Matching": "A.(*)", "LimitDependencies": true, "C4Style": false,
            "FocusOn": "^A", "IncludeDependenciesToOther": true,
            "DependencyFilters": ["IgnoreDependenciesToParents", "^System\\."]
        }))?;
        assert_eq!(
            all,
            PlantUmlOptions {
                from: Some(From::Slices),
                matching: Some("A.(*)".into()),
                matching_with_packages: None,
                limit_dependencies: true,
                c4_style: false,
                focus_on: Some("^A".into()),
                include_dependencies_to_other: true,
                dependency_filters: vec!["IgnoreDependenciesToParents".into(), "^System\\.".into()],
            }
        );
        let flag = PlantUmlOptions::from_reporter_options(
            Some(&json!({ "from": "types" })),
            Some("folders"),
        )?;
        assert_eq!(flag.from, Some(From::Folders), "--from wins");
        for (section, message) in [
            (json!({ "Typo": 1 }), "unknown key `Typo`"),
            (json!({ "from": "classes" }), "from `classes`"),
            (json!({ "from": 1 }), "from must be a string"),
            (
                json!({ "LimitDependencies": "yes" }),
                "LimitDependencies must be true or false",
            ),
            (
                json!({ "DependencyFilters": "^A" }),
                "DependencyFilters must be a list of strings",
            ),
            (
                json!({ "DependencyFilters": [1] }),
                "DependencyFilters must be a list of strings",
            ),
            (
                json!({ "DependencyFilters": ["("] }),
                "DependencyFilters `(` is not a regular expression",
            ),
            (
                json!({ "FocusOn": "(" }),
                "FocusOn `(` is not a regular expression",
            ),
            (
                json!({ "Matching": "A.(*)", "MatchingWithPackages": "A.(*)" }),
                "not both",
            ),
            (json!([]), "must be an object"),
        ] {
            let error = options(&section)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(error.starts_with("reporterOptions.plantuml: "), "{error}");
            assert!(error.contains(message), "{section}: {error}");
        }
        for name in ["slices", "types", "namespaces", "folders"] {
            assert!(From::parse(name).is_some());
        }
        assert_eq!(From::parse("Slices"), None);
        Ok(())
    }

    #[test]
    fn focus_and_filters_keep_dependencies_as_upstream() -> Result<(), PlantUmlError> {
        let focus = options(&json!({ "FocusOn": "^A\\.Billing" }))?;
        assert!(focus.keeps("A.Billing.X", "A.Orders.Y"));
        assert!(focus.keeps("A.Orders.Y", "A.Billing.X"));
        assert!(
            !focus.keeps("A.Billing.X", "A.Billing.Z"),
            "both ends focused"
        );
        assert!(
            !focus.keeps("A.Orders.Y", "A.Shared.Z"),
            "neither end focused"
        );
        let filters = options(&json!({ "DependencyFilters": [
            "IgnoreDependenciesToParents", "IgnoreDependenciesToChildren", "^System\\."
        ] }))?;
        assert!(filters.keeps("A.B", "A.C"));
        assert!(!filters.keeps("A.B+C", "A.B"), "to a parent");
        assert!(!filters.keeps("A.B", "A.B+C"), "to a child");
        assert!(
            !filters.keeps("A.B", "System.String"),
            "a target the pattern names"
        );
        let both =
            options(&json!({ "DependencyFilters": ["IgnoreDependenciesToChildrenAndParents"] }))?;
        assert!(
            !both.keeps("A.B", "A.B+C") && !both.keeps("A.B+C", "A.B") && both.keeps("A.B", "A.C")
        );
        Ok(())
    }

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_stereotype_holds_its_namespaces_and_their_types_only() {
        assert_eq!(
            stereotype(&set(&["A.Orders"]), &set(&["A", "A.Orders"]), '.'),
            "^A\\.Orders(?:\\.(?:[^\\x2E][^\\x2E]*))?$"
        );
        assert_eq!(
            stereotype(&set(&["A.B"]), &set(&["A.B", "A.B.C"]), '.'),
            "^A\\.B(?:\\.(?:[^\\x2EC][^\\x2E]*|C(?:[^\\x2E][^\\x2E]*)))?$"
        );
        assert_eq!(
            stereotype(&set(&["x<y", "z"]), &BTreeSet::new(), '/'),
            "^(?:x\\x3Cy(?:/(?:[^\\x2F][^\\x2F]*))?|z(?:/(?:[^\\x2F][^\\x2F]*))?)$"
        );
        let known = set(&["A", "A.Orders", "A.Orders.Internal", "A.Ordersx", "System"]);
        let orders = stereotype(&set(&["A.Orders"]), &known, '.');
        let global = stereotype(&set(&[""]), &known, '.');
        for (pattern, text, want) in [
            (&orders, "A.Orders", true),
            (&orders, "A.Orders.Order", true),
            (&orders, "A.Orders.Outer+Inner", true),
            (&orders, "A.Orders.Internal", false),
            (&orders, "A.Orders.Internal.Helper", false),
            (&orders, "A.Orders.Order.X", false),
            (&orders, "A.Ordersx", false),
            (&orders, "A.Order", false),
            (&global, "", true),
            (&global, "Program", true),
            (&global, "A", false),
            (&global, "System", false),
            (&global, "A.B", false),
        ] {
            assert_eq!(
                rb_rules::patterns::test(pattern, text),
                want,
                "{pattern} {text}"
            );
        }
    }

    proptest::proptest! {
        /// The complement is exact: over a small alphabet, a segment matches when it is not empty,
        /// holds no separator and is none of the excluded, under both the `regex` crate and the
        /// JavaScript-compatible matcher `adhereTo` uses.
        #[test]
        fn segment_not_in_is_the_complement(
            excluded in proptest::collection::btree_set("[ab]{1,3}", 0..6),
            candidate in "[ab.]{0,4}",
        ) {
            let excluded: BTreeSet<&str> = excluded.iter().map(String::as_str).collect();
            let pattern = format!("^{}$", segment_not_in(&excluded, '.'));
            let want = !candidate.is_empty()
                && !candidate.contains('.')
                && !excluded.contains(candidate.as_str());
            let compiled = regex::Regex::new(&pattern);
            proptest::prop_assert!(compiled.is_ok(), "{}", pattern);
            proptest::prop_assert_eq!(compiled.map_or(!want, |r| r.is_match(&candidate)), want);
            proptest::prop_assert_eq!(rb_rules::patterns::test(&pattern, &candidate), want);
        }
    }

    #[test]
    fn namespaces_are_components_with_arrows_and_circles() -> Result<(), PlantUmlError> {
        let text = draw(&json!({}))?;
        assert!(text.starts_with(ADHERE_HEADER) && text.ends_with("@enduml\n"));
        let components: Vec<&str> = text
            .lines()
            .filter(|l| l.contains(" <<"))
            .map(|l| l.split(" <<").next().unwrap_or_default())
            .collect();
        assert_eq!(
            components,
            [
                "[(global)]",
                "[A.Billing]",
                "[A.Orders]",
                "[A.Orders.Internal]",
                "[A.Shared]"
            ]
        );
        assert_eq!(
            arrows(&text),
            [
                "[(global)] --> [A.Orders]",
                "[A.Billing] --> [A.Orders]",
                "[A.Orders] --> [A.Billing]",
                "[A.Billing] --> [A.Shared]",
                "[A.Orders.Internal] --> [A.Orders]",
            ]
        );
        assert_eq!(
            text,
            draw(&json!({ "from": "namespaces" }))?,
            "the default for types"
        );
        assert_eq!(text, draw(&json!({}))?, "deterministic");
        // The parser `adhereTo` uses reads every component and arrow.
        let parsed =
            rb_rules::plantuml::parse(&text).map_err(|e| PlantUmlError::Slicing(e.to_string()))?;
        assert_eq!(parsed.components.len(), 5);
        assert_eq!(parsed.dependencies.values().map(Vec::len).sum::<usize>(), 5);
        Ok(())
    }

    #[test]
    fn every_generation_option_changes_the_arrows() -> Result<(), PlantUmlError> {
        let limited = draw(&json!({ "LimitDependencies": true }))?;
        assert_eq!(
            arrows(&limited),
            [
                "[A.Billing] --> [A.Orders]",
                "[A.Billing] --> [A.Shared]",
                "[A.Orders] --> [A.Billing]",
            ],
            "only between namespaces at the same depth, and no circles"
        );
        let focused = draw(&json!({ "FocusOn": "^A\\.Billing" }))?;
        assert_eq!(
            arrows(&focused),
            [
                "[A.Billing] --> [A.Orders]",
                "[A.Orders] --> [A.Billing]",
                "[A.Billing] --> [A.Shared]",
            ]
        );
        let filtered = draw(&json!({ "DependencyFilters": ["^A\\.Shared\\."] }))?;
        assert!(!filtered.contains("--> [A.Shared]"), "{filtered}");
        let slices = draw(&json!({ "Matching": "A.(*)" }))?;
        assert_eq!(
            arrows(&slices),
            [
                "[Billing] --> [Orders]",
                "[Orders] --> [Billing]",
                "[Billing] --> [Shared]",
            ],
            "Orders.Internal is deeper than the pattern's one (*)"
        );
        assert!(slices.contains("[Orders] <<^A\\.Orders(?:"), "{slices}");
        let other = draw(&json!({ "Matching": "A.(*)", "IncludeDependenciesToOther": true }))?;
        assert!(other.contains("\n[System] <<^System(?:"), "{other}");
        assert!(other.contains("\n[Orders] --> [System]\n"), "{other}");
        assert!(
            !other.contains("\n[A.Orders.Internal] <<"),
            "only depended-on namespaces"
        );
        Ok(())
    }

    #[test]
    fn types_and_packages_are_archunitnets_text() -> Result<(), PlantUmlError> {
        let types = draw(&json!({ "from": "types" }))?;
        assert_eq!(
            types,
            format!(
                "{}class \"A.Billing.Invoice\" {{\n}}\nclass \"A.Orders.Internal.Helper\" {{\n}}\nclass \"A.Orders.Order\" {{\n}}\nclass \"Program\" {{\n}}\ninterface \"A.Shared.Money\" {{\n}}\n[A.Billing.Invoice] --|> [A.Orders.Order]\n[A.Billing.Invoice] --|> [A.Shared.Money]\n[A.Orders.Internal.Helper] --|> [A.Orders.Order]\n[A.Orders.Order] --|> [A.Billing.Invoice]\n[Program] --|> [A.Orders.Order]\n@enduml\n",
                rb_rules::plantuml_export::HEADER
            )
        );
        let other = draw(&json!({ "from": "types", "IncludeDependenciesToOther": true }))?;
        assert!(other.contains("[A.Orders.Order] --|> [System.String]\n"));
        let packages = draw(&json!({ "MatchingWithPackages": "A.(*)" }))?;
        assert!(packages.starts_with(rb_rules::plantuml_export::HEADER));
        assert!(
            packages.contains("package A {\n[Orders] as A.Orders\n}\n"),
            "{packages}"
        );
        let c4 = draw(
            &json!({ "MatchingWithPackages": "A.(*)", "C4Style": true, "LimitDependencies": true }),
        )?;
        assert!(c4.contains("Container(A.Orders, Orders)"), "{c4}");
        Ok(())
    }

    #[test]
    fn options_that_do_not_apply_are_refused() {
        for (section, message) in [
            (json!({ "C4Style": true }), "C4Style"),
            (json!({ "Matching": "A.(*)", "C4Style": true }), "C4Style"),
            (
                json!({ "from": "types", "LimitDependencies": true }),
                "LimitDependencies",
            ),
            (
                json!({ "MatchingWithPackages": "A.(*)", "IncludeDependenciesToOther": true }),
                "IncludeDependenciesToOther",
            ),
            (json!({ "from": "slices" }), "needs the slice pattern"),
            (json!({ "Matching": "A.X" }), "A.X"),
        ] {
            let error = draw(&section)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(error.contains(message), "{section}: {error}");
        }
        let not_a_graph = render(&json!({ "modules": 3 }), &PlantUmlOptions::default());
        assert!(matches!(not_a_graph, Err(PlantUmlError::Document(_))));
    }

    #[test]
    fn folders_group_local_modules() -> Result<(), PlantUmlError> {
        let module = |source: &str, targets: &[&str]| {
            let mut m = Module::new(source);
            m.language = Some(Language::Typescript);
            m.dependencies = targets
                .iter()
                .map(|t| Edge::new("x", *t, ModuleSystem::Es6))
                .collect();
            m
        };
        let mut core = module("fs", &[]);
        core.core_module = Some(true);
        let document = GraphDocument {
            modules: vec![
                module("root.ts", &["src/a/x.ts"]),
                module("src/a/x.ts", &["src/b/y.ts", "fs"]),
                module("src/b/y.ts", &["src/a/x.ts"]),
                core,
            ],
            ..GraphDocument::default()
        };
        let value = serde_json::to_value(&document).unwrap_or(Value::Null);
        let text = render(&value, &PlantUmlOptions::default())?.output;
        assert!(
            text.contains("\n[src/a] <<^src/a(?:/(?:[^\\x2F][^\\x2F]*))?$>>\n"),
            "{text}"
        );
        assert_eq!(
            arrows(&text),
            [
                "[.] --> [src/a]",
                "[src/a] --> [src/b]",
                "[src/b] --> [src/a]"
            ]
        );
        let other = PlantUmlOptions {
            include_dependencies_to_other: true,
            ..PlantUmlOptions::default()
        };
        let with_core = render(&value, &other)?.output;
        assert!(with_core.contains("\n[fs] <<^fs(?:"), "{with_core}");
        assert!(
            with_core.contains("\n[src/a] --> [fs]\n"),
            "a core module is its own node: {with_core}"
        );
        Ok(())
    }

    #[test]
    fn illegal_names_are_refused() {
        let module = Module::new("app/[slug]/page.tsx");
        let document = GraphDocument {
            modules: vec![module],
            ..GraphDocument::default()
        };
        let value = serde_json::to_value(&document).unwrap_or(Value::Null);
        let error = render(&value, &PlantUmlOptions::default())
            .err()
            .map(|e| e.to_string());
        assert!(
            error
                .as_deref()
                .is_some_and(|e| e.contains("IllegalComponentNameException")),
            "{error:?}"
        );
    }
}
