//! Slice rules: types grouped by a namespace pattern, and the two conditions on the groups.
//!
//! - Source: [design § Slice rules](../../../docs/artifacts/design.md#slice-rules)
//! - Coverage: [`ArchUnitNET` § Slices](../../../docs/artifacts/archunitnet-0.13.4-coverage.md#slices-sliceruledefinition)
//! - Plan: [Wave 2, Step 6](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#26-step-6-slice-and-diagram-rules-2c)
//! - Decision: [ADR-0034](../../../docs/adr/0034-slices-group-types-or-modules-and-segments.md)
//! - Requirement: [FR-RULE-04](../../../docs/prd.md#fr-rule-04)
//! - Specification: `ArchUnitNET` 0.13.4 `Fluent/Slices/SliceRuleInitializer.cs` (`Matching`,
//!   `MatchingWithPackages`, `AssignFunc`), `SlicesShould.cs`, `Domain/SliceIdentifier.cs`
//!
//! `matching` takes `(*)` or one `(**)`, and either names a slice by everything between the
//! prefix and the postfix: `ArchUnitNET` 0.13.4 rewrites `Ns.(*)` to `Ns.(**).` and keeps the
//! count of `(*)` only for diagram generation, so its own `MatchingTest` finds seven slices for
//! both `TestAssembly.Slices.(*)` and `(**)`. To slice by the first segment only, end the pattern
//! with a separator after the asterisks (`Ns.(**)..`, three slices in the same test; in a path
//! pattern, `src/features/(**)//`). A prefix
//! starting with `.` is looked for anywhere in the namespace. A type whose namespace does not
//! match belongs to no slice; one that matches but cannot be cut is [`ElementError::Slice`]. The separator is `/` when the pattern has one (a TypeScript path),
//! `.` otherwise (a .NET namespace, a Python module).
//!
//! `segments: n` (a Rulebearing addition) keeps the first `n` segments of each name, so
//! `app.(*)` with `segments: 1` puts `app.sub` and `app.sub.deep` in one slice `sub`: the siblings
//! of import-linter's `acyclic_siblings`.
//!
//! A slice holds .NET types, joined by their dependencies as in `ArchUnitNET`, and TypeScript,
//! JavaScript and Python modules, joined by their imports (the capability table's slice unit):
//! a TypeScript module by its path, a Python module by its dotted name.
//!
//! `notDependOnEachOther` fails each slice with a dependency on another, listing the edges;
//! `beFreeOfCycles` fails each cycle of the slice graph (Tarjan's strongly connected components of
//! more than one slice).

use std::collections::{BTreeMap, BTreeSet};

use rb_config::capability::{SliceUnit, slice_unit};
use rb_config::elements::{SliceCondition, SliceRule};
use rb_model::Language;

use crate::elements::{Architecture, ElementError};

/// How a namespace is sliced.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Assignment {
    /// The pattern as `Parse` rewrites it: a single-asterisk pattern becomes `prefix(**).`.
    pattern: String,
    prefix: String,
    postfix: String,
    separator: char,
}

/// Parses `matching` as `SliceRuleInitializer.Parse` does.
///
/// # Errors
/// The message `ArchUnitNET` gives for a pattern without `(*)` or `(**)`, with both, or with
/// `(**)` twice.
fn parse(pattern: &str) -> Result<Assignment, String> {
    let single = pattern.contains("(*)");
    let double = pattern.contains("(**)");
    if !single && !double {
        return Err("Patterns for Slices have to contain (*) or (**).".into());
    }
    if single && double {
        return Err("Patterns for Slices can't contain both (*) and (**).".into());
    }
    if pattern.matches("(**").count() > 1 {
        return Err("Patterns for Slices can contain (**) only once.".into());
    }
    let separator = if pattern.contains('/') { '/' } else { '.' };
    let index = pattern.find("(*").unwrap_or(0);
    // `ArchUnitNET` keeps the number of `(*)` only for diagram generation: a single-asterisk
    // pattern names its slice by the whole remainder, exactly as `(**)` does.
    let effective = if double {
        pattern.to_owned()
    } else {
        format!("{}(**){separator}", &pattern[..index])
    };
    let prefix = effective[..index].to_owned();
    let postfix = effective
        .find("*)")
        .map(|i| effective[i + 2..].to_owned())
        .unwrap_or_default();
    Ok(Assignment {
        pattern: effective,
        prefix,
        postfix,
        separator,
    })
}

