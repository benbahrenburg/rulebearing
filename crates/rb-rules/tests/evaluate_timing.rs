//! The engine's share of a `guard --watch` re-check, timed on a real graph: `evaluate` alone,
//! without extraction or reporting.
//!
//! - Requirement: [NFR-PERF-03](../../../docs/prd.md#nfr-perf-03) (a saved file re-checked in
//!   under 100 ms on the 5,500-module synthetic tree)
//! - Plan: [Wave 3, Step 16](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)
//! - Benchmark: [testbeds/synth](../../../testbeds/synth/gen.mjs) generates the tree,
//!   [docs/perf.md](../../../docs/perf.md) records the numbers
//!
//! Ignored by default, because it needs a generated tree. Generate one, cruise it to JSON and
//! point the test at both files:
//!
//! ```sh
//! node testbeds/synth/gen.mjs /tmp/synth
//! cp testbeds/synth/dependency-cruiser.cjs /tmp/synth/.dependency-cruiser.cjs
//! (cd /tmp/synth && rulebearing cruise --config .dependency-cruiser.cjs -T json apps packages > graph.json)
//! RB_EVAL_GRAPH=/tmp/synth/graph.json RB_EVAL_CONFIG=/tmp/synth/.dependency-cruiser.cjs \
//!   cargo test --release -p rb-rules --test evaluate_timing -- --ignored --nocapture
//! ```
//!
//! `RB_EVAL_METRICS=1` turns metrics on (instability and folders). The derived fields of the cruised graph are stripped first, so the engine sees what the
//! extractor hands it. With `RB_EVAL_OUT` set, the evaluation (document, statistics, vacuous and
//! expired rules) is written there as JSON, so two builds of the engine can be compared byte for
//! byte.

use std::path::Path;
use std::time::Instant;

use rb_config::load::{LoadOptions, load};
use rb_model::GraphDocument;
use rb_rules::evaluate::{EvalOptions, evaluate};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// What the engine adds to a module and to a dependency, beyond `valid` and `circular`, which
/// the extractor writes as `true` and `false`.
const MODULE_DERIVED: [&str; 6] = [
    "dependents",
    "orphan",
    "rules",
    "reachable",
    "reaches",
    "instability",
];
const DEPENDENCY_DERIVED: [&str; 3] = ["cycle", "rules", "instability"];

fn extracted(path: &Path) -> Result<GraphDocument> {
    let mut value: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let mut modules = value
        .get_mut("modules")
        .map_or_else(|| json!([]), Value::take);
    for module in modules.as_array_mut().into_iter().flatten() {
        if let Some(map) = module.as_object_mut() {
            for key in MODULE_DERIVED {
                map.remove(key);
            }
            map.insert("valid".into(), Value::Bool(true));
        }
        let dependencies = module.get_mut("dependencies").and_then(Value::as_array_mut);
        for dependency in dependencies.into_iter().flatten() {
            if let Some(map) = dependency.as_object_mut() {
                for key in DEPENDENCY_DERIVED {
                    map.remove(key);
                }
                map.insert("valid".into(), Value::Bool(true));
                map.insert("circular".into(), Value::Bool(false));
            }
        }
    }
    Ok(GraphDocument {
        modules: serde_json::from_value(modules)?,
        ..GraphDocument::default()
    })
}

#[test]
#[ignore = "needs a generated synthetic tree; see the module documentation"]
fn evaluate_on_the_synthetic_tree() -> Result<()> {
    let (Ok(graph), Ok(config)) = (
        std::env::var("RB_EVAL_GRAPH"),
        std::env::var("RB_EVAL_CONFIG"),
    ) else {
        return Err("set RB_EVAL_GRAPH and RB_EVAL_CONFIG".into());
    };
    let document = extracted(Path::new(&graph))?;
    let config = load(Path::new(&config), &LoadOptions::default())?;
    let options = EvalOptions {
        metrics: std::env::var_os("RB_EVAL_METRICS").is_some(),
        ..EvalOptions::default()
    };
    let runs: usize = std::env::var("RB_EVAL_RUNS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(15);
    let mut times = Vec::with_capacity(runs);
    let mut last = None;
    for _ in 0..runs {
        let input = document.clone();
        let start = Instant::now();
        let evaluation = evaluate(input, &config, &options)?;
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        last = Some(evaluation);
    }
    times.sort_by(f64::total_cmp);
    let median = times.get(times.len() / 2).copied().unwrap_or_default();
    let fastest = times.first().copied().unwrap_or_default();
    println!(
        "evaluate: {} modules, median {median:.1} ms, fastest {fastest:.1} ms over {runs} runs",
        document.modules.len()
    );
    if let (Ok(out), Some(evaluation)) = (std::env::var("RB_EVAL_OUT"), last) {
        let record = json!({
            "document": evaluation.document,
            "vacuous": evaluation.vacuous,
            "ruleStats": evaluation.rule_stats,
            "expired": evaluation.expired,
            "unmatchedKnown": evaluation.unmatched_known,
        });
        std::fs::write(out, serde_json::to_vec_pretty(&record)?)?;
    }
    Ok(())
}
