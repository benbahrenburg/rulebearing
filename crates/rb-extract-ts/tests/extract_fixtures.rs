//! Conformance gate 1, layer 1: dependency-cruiser 18.2.0's `test/extract` cases, replayed
//! against the Rust extractor.
//!
//! - Plan: [Wave 0, Step 5](../../../docs/plans/pending/0000-wave-0-spike.md#step-5-conformance-gate-1-skeleton-0b)
//!   item 2 (the harness) and [Step 8](../../../docs/plans/pending/0000-wave-0-spike.md#step-8-spike-a-rb-extract-ts-0c)
//!   (the extractor it measures)
//! - Decision: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md),
//!   [ADR-0012](../../../docs/adr/0012-oxc-for-typescript.md)
//! - Requirements: [NFR-CONF-01](../../../docs/prd.md#nfr-conf-01), [FR-EXT-TS-01](../../../docs/prd.md#fr-ext-ts-01)
//! - Fixtures: `conformance/dependency-cruiser/fixtures/extract/`, recorded by
//!   `conformance/dependency-cruiser/harness/export-expectations.mjs` (see its header for how a
//!   case is defined and which upstream tests are outside layer 1, and why)
//!
//! Each recorded case names the dependency-cruiser function a passing upstream test called, the
//! input it passed and the value it returned. This test replays the input through the
//! corresponding Rust surface and compares the output as JSON, array order included, because the
//! upstream assertion is a `deepEqual`. It prints
//! `layer1: passed=<n> total=<t> ratio=<r>` and a timing line, writes the diff report to
//! `target/conformance/layer1.md`, and fails when the ratio is below
//! `conformance/dependency-cruiser/threshold.json`. With `RB_UPDATE_LAYER1_OPEN=1` it rewrites
//! `conformance/dependency-cruiser/layer1-open.json`, the list of failing cases with their class,
//! which is wave 1's worklist.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Deserialize;
use serde_json::Value;

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/dependency-cruiser")
}

fn fixtures() -> PathBuf {
    conformance().join("fixtures/extract")
}

#[derive(Deserialize)]
struct Index {
    pin: String,
    cases: usize,
    specs: Vec<SpecEntry>,
}

#[derive(Deserialize)]
struct SpecEntry {
    file: String,
}

/// One recorded call.
#[derive(Deserialize)]
struct Case {
    id: String,
    surface: String,
    cwd: String,
    input: Value,
    #[serde(default)]
    expected: Option<Value>,
    #[serde(default)]
    throws: Option<String>,
}

/// Where a failure sits, so the diff report ranks the work (plan 0000, Step 8 work order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    /// No Rust surface replays this kind of call yet.
    Surface,
    /// The dependency forms found differ.
    Walker,
    /// A specifier resolved differently.
    Resolver,
    /// The dependency types or npm classification differ.
    Classify,
    /// The output differs in a way none of the above explains.
    Expectation,
}

impl Class {
    fn name(self) -> &'static str {
        match self {
            Self::Surface => "surface",
            Self::Walker => "walker",
            Self::Resolver => "resolver",
            Self::Classify => "classify",
            Self::Expectation => "expectation",
        }
    }
}

struct Failure {
    class: Class,
    detail: String,
}

/// Replays one case through the Rust surface that corresponds to its dependency-cruiser function.
fn replay(root: &Path, case: &Case) -> Result<Value, Failure> {
    let _ = (root, &case.cwd, &case.input);
    Err(Failure {
        class: Class::Surface,
        detail: format!("no Rust replay for `{}` yet", case.surface),
    })
}

/// Classifies a mismatch by the first key whose value differs.
fn classify(actual: &Value, expected: &Value) -> Class {
    fn first_difference(a: &Value, e: &Value) -> Option<String> {
        match (a, e) {
            (Value::Object(a), Value::Object(e)) => e
                .iter()
                .find_map(|(key, ev)| match a.get(key) {
                    Some(av) if av == ev => None,
                    Some(av) => first_difference(av, ev).or_else(|| Some(key.clone())),
                    None => Some(key.clone()),
                })
                .or_else(|| a.keys().find(|k| !e.contains_key(*k)).cloned()),
            (Value::Array(a), Value::Array(e)) if a.len() == e.len() => a
                .iter()
                .zip(e)
                .find_map(|(av, ev)| (av != ev).then(|| first_difference(av, ev)).flatten()),
            (Value::Array(_), Value::Array(_)) => Some("module".to_owned()),
            _ => None,
        }
    }
    match first_difference(actual, expected).as_deref() {
        Some("module" | "moduleSystem" | "dynamic" | "exoticallyRequired" | "exoticRequire") => {
            Class::Walker
        }
        Some("resolved" | "couldNotResolve" | "coreModule" | "followable" | "source") => {
            Class::Resolver
        }
        Some("dependencyTypes" | "license" | "matchesDoNotFollow") => Class::Classify,
        _ => Class::Expectation,
    }
}

fn threshold() -> Result<f64, Box<dyn Error>> {
    let text = std::fs::read_to_string(conformance().join("threshold.json"))?;
    let value: Value = serde_json::from_str(&text)?;
    value["layer1"]
        .as_f64()
        .ok_or_else(|| "threshold.json has no numeric `layer1`".into())
}

