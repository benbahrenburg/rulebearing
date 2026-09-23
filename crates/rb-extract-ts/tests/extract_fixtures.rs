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

use oxc_span::SourceType;
use rb_extract_ts::pipeline::{self, Extracted, ExtractedModule, Settings};
use rb_extract_ts::resolve::{self, Context, ResolveConfig};
use rb_extract_ts::walk::{self, Flavour, Found, WalkOptions};
use rb_model::{DependencyType, ModuleSystem, TypeScriptOptions};
use serde::Deserialize;
use serde_json::Value;

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/dependency-cruiser")
}

fn fixtures() -> PathBuf {
    let path = conformance().join("fixtures/extract");
    // Canonical, so paths compared as strings (an absolute `modules` folder) agree; without the
    // Windows verbatim prefix `canonicalize` adds, which no other path in a run carries.
    rb_extract_ts::resolve::simplified(&path.canonicalize().unwrap_or(path))
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
    /// The source did not parse.
    Parser,
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
            Self::Parser => "parser",
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

/// Replaces the recorder's `<root>` token with the vendored fixture root, everywhere in a value.
fn rooted(value: &Value, root: &Path) -> Value {
    match value {
        Value::String(text) => Value::String(text.replace("<root>", &root.to_string_lossy())),
        Value::Array(items) => Value::Array(items.iter().map(|v| rooted(v, root)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), rooted(v, root)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn surface_error(detail: impl std::fmt::Display) -> Failure {
    Failure {
        class: Class::Expectation,
        detail: detail.to_string(),
    }
}

/// The keys of dependency-cruiser's cruise options that `TypeScriptOptions` models; the rest
/// (`ruleSet`, `validate`, reporter options) do not reach extraction.
const EXTRACTION_KEYS: &[&str] = &[
    "baseDir",
    "tsConfig",
    "tsPreCompilationDeps",
    "babelConfig",
    "webpackConfig",
    "enhancedResolveOptions",
    "moduleSystems",
    "parser",
    "exoticRequireStrings",
    "detectJSDocImports",
    "detectProcessBuiltinModuleCalls",
    "preserveSymlinks",
    "combinedDependencies",
    "externalModuleResolutionStrategy",
    "builtInModules",
    "extraExtensionsToScan",
    "doNotFollow",
    "exclude",
    "includeOnly",
    "maxDepth",
    "experimentalStats",
];

fn typescript_options(cruise: &Value) -> Result<TypeScriptOptions, Failure> {
    let mut kept = serde_json::Map::new();
    for (key, value) in cruise.as_object().into_iter().flatten() {
        if EXTRACTION_KEYS.contains(&key.as_str()) && !value.is_null() {
            kept.insert(key.clone(), value.clone());
        }
    }
    serde_json::from_value(Value::Object(kept)).map_err(|e| surface_error(format!("options: {e}")))
}

fn settings(cruise: &Value, cwd: &Path) -> Result<Settings, Failure> {
    let options = typescript_options(cruise)?;
    Settings::new(&options, cwd).map_err(surface_error)
}

/// The resolver settings a recorded `normalizeResolveOptions` call and transpile options describe.
fn resolve_config(
    resolve_options: &Value,
    transpile: &Value,
    cruise_fallback: &Value,
) -> Result<ResolveConfig, Failure> {
    let cruise = resolve_options
        .get("cruise")
        .filter(|c| !c.is_null())
        .unwrap_or(cruise_fallback);
    let mut config = rb_extract_ts::resolve_config(&typescript_options(cruise)?);
    if let Some(raw) = resolve_options.get("resolve").and_then(Value::as_object) {
        let list = |key: &str| raw.get(key).map(strings);
        if let Some(v) = list("extensions") {
            config.extensions = v;
        }
        if let Some(v) = list("modules") {
            config.modules = v;
        }
        if let Some(v) = list("exportsFields") {
            config.exports_fields = v;
        }
        if let Some(v) = list("conditionNames") {
            config.condition_names = v;
        }
        if let Some(v) = list("mainFields") {
            config.main_fields = v;
        }
        if let Some(v) = list("mainFiles") {
            config.main_files = v;
        }
        if let Some(v) = list("aliasFields") {
            config.alias_fields = v;
        }
        if let Some(alias) = raw.get("alias").and_then(Value::as_object) {
            config.alias = alias
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned())))
                .collect();
        }
        if let Some(symlinks) = raw.get("symlinks").and_then(Value::as_bool) {
            config.symlinks = symlinks;
        }
        if let Some(tsconfig) = raw.get("tsConfig").and_then(Value::as_str) {
            config.tsconfig = Some(PathBuf::from(tsconfig));
        }
        if let Some(builtins) = raw.get("builtInModules") {
            config.built_in_modules = serde_json::from_value(builtins.clone())
                .map_err(|e| surface_error(format!("builtInModules: {e}")))?;
        }
        if let Some(combined) = raw.get("combinedDependencies").and_then(Value::as_bool) {
            config.combined_dependencies = combined;
        }
        config.resolve_licenses = raw
            .get("resolveLicenses")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        config.resolve_deprecations = raw
            .get("resolveDeprecations")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    }
    let tsconfig_options = transpile
        .get("tsConfig")
        .or_else(|| resolve_options.get("tsConfig"))
        .and_then(|t| t.get("options"));
    if let Some(options) = tsconfig_options {
        config.tsconfig_base_url = options
            .get("baseUrl")
            .and_then(Value::as_str)
            .map(str::to_owned);
        config.tsconfig_paths = options
            .get("paths")
            .and_then(Value::as_object)
            .map(|p| p.keys().cloned().collect())
            .unwrap_or_default();
    }
    Ok(config)
}

fn found_json(found: &Found) -> Value {
    let mut object = serde_json::json!({
        "module": found.module,
        "moduleSystem": found.module_system.as_str(),
        "dynamic": found.dynamic,
        "exoticallyRequired": found.exotically_required,
        "dependencyTypes": found.dependency_types.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
    });
    if let Some(name) = &found.exotic_require {
        object["exoticRequire"] = Value::from(name.as_str());
    }
    object
}

fn extracted_json(d: &Extracted) -> Value {
    let mut object = serde_json::json!({
        "module": d.module,
        "moduleSystem": d.module_system.as_str(),
        "dynamic": d.dynamic,
        "exoticallyRequired": d.exotically_required,
        "dependencyTypes": d.dependency_types.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
        "resolved": d.resolved,
        "coreModule": d.core_module,
        "followable": d.followable,
        "couldNotResolve": d.could_not_resolve,
        "matchesDoNotFollow": d.matches_do_not_follow,
    });
    if let Some(name) = &d.exotic_require {
        object["exoticRequire"] = Value::from(name.as_str());
    }
    if let Some(protocol) = d.protocol {
        object["protocol"] = Value::from(protocol.as_str());
    }
    if let Some(mime) = &d.mime_type {
        object["mimeType"] = Value::from(mime.as_str());
    }
    if let Some(only) = d.pre_compilation_only {
        object["preCompilationOnly"] = Value::from(only);
    }
    if let Some(license) = &d.license {
        object["license"] = Value::from(license.as_str());
    }
    object
}

fn module_json(module: &ExtractedModule) -> Value {
    if let Some(d) = &module.as_dependency {
        return serde_json::json!({
            "source": module.source,
            "followable": d.followable,
            "coreModule": d.core_module,
            "couldNotResolve": d.could_not_resolve,
            "matchesDoNotFollow": d.matches_do_not_follow,
            "dependencyTypes": d.dependency_types.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
            "dependencies": [],
        });
    }
    let mut object = serde_json::json!({
        "source": module.source,
        "dependencies": module.dependencies.iter().map(extracted_json).collect::<Vec<_>>(),
    });
    if let Some(stats) = module.experimental_stats {
        object["experimentalStats"] = serde_json::json!({"topLevelStatementCount": stats.top_level_statement_count, "size": stats.size});
    }
    object
}

fn walk_case(
    input: &Value,
    flavour: Flavour,
    module_systems: Vec<ModuleSystem>,
    source_type: SourceType,
) -> Result<Value, Failure> {
    let source = input
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| surface_error("no source"))?;
    let options = WalkOptions {
        module_systems,
        exotic_require_strings: strings(input.get("exoticRequireStrings").unwrap_or(&Value::Null)),
        detect_jsdoc_imports: input
            .get("detectJSDocImports")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        detect_process_builtin_module_calls: input
            .get("detectProcessBuiltinModuleCalls")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };
    let found = walk::walk_source(source, source_type, flavour, &options).map_err(|e| Failure {
        class: Class::Parser,
        detail: e.to_string(),
    })?;
    Ok(Value::Array(found.iter().map(found_json).collect()))
}

