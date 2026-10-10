//! The playground's `evaluate` against the command line's own output, and its refusals.
//!
//! - Plan: [Wave 4, Step 2](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables)
//!   ("comparing `evaluate()` output to the committed CLI JSON")
//! - Requirement: [FR-REACH-01](../../../docs/prd.md#fr-reach-01)
//!
//! The guard catalogue's fixtures are a configuration, a graph and the CLI's `cruise --graph -T
//! json` output for them, which the `guards` job byte-compares on every pull request
//! ([cookbook/README.md](../../../cookbook/README.md)). The playground over the same two inputs
//! must find the same violations and the same vacuous rules.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use rb_wasm::playground::{Now, PlaygroundError, evaluate, syntax_of, version};
use serde_json::Value;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn now() -> Now {
    Now {
        today: NaiveDate::from_ymd_opt(2026, 10, 10).unwrap_or_default(),
        timestamp: "2026-10-10T12:00:00.000Z".to_owned(),
    }
}

fn guard(slug: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cookbook/guards")
        .join(slug)
}

fn read(path: &Path) -> Result<String> {
    Ok(std::fs::read_to_string(path)?)
}

#[test]
fn evaluate_finds_what_the_command_line_found() -> Result {
    let dir = guard("no-circular");
    let output = evaluate(
        &read(&dir.join("rulebearing.yaml"))?,
        &read(&dir.join("graph.json"))?,
        "json",
        &now(),
    )?;
    let result: Value = serde_json::from_str(&output)?;
    let expected: Value = serde_json::from_str(&read(&dir.join("expected.json"))?)?;
    assert_eq!(result["summary"]["violations"], expected["violations"]);
    let vacuous = result["summary"]
        .get("vacuousRules")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    assert_eq!(vacuous, expected["vacuousRules"]);
    assert!(
        result["summary"].get("inspected").is_none(),
        "a pasted graph has no extraction receipt"
    );
    Ok(())
}

#[test]
fn every_text_reporter_renders_the_same_run() -> Result {
    let dir = guard("no-circular");
    let (config, graph) = (
        read(&dir.join("rulebearing.yaml"))?,
        read(&dir.join("graph.json"))?,
    );
    let err = evaluate(&config, &graph, "err", &now())?;
    assert!(
        err.contains("no-circular") && err.contains("src/order/discount.ts"),
        "{err}"
    );
    let agent: Value = serde_json::from_str(&evaluate(&config, &graph, "agent", &now())?)?;
    assert_eq!(agent["rules"][0]["name"], "no-circular");
    assert_eq!(agent["rules"][0]["count"], 1);
    Ok(())
}

#[test]
fn a_json_configuration_loads_as_yaml_does() -> Result {
    let dir = guard("no-circular");
    let graph = read(&dir.join("graph.json"))?;
    let config = r#"{ "forbidden": [{ "name": "no-circular", "severity": "error", "from": {}, "to": { "circular": true } }] }"#;
    let result: Value = serde_json::from_str(&evaluate(config, &graph, "json", &now())?)?;
    let rules: Vec<&str> = result["summary"]["violations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["rule"]["name"].as_str())
        .collect();
    // Without `viaOnly`, the type-only cycle counts too: two violations.
    assert_eq!(rules, ["no-circular", "no-circular"]);
    Ok(())
}

/// The playground's wasm build has no JavaScript engine (QuickJS does not link for
/// `wasm32-unknown-unknown`), so a JavaScript configuration is refused there with the way out;
/// a native build that carries the engine evaluates it. `cargo test -p rb-wasm` is the first,
/// a workspace-wide run the second.
#[test]
fn a_javascript_configuration_needs_the_engine() -> Result {
    let dir = guard("no-circular");
    let graph = read(&dir.join("graph.json"))?;
    let config = r#"module.exports = { forbidden: [{ name: "no-circular", severity: "error", from: {}, to: { circular: true } }] };"#;
    let result = evaluate(config, &graph, "json", &now());
    if rb_config::js::JS_ENGINE {
        let value: Value = serde_json::from_str(&result?)?;
        assert_eq!(
            value["summary"]["violations"][0]["rule"]["name"],
            "no-circular"
        );
    } else {
        let refused = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            refused.contains("this build has no JavaScript engine")
                && refused.contains("write it as YAML or JSON"),
            "{refused}"
        );
    }
    Ok(())
}

#[test]
fn the_configuration_s_filters_and_dates_apply() -> Result {
    let dir = guard("no-circular");
    let graph = read(&dir.join("graph.json"))?;
    let config = r#"options:
  highlight: { path: "^src/user/" }
rules:
  dependencies:
    forbidden:
      - name: no-circular
        severity: error
        from: {}
        to: { circular: true }
      - name: stale
        severity: error
        expires: 2026-01-01
        from: { path: "^nothing-matches$" }
        to: {}
"#;
    let result: Value = serde_json::from_str(&evaluate(config, &graph, "json", &now())?)?;
    let highlighted: Vec<&str> = result["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["matchesHighlight"] == true)
        .filter_map(|m| m["source"].as_str())
        .collect();
    assert_eq!(highlighted, ["src/user/profile.ts", "src/user/user.ts"]);
    assert_eq!(
        result["summary"]["expired"][0]["name"], "stale",
        "an expired rule is reported against the page's date"
    );
    let vacuous: Vec<&str> = result["summary"]["vacuousRules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["name"].as_str())
        .collect();
    assert_eq!(
        vacuous,
        ["stale"],
        "the report's filters keep the engine's vacuous rules"
    );
    Ok(())
}

#[test]
fn what_cannot_be_answered_is_named() -> Result {
    let dir = guard("no-circular");
    let (config, graph) = (
        read(&dir.join("rulebearing.yaml"))?,
        read(&dir.join("graph.json"))?,
    );
    let refused = |result: std::result::Result<String, PlaygroundError>| {
        result.err().map(|e| e.to_string()).unwrap_or_default()
    };
    let unknown = refused(evaluate(&config, &graph, "nope", &now()));
    assert!(
        unknown.starts_with("no output type `nope`; the output types are ")
            && unknown.contains("err-long"),
        "{unknown}"
    );
    let not_a_graph = refused(evaluate(&config, "{ \"modules\": 3 }", "json", &now()));
    assert!(
        not_a_graph.starts_with("the graph is not a cruise result: "),
        "{not_a_graph}"
    );
    let extends = refused(evaluate("extends: ./base.yaml\n", &graph, "json", &now()));
    assert!(
        extends.starts_with("the configuration did not load: "),
        "{extends}"
    );
    let required = refused(evaluate(
        "const base = require('./base.js');\nmodule.exports = base;\n",
        &graph,
        "json",
        &now(),
    ));
    assert!(
        required.starts_with("the configuration did not load: "),
        "a pasted configuration reads no file, with or without the engine: {required}"
    );
    assert!(!std::path::Path::new(rb_wasm::playground::ROOT).exists());
    Ok(())
}

#[test]
fn syntax_comes_from_the_text() {
    use rb_config::read::Syntax;
    assert_eq!(syntax_of("module.exports = {};"), Syntax::JavaScript);
    assert_eq!(syntax_of("export default {};"), Syntax::JavaScript);
    assert_eq!(syntax_of("  { \"forbidden\": [] }"), Syntax::Json);
    assert_eq!(syntax_of("forbidden: []\n"), Syntax::Yaml);
}

#[test]
fn the_version_is_the_workspace_s() {
    assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    assert!(!version().is_empty());
}
