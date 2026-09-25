//! `PlantUML` component diagrams: parsing the subset `ArchUnitNET` reads, associating types with
//! components, and whether a type adheres.
//!
//! - Source: [design § Diagram rules](../../../docs/artifacts/design.md#diagram-rules)
//! - Coverage: [`ArchUnitNET` § `PlantUML`](../../../docs/artifacts/archunitnet-0.13.4-coverage.md#plantuml)
//! - Plan: [Wave 2, Step 6](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#26-step-6-slice-and-diagram-rules-2c)
//! - Decision: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md) (the
//!   upstream suite is the specification; the ported cases are in
//!   `conformance/archunitnet/ported/PlantUml*.yaml`)
//! - Requirement: [FR-RULE-05](../../../docs/prd.md#fr-rule-05)
//! - Specification: `ArchUnitNET` 0.13.4 `Domain/PlantUml/Import` (`PlantUmlParser`,
//!   `PlantUmlPatterns`, `PlantUmlComponents`, `PlantUmlDiagramBuilder`, `Alias`,
//!   `ClassDiagramAssociation`) and `TypeConditionsDefinition.AdhereToPlantUmlDiagram`
//!
//! | Diagram line | Read as |
//! | --- | --- |
//! | a line whose first non-blank character is `'` | a comment |
//! | `[Name] <<Pattern>> <<Other>> as Alias` (alias optionally quoted) | a component; every `<<...>>` on the line is a stereotype, a regular expression over a type's namespace, and there must be at least one |
//! | `A --> B`, `[A] -right-> [B]`, `A --[#green]-> B`, `B <-- A`, `: label` | a dependency from A to B; A and B are names or aliases, bracketed or not, and the arrow needs a blank on each side |
//! | `!include ...` | refused (`IllegalDiagramException`): the included diagram is not read |
//! | anything else | ignored (`@startuml`, `skinparam`, `note`, `..>`, `<-->`) |
//!
//! Identical component lines are one component; of two with the same name the first counts. The
//! stereotypes must be unique across components (checked when the diagram is associated, as
//! `ClassDiagramAssociation` does). A type adheres when it has no dependency on a type in some
//! component, or every such dependency's full name matches a stereotype of its own component or
//! of a component its component points to. A type that needs its component but lies in two is
//! `ComponentIntersectionException`, in none `InvalidOperationException`. Every failure surfaces
//! as [`ElementError::Diagram`], exit 3, whose message starts with the upstream exception's name.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::LazyLock;

use regex::Regex;

use crate::elements::{Architecture, ElementError, Evaluator, Object};

/// The .NET exception `ArchUnitNET` raises, which names each way a diagram or a class fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exception {
    /// A malformed diagram: a component without a stereotype, an illegal alias, a dependency on
    /// no component, a repeated stereotype.
    IllegalDiagram,
    /// A type contained in more than one component.
    ComponentIntersection,
    /// The diagram file cannot be read.
    PlantUmlParse,
    /// A type with dependencies into the diagram contained in no component.
    InvalidOperation,
    /// Two components with one alias (`ToDictionary`'s duplicate key), or a stereotype that is
    /// not a regular expression.
    Argument,
    /// A dependency line whose only arrow sits in its `: description`.
    ArgumentOutOfRange,
}

impl Exception {
    /// The upstream exception's class name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::IllegalDiagram => "IllegalDiagramException",
            Self::ComponentIntersection => "ComponentIntersectionException",
            Self::PlantUmlParse => "PlantUmlParseException",
            Self::InvalidOperation => "InvalidOperationException",
            Self::Argument => "ArgumentException",
            Self::ArgumentOutOfRange => "ArgumentOutOfRangeException",
        }
    }
}

/// Why a diagram cannot be read or a class cannot be placed: the upstream exception and its
/// message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}: {}", .exception.name(), .message)]
pub struct DiagramError {
    /// The exception `ArchUnitNET` raises.
    pub exception: Exception,
    /// Its message.
    pub message: String,
}

fn fail<T>(exception: Exception, message: String) -> Result<T, DiagramError> {
    Err(DiagramError { exception, message })
}

/// One component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    /// The name between the brackets.
    pub name: String,
    /// The stereotypes, each a namespace pattern, in the order they first appear on the line.
    pub stereotypes: Vec<String>,
    /// The alias, when given.
    pub alias: Option<String>,
}