impl Assignment {
    /// The slice a namespace belongs to, as `AssignFunc` decides: `Ok(None)` when it is ignored.
    ///
    /// # Errors
    /// The namespace passes the prefix and postfix tests but the postfix does not occur in what
    /// follows the prefix (`ArchUnitNET`'s "is not clearly assignable" `ArgumentException`).
    fn slice(&self, namespace: &str) -> Result<Option<String>, ()> {
        let sep = self.separator;
        let mut prefix = self.prefix.as_str();
        if let Some(rest) = prefix.strip_prefix(sep) {
            prefix = rest;
            if !namespace.contains(prefix) {
                return Ok(None);
            }
        } else if !namespace.starts_with(prefix) {
            return Ok(None);
        }
        let after = &namespace[namespace.find(prefix).unwrap_or(0) + prefix.len()..];
        let mut postfix = self.postfix.as_str();
        if let Some(rest) = postfix.strip_suffix(sep) {
            postfix = rest;
            if !after.contains(postfix) {
                return Ok(None);
            }
        } else if !namespace.ends_with(postfix) {
            return Ok(None);
        }
        let mut slice = if prefix.is_empty() {
            namespace
        } else {
            after.trim_start_matches(sep)
        };
        if !slice.contains(postfix) {
            return Err(());
        }
        if let Some(end) = slice.find(postfix).filter(|_| !postfix.is_empty()) {
            slice = &slice[..end];
        }
        Ok(Some(slice.to_owned()))
    }
}

/// One failing slice or cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceFailure {
    /// The condition that failed.
    pub condition: SliceCondition,
    /// The slice, or every slice of the cycle, sorted.
    pub slices: Vec<String>,
    /// The type edges that make it fail, `(from type, to type)`, sorted.
    pub edges: Vec<(String, String)>,
}

/// What a slice rule found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceOutcome {
    /// The rule.
    pub rule: String,
    /// Every slice, with its types, sorted.
    pub slices: BTreeMap<String, BTreeSet<String>>,
    /// The failures.
    pub failures: Vec<SliceFailure>,
    /// No type fell in any slice and the rule does not allow it.
    pub vacuous: bool,
}

/// Tarjan's strongly connected components over slice indexes.
fn components(graph: &BTreeMap<usize, BTreeSet<usize>>, count: usize) -> Vec<Vec<usize>> {
    struct State<'g> {
        graph: &'g BTreeMap<usize, BTreeSet<usize>>,
        index: usize,
        indexes: Vec<Option<usize>>,
        low: Vec<usize>,
        stack: Vec<usize>,
        on_stack: Vec<bool>,
        found: Vec<Vec<usize>>,
    }
    fn visit(s: &mut State<'_>, v: usize) {
        s.indexes[v] = Some(s.index);
        s.low[v] = s.index;
        s.index += 1;
        s.stack.push(v);
        s.on_stack[v] = true;
        let next: Vec<usize> = s.graph.get(&v).into_iter().flatten().copied().collect();
        for w in next {
            match s.indexes[w] {
                None => {
                    visit(s, w);
                    s.low[v] = s.low[v].min(s.low[w]);
                }
                Some(i) if s.on_stack[w] => s.low[v] = s.low[v].min(i),
                Some(_) => {}
            }
        }
        if Some(s.low[v]) == s.indexes[v] {
            let mut component = Vec::new();
            while let Some(w) = s.stack.pop() {
                s.on_stack[w] = false;
                component.push(w);
                if w == v {
                    break;
                }
            }
            component.sort_unstable();
            s.found.push(component);
        }
    }
    let mut state = State {
        graph,
        index: 0,
        indexes: vec![None; count],
        low: vec![0; count],
        stack: Vec::new(),
        on_stack: vec![false; count],
        found: Vec::new(),
    };
    for v in 0..count {
        if state.indexes[v].is_none() {
            visit(&mut state, v);
        }
    }
    state.found
}

/// One thing a slice can hold: its identity, the text the pattern is matched against, and the
/// identities it depends on.
struct Member<'a> {
    key: &'a str,
    text: &'a str,
    targets: Vec<&'a str>,
}