fn replay_resolve(root: &Path, cwd: &Path, input: &Value) -> Result<Value, Failure> {
    let null = Value::Null;
    let field = |key: &str| input.get(key).unwrap_or(&null);
    let text = |key: &str| field(key).as_str().unwrap_or_default().to_owned();
    let cwd = cwd.to_path_buf();
    let config = resolve_config(field("resolveOptions"), field("transpileOptions"), &null)?;
    let module = field("module");
    let form_types: Vec<DependencyType> = strings(module.get("dependencyTypes").unwrap_or(&null))
        .iter()
        .filter_map(|t| t.parse().ok())
        .collect();
    let system: ModuleSystem = module
        .get("moduleSystem")
        .and_then(Value::as_str)
        .unwrap_or("cjs")
        .parse()
        .map_err(surface_error)?;
    let base_dir = root.join(text("baseDir"));
    let file_dir = root.join(text("fileDir"));
    let context = Context {
        cwd: &cwd,
        base_dir: &base_dir,
        file_dir: &file_dir,
    };
    let r = resolve::resolve(
        module
            .get("module")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        system,
        &form_types,
        &context,
        &config,
    );
    let mut object = serde_json::json!({
        "resolved": r.resolved,
        "coreModule": r.core_module,
        "followable": r.followable,
        "couldNotResolve": r.could_not_resolve,
        "dependencyTypes": r.dependency_types.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
    });
    if let Some(license) = r.license {
        object["license"] = Value::from(license);
    }
    Ok(object)
}

