//! `IndexedModuleGraph` and `ModuleGraphWithDependencySet`: dependency-cruiser 18.2.0's graph
//! walks, ported, plus Tarjan's strongly connected components to skip hopeless cycle searches.
//!
//! - Specification: `test/graph-utl/indexed-module-graph.spec.mjs`,
//!   `module-graph-with-dependency-set.spec.mjs`, run by conformance gate 1 layer 2
//!   ([ADR-0009](../../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Plan: [Wave 1, Step 6](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-6-graph-analysis-1b)
//!   (Tarjan and cycle path order: hand-written, because the reporters print the path)
//! - Requirement: [FR-RULE-08](../../../../docs/prd.md#fr-rule-08),
//!   [NFR-PERF-01](../../../../docs/prd.md#nfr-perf-01)
//!
//! `get_cycle` is upstream's depth-first search, edge order and visited set included, so the
//! cycle it reports is the one dependency-cruiser reports. It runs only for an edge whose two
//! ends share a strongly connected component: an edge between components is on no cycle, and a
//! detour outside the component can never lead back, so pruning those vertices changes nothing
//! but the running time.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use crate::js;

/// One step of a path or cycle: `{ name, dependencyTypes }`.
pub type Step = Value;

#[derive(Debug, Clone)]
struct Vertex {
    /// `(name, dependencyTypes)` of each outgoing edge, in order.
    edges: Vec<(String, Value)>,
    dependents: Vec<String>,
}

/// `IndexedModuleGraph`, over modules (index `source`) or folders (index `name`).
#[derive(Debug, Clone, Default)]
pub struct IndexedGraph {
    names: Vec<String>,
    index: HashMap<String, usize>,
    vertices: Vec<Vertex>,
    component: Vec<usize>,
    /// The vertex each edge leads to, by index, parallel to `vertices[i].edges`; `None` for an
    /// edge to a name no module has.
    targets: Vec<Vec<Option<usize>>>,
}

impl IndexedGraph {
    /// Indexes `modules` by the string at `attribute`; a later duplicate replaces an earlier
    /// one, as `new Map(entries)` does.
    pub fn new(modules: &[Value], attribute: &str) -> Self {
        let mut graph = Self::default();
        for module in modules {
            let name = js::text(module, attribute).into_owned();
            let edges = js::array(module, "dependencies")
                .iter()
                .map(|d| {
                    let name = match d.get("name") {
                        Some(n) if js::truthy(Some(n)) => js::text(d, "name").into_owned(),
                        _ => js::text(d, "resolved").into_owned(),
                    };
                    let types = d
                        .get("dependencyTypes")
                        .cloned()
                        .unwrap_or_else(|| json!([]));
                    (name, types)
                })
                .collect();
            let dependents = js::strings(module, "dependents")
                .into_iter()
                .map(str::to_owned)
                .collect();
            let vertex = Vertex { edges, dependents };
            if let Some(&at) = graph.index.get(&name) {
                graph.vertices[at] = vertex;
            } else {
                graph.index.insert(name.clone(), graph.names.len());
                graph.names.push(name);
                graph.vertices.push(vertex);
            }
        }
        graph.targets = graph
            .vertices
            .iter()
            .map(|v| {
                v.edges
                    .iter()
                    .map(|(t, _)| graph.index.get(t).copied())
                    .collect()
            })
            .collect();
        graph.component = graph.components();
        graph
    }