/// What slices group, per language ([`slice_unit`]): the analysed types by namespace with their
/// dependencies, or the analysed modules with their imports, by path when the pattern's separator
/// is `/` and by dotted name otherwise.
fn members<'a>(architecture: &Architecture<'a>, separator: char) -> Vec<Member<'a>> {
    let by_types =
        |language: Option<Language>| language.is_some_and(|l| slice_unit(l) == SliceUnit::Types);
    let mut members: Vec<Member<'a>> = architecture
        .types
        .values()
        .filter(|t| t.referenced != Some(true) && by_types(Some(t.location.language)))
        .map(|t| Member {
            key: t.full_name.as_str(),
            text: t.namespace.as_deref().unwrap_or_default(),
            targets: t.dependencies.iter().map(|d| d.target.as_str()).collect(),
        })
        .collect();
    for module in architecture.modules {
        let local = module.core_module != Some(true)
            && module.could_not_resolve != Some(true)
            && module.followable != Some(false);
        if !local || by_types(module.language) || module.language.is_none() {
            continue;
        }
        let text = if separator == '/' {
            Some(module.source.as_str())
        } else {
            module
                .namespaces
                .as_ref()
                .and_then(|n| n.first())
                .map(String::as_str)
        };
        if let Some(text) = text {
            members.push(Member {
                key: module.source.as_str(),
                text,
                targets: module
                    .dependencies
                    .iter()
                    .map(|d| d.resolved.as_str())
                    .collect(),
            });
        }
    }
    members
}

