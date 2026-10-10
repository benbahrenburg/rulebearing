//! The project layer: modules aggregated into `projects[]` by the project each belongs to, with
//! the folder layer's couplings, instability, cycles and validation.
//!
//! - Plan: [Wave 2, 2D](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#wave-2d-cross-language-rule-additions-presets-vue-svelte-markdown-and-the-remaining-wave-2-option-rows)
//!   (the coverage tab's `metrics` row: "module, folder, and for .NET project instability")
//! - Decision: [ADR-0066](../../../docs/adr/0066-project-scope-and-the-project-layer.md)
//! - Coverage: [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   row `metrics`
//! - Requirement: [FR-RULE-08](../../../docs/prd.md#fr-rule-08) ("`moreUnstable` and `metrics`
//!   for modules, folders and .NET projects")
//!
//! A project is the `.csproj` or package an extractor records on a module (`Module::project`).
//! Each project's record is a folder record ([`crate::folders`]): a module's dependent or
//! dependency counts as a coupling when it belongs to another project, or to none (a package, a
//! core module), and is named by its project, or by its own `source` when it has none. Those
//! unowned names become sinks with `moduleCount` -1, as the folder layer's do. Projects come in
//! the order their first module is met; the modules are sorted, so the order is stable.

use std::collections::HashMap;

use rb_config::model::DependencyRules;
use serde_json::{Value, json};

use crate::derive::{cycles, metrics_are_calculable};
use crate::folders::{
    Aggregate, add_dependency_instability, add_validations, record, sinks, upsert,
};
use crate::js;
use crate::validate::validate_project;

/// Each coupled name once, in first-met order: a project is already its own level.
fn project_level(couplings: &[(String, usize)]) -> Vec<Value> {
    couplings
        .iter()
        .map(|(name, _)| json!({ "name": name }))
        .collect()
}

/// Each calculable module with a project counted into that project. Returns the project names
/// in first-met order with their aggregates.
fn aggregate(modules: &[Value]) -> (Vec<String>, HashMap<String, Aggregate>) {
    let project_of: HashMap<&str, &str> = modules
        .iter()
        .filter_map(|m| Some((js::str_of(m, "source")?, js::str_of(m, "project")?)))
        .collect();
    let owner = |name: &str| -> String {
        project_of
            .get(name)
            .map_or_else(|| name.to_owned(), |p| (*p).to_owned())
    };
    let mut order: Vec<String> = Vec::new();
    let mut all: HashMap<String, Aggregate> = HashMap::new();
    for module in modules.iter().filter(|m| metrics_are_calculable(m)) {
        let Some(project) = js::str_of(module, "project") else {
            continue;
        };
        let entry = all.entry(project.to_owned()).or_insert_with(|| {
            order.push(project.to_owned());
            Aggregate::default()
        });
        for dependent in js::strings(module, "dependents") {
            if project_of.get(dependent) != Some(&project) {
                upsert(&mut entry.dependents, &owner(dependent));
            }
        }
        for dependency in js::array(module, "dependencies") {
            let resolved = js::text(dependency, "resolved");
            if project_of.get(resolved.as_ref()) != Some(&project) {
                upsert(&mut entry.dependencies, &owner(&resolved));
            }
        }
        entry.module_count += 1;
        entry.add_stats(module);
    }
    (order, all)
}