impl Component {
    /// `PlantUmlComponent.Equals`: the same name, alias and stereotype set.
    fn same_as(&self, other: &Self) -> bool {
        self.name == other.name
            && self.alias == other.alias
            && self.stereotypes.len() == other.stereotypes.len()
            && self
                .stereotypes
                .iter()
                .all(|s| other.stereotypes.contains(s))
    }
}

/// A parsed diagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagram {
    /// Every component, in the order they first appear.
    pub components: Vec<Component>,
    /// For each component index with dependencies, the components it depends on, in the order
    /// the diagram draws them, each once.
    pub dependencies: BTreeMap<usize, Vec<usize>>,
}

impl Diagram {
    /// The components component `index` depends on.
    #[must_use]
    pub fn dependencies_of(&self, index: usize) -> &[usize] {
        self.dependencies.get(&index).map_or(&[], Vec::as_slice)
    }
}

/// `PlantUmlPatterns.PlantUmlComponentPattern`
/// (`^\s*\[(?<componentName>[^\[\]]+)]\s*(?:<<[^<>]+>>\s*)*\s*(?:as "?(?<alias>[^"]+)"?)?\s*$`):
/// the name and the alias of a component line.
fn component_line(line: &str) -> Option<(&str, Option<&str>)> {
    let rest = line.trim_start().strip_prefix('[')?;
    let end = rest.find(['[', ']'])?;
    if end == 0 || !rest[end..].starts_with(']') {
        return None;
    }
    let name = &rest[..end];
    let mut rest = rest[end + 1..].trim_start();
    while let Some(after) = rest.strip_prefix("<<") {
        let end = after.find(['<', '>'])?;
        if end == 0 || !after[end..].starts_with(">>") {
            return None;
        }
        rest = after[end + 2..].trim_start();
    }
    if rest.is_empty() {
        return Some((name, None));
    }
    let after = rest.strip_prefix("as ")?;
    let after = after.strip_prefix('"').unwrap_or(after);
    // `[^"]+` is greedy, so an unquoted alias keeps its trailing blanks, as upstream's does.
    let end = after.find('"').unwrap_or(after.len());
    if end == 0 {
        return None;
    }
    let tail = &after[end..];
    let tail = tail.strip_prefix('"').unwrap_or(tail);
    tail.trim()
        .is_empty()
        .then_some((name, Some(&after[..end])))
}

static STEREOTYPE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new("<<([^<>]+)>>").ok());

/// `PlantUmlComponentMatcher.MatchStereoTypes` (`<<([^<>]+)>>`): every stereotype anywhere on
/// the line, each once.
fn stereotypes(line: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for captures in STEREOTYPE.iter().flat_map(|re| re.captures_iter(line)) {
        if let Some(stereotype) = captures.get(1).map(|m| m.as_str())
            && !found.iter().any(|f| f == stereotype)
        {
            found.push(stereotype.to_owned());
        }
    }
    found
}

/// `new Alias(value)`: an alias, or any name a dependency uses, may not hold `[`, `]` or `"`.
fn check_alias(value: &str) -> Result<(), DiagramError> {
    if value.contains(['[', ']', '"']) {
        return fail(
            Exception::IllegalDiagram,
            format!("Alias '{value}' should not contain character(s): '[' or ']' or '\"'"),
        );
    }
    Ok(())
}

const ARROW_CENTRE: &str = r"(?:left|right|up|down|\[[^\]]+\])?";
static RIGHT_ARROW: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(&format!(r"\s-+{ARROW_CENTRE}-*>\s")).ok());
static LEFT_ARROW: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(&format!(r"\s<-*{ARROW_CENTRE}-+\s")).ok());

/// `PlantUmlDependencyMatcher.TryParseFromLeftToRight` then `TryParseFromRightToLeft`: the
/// (origin, target) texts of the line's dependencies. The arrow is looked for in the whole line
/// and the parts are read once its `: description` is cut, as upstream does.
fn dependency_line(line: &str) -> Result<Vec<(String, String)>, DiagramError> {
    let mut found = Vec::new();
    for (arrow, left_to_right) in [(&*RIGHT_ARROW, true), (&*LEFT_ARROW, false)] {
        let Some(arrow) = arrow.as_ref().filter(|a| a.is_match(line)) else {
            continue;
        };
        let described = line.find(':').map_or(line, |colon| &line[..colon]);
        let replaced = arrow.replace_all(described, " ");
        let parts: Vec<&str> = replaced.split(' ').map(str::trim).take(2).collect();
        let [first, second] = parts[..] else {
            return fail(
                Exception::ArgumentOutOfRange,
                format!("the dependency line `{line}` has its arrow only in its `: description`"),
            );
        };
        let (origin, target) = if left_to_right {
            (first, second)
        } else {
            (second, first)
        };
        found.push((origin.to_owned(), target.to_owned()));
    }
    Ok(found)
}

