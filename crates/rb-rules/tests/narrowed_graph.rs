//! A rule's `graph` over a fixture module graph: which violations each narrowing removes, the
//! chain each remaining one reports, and the liveness of `graph.ignore`.
//!
//! - Decision: [ADR-0038](../../../docs/adr/0038-a-rule-narrows-the-graph-it-sees.md)
//! - Plan: [Wave 2, Step 11](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f)
//!   (the import-linter filters the oracle harness found)
//! - Requirements: [FR-RULE-07](../../../docs/prd.md#fr-rule-07),
//!   [FR-RULE-08](../../../docs/prd.md#fr-rule-08)
//!
//! `tests/fixtures/narrowed/` holds the graph (the shape the Python extractor writes), the rules
//! (`rulebearing.yaml`, whose header describes the graph) and `expected.json`: every violation as
//! `[rule, from, to, via]` and every vacuous rule as `[rule, side]`.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use rb_config::load::{LoadOptions, load};
use rb_config::{CompatMode, Config};
use rb_model::GraphDocument;
use rb_rules::evaluate::{EvalOptions, Evaluation, evaluate};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/narrowed")
        .join(name)
}

fn document() -> Result<GraphDocument> {
    let value: Value = serde_json::from_str(&std::fs::read_to_string(fixture("graph.json"))?)?;
    Ok(GraphDocument {
        modules: serde_json::from_value(value["modules"].clone())?,
        ..GraphDocument::default()
    })
}

fn options() -> EvalOptions {
    EvalOptions {
        today: NaiveDate::from_ymd_opt(2026, 9, 24).unwrap_or_default(),
        ..EvalOptions::default()
    }
}

fn run(config: &Config) -> Result<Evaluation> {
    Ok(evaluate(document()?, config, &options())?)
}

/// The violations as `[rule, from, to, via]` and the vacuous rules as `[rule, side]`.
fn outcome(evaluation: &Evaluation) -> Value {
    let violations: Vec<Value> = evaluation
        .violations()
        .iter()
        .map(|v| {
            let via: Vec<&str> = v
                .via
                .iter()
                .flatten()
                .map(|step| step.name.as_str())
                .collect();
            json!([v.rule.name, v.from, v.to, via])
        })
        .collect();
    let vacuous: Vec<Value> = evaluation
        .vacuous
        .iter()
        .map(|v| json!([v.name, v.side]))
        .collect();
    json!({ "violations": violations, "vacuous": vacuous })
}

#[test]
fn each_narrowing_removes_what_it_names() -> Result<()> {
    let config = load(&fixture("rulebearing.yaml"), &LoadOptions::default())?;
    let evaluation = run(&config)?;
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(fixture("expected.json"))?)?;
    assert_eq!(
        outcome(&evaluation),
        expected,
        "{}",
        serde_json::to_string_pretty(&outcome(&evaluation))?
    );
    // Byte for byte the same on a second run.
    let again = run(&config)?;
    assert_eq!(
        serde_json::to_string(&again.document)?,
        serde_json::to_string(&evaluation.document)?
    );
    Ok(())
}

#[test]
fn liveness_off_reports_no_stale_ignore() -> Result<()> {
    let config = load(&fixture("rulebearing.yaml"), &LoadOptions::default())?;
    let evaluation = evaluate(
        document()?,
        &config,
        &EvalOptions {
            liveness: false,
            ..options()
        },
    )?;
    assert!(evaluation.vacuous.is_empty());
    Ok(())
}

/// One rule, from the canonical shape.
fn one(rule: &Value) -> Result<Config> {
    let mut map = serde_json::Map::new();
    map.insert("forbidden".into(), json!([rule]));
    Ok(rb_config::load::from_canonical(map, CompatMode::Native)?)
}

#[test]
fn a_rule_not_reached_through_the_narrowed_graph_is_reported() -> Result<()> {
    // `reachable: false`: c is reached from h over the whole graph, not once h's import is gone.
    let unreached = |graph: Option<Value>| -> Result<Vec<String>> {
        let mut rule = json!({
            "name": "c-reached-from-h",
            "severity": "error",
            "from": { "path": "^pkg/low/h\\.py$" },
            "to": { "path": "^pkg/high/c\\.py$", "reachable": false }
        });
        if let Some(graph) = graph {
            rule["graph"] = graph;
        }
        Ok(run(&one(&rule)?)?
            .violations()
            .iter()
            .map(|v| format!("{} -> {}", v.from, v.to))
            .collect())
    };
    assert!(unreached(None)?.is_empty());
    assert_eq!(
        unreached(Some(json!({ "ignore": [{ "from": "^pkg/low/h\\.py$" }] })))?,
        ["pkg/high/c.py -> pkg/high/c.py"]
    );
    Ok(())
}

#[test]
fn rules_with_different_graphs_see_different_graphs() -> Result<()> {
    let rules = json!([
        { "name": "cut-at-b", "severity": "error", "from": { "path": "^pkg/low/a\\.py$" }, "to": { "path": "^pkg/high/", "reachable": true },
          "graph": { "ignore": [{ "from": "^pkg/mid/b\\.py$" }] } },
        { "name": "cut-at-a", "severity": "error", "from": { "path": "^pkg/low/a\\.py$" }, "to": { "path": "^pkg/high/", "reachable": true },
          "graph": { "ignore": [{ "from": "^pkg/low/a\\.py$" }] } },
        { "name": "same-as-cut-at-b", "severity": "error", "from": { "path": "^pkg/low/a\\.py$" }, "to": { "path": "^pkg/high/", "reachable": true },
          "graph": { "ignore": [{ "from": "^pkg/mid/b\\.py$" }] } },
        { "name": "whole", "severity": "error", "from": { "path": "^pkg/low/a\\.py$" }, "to": { "path": "^pkg/high/", "reachable": true } }
    ]);
    let mut map = serde_json::Map::new();
    map.insert("forbidden".into(), rules);
    let config = rb_config::load::from_canonical(map, CompatMode::Native)?;
    let names: Vec<String> = run(&config)?
        .violations()
        .iter()
        .map(|v| v.rule.name.clone())
        .collect();
    assert_eq!(names, ["whole"]);
    Ok(())
}