/// The `projects[]` array: one record per project, then the sinks, with cycles and the
/// project-scoped rules' verdicts on each project dependency. Empty when no module has a project.
pub fn projects(modules: &[Value], skip: bool, rules: &DependencyRules) -> Vec<Value> {
    let (order, all) = aggregate(modules);
    let mut result: Vec<Value> = order
        .iter()
        .filter_map(|name| all.get(name).map(|a| record(name, a, project_level)))
        .collect();
    if result.is_empty() {
        return result;
    }
    let known = add_dependency_instability(&mut result);
    let sinks = sinks(&result, &known);
    result.extend(sinks);
    cycles(&mut result, "name", "name", skip, rules);
    add_validations(&mut result, rules, validate_project);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(value: &Value) -> DependencyRules {
        let map = value.as_object().cloned().unwrap_or_default();
        rb_config::normalize::rule_set(&map).unwrap_or_default()
    }

    /// Web (two files) depends on Core twice and on a package; Core depends on Web once, a
    /// cycle; Tools has no dependents; a module with no project is left out.
    fn modules() -> Vec<Value> {
        let edge = |to: &str| json!({ "resolved": to });
        vec![
            json!({ "source": "src/Core/Order.cs", "project": "Core", "dependencies": [edge("src/Web/Page.cs")],
                    "dependents": ["src/Web/Page.cs", "src/Web/Api.cs"] }),
            json!({ "source": "src/Tools/Gen.cs", "project": "Tools", "dependencies": [edge("src/Core/Order.cs")], "dependents": [] }),
            json!({ "source": "src/Web/Api.cs", "project": "Web", "dependencies": [edge("src/Core/Order.cs"), edge("Newtonsoft.Json")],
                    "dependents": [], "experimentalStats": { "size": 7, "topLevelStatementCount": 1 } }),
            json!({ "source": "src/Web/Page.cs", "project": "Web", "dependencies": [edge("src/Core/Order.cs"), edge("src/Web/Api.cs")],
                    "dependents": ["src/Core/Order.cs"] }),
            json!({ "source": "scripts/build.cs", "dependencies": [edge("src/Core/Order.cs")], "dependents": [] }),
            json!({ "source": "Newtonsoft.Json", "dependencies": [], "dependents": ["src/Web/Api.cs"], "coreModule": false,
                    "dependencyTypes": ["nuget"] }),
        ]
    }

    #[test]
    fn projects_aggregate_couplings_across_projects_only() {
        let out = projects(&modules(), false, &DependencyRules::default());
        let names: Vec<&str> = out.iter().filter_map(|p| p["name"].as_str()).collect();
        assert_eq!(names, ["Core", "Tools", "Web", "Newtonsoft.Json"]);
        let core = &out[0];
        assert_eq!(core["moduleCount"], 1);
        // Web's two modules depend on Core: two afferent couplings, one dependent project.
        assert_eq!(core["afferentCouplings"], 2);
        assert_eq!(core["efferentCouplings"], 1);
        assert_eq!(core["dependents"], json!([{ "name": "Web" }]));
        assert_eq!(core["instability"], json!(1.0 / 3.0));
        let web = &out[2];
        assert_eq!(web["moduleCount"], 2);
        // Page -> Api stays inside Web; Api and Page each reach Core, Api the package.
        assert_eq!(web["efferentCouplings"], 3);
        assert_eq!(web["afferentCouplings"], 1);
        let targets: Vec<&str> = js::array(web, "dependencies")
            .iter()
            .filter_map(|d| d["name"].as_str())
            .collect();
        assert_eq!(targets, ["Core", "Newtonsoft.Json"]);
        assert_eq!(web["dependencies"][0]["instability"], json!(1.0 / 3.0));
        assert_eq!(
            web["experimentalStats"],
            json!({ "size": 7, "topLevelStatementCount": 1 })
        );
        assert_eq!(out[1]["instability"], json!(1.0), "Tools only depends");
        assert_eq!(out[3]["moduleCount"], -1, "a package is a sink");
    }

    #[test]
    fn project_rules_see_cycles_and_instability() {
        let rules = rules(&json!({ "forbidden": [
            { "name": "no-project-cycles", "scope": "project", "severity": "error", "from": {}, "to": { "circular": true } },
            { "name": "stable-core", "scope": "project", "severity": "warn", "from": { "path": "^Core$" }, "to": { "moreUnstable": true } }
        ] }));
        let out = projects(&modules(), false, &rules);
        let core_to_web = &out[0]["dependencies"][0];
        assert_eq!(core_to_web["name"], "Web");
        assert_eq!(core_to_web["circular"], true);
        assert_eq!(core_to_web["valid"], false);
        let fired: Vec<&str> = js::array(core_to_web, "rules")
            .iter()
            .filter_map(|r| r["name"].as_str())
            .collect();
        assert_eq!(fired, ["no-project-cycles", "stable-core"]);
        let tools_to_core = &out[1]["dependencies"][0];
        assert_eq!(
            tools_to_core["valid"], true,
            "Tools is more unstable than Core"
        );
    }

    #[test]
    fn no_project_no_layer() {
        let modules = [json!({ "source": "a.ts", "dependencies": [], "dependents": [] })];
        assert!(projects(&modules, false, &DependencyRules::default()).is_empty());
    }
}