/// A component's identity for dependencies (`ComponentIdentifier`): its name and alias.
type Identifier = (String, Option<String>);

/// The components as `PlantUmlComponents` indexes them.
struct Components {
    parsed: Vec<Component>,
    by_name: BTreeMap<String, usize>,
    by_alias: BTreeMap<String, usize>,
}

impl Components {
    fn new(parsed: Vec<Component>) -> Result<Self, DiagramError> {
        let mut by_name = BTreeMap::new();
        let mut by_alias = BTreeMap::new();
        for (index, component) in parsed.iter().enumerate() {
            by_name.entry(component.name.clone()).or_insert(index);
            if let Some(alias) = &component.alias
                && by_alias.insert(alias.clone(), index).is_some()
            {
                return fail(
                    Exception::Argument,
                    format!(
                        "An item with the same key has already been added: two components have the alias '{alias}'"
                    ),
                );
            }
        }
        Ok(Self {
            parsed,
            by_name,
            by_alias,
        })
    }

    /// `PlantUmlParser.FindComponentMatching`: the component a dependency's origin or target
    /// names, by alias first, then by name.
    fn find(&self, text: &str) -> Result<Identifier, DiagramError> {
        let text = text.trim();
        let text = text.strip_prefix('[').unwrap_or(text);
        let text = text.strip_suffix(']').unwrap_or(text);
        check_alias(text)?;
        let Some(&index) = self.by_alias.get(text).or_else(|| self.by_name.get(text)) else {
            return fail(
                Exception::IllegalDiagram,
                format!(
                    "There is no Component with name or alias = '{text}'. Components must be specified separately from dependencies."
                ),
            );
        };
        let component = &self.parsed[index];
        Ok((component.name.clone(), component.alias.clone()))
    }
}

/// Parses a diagram's text.
///
/// # Errors
/// [`DiagramError`]: `IllegalDiagramException` for a component without a stereotype, an alias
/// holding `[`, `]` or `"`, a dependency naming no component, or an `!include`;
/// `ArgumentException` for two components with one alias; `ArgumentOutOfRangeException` for a
/// line whose only arrow is in its description.
pub fn parse(text: &str) -> Result<Diagram, DiagramError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('\''))
        .collect();
    let mut parsed: Vec<Component> = Vec::new();
    for line in &lines {
        if line.trim_start().starts_with("!include") {
            return fail(
                Exception::IllegalDiagram,
                format!(
                    "`{}` is not supported; inline the included diagram",
                    line.trim()
                ),
            );
        }
        let Some((name, alias)) = component_line(line) else {
            continue;
        };
        let stereotypes = stereotypes(line);
        if stereotypes.is_empty() {
            return fail(
                Exception::IllegalDiagram,
                format!(
                    "Components must include at least one stereotype specifying the namespace identifier (<<.*>>), but component '{name}' does not"
                ),
            );
        }
        if let Some(alias) = alias {
            check_alias(alias)?;
        }
        let component = Component {
            name: name.to_owned(),
            stereotypes,
            alias: alias.map(str::to_owned),
        };
        if !parsed.iter().any(|p| p.same_as(&component)) {
            parsed.push(component);
        }
    }
    let components = Components::new(parsed)?;
    // Every line's arrows are read before any name is resolved, as `MatchDependencies` does.
    let mut texts = Vec::new();
    for line in &lines {
        texts.extend(dependency_line(line)?);
    }
    let mut drawn: Vec<(Identifier, Identifier)> = Vec::new();
    for (origin, target) in &texts {
        let pair = (components.find(origin)?, components.find(target)?);
        if !drawn.contains(&pair) {
            drawn.push(pair);
        }
    }
    // `AllComponents` holds the first component of each name; a dependency drawn from a
    // component that lost its name to an earlier one is dropped, and a target is found by name.
    let kept: Vec<Component> = components
        .parsed
        .iter()
        .enumerate()
        .filter(|(index, c)| components.by_name.get(&c.name) == Some(index))
        .map(|(_, c)| c.clone())
        .collect();
    let mut dependencies: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for ((origin_name, origin_alias), (target_name, _)) in &drawn {
        let origin = kept
            .iter()
            .position(|c| &c.name == origin_name && &c.alias == origin_alias);
        let target = kept.iter().position(|c| &c.name == target_name);
        if let (Some(origin), Some(target)) = (origin, target) {
            dependencies.entry(origin).or_default().push(target);
        }
    }
    Ok(Diagram {
        components: kept,
        dependencies,
    })
}

