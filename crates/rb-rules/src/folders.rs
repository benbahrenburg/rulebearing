//! The folder layer: modules aggregated into `folders[]` with couplings, instability and cycles.
//! dependency-cruiser 18.2.0's `src/analyze/derive/folders`, ported.
//!
//! - Plan: [Wave 1, Step 6](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-6-graph-analysis-1b)
//!   (`folders.rs`: `moduleCount`, `dependencies[]`, `dependents[]`, couplings, instability)
//! - Coverage: [coverage § Rules](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#rules),
//!   row `scope: folder`; [coverage § Result document](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#result-document-cruise-result-schema)
//! - Requirement: [FR-RULE-08](../../../docs/prd.md#fr-rule-08)
//!
//! Folders are derived when metrics are on, which dependency-cruiser switches on for any rule with
//! `scope: folder` or `to.moreUnstable`; each folder dependency is then validated against the
//! folder-scoped rules. Folder order is the order folders are first met, as upstream's object keys.

use std::collections::{HashMap, HashSet};

use rb_config::model::DependencyRules;
use serde_json::{Map, Value, json};

use crate::derive::{cycles, instability, metrics_are_calculable};
use crate::graph::consolidate::dirname;
use crate::js;
use crate::validate::validate_folder;

#[derive(Default)]
struct Aggregate {
    /// Name to count, in first-seen order.
    dependents: Vec<(String, usize)>,
    dependencies: Vec<(String, usize)>,
    module_count: i64,
    stats: Option<(u64, u64)>,
}

fn upsert(list: &mut Vec<(String, usize)>, name: &str) {
    if let Some(entry) = list.iter_mut().find(|(n, _)| n == name) {
        entry.1 += 1;
    } else {
        list.push((name.to_owned(), 1));
    }
}

/// `getParentFolders`: `a/b/c` gives `a`, `a/b`, `a/b/c`.
pub fn parent_folders(path: &str) -> Vec<String> {
    let fragments: Vec<&str> = path.split('/').collect();
    (1..=fragments.len())
        .map(|n| fragments[..n].join("/"))
        .collect()
}

/// `getFolderLevelCouplings`: each coupled module's folder, once.
fn folder_level(couplings: &[(String, usize)]) -> Vec<Value> {
    let mut seen = HashSet::new();
    couplings
        .iter()
        .map(|(name, _)| {
            let dir = dirname(name);
            if dir == "." { name.clone() } else { dir }
        })
        .filter(|name| seen.insert(name.clone()))
        .map(|name| json!({ "name": name }))
        .collect()
}