#[test]
fn a_dependency_cruiser_file_refuses_graph() {
    let mut map = serde_json::Map::new();
    map.insert(
        "forbidden".into(),
        json!([{ "name": "r", "from": {}, "to": {}, "graph": { "modulesNot": "^x" } }]),
    );
    let error = rb_config::load::from_canonical(map, CompatMode::DependencyCruiser)
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(
        error.contains("`forbidden[0].graph` is a Rulebearing addition for native configurations"),
        "{error}"
    );
}

/// Two Python packages of `app` importing each other: `app/a/x.py` imports `app/b/y.py`, and
/// `app/b/y.py` imports `app/a/x.py` with the given dependency types.
fn cycle(back: &[&str]) -> Result<GraphDocument> {
    let edge = |to: &str, types: &[&str]| {
        json!({ "module": to, "moduleSystem": "py", "resolved": to, "dependencyTypes": types,
                "coreModule": false, "couldNotResolve": false, "followable": true, "dynamic": false,
                "exoticallyRequired": false, "circular": false, "valid": true })
    };
    let module = |source: &str, dotted: &str, dependencies: Vec<Value>| {
        json!({ "source": source, "dependencies": dependencies, "valid": true, "language": "python",
                "namespaces": [dotted] })
    };
    let modules = json!([
        module(
            "app/a/x.py",
            "app.a.x",
            vec![edge("app/b/y.py", &["local"])]
        ),
        module("app/b/y.py", "app.b.y", vec![edge("app/a/x.py", back)]),
    ]);
    Ok(GraphDocument {
        modules: serde_json::from_value(modules)?,
        ..GraphDocument::default()
    })
}

fn slices(document: GraphDocument, graph: Option<Value>) -> Result<Evaluation> {
    slices_allowing(document, graph, false)
}

fn slices_allowing(
    document: GraphDocument,
    graph: Option<Value>,
    allow_empty: bool,
) -> Result<Evaluation> {
    let mut rule = json!({ "name": "siblings", "severity": "error", "matching": "app.(*)", "segments": 1, "should": "beFreeOfCycles" });
    if let Some(graph) = graph {
        rule["graph"] = graph;
    }
    if allow_empty {
        rule["allowEmpty"] = json!(true);
    }
    let mut map = serde_json::Map::new();
    map.insert("slices".into(), json!([rule]));
    let config = rb_config::load::from_canonical(map, CompatMode::Native)?;
    Ok(evaluate(document, &config, &options())?)
}

#[test]
fn a_slice_rule_joins_slices_over_its_narrowed_graph() -> Result<()> {
    let count = |e: &Evaluation| e.violations().len();
    assert_eq!(
        count(&slices(cycle(&["local"])?, None)?),
        1,
        "the cycle a -> b -> a"
    );
    let ignored = slices(
        cycle(&["local"])?,
        Some(json!({ "ignore": [{ "from": "^app/b/y\\.py$", "to": "^app/a/x\\.py$" }] })),
    )?;
    assert_eq!(count(&ignored), 0);
    assert!(ignored.vacuous.is_empty());
    let typed = slices(
        cycle(&["local", "type-only"])?,
        Some(json!({ "dependencyTypesNot": ["type-only"] })),
    )?;
    assert_eq!(count(&typed), 0);
    let dropped = slices(cycle(&["local"])?, Some(json!({ "modulesNot": "^app/b/" })))?;
    assert_eq!(count(&dropped), 0);
    let stale = slices(
        cycle(&["local"])?,
        Some(json!({ "ignore": [{ "from": "^app/b/y\\.py$" }, { "to": "^app/gone\\.py$" }] })),
    )?;
    assert_eq!(count(&stale), 0);
    let vacuous: Vec<(&str, &str)> = stale
        .vacuous
        .iter()
        .map(|v| (v.name.as_str(), v.side.as_str()))
        .collect();
    assert_eq!(vacuous, [("siblings", "graph.ignore[1]")]);
    let allowed = slices_allowing(
        cycle(&["local"])?,
        Some(json!({ "ignore": [{ "to": "^app/gone\\.py$" }] })),
        true,
    )?;
    assert_eq!(count(&allowed), 1);
    assert!(allowed.vacuous.is_empty());
    Ok(())
}

#[test]
fn a_slice_rule_over_dotnet_types_refuses_graph() -> Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/archunitnet/graphs/TestAssembly.json");
    let value: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let document = GraphDocument {
        modules: serde_json::from_value(value["modules"].clone())?,
        code: serde_json::from_value(value["code"].clone())?,
        ..GraphDocument::default()
    };
    let mut map = serde_json::Map::new();
    map.insert(
        "slices".into(),
        json!([{ "name": "types", "matching": "TestAssembly.Slices.(*)", "should": "notDependOnEachOther",
                 "graph": { "modulesNot": "^x" } }]),
    );
    let config = rb_config::load::from_canonical(map, CompatMode::Native)?;
    let error = evaluate(document, &config, &options())
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(
        error.starts_with("rule `types`: `graph` narrows module imports, but the rule slices .NET types (\"TestAssembly.Slices."),
        "{error}"
    );
    Ok(())
}
