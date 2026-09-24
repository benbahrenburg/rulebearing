//! `PlantUML` component diagrams: parsing the subset `ArchUnitNET` reads, and whether a type
//! adheres.
//!
//! - Source: [design § Diagram rules](../../../docs/artifacts/design.md#diagram-rules)
//! - Coverage: [`ArchUnitNET` § `PlantUML`](../../../docs/artifacts/archunitnet-0.13.4-coverage.md#plantuml)
//! - Plan: [Wave 2, Step 6](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#26-step-6-slice-and-diagram-rules-2c)
//! - Requirement: [FR-RULE-05](../../../docs/prd.md#fr-rule-05)
//! - Specification: `ArchUnitNET` 0.13.4 `Domain/PlantUml/Import` (`PlantUmlParser`,
//!   `PlantUmlPatterns`, `ClassDiagramAssociation`) and `TypeConditionsDefinition.AdhereToPlantUmlDiagram`
//!
//! | Diagram line | Read as |
//! | --- | --- |
//! | `' ...` | a comment |
//! | `[Name] <<Pattern>> <<Other>> as Alias` | a component; each stereotype is a regular expression over a type's namespace, and there must be at least one |
//! | `[A] --> [B]`, `A -right-> B`, `A --[#green]-> B`, `B <-- A`, `: label` | a dependency from A to B |
//!
//! A type adheres when every dependency it has on a type inside some component is on its own
//! component or on a component its component points to. A type in two components is
//! `ComponentIntersectionException`; a component without a stereotype, a repeated stereotype, or
//! a dependency naming no component is `IllegalDiagramException`; `!include` is not read and is
//! reported. Both surface as [`ElementError::Diagram`], exit 3.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::elements::{ElementError, Evaluator, Object};

/// One component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    /// The name between the brackets.
    pub name: String,
    /// The stereotypes, each a namespace pattern.
    pub stereotypes: Vec<String>,
    /// The alias, when given.
    pub alias: Option<String>,
}

/// A parsed diagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagram {
    /// Every component, in the order they appear.
    pub components: Vec<Component>,
    /// For each component index, the indexes of the components it depends on.
    pub dependencies: BTreeMap<usize, BTreeSet<usize>>,
}

fn component_line(line: &str) -> Option<Component> {
    let rest = line.trim_start().strip_prefix('[')?;
    let (name, mut rest) = rest.split_once(']')?;
    if name.is_empty() || name.contains('[') {
        return None;
    }
    let mut stereotypes = Vec::new();
    rest = rest.trim_start();
    while let Some(after) = rest.strip_prefix("<<") {
        let (stereotype, tail) = after.split_once(">>")?;
        if stereotype.is_empty() || stereotype.contains('<') || stereotype.contains('>') {
            return None;
        }
        stereotypes.push(stereotype.to_owned());
        rest = tail.trim_start();
    }
    let alias = if let Some(after) = rest.strip_prefix("as ") {
        let alias = after.trim().trim_matches('"');
        (!alias.is_empty() && !alias.contains('"')).then(|| alias.to_owned())
    } else if rest.trim().is_empty() {
        None
    } else {
        return None;
    };
    Some(Component {
        name: name.to_owned(),
        stereotypes,
        alias,
    })
}

/// Splits a dependency line into (origin, target) when it has an arrow.
fn dependency_line(line: &str) -> Option<(String, String)> {
    let line = line.split(':').next().unwrap_or(line);
    let right = regex::Regex::new(r"\s-+(left|right|up|down|\[[^\]]+\])?-*>\s").ok()?;
    let left = regex::Regex::new(r"\s<-*(left|right|up|down|\[[^\]]+\])?-+\s").ok()?;
    let parts = |re: &regex::Regex| -> Vec<String> {
        re.replace(line, " ")
            .split(' ')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .take(2)
            .map(str::to_owned)
            .collect()
    };
    if right.is_match(line) {
        let p = parts(&right);
        return (p.len() == 2).then(|| (p[0].clone(), p[1].clone()));
    }
    if left.is_match(line) {
        let p = parts(&left);
        return (p.len() == 2).then(|| (p[1].clone(), p[0].clone()));
    }
    None
}