    /// Whether a vertex has this name.
    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    /// Tarjan's algorithm, iterative, over the edges that lead to a known vertex.
    ///
    /// Each frame of the work stack holds its own edge iterator, so no counter can stall, and
    /// an unvisited vertex is `None` rather than a sentinel number.
    fn components(&self) -> Vec<usize> {
        let n = self.vertices.len();
        let mut index: Vec<Option<usize>> = vec![None; n];
        let mut low = vec![0; n];
        let mut on_stack = vec![false; n];
        let mut component = vec![usize::MAX; n];
        let mut stack = Vec::new();
        let mut next = 0;
        let mut count = 0;
        let targets: Vec<Vec<usize>> = self
            .targets
            .iter()
            .map(|t| t.iter().flatten().copied().collect())
            .collect();
        for root in 0..n {
            if index[root].is_some() {
                continue;
            }
            index[root] = Some(next);
            low[root] = next;
            next += 1;
            stack.push(root);
            on_stack[root] = true;
            let mut work = vec![(root, targets[root].iter())];
            while let Some((v, edges)) = work.last_mut() {
                let v = *v;
                if let Some(&w) = edges.next() {
                    match index[w] {
                        None => {
                            index[w] = Some(next);
                            low[w] = next;
                            next += 1;
                            stack.push(w);
                            on_stack[w] = true;
                            work.push((w, targets[w].iter()));
                        }
                        Some(at) if on_stack[w] => low[v] = low[v].min(at),
                        Some(_) => {}
                    }
                    continue;
                }
                work.pop();
                if let Some(&(parent, _)) = work.last() {
                    low[parent] = low[parent].min(low[v]);
                }
                if Some(low[v]) == index[v] {
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        component[w] = count;
                        if w == v {
                            break;
                        }
                    }
                    count += 1;
                }
            }
        }
        component
    }

    /// Whether `a` and `b` share a strongly connected component.
    pub fn same_component(&self, a: &str, b: &str) -> bool {
        match (self.index.get(a), self.index.get(b)) {
            (Some(&x), Some(&y)) => self.component[x] == self.component[y],
            _ => false,
        }
    }

    /// `findVertexByName(name)` presence and its module index.
    pub fn position(&self, name: &str) -> Option<usize> {
        self.index.get(name).copied()
    }

    fn walk(
        &self,
        name: &str,
        max_depth: u32,
        depth: u32,
        visited: &mut Vec<String>,
        seen: &mut HashSet<String>,
        dependents: bool,
    ) {
        let Some(&at) = self.index.get(name) else {
            return;
        };
        if max_depth != 0 && depth > max_depth {
            return;
        }
        if seen.insert(name.to_owned()) {
            visited.push(name.to_owned());
        }
        let vertex = &self.vertices[at];
        let next: Vec<&String> = if dependents {
            vertex.dependents.iter().collect()
        } else {
            vertex.edges.iter().map(|(n, _)| n).collect()
        };
        for n in next {
            if !seen.contains(n) {
                self.walk(n, max_depth, depth + 1, visited, seen, dependents);
            }
        }
    }

    /// `findTransitiveDependents(name, maxDepth)`: the start and everything that depends on it
    /// within `max_depth` steps (0: no limit), in visiting order.
    pub fn transitive_dependents(&self, name: &str, max_depth: u32) -> Vec<String> {
        let mut visited = Vec::new();
        self.walk(name, max_depth, 0, &mut visited, &mut HashSet::new(), true);
        visited
    }

    /// `findTransitiveDependencies(name, maxDepth)`.
    pub fn transitive_dependencies(&self, name: &str, max_depth: u32) -> Vec<String> {
        let mut visited = Vec::new();
        self.walk(name, max_depth, 0, &mut visited, &mut HashSet::new(), false);
        visited
    }

    fn step(name: &str, types: &Value) -> Step {
        json!({ "name": name, "dependencyTypes": types })
    }

    /// Whether each vertex, by index, is reachable from `from` over one or more edges.
    ///
    /// [`Self::path`] is non-empty exactly when its `to` is one of these and is not `from`: the
    /// depth-first search behind it visits every vertex it can reach before it gives up, and a
    /// route back through `from` has a shorter one after it. So a caller that asks for many
    /// paths from one module computes this once and asks only for the paths that exist, which
    /// changes nothing but the running time (one walk instead of one per target).
    pub fn reachable_from(&self, from: &str) -> Vec<bool> {
        let mut seen = vec![false; self.vertices.len()];
        let Some(&start) = self.index.get(from) else {
            return seen;
        };
        let mut stack = vec![start];
        while let Some(at) = stack.pop() {
            for (name, _) in &self.vertices[at].edges {
                if let Some(&next) = self.index.get(name)
                    && !seen[next]
                {
                    seen[next] = true;
                    stack.push(next);
                }
            }
        }
        seen
    }

    /// Whether a path to `to` can exist, given a [`Self::reachable_from`] result: false only for
    /// a vertex the walk did not reach. A name that is no vertex (an edge target the modules do
    /// not list) is left to [`Self::path`] to decide.
    pub fn may_reach(&self, reachable: &[bool], to: &str) -> bool {
        self.index
            .get(to)
            .is_none_or(|&at| reachable.get(at).copied().unwrap_or(false))
    }

    /// `getPath(from, to)`: the first path found depth first, or empty.
    ///
    /// Upstream's depth-first search, by vertex index: the edges in order, a visited vertex
    /// skipped, the first edge that names `to` ending the search. An edge to a name no module has
    /// is only compared with `to`, since searching from it finds nothing. The search is
    /// iterative, one frame per step of the path being tried (the vertex and the position of
    /// the edge it is following), so a chain through every module of a large repository cannot
    /// exhaust the call stack; when `to` is found, the edges the frames are following are the
    /// path.
    pub fn path(&self, from: &str, to: &str) -> Vec<Step> {
        let Some(&start) = self.index.get(from) else {
            return Vec::new();
        };
        let mut visited = vec![false; self.vertices.len()];
        visited[start] = true;
        // `(vertex, index of the next edge to try)`; the edge being followed is the one before.
        let mut frames: Vec<(usize, usize)> = vec![(start, 0)];
        while let Some((at, next)) = frames.last_mut() {
            let at = *at;
            let Some((name, _)) = self.vertices[at].edges.get(*next) else {
                frames.pop();
                continue;
            };
            let target = self.targets[at][*next];
            *next += 1;
            if target.is_some_and(|t| visited[t]) {
                continue;
            }
            if name == to {
                return frames
                    .iter()
                    .filter_map(|&(v, n)| self.vertices[v].edges.get(n.checked_sub(1)?))
                    .map(|(name, types)| Self::step(name, types))
                    .collect();
            }
            if let Some(t) = target {
                visited[t] = true;
                frames.push((t, 0));
            }
        }
        Vec::new()
    }

    /// `getCycle(initial, current)`: the first cycle from `initial` through its edge to
    /// `current`, as dependency-cruiser finds it, or empty.
    pub fn cycle(&self, initial: &str, current: &str) -> Vec<Step> {
        let Some(&at) = self.index.get(initial) else {
            return Vec::new();
        };
        let Some((name, types)) = self.vertices[at].edges.iter().find(|(n, _)| n == current) else {
            return Vec::new();
        };
        if !self.same_component(initial, current) {
            return Vec::new();
        }
        self.cycle_from(initial, name, types, &mut HashSet::new())
    }

    fn cycle_from(
        &self,
        initial: &str,
        current: &str,
        current_types: &Value,
        visited: &mut HashSet<String>,
    ) -> Vec<Step> {
        let Some(&at) = self.index.get(current) else {
            return Vec::new();
        };
        let component = self.component[at];
        let edges: Vec<&(String, Value)> = self.vertices[at]
            .edges
            .iter()
            .filter(|(n, _)| !visited.contains(n))
            .collect();
        if let Some((name, types)) = edges.iter().find(|(n, _)| n == initial) {
            return if initial == current {
                vec![Self::step(name, types)]
            } else {
                vec![Self::step(current, current_types), Self::step(name, types)]
            };
        }
        for (name, types) in edges {
            // Outside the component a path never returns to `initial`: prune, as the upstream
            // search would find nothing there either.
            if self.index.get(name).map(|&i| self.component[i]) != Some(component) {
                visited.insert(name.clone());
                continue;
            }
            visited.insert(name.clone());
            let cycle = self.cycle_from(initial, name, types, visited);
            if !cycle.is_empty() && !cycle.iter().any(|s| js::str_of(s, "name") == Some(current)) {
                let mut out = vec![Self::step(current, current_types)];
                out.extend(cycle);
                return out;
            }
        }
        Vec::new()
    }
}