/// `FullNameMatches(pattern)`: .NET's `Regex.IsMatch`, unanchored.
fn matches(pattern: &str, text: &str) -> Result<bool, DiagramError> {
    if crate::patterns::get(pattern).is_none() {
        return fail(
            Exception::Argument,
            format!("Invalid pattern '{pattern}': a stereotype must be a regular expression"),
        );
    }
    Ok(crate::patterns::test(pattern, text))
}

/// `String.Compare` under the invariant culture, near enough for component names: letters
/// compare case-insensitively first, and on a tie a lower-case letter sorts first.
fn culture_key(name: &str) -> (String, Vec<bool>) {
    (
        name.to_lowercase(),
        name.chars().map(char::is_uppercase).collect(),
    )
}

/// A diagram associated with types (`ClassDiagramAssociation`): which components a namespace
/// lies in, and which namespace patterns a type may depend on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Association {
    diagram: Diagram,
}

impl Association {
    /// Associates a parsed diagram.
    ///
    /// # Errors
    /// `IllegalDiagramException` when two components share a stereotype.
    pub fn new(diagram: Diagram) -> Result<Self, DiagramError> {
        let mut seen: Vec<&str> = Vec::new();
        for stereotype in diagram.components.iter().flat_map(|c| &c.stereotypes) {
            if seen.contains(&stereotype.as_str()) {
                return fail(
                    Exception::IllegalDiagram,
                    format!("Stereotype '{stereotype}' should be unique"),
                );
            }
            seen.push(stereotype);
        }
        Ok(Self { diagram })
    }

    /// The diagram.
    #[must_use]
    pub fn diagram(&self) -> &Diagram {
        &self.diagram
    }

    /// The components with a stereotype matching `namespace`.
    fn associated(&self, namespace: &str) -> Result<Vec<usize>, DiagramError> {
        let mut found = Vec::new();
        for (index, component) in self.diagram.components.iter().enumerate() {
            for stereotype in &component.stereotypes {
                if matches(stereotype, namespace)? {
                    found.push(index);
                    break;
                }
            }
        }
        Ok(found)
    }

    /// `Contains`: whether a type in `namespace` lies in some component.
    ///
    /// # Errors
    /// `ArgumentException` for a stereotype that is not a regular expression.
    pub fn contains(&self, namespace: &str) -> Result<bool, DiagramError> {
        Ok(!self.associated(namespace)?.is_empty())
    }

    /// `GetComponentOf`: the one component of the type named `class` in `namespace`.
    ///
    /// # Errors
    /// `ComponentIntersectionException` when it lies in two components, `InvalidOperationException`
    /// when in none.
    pub fn component_of(&self, class: &str, namespace: &str) -> Result<usize, DiagramError> {
        match self.associated(namespace)?[..] {
            [one] => Ok(one),
            [] => fail(
                Exception::InvalidOperation,
                format!("Class {class} is not contained in any component"),
            ),
            ref many => {
                let mut names: Vec<&str> = many
                    .iter()
                    .map(|i| self.diagram.components[*i].name.as_str())
                    .collect();
                names.sort_by_cached_key(|n| culture_key(n));
                names.dedup();
                fail(
                    Exception::ComponentIntersection,
                    format!(
                        "Class {class} may not be contained in more than one component, but is contained in [{}]",
                        names.join(", ")
                    ),
                )
            }
        }
    }

    /// `GetNamespaceIdentifiersFromComponentOf`: the stereotypes of the type's component.
    ///
    /// # Errors
    /// As [`Association::component_of`].
    pub fn namespace_identifiers_of(
        &self,
        class: &str,
        namespace: &str,
    ) -> Result<Vec<&str>, DiagramError> {
        let component = self.component_of(class, namespace)?;
        Ok(self.diagram.components[component]
            .stereotypes
            .iter()
            .map(String::as_str)
            .collect())
    }