/// Creates the symlink upstream's `get-dependencies.cjs` spec makes in its `before` hook.
fn prepare(root: &Path) {
    let mocks = root.join("test/extract/__mocks__");
    let link = mocks.join("symlinked");
    if link.symlink_metadata().is_ok() {
        return;
    }
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink("symlinkTarget", &link);
    #[cfg(windows)]
    let _ = std::os::windows::fs::symlink_dir(mocks.join("symlinkTarget"), &link);
}

#[test]
fn layer1_extract_fixtures() -> Result<(), Box<dyn Error>> {
    let root = fixtures();
    let index: Index = serde_json::from_str(&std::fs::read_to_string(root.join("INDEX.json"))?)?;
    prepare(&root);

    let mut cases = Vec::new();
    for spec in &index.specs {
        let text = std::fs::read_to_string(root.join(&spec.file))?;
        let recorded: Vec<Case> = serde_json::from_str(&text)?;
        cases.extend(recorded);
    }
    assert_eq!(
        cases.len(),
        index.cases,
        "INDEX.json and the expectation files disagree; re-run vendor.sh"
    );

    let started = Instant::now();
    let mut failures: Vec<(&Case, Failure)> = Vec::new();
    for case in &cases {
        let outcome = replay(&root, case);
        let failure = match (outcome, &case.expected, &case.throws) {
            (Ok(actual), Some(expected), _) if &actual == expected => None,
            (Ok(actual), Some(expected), _) => Some(Failure {
                class: classify(&actual, expected),
                detail: format!(
                    "expected {}\nactual   {}",
                    serde_json::to_string(expected)?,
                    serde_json::to_string(&actual)?
                ),
            }),
            (Ok(actual), None, _) => Some(Failure {
                class: Class::Expectation,
                detail: format!("expected an error, got {}", serde_json::to_string(&actual)?),
            }),
            (Err(failure), _, Some(_)) if failure.class != Class::Surface => None,
            (Err(failure), _, _) => Some(failure),
        };
        if let Some(failure) = failure {
            failures.push((case, failure));
        }
    }
    let elapsed = started.elapsed();

    let total = cases.len();
    let passed = total - failures.len();
    #[allow(clippy::cast_precision_loss)] // counts in the hundreds
    let ratio = if total == 0 {
        0.0
    } else {
        passed as f64 / total as f64
    };
    let mut by_class: BTreeMap<Class, usize> = BTreeMap::new();
    for (_, failure) in &failures {
        *by_class.entry(failure.class).or_default() += 1;
    }

    let mut report = format!(
        "# Layer 1: dependency-cruiser {} `test/extract`\n\npassed {passed} of {total}, ratio {ratio:.4}, replayed in {} ms\n\n| Class | Failing |\n| --- | --- |\n",
        index.pin,
        elapsed.as_millis()
    );
    for (class, count) in &by_class {
        writeln!(report, "| {} | {count} |", class.name())?;
    }
    for (case, failure) in &failures {
        write!(
            report,
            "\n## {} ({})\n\n```\n{}\n```\n",
            case.id,
            failure.class.name(),
            failure.detail
        )?;
    }
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/conformance");
    std::fs::create_dir_all(&out)?;
    std::fs::write(out.join("layer1.md"), report)?;

    if std::env::var_os("RB_UPDATE_LAYER1_OPEN").is_some() {
        let open: Vec<Value> = failures
            .iter()
            .map(|(case, failure)| {
                serde_json::json!({ "id": case.id, "surface": case.surface, "class": failure.class.name() })
            })
            .collect();
        std::fs::write(
            conformance().join("layer1-open.json"),
            format!("{}\n", serde_json::to_string_pretty(&open)?),
        )?;
    }

    println!("layer1: passed={passed} total={total} ratio={ratio:.4}");
    println!(
        "layer1: timing replay_ms={} cases={total}",
        elapsed.as_millis()
    );
    for (class, count) in &by_class {
        println!("layer1: failing class={} count={count}", class.name());
    }
    let floor = threshold()?;
    assert!(
        ratio >= floor,
        "layer 1 ratio {ratio:.4} is below the threshold {floor}; see target/conformance/layer1.md"
    );
    Ok(())
}

#[test]
fn mismatches_are_classified_by_the_first_differing_key() {
    let expected =
        serde_json::json!([{ "module": "./a", "resolved": "a.js", "dependencyTypes": ["local"] }]);
    let walker =
        serde_json::json!([{ "module": "./b", "resolved": "a.js", "dependencyTypes": ["local"] }]);
    let resolver =
        serde_json::json!([{ "module": "./a", "resolved": "b.js", "dependencyTypes": ["local"] }]);
    let classify_types =
        serde_json::json!([{ "module": "./a", "resolved": "a.js", "dependencyTypes": ["npm"] }]);
    let count = serde_json::json!([]);
    assert_eq!(classify(&walker, &expected), Class::Walker);
    assert_eq!(classify(&resolver, &expected), Class::Resolver);
    assert_eq!(classify(&classify_types, &expected), Class::Classify);
    assert_eq!(classify(&count, &expected), Class::Walker);
    assert_eq!(
        classify(&serde_json::json!(1), &serde_json::json!(2)),
        Class::Expectation
    );
}
