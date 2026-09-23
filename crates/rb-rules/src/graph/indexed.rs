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
        graph.component = graph.components();
        graph
    }

    /// Whether a vertex has this name.
    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    /// Tarjan's algorithm, iterative, over the edges that lead to a known vertex.
    fn components(&self) -> Vec<usize> {
        let n = self.vertices.len();
        let mut index = vec![usize::MAX; n];
        let mut low = vec![0; n];
        let mut on_stack = vec![false; n];
        let mut component = vec![usize::MAX; n];
        let mut stack = Vec::new();
        let mut next = 0;
        let mut count = 0;
        let targets: Vec<Vec<usize>> = self
            .vertices
            .iter()
            .map(|v| {
                v.edges
                    .iter()
                    .filter_map(|(t, _)| self.index.get(t).copied())
                    .collect()
            })
            .collect();
        for root in 0..n {
            if index[root] != usize::MAX {
                continue;
            }
            let mut work: Vec<(usize, usize)> = vec![(root, 0)];
            index[root] = next;
            low[root] = next;
            next += 1;
            stack.push(root);
            on_stack[root] = true;
            while let Some(&mut (v, ref mut edge)) = work.last_mut() {
                if let Some(&w) = targets[v].get(*edge) {
                    *edge += 1;
                    if index[w] == usize::MAX {
                        index[w] = next;
                        low[w] = next;
                        next += 1;
                        stack.push(w);
                        on_stack[w] = true;
                        work.push((w, 0));
                    } else if on_stack[w] {
                        low[v] = low[v].min(index[w]);
                    }
                    continue;
                }
                work.pop();
                if let Some(&(parent, _)) = work.last() {
                    low[parent] = low[parent].min(low[v]);
                }
                if low[v] == index[v] {
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

    /// `getPath(from, to)`: the first path found depth first, or empty.
    pub fn path(&self, from: &str, to: &str) -> Vec<Step> {
        self.path_from(from, to, &mut HashSet::new())
    }

    fn path_from(&self, from: &str, to: &str, visited: &mut HashSet<String>) -> Vec<Step> {
        visited.insert(from.to_owned());
        let Some(&at) = self.index.get(from) else {
            return Vec::new();
        };
        for (name, types) in &self.vertices[at].edges {
            if !visited.contains(name) {
                if name == to {
                    return vec![Self::step(name, types)];
                }
                let rest = self.path_from(name, to, visited);
                if !rest.is_empty() {
                    let mut out = vec![Self::step(name, types)];
                    out.extend(rest);
                    return out;
                }
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

    #[test]
    fn dependents_in_module_order() {
        let set = DependencySet::new(&modules());
        assert_eq!(set.dependents(&json!({ "source": "a" })), ["c"]);
        assert!(set.has_dependents(&json!({ "source": "y" })));
        assert!(!set.has_dependents(&json!({ "source": "zzz" })));
        assert!(set.dependents(&json!({ "source": "zzz" })).is_empty());
    }
}
