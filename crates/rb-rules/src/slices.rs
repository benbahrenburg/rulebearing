//! Slice rules: types or modules grouped by a pattern, and the two conditions on the groups.
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
//! A slice rule's `graph` ([ADR-0038](../../../docs/adr/0038-a-rule-narrows-the-graph-it-sees.md))
//! takes module imports out before the slices are joined; an `ignore` entry that matches no
//! import is reported as the rule's vacuous side `graph.ignore[<n>]`, unless `allowEmpty`.
//!
//! `notDependOnEachOther` fails each slice with a dependency on another, listing the edges;
//! `beFreeOfCycles` fails each cycle of the slice graph (Tarjan's strongly connected components of
//! more than one slice).

use std::collections::{BTreeMap, BTreeSet};

use rb_config::capability::{SliceUnit, slice_unit};
use rb_config::elements::{SliceCondition, SliceRule};
use rb_model::Language;

use crate::elements::{Architecture, ElementError};
use crate::graph::view::View;

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
    /// The `graph.ignore` entries, by index, that match no module import, when the rule does
    /// not allow it.
    pub unmatched_ignores: Vec<usize>,
}

/// Tarjan's strongly connected components over slice indexes, iterative: each frame of the
/// work stack holds its own edge iterator, as `IndexedGraph::components` does, so a cycle through
/// every module of a large repository cannot exhaust the call stack. Components come out in the
/// order the recursive algorithm completes them, each sorted.
fn components(graph: &BTreeMap<usize, BTreeSet<usize>>, count: usize) -> Vec<Vec<usize>> {
    static NONE: BTreeSet<usize> = BTreeSet::new();
    let edges_of = |v: usize| graph.get(&v).unwrap_or(&NONE).iter();
    let mut index: Vec<Option<usize>> = vec![None; count];
    let mut low = vec![0; count];
    let mut on_stack = vec![false; count];
    let mut stack = Vec::new();
    let mut found = Vec::new();
    let mut next = 0;
    for root in 0..count {
        if index[root].is_some() {
            continue;
        }
        index[root] = Some(next);
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        let mut work = vec![(root, edges_of(root))];
        while let Some((v, edges)) = work.last_mut() {
            let v = *v;
            if let Some(&w) = edges.next() {
                // An edge to an index outside the slices is no edge.
                match index.get(w).copied() {
                    Some(None) => {
                        index[w] = Some(next);
                        low[w] = next;
                        next += 1;
                        stack.push(w);
                        on_stack[w] = true;
                        work.push((w, edges_of(w)));
                    }
                    Some(Some(at)) if on_stack[w] => low[v] = low[v].min(at),
                    _ => {}
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low[parent] = low[parent].min(low[v]);
            }
            if Some(low[v]) == index[v] {
                let mut component = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                component.sort_unstable();
                found.push(component);
            }
        }
    }
    found
}

/// The slice edges with the member edges behind them, by `(from slice, to slice)` index.
type SliceEdges = BTreeMap<(usize, usize), BTreeSet<(String, String)>>;

/// Each condition's failures over the slice graph: for `notDependOnEachOther` one per slice with
/// an outgoing edge (its edges read with one range of the ordered map), for `beFreeOfCycles` one
/// per strongly connected component of two or more slices (the edges grouped by component in
/// one pass). Sorted by slices, then condition.
fn failures(names: &[&String], edges: &SliceEdges, should: &[SliceCondition]) -> Vec<SliceFailure> {
    let mut failures = Vec::new();
    for condition in should {
        match condition {
            SliceCondition::NotDependOnEachOther => {
                for (i, name) in names.iter().enumerate() {
                    let behind: BTreeSet<(String, String)> = edges
                        .range((i, 0)..=(i, usize::MAX))
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
                let cycles: Vec<Vec<usize>> = components(&graph, names.len())
                    .into_iter()
                    .filter(|c| c.len() >= 2)
                    .collect();
                let mut group = vec![None; names.len()];
                for (at, component) in cycles.iter().enumerate() {
                    for &slice in component {
                        group[slice] = Some(at);
                    }
                }
                let mut behind: Vec<BTreeSet<(String, String)>> =
                    vec![BTreeSet::new(); cycles.len()];
                for ((from, to), e) in edges {
                    let (Some(Some(a)), Some(Some(b))) = (group.get(*from), group.get(*to)) else {
                        continue;
                    };
                    if a == b {
                        behind[*a].extend(e.iter().cloned());
                    }
                }
                for (component, behind) in cycles.iter().zip(behind) {
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
    failures
}

/// One thing a slice can hold: its identity, the text the pattern is matched against, the
/// identities it depends on, and whether it is a .NET type.
struct Member<'a> {
    key: &'a str,
    text: &'a str,
    targets: Vec<&'a str>,
    is_type: bool,
}

/// What slices group, per language ([`slice_unit`]): the analysed types by namespace with their
/// dependencies, or the analysed modules with their imports, by path when the pattern's separator
/// is `/` and by dotted name otherwise.
fn members<'a>(
    architecture: &Architecture<'a>,
    separator: char,
    view: Option<&View>,
) -> Vec<Member<'a>> {
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
            is_type: true,
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
                    .filter(|d| {
                        view.is_none_or(|v| {
                            !v.removes(
                                &module.source,
                                &d.resolved,
                                d.dependency_types.iter().map(|t| t.as_str()),
                            )
                        })
                    })
                    .map(|d| d.resolved.as_str())
                    .collect(),
                is_type: false,
            });
        }
    }
    members
}