    /// `GetTargetNamespaceIdentifiers`: the stereotypes of every component the type's component
    /// depends on, each once.
    ///
    /// # Errors
    /// As [`Association::component_of`].
    pub fn target_namespace_identifiers(
        &self,
        class: &str,
        namespace: &str,
    ) -> Result<Vec<&str>, DiagramError> {
        let component = self.component_of(class, namespace)?;
        let mut found: Vec<&str> = Vec::new();
        for target in self.diagram.dependencies_of(component) {
            for stereotype in &self.diagram.components[*target].stereotypes {
                if !found.contains(&stereotype.as_str()) {
                    found.push(stereotype);
                }
            }
        }
        Ok(found)
    }
}

/// The namespace of the type named `full_name`: the loaded type's, else the text before the
/// last `.` of its outermost declaring type.
#[must_use]
pub fn namespace_of(architecture: &Architecture<'_>, full_name: &str) -> String {
    if let Some(ty) = architecture.types.get(full_name) {
        return ty.namespace.clone().unwrap_or_default();
    }
    let outer = full_name.split('+').next().unwrap_or(full_name);
    outer
        .rsplit_once('.')
        .map(|(namespace, _)| namespace.to_owned())
        .unwrap_or_default()
}

/// The simple name of the type named `full_name` (`IType.Name`): the loaded type's, else the
/// text after the last `.`.
#[must_use]
pub fn name_of(architecture: &Architecture<'_>, full_name: &str) -> String {
    if let Some(ty) = architecture.types.get(full_name) {
        return ty.name.clone();
    }
    full_name
        .rsplit_once('.')
        .map_or(full_name, |(_, name)| name)
        .to_owned()
}

/// Reads, parses and associates the diagram at `path` (relative to the architecture's base),
/// once per evaluator.
///
/// # Errors
/// [`ElementError::Diagram`] naming the upstream exception.
pub(crate) fn load(e: &Evaluator<'_, '_>, path: &str) -> Result<Rc<Association>, ElementError> {
    if let Some(hit) = e.diagrams.borrow().get(path) {
        return Ok(Rc::clone(hit));
    }
    let error = |error: DiagramError| ElementError::Diagram {
        rule: e.rule().to_owned(),
        message: error.to_string(),
    };
    let file = e.architecture().base.join(path);
    let text = std::fs::read_to_string(&file).map_err(|err| {
        error(DiagramError {
            exception: Exception::PlantUmlParse,
            message: format!("Could not parse diagram from {}: {err}", file.display()),
        })
    })?;
    let association = Rc::new(parse(&text).and_then(Association::new).map_err(error)?);
    e.diagrams
        .borrow_mut()
        .insert(path.to_owned(), Rc::clone(&association));
    Ok(association)
}