/// `ModuleGraphWithDependencySet`: who depends on a module, in module order.
#[derive(Debug, Clone, Default)]
pub struct DependencySet {
    dependents: HashMap<String, Vec<String>>,
}

impl DependencySet {
    /// Indexes the `resolved` of every module's dependencies.
    pub fn new(modules: &[Value]) -> Self {
        let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
        for module in modules {
            let source = js::text(module, "source").into_owned();
            let mut targets: Vec<String> = js::array(module, "dependencies")
                .iter()
                .map(|d| js::text(d, "resolved").into_owned())
                .collect();
            targets.sort();
            targets.dedup();
            for target in targets {
                dependents.entry(target).or_default().push(source.clone());
            }
        }
        Self { dependents }
    }

    /// `moduleHasDependents(module)`.
    pub fn has_dependents(&self, module: &Value) -> bool {
        self.dependents
            .contains_key(js::text(module, "source").as_ref())
    }

    /// `getDependents(module)`.
    pub fn dependents(&self, module: &Value) -> Vec<String> {
        self.dependents
            .get(js::text(module, "source").as_ref())
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modules() -> Vec<Value> {
        let edge = |to: &str| json!({ "resolved": to, "dependencyTypes": ["local"] });
        vec![
            json!({ "source": "a", "dependencies": [edge("b"), edge("x")], "dependents": ["c"] }),
            json!({ "source": "b", "dependencies": [edge("c")], "dependents": ["a"] }),
            json!({ "source": "c", "dependencies": [edge("a")], "dependents": ["b"] }),
            json!({ "source": "x", "dependencies": [edge("y")], "dependents": ["a"] }),
            json!({ "source": "y", "dependencies": [], "dependents": ["x"] }),
            json!({ "source": "self", "dependencies": [edge("self")] }),
        ]
    }

    #[test]
    fn cycles_follow_upstream_order() {
        let graph = IndexedGraph::new(&modules(), "source");
        let names = |steps: Vec<Step>| -> Vec<String> {
            steps
                .iter()
                .filter_map(|s| s["name"].as_str().map(str::to_owned))
                .collect()
        };
        assert_eq!(names(graph.cycle("a", "b")), ["b", "c", "a"]);
        assert_eq!(names(graph.cycle("b", "c")), ["c", "a", "b"]);
        assert!(graph.cycle("a", "x").is_empty());
        assert!(graph.cycle("a", "nope").is_empty());
        assert!(graph.cycle("nope", "a").is_empty());
        assert_eq!(names(graph.cycle("self", "self")), ["self"]);
        assert!(graph.same_component("a", "c"));
        assert!(!graph.same_component("a", "y"));
        assert!(!graph.same_component("a", "missing"));
        assert_eq!(
            graph.cycle("a", "b")[0]["dependencyTypes"],
            json!(["local"])
        );
    }

    #[test]
    fn paths_and_transitive_walks() {
        let graph = IndexedGraph::new(&modules(), "source");
        let names = |steps: Vec<Step>| -> Vec<String> {
            steps
                .iter()
                .filter_map(|s| s["name"].as_str().map(str::to_owned))
                .collect()
        };
        assert_eq!(names(graph.path("a", "y")), ["x", "y"]);
        assert_eq!(names(graph.path("a", "c")), ["b", "c"]);
        assert!(graph.path("y", "a").is_empty());
        assert!(graph.path("missing", "a").is_empty());
        assert_eq!(
            graph.transitive_dependencies("a", 0),
            ["a", "b", "c", "x", "y"]
        );
        assert_eq!(graph.transitive_dependencies("a", 1), ["a", "b", "x"]);
        assert_eq!(graph.transitive_dependents("a", 0), ["a", "c", "b"]);
        assert_eq!(graph.transitive_dependents("y", 1), ["y", "x"]);
        assert!(graph.transitive_dependents("missing", 0).is_empty());
        assert!(graph.contains("a"));
        assert_eq!(graph.position("b"), Some(1));
    }

    #[test]
    fn duplicates_and_names() {
        let graph = IndexedGraph::new(
            &[
                json!({ "name": "f", "dependencies": [{ "name": "g" }] }),
                json!({ "name": "g", "dependencies": [{ "name": "f" }] }),
                json!({ "name": "f", "dependencies": [{ "name": "g" }] }),
            ],
            "name",
        );
        assert_eq!(graph.cycle("f", "g").len(), 2);
        assert_eq!(graph.cycle("f", "g")[0]["dependencyTypes"], json!([]));
    }

    fn graph_of(edges: &[(&str, &[&str])]) -> IndexedGraph {
        let modules: Vec<Value> = edges
            .iter()
            .map(|(source, to)| {
                let dependencies: Vec<Value> =
                    to.iter().map(|t| json!({ "resolved": t })).collect();
                json!({ "source": source, "dependencies": dependencies })
            })
            .collect();
        IndexedGraph::new(&modules, "source")
    }

    fn names(steps: &[Step]) -> Vec<&str> {
        steps.iter().filter_map(|s| s["name"].as_str()).collect()
    }

    #[test]
    fn components_are_the_strongly_connected_ones() {
        // A cycle through the first root, a cycle below a root, a chain, a lone vertex, and a
        // cross edge.
        let graph = graph_of(&[
            ("r", &["s"]),
            ("s", &["r", "t"]),
            ("t", &["u"]),
            ("u", &["v"]),
            ("v", &["u", "w"]),
            ("w", &[]),
            ("lone", &[]),
            ("p", &["q"]),
            ("q", &["p"]),
            // A cross edge, z to the finished y, lowers nothing: y is off the stack.
            ("x", &["y", "z"]),
            ("y", &[]),
            ("z", &["y"]),
        ]);
        let pairs = [
            ("r", "s", true),
            ("u", "v", true),
            ("p", "q", true),
            ("r", "t", false),
            ("s", "t", false),
            ("t", "u", false),
            ("v", "w", false),
            ("r", "u", false),
            ("w", "lone", false),
            ("lone", "p", false),
            ("r", "p", false),
            ("u", "p", false),
            ("x", "z", false),
            ("x", "y", false),
            ("y", "z", false),
        ];
        for (a, b, same) in pairs {
            assert_eq!(graph.same_component(a, b), same, "{a} {b}");
            assert_eq!(graph.same_component(b, a), same, "{b} {a}");
        }
        assert!(graph.same_component("lone", "lone"));
        assert_eq!(names(&graph.cycle("r", "s")), ["s", "r"]);
        assert_eq!(names(&graph.cycle("u", "v")), ["v", "u"]);
        assert_eq!(names(&graph.cycle("p", "q")), ["q", "p"]);
        assert!(graph.cycle("s", "t").is_empty());
    }

    #[test]
    fn a_cycle_that_revisits_the_current_vertex_is_skipped() {
        // Upstream's getCycle: the search from b through c comes back through b, so that
        // cycle is dropped and the next edge, d, gives the answer.
        let graph = graph_of(&[
            ("a", &["b"]),
            ("b", &["c", "d"]),
            ("c", &["b"]),
            ("d", &["a"]),
        ]);
        assert_eq!(names(&graph.cycle("a", "b")), ["b", "d", "a"]);
    }

    #[test]
    fn lookups_answer_for_known_names_only() {
        let graph = IndexedGraph::new(&modules(), "source");
        assert!(!graph.contains("missing"));
        assert_eq!(graph.position("a"), Some(0));
        assert_eq!(graph.position("x"), Some(3));
        assert_eq!(graph.position("missing"), None);
    }

    #[test]
    fn reachable_from_marks_what_a_path_reaches() {
        let graph = IndexedGraph::new(&modules(), "source");
        let from_a = graph.reachable_from("a");
        for (to, reached) in [
            ("b", true),
            ("c", true),
            ("x", true),
            ("y", true),
            ("self", false),
        ] {
            assert_eq!(graph.may_reach(&from_a, to), reached, "a to {to}");
        }
        // a reaches itself through c, but a path never returns to its start.
        assert!(graph.may_reach(&from_a, "a"));
        assert!(graph.path("a", "a").is_empty());
        // y reaches nothing; an unknown start reaches nothing; an unknown target is left to path.
        let from_y = graph.reachable_from("y");
        assert!(!graph.may_reach(&from_y, "a"));
        assert!(graph.reachable_from("missing").iter().all(|r| !r));
        assert!(graph.may_reach(&from_y, "not-a-vertex"));
    }

    /// Upstream's `getPath` as dependency-cruiser writes it, over names: the reference the
    /// index-based search must reproduce step for step.
    fn reference_path(
        graph: &IndexedGraph,
        from: &str,
        to: &str,
        visited: &mut HashSet<String>,
    ) -> Vec<Step> {
        visited.insert(from.to_owned());
        let Some(&at) = graph.index.get(from) else {
            return Vec::new();
        };
        for (name, types) in &graph.vertices[at].edges {
            if !visited.contains(name) {
                if name == to {
                    return vec![IndexedGraph::step(name, types)];
                }
                let rest = reference_path(graph, name, to, visited);
                if !rest.is_empty() {
                    let mut out = vec![IndexedGraph::step(name, types)];
                    out.extend(rest);
                    return out;
                }
            }
        }
        Vec::new()
    }

    #[test]
    fn a_path_along_a_hundred_thousand_module_chain_does_not_exhaust_the_stack() {
        const N: usize = 100_000;
        let found = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let modules: Vec<Value> = (0..N)
                    .map(|i| {
                        json!({ "source": format!("m{i}"),
                                "dependencies": [{ "resolved": format!("m{}", (i + 1) % N) }] })
                    })
                    .collect();
                let graph = IndexedGraph::new(&modules, "source");
                let path = graph.path("m0", &format!("m{}", N - 1));
                let back = graph.path("m1", "m0");
                (
                    path.len(),
                    path.first().cloned(),
                    path.last().cloned(),
                    back.len(),
                    graph.path("m0", "nowhere").len(),
                )
            })
            .ok()
            .and_then(|t| t.join().ok());
        let (len, first, last, back, none) = found.unwrap_or_default();
        assert_eq!(len, N - 1, "one step per edge of the chain");
        assert_eq!(first, Some(json!({ "name": "m1", "dependencyTypes": [] })));
        assert_eq!(
            last,
            Some(json!({ "name": format!("m{}", N - 1), "dependencyTypes": [] }))
        );
        assert_eq!(back, N - 1, "round the cycle back to the start");
        assert_eq!(none, 0);
    }

    proptest::proptest! {
        /// Every path, between vertices and to a name no module has (m8, m9), is upstream's.
        #[test]
        fn path_is_upstreams_depth_first_search(
            edges in proptest::collection::vec((0u8..8, 0u8..10), 0..30)
        ) {
            let names: Vec<String> = (0..10).map(|n| format!("m{n}")).collect();
            let modules: Vec<Value> = names[..8]
                .iter()
                .enumerate()
                .map(|(at, name)| {
                    let dependencies: Vec<Value> = edges
                        .iter()
                        .filter(|(from, _)| usize::from(*from) == at)
                        .map(|(_, to)| json!({ "resolved": names[usize::from(*to)], "dependencyTypes": [to] }))
                        .collect();
                    json!({ "source": name, "dependencies": dependencies })
                })
                .collect();
            let graph = IndexedGraph::new(&modules, "source");
            for from in &names {
                for to in &names {
                    proptest::prop_assert_eq!(
                        graph.path(from, to),
                        reference_path(&graph, from, to, &mut HashSet::new()),
                        "{} to {}", from, to
                    );
                }
            }
        }

        /// The shortcut `derive::reachables` takes: for every pair of distinct vertices, a
        /// path exists exactly when the walk marks the target.
        #[test]
        fn may_reach_agrees_with_path(
            edges in proptest::collection::vec((0u8..8, 0u8..8), 0..24)
        ) {
            let names: Vec<String> = (0..8).map(|n| format!("m{n}")).collect();
            let modules: Vec<Value> = names
                .iter()
                .enumerate()
                .map(|(at, name)| {
                    let dependencies: Vec<Value> = edges
                        .iter()
                        .filter(|(from, _)| usize::from(*from) == at)
                        .map(|(_, to)| json!({ "resolved": names[usize::from(*to)] }))
                        .collect();
                    json!({ "source": name, "dependencies": dependencies })
                })
                .collect();
            let graph = IndexedGraph::new(&modules, "source");
            for from in &names {
                let reach = graph.reachable_from(from);
                for to in names.iter().filter(|to| *to != from) {
                    proptest::prop_assert_eq!(
                        graph.may_reach(&reach, to),
                        !graph.path(from, to).is_empty(),
                        "{} to {}", from, to
                    );
                }
            }
        }
    }

    #[test]
    fn dependents_in_module_order() {
        let set = DependencySet::new(&modules());
        assert_eq!(set.dependents(&json!({ "source": "a" })), ["c"]);
        assert!(set.has_dependents(&json!({ "source": "y" })));
        assert!(!set.has_dependents(&json!({ "source": "zzz" })));
        assert!(set.dependents(&json!({ "source": "zzz" })).is_empty());
    }
}