fn replay_determine(root: &Path, cwd: &Path, input: &Value) -> Result<Value, Failure> {
    let null = Value::Null;
    let field = |key: &str| input.get(key).unwrap_or(&null);
    let text = |key: &str| field(key).as_str().unwrap_or_default().to_owned();
    let cwd = cwd.to_path_buf();
    let config = if field("resolveOptions").is_null() {
        ResolveConfig::default()
    } else {
        resolve_config(field("resolveOptions"), field("transpileOptions"), &null)?
    };
    let dependency = field("dependency");
    let resolution = resolve::Resolution {
        resolved: dependency
            .get("resolved")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        core_module: dependency
            .get("coreModule")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        followable: false,
        could_not_resolve: dependency
            .get("couldNotResolve")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        dependency_types: Vec::new(),
        license: None,
    };
    let manifest: Option<rb_extract_ts::npm::Manifest> = if field("manifest").is_object() {
        serde_json::from_value(field("manifest").clone()).ok()
    } else {
        None
    };
    let base_dir = if field("baseDir").is_null() {
        root.to_path_buf()
    } else {
        root.join(text("baseDir"))
    };
    let file_dir = if field("fileDir").is_null() {
        cwd.clone()
    } else {
        root.join(text("fileDir"))
    };
    let context = Context {
        cwd: &cwd,
        base_dir: &base_dir,
        file_dir: &file_dir,
    };
    let mut types = resolve::dependency_types(
        &resolution,
        &text("moduleName"),
        manifest.as_ref(),
        &context,
        &config,
    );
    types.extend(
        strings(dependency.get("dependencyTypes").unwrap_or(&null))
            .iter()
            .filter_map(|t| t.parse::<DependencyType>().ok()),
    );
    Ok(Value::Array(
        types.iter().map(|t| Value::from(t.as_str())).collect(),
    ))
}