/// Parses a diagram's text.
///
/// # Errors
/// The `IllegalDiagramException` message for a component without a stereotype, a repeated
/// stereotype, a dependency on no component, or an `!include`.
pub fn parse(text: &str) -> Result<Diagram, String> {
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('\''))
        .collect();
    let mut components: Vec<Component> = Vec::new();
    for line in &lines {
        if line.trim_start().starts_with("!include") {
            return Err(format!(
                "IllegalDiagramException: `{}` is not supported; inline the included diagram",
                line.trim()
            ));
        }
        if let Some(component) = component_line(line) {
            if component.stereotypes.is_empty() {
                return Err(format!(
                    "IllegalDiagramException: Components must include at least one stereotype specifying the namespace identifier (<<.*>>), but component '{}' does not",
                    component.name
                ));
            }
            if !components.iter().any(|c| c.name == component.name) {
                components.push(component);
            }
        }
    }
    let mut seen = BTreeSet::new();
    for stereotype in components.iter().flat_map(|c| &c.stereotypes) {
        if !seen.insert(stereotype) {
            return Err(format!(
                "IllegalDiagramException: Stereotype '{stereotype}' should be unique"
            ));
        }
    }
    let find = |text: &str| -> Result<usize, String> {
        let name = text.trim().trim_start_matches('[').trim_end_matches(']');
        components
            .iter()
            .position(|c| c.name == name || c.alias.as_deref() == Some(name))
            .ok_or_else(|| {
                format!(
                    "IllegalDiagramException: there is no component with name or alias = '{name}'"
                )
            })
    };
    let mut dependencies: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for line in &lines {
        if component_line(line).is_some() {
            continue;
        }
        if let Some((origin, target)) = dependency_line(line) {
            let (o, t) = (find(&origin)?, find(&target)?);
            dependencies.entry(o).or_default().insert(t);
        }
    }
    Ok(Diagram {
        components,
        dependencies,
    })
}

/// The namespace of a type named `full_name`: the loaded type's, else the text before its last `.`.
fn namespace_of(e: &Evaluator<'_, '_>, full_name: &str) -> String {
    if let Some(ty) = e.architecture().types.get(full_name) {
        return ty.namespace.clone().unwrap_or_default();
    }
    let outer = full_name.split('+').next().unwrap_or(full_name);
    outer
        .rsplit_once('.')
        .map(|(ns, _)| ns.to_owned())
        .unwrap_or_default()
}

impl Diagram {
    /// The components whose stereotypes match a namespace.
    fn components_of(&self, namespace: &str) -> Vec<usize> {
        self.components
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.stereotypes
                    .iter()
                    .any(|s| crate::patterns::test(s, namespace))
            })
            .map(|(i, _)| i)
            .collect()
    }
}

fn load(e: &Evaluator<'_, '_>, path: &str) -> Result<Rc<Diagram>, ElementError> {
    if let Some(hit) = e.diagrams.borrow().get(path) {
        return Ok(Rc::clone(hit));
    }
    let error = |message: String| ElementError::Diagram {
        rule: e.rule().to_owned(),
        message,
    };
    let file = e.architecture().base.join(path);
    let text = std::fs::read_to_string(&file).map_err(|err| {
        error(format!(
            "PlantUmlParseException: Could not parse diagram from {}: {err}",
            file.display()
        ))
    })?;
    let diagram = Rc::new(parse(&text).map_err(error)?);
    e.diagrams
        .borrow_mut()
        .insert(path.to_owned(), Rc::clone(&diagram));
    Ok(diagram)
}