/// Evaluates one slice rule.
///
/// # Errors
/// [`ElementError::Pattern`] for a `matching` or `where` pattern `ArchUnitNET` would refuse.
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
    let view = rule.graph.as_ref().map(View::new);
    let members = members(architecture, assignment.separator, view.as_ref());
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
        if member.is_type && view.is_some() {
            return Err(ElementError::SliceGraph {
                rule: rule.name.clone(),
                object: member.key.to_owned(),
            });
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
    let mut edges: SliceEdges = BTreeMap::new();
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
    let failures = failures(&names, &edges, &rule.should);
    let unmatched_ignores = match &view {
        Some(view) if !rule.allow_empty => {
            view.unmatched(architecture.modules.iter().flat_map(|m| {
                m.dependencies
                    .iter()
                    .map(|d| (m.source.as_str(), d.resolved.as_str()))
            }))
        }
        _ => Vec::new(),
    };
    Ok(SliceOutcome {
        rule: rule.name.clone(),
        unmatched_ignores,
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

    /// The recursive Tarjan the iterative one replaced: the oracle for its output order.
    fn components_recursive(
        graph: &BTreeMap<usize, BTreeSet<usize>>,
        count: usize,
    ) -> Vec<Vec<usize>> {
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

    /// The scans the grouped passes replaced: every edge per slice, every edge per cycle.
    fn failures_by_scan(
        names: &[&String],
        edges: &SliceEdges,
        should: &[SliceCondition],
    ) -> Vec<SliceFailure> {
        let mut failures = Vec::new();
        for condition in should {
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
                    for component in components_recursive(&graph, names.len()) {
                        if component.len() < 2 {
                            continue;
                        }
                        let members: BTreeSet<usize> = component.iter().copied().collect();
                        let behind: BTreeSet<(String, String)> = edges
                            .iter()
                            .filter(|((from, to), _)| {
                                members.contains(from) && members.contains(to)
                            })
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
        failures
            .sort_by(|a, b| (&a.slices, a.condition as u8).cmp(&(&b.slices, b.condition as u8)));
        failures
    }

    proptest::proptest! {
        #[test]
        fn iterative_tarjan_and_grouped_edges_agree_with_the_recursive_scans(
            count in 1usize..9,
            raw in proptest::collection::vec((0usize..9, 0usize..9, 0u8..3), 0..30),
        ) {
            let names: Vec<String> = (0..count).map(|i| format!("s{i}")).collect();
            let refs: Vec<&String> = names.iter().collect();
            let mut edges: SliceEdges = BTreeMap::new();
            let mut graph: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
            for (from, to, n) in raw {
                let (from, to) = (from % count, to % count);
                graph.entry(from).or_default().insert(to);
                if from != to {
                    edges
                        .entry((from, to))
                        .or_default()
                        .insert((format!("m{from}.{n}"), format!("m{to}.{n}")));
                }
            }
            proptest::prop_assert_eq!(components(&graph, count), components_recursive(&graph, count));
            let both = [SliceCondition::NotDependOnEachOther, SliceCondition::BeFreeOfCycles];
            proptest::prop_assert_eq!(
                failures(&refs, &edges, &both),
                failures_by_scan(&refs, &edges, &both)
            );
        }
    }

    /// Runs `f` on a thread with a 2 MiB stack, the size a spawned thread gets by default.
    fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(f)
            .ok()?
            .join()
            .ok()
    }

    #[test]
    fn a_cycle_through_a_hundred_thousand_slices_does_not_exhaust_the_stack() {
        const N: usize = 100_000;
        let found = on_small_stack(|| {
            let graph: BTreeMap<usize, BTreeSet<usize>> =
                (0..N).map(|i| (i, BTreeSet::from([(i + 1) % N]))).collect();
            components(&graph, N)
        });
        let found = found.unwrap_or_default();
        assert_eq!(found.len(), 1, "one component");
        assert_eq!(found[0].len(), N);
        assert_eq!(found[0].first(), Some(&0));
        assert_eq!(found[0].last(), Some(&(N - 1)));
    }

    #[test]
    fn a_hundred_thousand_module_import_cycle_is_one_failing_cycle() {
        const N: usize = 100_000;
        let outcome = on_small_stack(|| {
            let modules: Vec<rb_model::Module> = (0..N)
                .map(|i| {
                    let mut m = rb_model::Module::new(format!("src/m{i}.ts"));
                    m.language = Some(Language::Typescript);
                    m.dependencies = vec![rb_model::Dependency::new(
                        "x",
                        format!("src/m{}.ts", (i + 1) % N),
                        rb_model::ModuleSystem::Es6,
                    )];
                    m
                })
                .collect();
            let document = rb_model::GraphDocument {
                modules,
                ..rb_model::GraphDocument::default()
            };
            let architecture = Architecture::new(&document);
            let rule = rb_config::elements::parse_slices(&serde_json::json!([
                { "name": "s", "matching": "src/(**)", "should": "beFreeOfCycles" }
            ]))
            .ok()?
            .pop()?;
            evaluate(&architecture, &rule).ok().map(|o| {
                (
                    o.slices.len(),
                    o.failures.len(),
                    o.failures.first().map(|f| f.slices.len()),
                )
            })
        });
        assert_eq!(outcome.flatten(), Some((N, 1, Some(N))));
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