/// Replays one case through the Rust surface that corresponds to its dependency-cruiser function.
fn replay(root: &Path, case: &Case) -> Result<Value, Failure> {
    let input = rooted(&case.input, root);
    let cwd = root.join(&case.cwd);
    let null = Value::Null;
    let field = |key: &str| input.get(key).unwrap_or(&null);
    let text = |key: &str| field(key).as_str().unwrap_or_default().to_owned();
    let pipeline_error = |e: rb_extract_ts::pipeline::PipelineError| Failure {
        class: Class::Expectation,
        detail: e.to_string(),
    };
    match case.surface.as_str() {
        "walk-tsc" => walk_case(&input, Flavour::Tsc, Vec::new(), SourceType::ts()),
        "walk-swc" => walk_case(&input, Flavour::Swc, Vec::new(), SourceType::ts()),
        "walk-acorn-cjs" => walk_case(
            &input,
            Flavour::Acorn,
            vec![ModuleSystem::Cjs],
            SourceType::mjs().with_jsx(true),
        ),
        "walk-acorn-es6" => walk_case(
            &input,
            Flavour::Acorn,
            vec![ModuleSystem::Es6],
            SourceType::mjs().with_jsx(true),
        ),
        "walk-acorn-amd" => walk_case(
            &input,
            Flavour::Acorn,
            vec![ModuleSystem::Amd],
            SourceType::mjs().with_jsx(true),
        ),
        "extract-dependencies" => {
            let settings = settings(field("cruiseOptions"), &cwd)?;
            let config = resolve_config(
                field("resolveOptions"),
                field("transpileOptions"),
                field("cruiseOptions"),
            )?;
            let deps = pipeline::extract_dependencies(&text("fileName"), &settings, &config)
                .map_err(pipeline_error)?;
            Ok(Value::Array(deps.iter().map(extracted_json).collect()))
        }
        "resolve" => replay_resolve(root, &cwd, &input),
        "determine-dependency-types" => replay_determine(root, &cwd, &input),
        "extract" => {
            let settings = settings(field("cruiseOptions"), &cwd)?;
            let config = resolve_config(
                field("resolveOptions"),
                &serde_json::json!({"tsConfig": field("tsConfig")}),
                field("cruiseOptions"),
            )?;
            let modules = pipeline::extract(&strings(field("files")), &settings, &config)
                .map_err(pipeline_error)?;
            Ok(Value::Array(modules.iter().map(module_json).collect()))
        }
        "gather-initial-sources" => {
            let settings = settings(field("cruiseOptions"), &cwd)?;
            let files = pipeline::gather_initial_sources(&strings(field("files")), &settings)
                .map_err(pipeline_error)?;
            Ok(Value::Array(files.into_iter().map(Value::from).collect()))
        }
        "extract-stats" | "stats-acorn" | "stats-tsc" => {
            let settings = settings(field("cruiseOptions"), &cwd)?;
            let stats = pipeline::stats(&text("fileName"), &settings).map_err(pipeline_error)?;
            Ok(
                serde_json::json!({"topLevelStatementCount": stats.top_level_statement_count, "size": stats.size}),
            )
        }
        other => Err(Failure {
            class: Class::Surface,
            detail: format!("no Rust replay for `{other}`"),
        }),
    }
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

/// Upstream's cache-busting spec renames `cache-busting-first-tree` (call 1) or
/// `cache-busting-second-tree` (call 2) to `cache-busting` around each `extract` call, to prove
/// the resolver cache does not outlive a run. The guard does the same rename and undoes it when
/// dropped, a failing assertion included.
struct CacheBustingTree {
    from: PathBuf,
    to: PathBuf,
}

impl CacheBustingTree {
    const SPEC: &'static str = "test/extract/index.cachebusting.spec.mjs#";

    fn for_case(root: &Path, id: &str) -> Option<Self> {
        let call = id.strip_prefix(Self::SPEC)?;
        let tree = match call {
            "1" => "first",
            "2" => "second",
            _ => return None,
        };
        let mocks = root.join("test/extract/__mocks__");
        let guard = Self {
            from: mocks.join(format!("cache-busting-{tree}-tree")),
            to: mocks.join("cache-busting"),
        };
        std::fs::rename(&guard.from, &guard.to).ok()?;
        Some(guard)
    }
}

impl Drop for CacheBustingTree {
    fn drop(&mut self) {
        let _ = std::fs::rename(&self.to, &self.from);
    }
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
        let tree = CacheBustingTree::for_case(&root, &case.id);
        let outcome = replay(&root, case);
        drop(tree);
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