/// `AdhereToPlantUmlDiagram(path)` for one object.
///
/// # Errors
/// [`ElementError::Diagram`] when the diagram cannot be read, or the object lies in two
/// components or, having dependencies into the diagram, in none.
pub fn adheres(
    e: &Evaluator<'_, '_>,
    object: &Object<'_>,
    path: &str,
) -> Result<bool, ElementError> {
    let diagram = load(e, path)?;
    let Object::Type(ty) = object else {
        return Ok(true);
    };
    let targets: BTreeSet<&str> = ty.dependencies.iter().map(|d| d.target.as_str()).collect();
    let inside: Vec<&str> = targets
        .iter()
        .copied()
        .filter(|t| !diagram.components_of(&namespace_of(e, t)).is_empty())
        .collect();
    if inside.is_empty() {
        return Ok(true);
    }
    let error = |message: String| ElementError::Diagram {
        rule: e.rule().to_owned(),
        message,
    };
    let own = diagram.components_of(ty.namespace.as_deref().unwrap_or_default());
    let component = match own.as_slice() {
        [one] => *one,
        [] => {
            return Err(error(format!(
                "Class {} is not contained in any component",
                ty.name
            )));
        }
        many => {
            let mut names: Vec<&str> = many
                .iter()
                .map(|i| diagram.components[*i].name.as_str())
                .collect();
            names.sort_unstable();
            return Err(error(format!(
                "ComponentIntersectionException: Class {} may not be contained in more than one component, but is contained in [{}]",
                ty.name,
                names.join(", ")
            )));
        }
    };
    let mut allowed: Vec<&str> = diagram.components[component]
        .stereotypes
        .iter()
        .map(String::as_str)
        .collect();
    for target in diagram.dependencies.get(&component).into_iter().flatten() {
        allowed.extend(
            diagram.components[*target]
                .stereotypes
                .iter()
                .map(String::as_str),
        );
    }
    Ok(inside
        .iter()
        .all(|t| allowed.iter().any(|p| crate::patterns::test(p, t))))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIAGRAM: &str = "@startuml\n' the shop\n[Orders] <<Shop.Orders.*>> as O\n[Catalog] <<Shop.Catalog.*>>\n[Billing] <<Shop.Billing.*>> <<Shop.Payments.*>>\nO -right-> [Catalog] : reads\n[Billing] <-- O\n[Catalog] --[#green]-> [Billing]\n@enduml\n";

    #[test]
    fn parses_components_aliases_and_every_arrow_form() -> Result<(), String> {
        let diagram = parse(DIAGRAM)?;
        let names: Vec<&str> = diagram.components.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Orders", "Catalog", "Billing"]);
        assert_eq!(diagram.components[0].alias.as_deref(), Some("O"));
        assert_eq!(diagram.components[2].stereotypes.len(), 2);
        assert_eq!(diagram.dependencies.get(&0), Some(&BTreeSet::from([1, 2])));
        assert_eq!(diagram.dependencies.get(&1), Some(&BTreeSet::from([2])));
        assert_eq!(diagram.components_of("Shop.Orders.Api"), [0]);
        assert!(diagram.components_of("Other").is_empty());
        Ok(())
    }

    #[test]
    fn illegal_diagrams_are_named() {
        for (text, needle) in [
            ("[A]\n", "at least one stereotype"),
            ("[A] <<X.*>>\n[B] <<X.*>>\n", "should be unique"),
            (
                "[A] <<X.*>>\n[A] --> [Z]\n",
                "no component with name or alias = 'Z'",
            ),
            ("!include other.puml\n", "not supported"),
        ] {
            let message = parse(text).err().unwrap_or_default();
            assert!(message.contains(needle), "{text:?}: {message}");
        }
        assert_eq!(component_line("[A] <<X>> junk"), None);
        assert_eq!(component_line("no brackets"), None);
        assert_eq!(dependency_line("A B"), None);
    }
}