/// Evaluates one slice rule.
///
/// # Errors
/// [`ElementError::Pattern`] for a `matching` or `where` pattern `ArchUnitNET` would refuse.
#[expect(
    clippy::too_many_lines,
    reason = "assign, connect, then test each condition: one pass kept together"
)]
pub fn evaluate(
    architecture: &Architecture<'_>,
    rule: &SliceRule,
) -> Result<SliceOutcome, ElementError> {
    let error = |_| ElementError::Pattern {
        rule: rule.name.clone(),
        pattern: rule.matching.clone(),
    };
    let assignment = parse(&rule.matching).map_err(error)?;
    let mut slices: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut slice_of: BTreeMap<&str, String> = BTreeMap::new();
    let members = members(architecture, assignment.separator);
    for member in &members {
        let assigned = assignment
            .slice(member.text)
            .map_err(|()| ElementError::Slice {
                rule: rule.name.clone(),
                object: member.key.to_owned(),
                pattern: assignment.pattern.clone(),
            })?;
        let Some(mut slice) = assigned else {
            continue;
        };
        if let Some(n) = rule.segments {
            let separator = assignment.separator.to_string();
            slice = slice
                .split(assignment.separator)
                .take(n)
                .collect::<Vec<_>>()
                .join(&separator);
        }
        if rule.ignore.contains(&slice) {
            continue;
        }
        if let Some(where_) = &rule.where_ {
            if crate::patterns::get(where_).is_none() {
                return Err(ElementError::Pattern {
                    rule: rule.name.clone(),
                    pattern: where_.clone(),
                });
            }
            if !crate::patterns::test(where_, &slice) {
                continue;
            }
        }
        slice_of.insert(member.key, slice.clone());
        slices
            .entry(slice)
            .or_default()
            .insert(member.key.to_owned());
    }
    let names: Vec<&String> = slices.keys().collect();
    let index: BTreeMap<&str, usize> = names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();
    // Slice edges with the member edges behind them.
    let mut edges: BTreeMap<(usize, usize), BTreeSet<(String, String)>> = BTreeMap::new();
    for member in &members {
        let Some(from_slice) = slice_of.get(member.key) else {
            continue;
        };
        for target in &member.targets {
            if let Some(to_slice) = slice_of.get(target)
                && to_slice != from_slice
            {
                edges
                    .entry((index[from_slice.as_str()], index[to_slice.as_str()]))
                    .or_default()
                    .insert((member.key.to_owned(), (*target).to_owned()));
            }
        }
    }
    let mut failures = Vec::new();
    for condition in &rule.should {
        match condition {
            SliceCondition::NotDependOnEachOther => {
                for (i, name) in names.iter().enumerate() {
                    let behind: BTreeSet<(String, String)> = edges
                        .iter()
                        .filter(|((from, _), _)| *from == i)
                        .flat_map(|(_, e)| e.iter().cloned())
                        .collect();
                    if !behind.is_empty() {
                        failures.push(SliceFailure {
                            condition: *condition,
                            slices: vec![(*name).clone()],
                            edges: behind.into_iter().collect(),
                        });
                    }
                }
            }
            SliceCondition::BeFreeOfCycles => {
                let mut graph: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
                for (from, to) in edges.keys() {
                    graph.entry(*from).or_default().insert(*to);
                }
                for component in components(&graph, names.len()) {
                    if component.len() < 2 {
                        continue;
                    }
                    let members: BTreeSet<usize> = component.iter().copied().collect();
                    let behind: BTreeSet<(String, String)> = edges
                        .iter()
                        .filter(|((from, to), _)| members.contains(from) && members.contains(to))
                        .flat_map(|(_, e)| e.iter().cloned())
                        .collect();
                    failures.push(SliceFailure {
                        condition: *condition,
                        slices: component.iter().map(|i| names[*i].clone()).collect(),
                        edges: behind.into_iter().collect(),
                    });
                }
            }
        }
    }
    failures.sort_by(|a, b| (&a.slices, a.condition as u8).cmp(&(&b.slices, b.condition as u8)));
    Ok(SliceOutcome {
        rule: rule.name.clone(),
        vacuous: slices.is_empty() && !rule.allow_empty,
        slices,
        failures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_and_double_asterisks_name_slices_as_archunitnet_does() -> Result<(), String> {
        let slice =
            |a: &Assignment, ns: &str| a.slice(ns).map_err(|()| format!("{ns}: unassignable"));
        let single = parse("TestAssembly.Slices.(*)")?;
        assert_eq!(single.pattern, "TestAssembly.Slices.(**).");
        assert_eq!(
            slice(&single, "TestAssembly.Slices.Slice1")?,
            Some("Slice1".into())
        );
        assert_eq!(
            slice(&single, "TestAssembly.Slices.Slice3.Group1")?,
            Some("Slice3.Group1".into()),
            "(*) keeps the whole remainder, as (**) does"
        );
        assert_eq!(slice(&single, "TestAssembly.Slices")?, None);
        assert_eq!(slice(&single, "Other.Slices.Slice1")?, None);
        let double = parse("TestAssembly.Slices.(**)")?;
        assert_eq!(
            slice(&double, "TestAssembly.Slices.Slice3.Group1")?,
            Some("Slice3.Group1".into())
        );
        let first = parse("TestAssembly.Slices.(**)..")?;
        assert_eq!(
            slice(&first, "TestAssembly.Slices.Slice3.Group1")?,
            Some("Slice3".into())
        );
        assert_eq!(
            slice(&first, "TestAssembly.Slices.Slice3")?,
            None,
            "no segment after"
        );
        let middle = parse("App.(**).Service")?;
        assert_eq!(slice(&middle, "App.Orders.Service")?, Some("Orders".into()));
        assert_eq!(slice(&middle, "App.Orders.Other")?, None);
        let anywhere = parse(".Slices.(**)")?;
        assert_eq!(slice(&anywhere, "Deep.Down.Slices.X")?, Some("X".into()));
        assert_eq!(slice(&anywhere, "Deep.Down.X")?, None);
        let paths = parse("src/features/(**)//")?;
        assert_eq!(paths.separator, '/');
        assert_eq!(
            slice(&paths, "src/features/cart/index.ts")?,
            Some("cart".into())
        );
        let everything = parse("(**)")?;
        assert_eq!(slice(&everything, "A.B")?, Some("A.B".into()));
        // `ABC` starts with `AB` and ends with `BC`, but `C`, what follows the prefix, holds no
        // `BC` to cut at.
        let unclear = parse("AB(**)BC")?;
        assert_eq!(unclear.slice("ABC"), Err(()));
        for bad in ["App.X", "App.(*).(**)", "A.(**).(**)"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        Ok(())
    }

    #[test]
    fn tarjan_finds_the_cycles() {
        let graph: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::from([
            (0, BTreeSet::from([1])),
            (1, BTreeSet::from([0, 2])),
            (2, BTreeSet::from([3])),
            (3, BTreeSet::from([2])),
        ]);
        let mut found: Vec<Vec<usize>> = components(&graph, 5)
            .into_iter()
            .filter(|c| c.len() > 1)
            .collect();
        found.sort();
        assert_eq!(found, [vec![0, 1], vec![2, 3]]);
    }
}