/// `aggregateToFolders`, first half: each calculable module counted into every parent folder.
/// Returns the folder names in first-seen order with their aggregates.
fn aggregate(modules: &[Value]) -> (Vec<String>, HashMap<String, Aggregate>) {
    let mut order: Vec<String> = Vec::new();
    let mut all: HashMap<String, Aggregate> = HashMap::new();
    for module in modules.iter().filter(|m| metrics_are_calculable(m)) {
        let dir = dirname(&js::text(module, "source"));
        for folder in parent_folders(&dir) {
            let prefix = format!("{folder}/");
            let entry = all.entry(folder.clone()).or_insert_with(|| {
                order.push(folder.clone());
                Aggregate::default()
            });
            for dependent in js::strings(module, "dependents") {
                if !dependent.starts_with(&prefix) {
                    upsert(&mut entry.dependents, dependent);
                }
            }
            for dependency in js::array(module, "dependencies") {
                let resolved = js::text(dependency, "resolved");
                if !resolved.starts_with(&prefix) {
                    upsert(&mut entry.dependencies, &resolved);
                }
            }
            entry.module_count += 1;
            if let Some(stats) = module.get("experimentalStats") {
                let (size, statements) = entry.stats.get_or_insert((0, 0));
                *size += stats.get("size").and_then(Value::as_u64).unwrap_or(0);
                *statements += stats
                    .get("topLevelStatementCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
            }
        }
    }
    (order, all)
}

/// One folder's record: couplings, module count, stats and instability.
fn folder_record(name: &str, aggregate: &Aggregate) -> Value {
    let afferent: usize = aggregate.dependents.iter().map(|(_, c)| c).sum();
    let efferent: usize = aggregate.dependencies.iter().map(|(_, c)| c).sum();
    let mut folder = Map::new();
    folder.insert("name".into(), json!(name));
    folder.insert(
        "dependencies".into(),
        Value::Array(folder_level(&aggregate.dependencies)),
    );
    folder.insert(
        "dependents".into(),
        Value::Array(folder_level(&aggregate.dependents)),
    );
    folder.insert("moduleCount".into(), json!(aggregate.module_count));
    if let Some((size, statements)) = aggregate.stats {
        folder.insert(
            "experimentalStats".into(),
            json!({ "size": size, "topLevelStatementCount": statements }),
        );
    }
    folder.insert("afferentCouplings".into(), json!(afferent));
    folder.insert("efferentCouplings".into(), json!(efferent));
    folder.insert("instability".into(), json!(instability(efferent, afferent)));
    Value::Object(folder)
}

/// Copies each target folder's instability onto the folder dependencies that point at it, and
/// returns the names of the folders known so far.
fn add_dependency_instability(result: &mut [Value]) -> HashSet<String> {
    let by_name: HashMap<String, f64> = result
        .iter()
        .map(|f| {
            (
                js::text(f, "name").into_owned(),
                js::number(f, "instability").unwrap_or(0.0),
            )
        })
        .collect();
    for folder in result.iter_mut() {
        if let Some(Value::Array(dependencies)) = folder.get_mut("dependencies") {
            for dependency in dependencies {
                let value = by_name
                    .get(js::text(dependency, "name").as_ref())
                    .copied()
                    .filter(|v| *v >= 0.0)
                    .unwrap_or(0.0);
                js::set(dependency, "instability", json!(value));
            }
        }
    }
    by_name.into_keys().collect()
}

/// Sinks: folders depended on that hold no calculable module, once each, in first-met order.
fn sinks(result: &[Value], known: &HashSet<String>) -> Vec<Value> {
    let mut sinks = Vec::new();
    let mut seen = HashSet::new();
    for folder in result {
        for dependency in js::array(folder, "dependencies") {
            let name = js::text(dependency, "name").into_owned();
            if !known.contains(&name) && seen.insert(name.clone()) {
                sinks.push(json!({ "name": name, "moduleCount": -1, "dependencies": [], "dependents": [] }));
            }
        }
    }
    sinks
}

/// `addFolderDependencyViolations`: each folder dependency validated against the folder rules.
fn add_violations(result: &mut [Value], rules: &DependencyRules) {
    for folder in result.iter_mut() {
        let from = folder.clone();
        if let Some(Value::Array(dependencies)) = folder.get_mut("dependencies") {
            for dependency in dependencies.iter_mut() {
                let verdict = validate_folder(rules, &from, dependency);
                if let (Value::Object(target), Value::Object(verdict)) = (dependency, verdict) {
                    target.extend(verdict);
                }
            }
        }
    }
}

/// `aggregateToFolders` followed by `addFolderDependencyViolations`: the `folders[]` array.
pub fn folders(modules: &[Value], skip: bool, rules: &DependencyRules) -> Vec<Value> {
    let (order, all) = aggregate(modules);
    let mut result: Vec<Value> = order
        .iter()
        .filter_map(|name| all.get(name).map(|a| folder_record(name, a)))
        .collect();
    let known = add_dependency_instability(&mut result);
    let sinks = sinks(&result, &known);
    result.extend(sinks);
    cycles(&mut result, "name", "name", skip, rules);
    add_violations(&mut result, rules);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modules() -> Vec<Value> {
        vec![
            json!({ "source": "src/a/x.ts", "dependencies": [{ "resolved": "src/b/y.ts" }], "dependents": ["src/b/y.ts"],
                    "experimentalStats": { "size": 10, "topLevelStatementCount": 2 } }),
            json!({ "source": "src/b/y.ts", "dependencies": [{ "resolved": "src/a/x.ts" }, { "resolved": "fs" }], "dependents": ["src/a/x.ts"] }),
            json!({ "source": "fs", "dependencies": [], "dependents": ["src/b/y.ts"], "coreModule": true }),
        ]
    }

    #[test]
    fn folders_aggregate_with_couplings_sinks_and_cycles() {
        let rules = rb_config::normalize::rule_set(
            json!({ "forbidden": [{ "name": "no-folder-cycles", "scope": "folder", "severity": "error", "from": {}, "to": { "circular": true } }] })
                .as_object()
                .unwrap_or(&Map::new()),
        )
        .unwrap_or_default();
        let out = folders(&modules(), false, &rules);
        let names: Vec<&str> = out.iter().filter_map(|f| f["name"].as_str()).collect();
        assert_eq!(names, ["src", "src/a", "src/b", "fs"]);
        let src = &out[0];
        assert_eq!(src["moduleCount"], 2);
        assert_eq!(src["efferentCouplings"], 1);
        assert_eq!(
            src["dependencies"],
            json!([{ "name": "fs", "instability": 0.0, "circular": false, "valid": true }])
        );
        let a = &out[1];
        assert_eq!(
            a["experimentalStats"],
            json!({ "size": 10, "topLevelStatementCount": 2 })
        );
        assert_eq!(a["dependencies"][0]["name"], "src/b");
        assert_eq!(a["dependencies"][0]["circular"], true);
        assert_eq!(a["dependencies"][0]["valid"], false);
        assert_eq!(a["dependencies"][0]["rules"][0]["name"], "no-folder-cycles");
        assert_eq!(a["instability"], json!(0.5));
        assert_eq!(out[3]["moduleCount"], -1);
    }

    #[test]
    fn couplings_count_every_edge_and_list_each_folder_once() {
        let edge = |to: &str| json!({ "resolved": to });
        let modules = [
            json!({ "source": "src/a/x.ts", "dependencies": [edge("src/b/y.ts"), edge("src/c/w.ts")], "dependents": [] }),
            json!({ "source": "src/a/z.ts", "dependencies": [edge("src/b/y.ts")], "dependents": [] }),
            json!({ "source": "src/b/y.ts", "dependencies": [edge("src/c/w.ts")], "dependents": ["src/a/x.ts", "src/a/z.ts"] }),
            json!({ "source": "src/c/w.ts", "dependencies": [], "dependents": ["src/a/x.ts", "src/b/y.ts"] }),
        ];
        let out = folders(&modules, false, &DependencyRules::default());
        let a = &out[1];
        assert_eq!(a["name"], "src/a");
        // Three module-level edges leave src/a, two of them to the same module: upstream counts
        // each edge, and lists each target folder once.
        assert_eq!(a["efferentCouplings"], 3);
        let names: Vec<&str> = js::array(a, "dependencies")
            .iter()
            .filter_map(|d| d["name"].as_str())
            .collect();
        assert_eq!(names, ["src/b", "src/c"]);
        let b = &out[2];
        assert_eq!(b["afferentCouplings"], 2);
        assert_eq!(b["instability"], json!(1.0 / 3.0));
        assert_eq!(
            a["dependencies"][0]["instability"],
            json!(1.0 / 3.0),
            "a folder dependency carries its target's instability"
        );
    }

    #[test]
    fn parents_are_every_prefix() {
        assert_eq!(parent_folders("a/b/c"), ["a", "a/b", "a/b/c"]);
        assert_eq!(parent_folders("."), ["."]);
    }
}