/// `AdhereToPlantUmlDiagram(path)` for one object.
///
/// # Errors
/// [`ElementError::Diagram`] when the diagram cannot be read, or the object has a dependency
/// into the diagram and lies in two components or in none; [`ElementError::Inapplicable`] for an
/// object that is not a type.
pub fn adheres(
    e: &Evaluator<'_, '_>,
    object: &Object<'_>,
    path: &str,
) -> Result<bool, ElementError> {
    let association = load(e, path)?;
    // Validation refuses a diagram over any other kind first (the applicability table); an
    // object that is no type is refused here too rather than passed.
    let Object::Type(ty) = object else {
        return Err(ElementError::Inapplicable {
            rule: e.rule().to_owned(),
            key: "adhereToPlantUmlDiagram".to_owned(),
            kind: if matches!(object, Object::Module(_)) {
                "module"
            } else {
                "member"
            }
            .to_owned(),
            why: "a diagram's components hold types".to_owned(),
        });
    };
    let error = |error: DiagramError| ElementError::Diagram {
        rule: e.rule().to_owned(),
        message: error.to_string(),
    };
    let architecture = e.architecture();
    let mut targets: Vec<(&str, bool)> = Vec::new();
    for dependency in &ty.dependencies {
        let target = dependency.target.as_str();
        if targets.iter().all(|(t, _)| *t != target) {
            let inside = association
                .contains(&namespace_of(architecture, target))
                .map_err(error)?;
            targets.push((target, inside));
        }
    }
    if targets.iter().all(|(_, inside)| !inside) {
        return Ok(true);
    }
    let namespace = ty.namespace.as_deref().unwrap_or_default();
    let mut allowed = association
        .namespace_identifiers_of(&ty.name, namespace)
        .map_err(error)?;
    allowed.extend(
        association
            .target_namespace_identifiers(&ty.name, namespace)
            .map_err(error)?,
    );
    for (target, inside) in targets {
        if !inside {
            continue;
        }
        let mut permitted = false;
        for pattern in &allowed {
            if matches(pattern, target).map_err(error)? {
                permitted = true;
                break;
            }
        }
        if !permitted {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIAGRAM: &str = "\u{feff}@startuml\n' the shop\n[Orders] <<Shop.Orders.*>> as O\n[Catalog] <<Shop.Catalog.*>>\n[Billing] <<Shop.Billing.*>> <<Shop.Payments.*>>\nO -right-> [Catalog] : reads\n[Billing] <-- O\n[Catalog] --[#green]-> [Billing]\nO -> Catalog\n@enduml\n";

    fn error(text: &str) -> DiagramError {
        parse(text).err().unwrap_or(DiagramError {
            exception: Exception::PlantUmlParse,
            message: "parsed".into(),
        })
    }

    #[test]
    fn parses_components_aliases_and_every_arrow_form() -> Result<(), DiagramError> {
        let diagram = parse(DIAGRAM)?;
        let names: Vec<&str> = diagram.components.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Orders", "Catalog", "Billing"]);
        assert_eq!(diagram.components[0].alias.as_deref(), Some("O"));
        assert_eq!(
            diagram.components[2].stereotypes,
            ["Shop.Billing.*", "Shop.Payments.*"]
        );
        assert_eq!(diagram.dependencies_of(0), [1, 2], "drawn order, each once");
        assert_eq!(diagram.dependencies_of(1), [2]);
        assert!(diagram.dependencies_of(2).is_empty());
        Ok(())
    }

    #[test]
    fn component_lines_follow_upstreams_pattern() {
        assert_eq!(component_line("[A] <<X>>"), Some(("A", None)));
        assert_eq!(component_line("  [A B]<<X>><<Y>>  "), Some(("A B", None)));
        assert_eq!(component_line("[A] <<X>> as a"), Some(("A", Some("a"))));
        assert_eq!(
            component_line("[A] <<X>> as \"it's a\"  "),
            Some(("A", Some("it's a")))
        );
        assert_eq!(
            component_line("[A] <<X>> as a  "),
            Some(("A", Some("a  "))),
            "an unquoted alias keeps its trailing blanks"
        );
        assert_eq!(component_line("[A]"), Some(("A", None)));
        assert_eq!(
            component_line("[A] as bad[]alias"),
            Some(("A", Some("bad[]alias")))
        );
        for line in [
            "[A] <<X>> junk",
            "no brackets",
            "[] <<X>>",
            "[A[B] <<X>>",
            "[A",
            "[A] <<>>",
            "[A] <<X>",
            "[A] <<X",
            "[A] <<X>> as ",
            "[A] <<X>> as \"",
            "[A] <<X>> as a\" b",
            "[A] <<X>> asa",
        ] {
            assert_eq!(component_line(line), None, "{line:?}");
        }
    }

    #[test]
    fn stereotypes_are_every_bracketed_pattern_on_the_line_once() {
        assert_eq!(stereotypes("[A] <<X>> <<Y>> <<X>>"), ["X", "Y"]);
        assert_eq!(stereotypes("[A <<N>>] <<<Y>>"), ["N", "Y"]);
        assert_eq!(stereotypes("[A] <<>> <<X"), Vec::<String>::new());
        assert!(stereotypes("<<a<b>>").is_empty());
    }

    #[test]
    fn dependency_lines_follow_upstreams_arrows() -> Result<(), DiagramError> {
        let pair = |a: &str, b: &str| (a.to_owned(), b.to_owned());
        assert_eq!(dependency_line("[A] --> [B]")?, [pair("[A]", "[B]")]);
        assert_eq!(dependency_line("B <-down- A")?, [pair("A", "B")]);
        assert_eq!(
            dependency_line("A -[#red]-> B : x --> y")?,
            [pair("A", "B")]
        );
        assert_eq!(
            dependency_line("  A --> B")?,
            [pair("", "")],
            "a leading blank splits into empty parts, as upstream's Split(' ') does"
        );
        assert_eq!(
            dependency_line("A --> B <-- C")?,
            [pair("A", "B"), pair("-->", "A")],
            "each arrow is cut out alone, so the other stays a part"
        );
        assert_eq!(dependency_line("A --> ")?, [pair("A", "")]);
        for line in ["A ..> B", "A <--> B", "A-->B", "@startuml", "A -->"] {
            assert!(dependency_line(line)?.is_empty(), "{line:?}");
        }
        let only_described = dependency_line("[A]: x --> y").err();
        assert_eq!(
            only_described.map(|e| e.exception),
            Some(Exception::ArgumentOutOfRange)
        );
        Ok(())
    }

    #[test]
    fn identical_components_collapse_and_the_first_of_a_name_wins() -> Result<(), DiagramError> {
        let diagram = parse("[A] <<X>> <<Y>> as a\n[A] <<Y>> <<X>> as a\n[B] <<Z>>\n")?;
        assert_eq!(diagram.components.len(), 2);
        let diagram = parse("[A] <<X>> as a\n[A] <<W>> as b\n[B] <<Z>>\nb --> B\nB --> b\n")?;
        assert_eq!(diagram.components.len(), 2);
        assert_eq!(diagram.components[0].stereotypes, ["X"]);
        assert!(
            diagram.dependencies_of(0).is_empty(),
            "a dependency from the losing component is dropped"
        );
        assert_eq!(diagram.dependencies_of(1), [0], "a target is found by name");
        Ok(())
    }

    #[test]
    fn dependencies_resolve_aliases_before_names() -> Result<(), DiagramError> {
        let diagram = parse("[A] <<X>> as B\n[B] <<Y>>\nA --> B\n")?;
        assert_eq!(diagram.dependencies_of(0), [0], "B is A's alias first");
        Ok(())
    }

    #[test]
    fn illegal_diagrams_name_the_exception_and_upstreams_message() {
        let cases = [
            (
                "[A]\n",
                Exception::IllegalDiagram,
                "Components must include at least one stereotype specifying the namespace identifier (<<.*>>), but component 'A' does not",
            ),
            (
                "[A] <<X>>\n[A] --> [Z]\n",
                Exception::IllegalDiagram,
                "There is no Component with name or alias = 'Z'. Components must be specified separately from dependencies.",
            ),
            (
                "[A] <<X>> as bad[]alias\n",
                Exception::IllegalDiagram,
                "Alias 'bad[]alias' should not contain character(s): '[' or ']' or '\"'",
            ),
            (
                "[A] <<X>>\n[[A]] --> A\n",
                Exception::IllegalDiagram,
                "Alias '[A]' should not contain character(s): '[' or ']' or '\"'",
            ),
            (
                "[A] <<X>> as a\n[B] <<Y>> as a\n",
                Exception::Argument,
                "An item with the same key has already been added: two components have the alias 'a'",
            ),
            (
                "!include other.puml\n",
                Exception::IllegalDiagram,
                "`!include other.puml` is not supported; inline the included diagram",
            ),
            (
                "[A] <<X>>\n[A] --> [Z]\n[A]: q --> r\n",
                Exception::ArgumentOutOfRange,
                "the dependency line `[A]: q --> r` has its arrow only in its `: description`",
            ),
        ];
        for (text, exception, message) in cases {
            let got = error(text);
            assert_eq!((got.exception, got.message.as_str()), (exception, message));
        }
        assert_eq!(
            error("[A]\n").to_string(),
            "IllegalDiagramException: Components must include at least one stereotype specifying the namespace identifier (<<.*>>), but component 'A' does not"
        );
        assert!(parse("' [A]\n").is_ok_and(|d| d.components.is_empty()));
    }

    #[test]
    fn exception_names_are_upstreams() {
        let names: Vec<&str> = [
            Exception::IllegalDiagram,
            Exception::ComponentIntersection,
            Exception::PlantUmlParse,
            Exception::InvalidOperation,
            Exception::Argument,
            Exception::ArgumentOutOfRange,
        ]
        .into_iter()
        .map(Exception::name)
        .collect();
        assert_eq!(
            names,
            [
                "IllegalDiagramException",
                "ComponentIntersectionException",
                "PlantUmlParseException",
                "InvalidOperationException",
                "ArgumentException",
                "ArgumentOutOfRangeException",
            ]
        );
    }

    #[test]
    fn association_places_classes_and_lists_their_patterns() -> Result<(), DiagramError> {
        let association = Association::new(parse(DIAGRAM)?)?;
        assert_eq!(association.diagram().components.len(), 3);
        assert!(association.contains("Shop.Orders.Api")?);
        assert!(!association.contains("Other")?);
        assert_eq!(association.component_of("C", "Shop.Payments")?, 2);
        assert_eq!(
            association.namespace_identifiers_of("C", "Shop.Orders")?,
            ["Shop.Orders.*"]
        );
        assert_eq!(
            association.target_namespace_identifiers("C", "Shop.Orders")?,
            ["Shop.Catalog.*", "Shop.Billing.*", "Shop.Payments.*"]
        );
        assert!(
            association
                .target_namespace_identifiers("C", "Shop.Billing")?
                .is_empty()
        );
        let none = association.component_of("Object", "System").err();
        assert_eq!(
            none.map(|e| e.to_string()),
            Some(
                "InvalidOperationException: Class Object is not contained in any component".into()
            )
        );
        Ok(())
    }

    #[test]
    fn association_refuses_intersections_duplicates_and_bad_patterns() -> Result<(), DiagramError> {
        let association = Association::new(parse("[b] <<Foo>>\n[B] <<Bar>>\n[a] <<Baz>>\n")?)?;
        let two = association.component_of("C", "Foo.Bar.Baz").err();
        assert_eq!(
            two.map(|e| e.to_string()),
            Some("ComponentIntersectionException: Class C may not be contained in more than one component, but is contained in [a, b, B]".into())
        );
        let duplicate = Association::new(parse("[A] <<.*.X.*>>\n[B] <<.*.X.*>>\n")?).err();
        assert_eq!(
            duplicate.map(|e| e.to_string()),
            Some("IllegalDiagramException: Stereotype '.*.X.*' should be unique".into())
        );
        let bad = Association::new(parse("[A] <<a(?=b)>>\n")?)?;
        assert_eq!(
            bad.contains("a").err().map(|e| e.exception),
            Some(Exception::Argument)
        );
        Ok(())
    }

    #[test]
    fn a_malformed_diagram_fails_the_rule_even_when_nothing_is_selected() {
        let document = rb_model::GraphDocument::default();
        let architecture = Architecture::new(&document);
        let rule = rb_config::elements::parse_diagrams(&serde_json::json!([
            { "name": "d", "select": { "kind": "type" }, "adhereTo": "no-such.puml" }
        ]))
        .map(|rules| rules[0].as_element_rule());
        let outcome = rule.map_err(|e| e.to_string()).and_then(|rule| {
            crate::elements::evaluate(&architecture, &rule).map_err(|e| e.to_string())
        });
        assert!(
            outcome
                .as_ref()
                .is_err_and(|e| e.contains("PlantUmlParseException: Could not parse diagram from")),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_member_or_a_module_never_adheres_silently() -> Result<(), Box<dyn std::error::Error>> {
        let folder = std::env::temp_dir().join(format!("rb-plantuml-{}", std::process::id()));
        std::fs::create_dir_all(&folder)?;
        std::fs::write(folder.join("d.puml"), DIAGRAM)?;
        let location = rb_model::Location::in_file(rb_model::Language::Dotnet, None);
        let member = rb_model::MemberElement::new("Shop.Orders.O", "m", "method", location);
        let module = rb_model::Module::new("src/a.cs");
        let document = rb_model::GraphDocument::default();
        let mut architecture = Architecture::new(&document);
        architecture.base.clone_from(&folder);
        let evaluator = Evaluator::new(&architecture, "d");
        let kinds: Vec<Option<String>> = [Object::Member(&member), Object::Module(&module)]
            .iter()
            .map(|object| match adheres(&evaluator, object, "d.puml") {
                Err(ElementError::Inapplicable { kind, key, .. })
                    if key == "adhereToPlantUmlDiagram" =>
                {
                    Some(kind)
                }
                _ => None,
            })
            .collect();
        std::fs::remove_dir_all(&folder)?;
        assert_eq!(
            kinds,
            [Some("member".to_owned()), Some("module".to_owned())]
        );
        Ok(())
    }

    #[test]
    fn culture_order_puts_lower_case_first_on_a_tie() {
        let mut names = vec!["b", "B", "a", "A2", "a1"];
        names.sort_by_cached_key(|n| culture_key(n));
        assert_eq!(names, ["a", "a1", "A2", "b", "B"]);
    }
}
