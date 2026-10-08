//! One fixture per Wave 1 option row: a mini-repository under `tests/options/<option>/`, extracted
//! through the same entry points `TypeScriptExtractor::extract` uses, and the modules and
//! dependencies it yields asserted.
//!
//! - Plan: [Wave 1, Step 10](../../../docs/plans/implemented/0001-wave-1-typescript-parity.md#step-10-rb-extract-ts-to-100-and-the-option-set-1c)
//!   ("every new option gets a fixture under `rb-extract-ts/tests/options/`") and
//!   [Wave 1C](../../../docs/plans/implemented/0001-wave-1-typescript-parity.md#wave-1c-rb-extract-ts-completion)
//! - Source: [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   the Wave 1 rows; [coverage § Extraction and resolution](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution)
//! - Decisions: [ADR-0012](../../../docs/adr/0012-oxc-for-typescript.md),
//!   [ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md) (the sidecar failure)
//! - Requirements: [FR-EXT-TS-02](../../../docs/prd.md#fr-ext-ts-02), [FR-EXT-TS-03](../../../docs/prd.md#fr-ext-ts-03)
//!
//! Each fixture is extracted with its own folder as the working directory, passed to
//! [`rb_extract_ts::prepare`] rather than set on the process, so the tests run in parallel.
//!
//! Every extraction through [`run`], [`run_in`] and [`run_native`] is repeated incrementally
//! ([Wave 3, Step 2](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)):
//! every file unchanged, every file changed, and each file changed on its own must give the same
//! extraction byte for byte, so each option fixture is also a fixture of incremental extraction.

use std::path::{Path, PathBuf};

use rb_extract_ts::pipeline::Settings;
use rb_extract_ts::resolve::ResolveConfig;
use rb_extract_ts::{TypeScriptExtractor, extract_incremental, extract_with, prepare};
use rb_model::{ExtractError, ExtractRequest, Extraction, Extractor, Module, TypeScriptOptions};

/// Extracts in full with `setup`'s settings, then asserts every incremental variant over the
/// files read gives the same extraction, and returns the full one.
fn with_incremental(
    setup: &dyn Fn() -> Result<(Settings, ResolveConfig), ExtractError>,
    roots: &[PathBuf],
) -> Result<Extraction, ExtractError> {
    let (settings, config) = setup()?;
    let plain = extract_with(roots, &settings, &config)?;
    let (mut settings, config) = setup()?;
    settings.keep_file_states = true;
    let full = extract_with(roots, &settings, &config)?;
    let without_states = |e: &Extraction| {
        let mut e = e.clone();
        e.files.clear();
        serde_json::to_string(&e).unwrap_or_default()
    };
    let expected = without_states(&full);
    assert_eq!(
        expected,
        without_states(&plain),
        "keeping the file states changes the extraction"
    );
    if settings.code_layer {
        assert_eq!(
            full.files.len(),
            full.modules.iter().filter(|m| m.language.is_some()).count(),
            "every file read keeps its state"
        );
    }
    let read: Vec<PathBuf> = full
        .modules
        .iter()
        .filter(|m| m.language.is_some())
        .map(|m| PathBuf::from(&m.source))
        .collect();
    let mut variants = vec![(Vec::new(), read.clone()), (read.clone(), Vec::new())];
    for (index, file) in read.iter().enumerate() {
        let mut others = read.clone();
        others.remove(index);
        variants.push((vec![file.clone()], others));
    }
    for (changed, unchanged) in variants {
        let (settings, config) = setup()?;
        let request = ExtractRequest {
            changed: changed.clone(),
            unchanged,
            previous: full.clone(),
            walk_unchanged: false,
        };
        let again = extract_incremental(roots, &settings, &config, &request)?;
        assert_eq!(
            without_states(&again),
            expected,
            "incremental extraction with {changed:?} changed differs from the full one"
        );
    }
    Ok(plain)
}

fn fixture(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/options")
        .join(name);
    path.canonicalize().unwrap_or(path)
}

fn run_in(cwd: &Path, options: &str, roots: &[&str]) -> Result<Extraction, ExtractError> {
    let options: TypeScriptOptions =
        serde_json::from_str(options).map_err(|e| ExtractError::UnsupportedFile {
            path: PathBuf::from("options"),
            reason: e.to_string(),
        })?;
    let roots: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
    with_incremental(&|| prepare(&options, cwd), &roots)
}

fn run(name: &str, options: &str, roots: &[&str]) -> Result<Extraction, ExtractError> {
    run_in(&fixture(name), options, roots)
}

/// [`run`] as a native configuration runs it: Markdown fences read when `.md` is listed
/// (ADR-0036).
fn run_native(name: &str, options: &str, roots: &[&str]) -> Result<Extraction, ExtractError> {
    let options: TypeScriptOptions =
        serde_json::from_str(options).map_err(|e| ExtractError::UnsupportedFile {
            path: PathBuf::from("options"),
            reason: e.to_string(),
        })?;
    let roots: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
    with_incremental(
        &|| {
            let (mut settings, config) = prepare(&options, &fixture(name))?;
            settings.markdown_fences = true;
            Ok((settings, config))
        },
        &roots,
    )
}

/// `(module, line, column)` of every edge leaving `source`.
fn positions(extraction: &Extraction, source: &str) -> Vec<(String, Option<u32>, Option<u32>)> {
    module(extraction, source)
        .map(|m| {
            m.dependencies
                .iter()
                .map(|d| (d.module.clone(), d.line, d.column))
                .collect()
        })
        .unwrap_or_default()
}

fn module<'e>(extraction: &'e Extraction, source: &str) -> Option<&'e Module> {
    extraction.modules.iter().find(|m| m.source == source)
}

/// The module sources in extraction order, which is dependency-cruiser's: the expected lists
/// below are what 18.2.0's `cruise()` returns for the same fixture and options.
fn sources(extraction: &Extraction) -> Vec<&str> {
    extraction
        .modules
        .iter()
        .map(|m| m.source.as_str())
        .collect()
}

/// `(module, resolved, dependency types)` of every edge leaving `source`.
fn edges(extraction: &Extraction, source: &str) -> Vec<(String, String, Vec<String>)> {
    module(extraction, source)
        .map(|m| {
            m.dependencies
                .iter()
                .map(|d| {
                    (
                        d.module.clone(),
                        d.resolved.clone(),
                        d.dependency_types
                            .iter()
                            .map(|t| t.as_str().to_owned())
                            .collect(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn resolved(extraction: &Extraction, source: &str) -> Vec<String> {
    edges(extraction, source)
        .into_iter()
        .map(|(_, resolved, _)| resolved)
        .collect()
}

fn types(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// Copies a fixture into a fresh temporary folder, for fixtures a test changes (a symlink).
fn scratch(name: &str, tag: &str) -> PathBuf {
    fn copy(from: &Path, to: &Path) {
        let _ = std::fs::create_dir_all(to);
        for entry in std::fs::read_dir(from).into_iter().flatten().flatten() {
            let target = to.join(entry.file_name());
            if entry.path().is_dir() {
                copy(&entry.path(), &target);
            } else {
                let _ = std::fs::copy(entry.path(), target);
            }
        }
    }
    let root = std::env::temp_dir().join(format!("rb-options-{name}-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    copy(&fixture(name), &root);
    root.canonicalize().unwrap_or(root)
}

#[test]
fn babel_config_module_resolver_aliases_rewrite_specifiers() {
    let without = run("babel-config", "{}", &["src"]);
    assert_eq!(
        without.ok().map(|e| edges(&e, "src/index.js")),
        Some(vec![(
            "@lib/util".to_owned(),
            "@lib/util".to_owned(),
            types(&["unknown"])
        )])
    );
    let with = run(
        "babel-config",
        r#"{"babelConfig": {"fileName": ".babelrc"}}"#,
        &["src"],
    );
    let Ok(with) = with else {
        unreachable!("{with:?}");
    };
    assert_eq!(
        edges(&with, "src/index.js"),
        [(
            "./lib/util".to_owned(),
            "src/lib/util.js".to_owned(),
            types(&["local", "import"])
        )]
    );
    assert_eq!(sources(&with), ["src/index.js", "src/lib/util.js"]);
    let js = run(
        "babel-config",
        r#"{"babelConfig": {"fileName": "babel.config.js"}}"#,
        &["src"],
    );
    assert!(
        matches!(js, Err(ExtractError::UnsupportedFile { reason, .. }) if reason.contains("babel.config.json"))
    );
}

#[test]
fn base_dir_is_the_root_of_every_path() {
    let extraction = run("base-dir", r#"{"baseDir": "project"}"#, &["src"]);
    let Ok(extraction) = extraction else {
        unreachable!("{extraction:?}");
    };
    assert_eq!(sources(&extraction), ["src/a.js", "src/b.js"]);
    assert_eq!(resolved(&extraction, "src/a.js"), ["src/b.js"]);
}

#[test]
fn built_in_modules_are_added_to_or_replaced() {
    let core = |extraction: &Extraction| -> Vec<(String, bool)> {
        module(extraction, "src/index.js")
            .map(|m| {
                m.dependencies
                    .iter()
                    .map(|d| (d.module.clone(), d.core_module))
                    .collect()
            })
            .unwrap_or_default()
    };
    let plain = run("built-in-modules", "{}", &["src"]).map(|e| core(&e));
    assert_eq!(
        plain.ok(),
        Some(vec![
            ("electron".to_owned(), false),
            ("fs".to_owned(), true),
            ("path".to_owned(), true)
        ])
    );
    let added = run(
        "built-in-modules",
        r#"{"builtInModules": {"add": ["electron"]}}"#,
        &["src"],
    )
    .map(|e| core(&e));
    assert_eq!(
        added.ok(),
        Some(vec![
            ("electron".to_owned(), true),
            ("fs".to_owned(), true),
            ("path".to_owned(), true)
        ])
    );
    let replaced = run(
        "built-in-modules",
        r#"{"builtInModules": {"override": ["electron", "path"]}}"#,
        &["src"],
    )
    .map(|e| core(&e));
    assert_eq!(
        replaced.ok(),
        Some(vec![
            ("electron".to_owned(), true),
            ("fs".to_owned(), false),
            ("path".to_owned(), true)
        ])
    );
}

#[test]
fn combined_dependencies_reads_every_manifest_up_to_the_base() {
    let edge = |options: &str| {
        run("combined-dependencies", options, &["packages/app"])
            .ok()
            .map(|e| edges(&e, "packages/app/index.js"))
    };
    let resolved = "node_modules/left-pad/index.js".to_owned();
    assert_eq!(
        edge("{}"),
        Some(vec![(
            "left-pad".to_owned(),
            resolved.clone(),
            types(&["npm-no-pkg", "require"])
        )])
    );
    assert_eq!(
        edge(r#"{"combinedDependencies": true}"#),
        Some(vec![(
            "left-pad".to_owned(),
            resolved,
            types(&["npm", "require"])
        )])
    );
}

#[test]
fn detect_jsdoc_imports_reads_import_tags_and_bracket_imports() {
    let edge = |options: &str| {
        run("jsdoc-imports", options, &["src/index.js"])
            .ok()
            .map(|e| edges(&e, "src/index.js"))
    };
    assert_eq!(edge(r#"{"parser": "tsc"}"#), Some(Vec::new()));
    assert_eq!(
        edge(r#"{"parser": "tsc", "detectJSDocImports": true}"#),
        Some(vec![
            (
                "./shape".to_owned(),
                "src/shape.js".to_owned(),
                types(&["local", "type-only", "import", "jsdoc", "jsdoc-import-tag"])
            ),
            (
                "./size".to_owned(),
                "src/size.js".to_owned(),
                types(&[
                    "local",
                    "type-only",
                    "import",
                    "jsdoc",
                    "jsdoc-bracket-import"
                ])
            ),
        ])
    );
}

/// A `.js` specifier that does not exist is retried without its extension. Upstream asks for the
/// TypeScript variants, but its resolver cache is per run, so outside its own tests (which bust
/// the cache) the retry searches the configured extensions and finds a `.d.mts` for a `.js`, as
/// 18.2.0's `cruise()` does on this fixture and on dependency-cruiser's own `src/`.
#[test]
fn an_unresolvable_js_specifier_is_retried_with_the_configured_extensions() {
    let found = run(
        "ts-variant-retry",
        r#"{"parser": "tsc", "detectJSDocImports": true,
            "enhancedResolveOptions": {"extensions": [".js", ".mjs", ".d.mts"]}}"#,
        &["src/index.mjs"],
    );
    let Ok(found) = found else {
        unreachable!("{found:?}");
    };
    assert_eq!(sources(&found), ["src/index.mjs", "types/thing.d.mts"]);
    assert_eq!(
        edges(&found, "src/index.mjs"),
        [(
            "../types/thing.js".to_owned(),
            "types/thing.d.mts".to_owned(),
            types(&["local", "type-only", "import", "jsdoc", "jsdoc-import-tag"])
        )]
    );
}

/// `(module, resolved, module system, types, followable, dynamic)` of each dependency of `source`.
fn details(
    extraction: &Extraction,
    source: &str,
) -> Vec<(String, String, String, Vec<String>, bool, bool)> {
    module(extraction, source)
        .map(|m| {
            m.dependencies
                .iter()
                .map(|d| {
                    (
                        d.module.clone(),
                        d.resolved.clone(),
                        d.module_system.to_string(),
                        d.dependency_types
                            .iter()
                            .map(|t| t.as_str().to_owned())
                            .collect(),
                        d.followable,
                        d.dynamic,
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Upstream caches the followable extensions of the run's first successful resolution. Here
/// that is the retry of `./a.js` as TypeScript, so `.cts` is not followable for the rest of the
/// run: 18.2.0's `cruise(["src"], {})` on this fixture reports `./c.cjs` as not followable.
#[test]
fn the_first_successful_resolution_decides_what_is_followable() {
    let found = run("followable-settles", "{}", &["src"]);
    let Ok(found) = found else {
        unreachable!("{found:?}");
    };
    assert_eq!(sources(&found), ["src/a.ts", "src/c.cts", "src/index.ts"]);
    let followable: Vec<(String, bool)> = details(&found, "src/index.ts")
        .into_iter()
        .map(|d| (d.1, d.4))
        .collect();
    assert_eq!(
        followable,
        [
            ("src/a.ts".to_owned(), true),
            ("src/c.cts".to_owned(), false)
        ]
    );
}

/// Without `tsPreCompilationDeps`, upstream compiles `.mts` for acorn with `module: nodenext`
/// into CommonJS: static imports and re-exports become `require`, `import()` stays, and a
/// type-only import is gone. The expectations are 18.2.0's `cruise(["src"], {})` on the fixture.
#[test]
fn an_mts_file_compiled_for_acorn_requires_what_it_imports() {
    let found = run("mts-compiled", "{}", &["src"]);
    let Ok(found) = found else {
        unreachable!("{found:?}");
    };
    assert_eq!(
        sources(&found),
        [
            "src/a.js",
            "src/b.js",
            "src/c.js",
            "src/index.mts",
            "src/t.ts"
        ]
    );
    let entry = |module: &str, system: &str, kinds: &[&str], dynamic: bool| {
        (
            module.to_owned(),
            format!("src/{}", &module[2..]),
            system.to_owned(),
            types(kinds),
            true,
            dynamic,
        )
    };
    assert_eq!(
        details(&found, "src/index.mts"),
        [
            entry("./a.js", "cjs", &["local", "require"], false),
            entry("./b.js", "cjs", &["local", "require"], false),
            entry("./c.js", "es6", &["local", "dynamic-import"], true),
        ]
    );
}

/// `(module, module system, types, dynamic)` of each dependency of `file`, extracted with
/// `tsConfig` naming `tsconfig` in the `ts-config-module` fixture.
fn module_kinds(tsconfig: &str, file: &str) -> Vec<(String, String, Vec<String>, bool)> {
    let options = format!(r#"{{"tsConfig": {{"fileName": "{tsconfig}"}}}}"#);
    run("ts-config-module", &options, &[file])
        .ok()
        .map(|found| {
            details(&found, file)
                .into_iter()
                .map(|(module, _, system, kinds, _, dynamic)| (module, system, kinds, dynamic))
                .collect()
        })
        .unwrap_or_default()
}

/// Upstream spreads the tsconfig's compiler options over its own when it compiles TypeScript for
/// acorn, so the tsconfig's `module` (or, when unset, an ES3 or ES5 `target`) decides whether
/// imports reach acorn as `import` or `require`, whether `export * as ns` is lowered, and
/// whether `import()` survives. Each row is 18.2.0's `cruise([file], {tsConfig})` on the
/// fixture with TypeScript 6.0.3; `import type` is gone in every row.
#[test]
fn the_tsconfig_module_decides_what_acorn_reads() {
    let row = |kinds: [(&str, &[&str], bool); 4]| -> Vec<(String, String, Vec<String>, bool)> {
        ["./b.js", "./c.js", "./d.js", "./e.js"]
            .iter()
            .zip(kinds)
            .map(|(module, (system, types_, dynamic))| {
                (
                    (*module).to_owned(),
                    system.to_owned(),
                    types(types_),
                    dynamic,
                )
            })
            .collect()
    };
    let require = ("cjs", &["local", "require"][..], false);
    let dynamic = ("es6", &["local", "dynamic-import"][..], true);
    let import = ("es6", &["local", "import"][..], false);
    let export = ("es6", &["local", "export"][..], false);
    let es2015 = row([import, export, import, dynamic]);
    let es2020 = row([import, export, export, dynamic]);
    let node = row([require, require, require, dynamic]);
    let commonjs = row([require; 4]);
    let cases: [(&str, &str, &Vec<_>); 18] = [
        ("tsconfig.json", "src/a.ts", &es2015),
        ("tsconfig.json", "src/m.mts", &node),
        ("tsconfig.commonjs.json", "src/a.ts", &commonjs),
        ("tsconfig.commonjs.json", "src/m.mts", &commonjs),
        ("tsconfig.node16.json", "src/a.ts", &node),
        ("tsconfig.node16.json", "src/m.mts", &node),
        ("tsconfig.nodenext.json", "src/a.ts", &node),
        ("tsconfig.nodenext.json", "src/m.mts", &node),
        ("tsconfig.extends.json", "src/a.ts", &node),
        ("tsconfig.extends.json", "src/m.mts", &node),
        ("tsconfig.es2015.json", "src/a.ts", &es2015),
        ("tsconfig.es2015.json", "src/m.mts", &es2015),
        ("tsconfig.esnext.json", "src/a.ts", &es2020),
        ("tsconfig.esnext.json", "src/m.mts", &es2020),
        ("tsconfig.preserve.json", "src/a.ts", &es2020),
        ("tsconfig.preserve.json", "src/m.mts", &es2020),
        ("tsconfig.es5.json", "src/a.ts", &commonjs),
        ("tsconfig.es5.json", "src/m.mts", &node),
    ];
    for (tsconfig, file, expected) in cases {
        assert_eq!(&module_kinds(tsconfig, file), expected, "{tsconfig} {file}");
    }
}

/// `amd`, `umd`, `system` and `none` stop the run with the tsconfig named, when TypeScript is
/// compiled for acorn (`"specify"` compiles too); with `tsPreCompilationDeps: true`, or `tsc` as
/// the parser, nothing is compiled and the run goes on.
#[test]
fn a_module_kind_typescript_7_removes_stops_a_compiling_run() {
    let options =
        |extra: &str| format!(r#"{{"tsConfig": {{"fileName": "tsconfig.amd.json"}}{extra}}}"#);
    let stopped = |extra: &str| {
        matches!(
            run("ts-config-module", &options(extra), &["src/a.ts"]),
            Err(ExtractError::UnsupportedFile { path, reason })
                if path.ends_with("tsconfig.amd.json")
                    && reason.contains(r#"compilerOptions.module "amd""#)
                    && reason.contains("tsPreCompilationDeps")
        )
    };
    assert!(stopped(""));
    assert!(stopped(r#", "tsPreCompilationDeps": "specify""#));
    assert!(!stopped(r#", "tsPreCompilationDeps": true"#));
    assert!(!stopped(r#", "parser": "tsc""#));
    assert!(
        run(
            "ts-config-module",
            &options(r#", "tsPreCompilationDeps": true"#),
            &["src/a.ts"]
        )
        .is_ok()
    );
}

/// A TypeScript 7 tsconfig: no `baseUrl`, `paths` inherited from a config in another folder
/// (relative to that folder, and through `${configDir}`), `moduleResolution: bundler`, `.js`
/// specifiers for `.ts` files and `#` subpath imports. The resolutions are TypeScript's.
/// 18.2.0 leaves `@lib/lib` and `@shared/shared` unresolved, because `tsconfig-paths` 4.2.0
/// takes inherited `paths` against the extending config and does not expand `${configDir}`;
/// the extractor follows TypeScript there
/// ([ADR-0040](../../../docs/adr/0040-typescript-tsconfig-semantics-where-tsconfig-paths-departs.md)).
#[test]
fn a_typescript_7_tsconfig_resolves_inherited_paths_without_base_url() {
    let found = run(
        "ts7-config",
        r#"{"tsConfig": {"fileName": "tsconfig.json"}}"#,
        &["src/index.ts"],
    );
    let Ok(found) = found else {
        unreachable!("{found:?}");
    };
    let paths = [
        "aliased",
        "aliased-tsconfig",
        "aliased-tsconfig-paths",
        "local",
        "import",
    ];
    assert_eq!(
        edges(&found, "src/index.ts"),
        [
            (
                "./util.js".to_owned(),
                "src/util.ts".to_owned(),
                types(&["local", "import"])
            ),
            (
                "@lib/lib".to_owned(),
                "lib/lib.ts".to_owned(),
                types(&paths)
            ),
            (
                "@shared/shared".to_owned(),
                "shared/shared.ts".to_owned(),
                types(&paths)
            ),
            (
                "#internal/internal.js".to_owned(),
                "internal/internal.ts".to_owned(),
                types(&["aliased", "aliased-subpath-import", "local", "import"])
            ),
        ]
    );
}

/// `paths` of the tsconfig itself, with no `baseUrl`, resolve against its folder, as in 18.2.0;
/// under `module: nodenext` the imports reach acorn as `require`. A bare `src/util.js` stays
/// unresolved, which is TypeScript's answer without `baseUrl`: 18.2.0 resolves it, because
/// `tsconfig-paths-webpack-plugin` 4.2.0 adds a match-all `*` against the tsconfig's folder
/// whether or not `baseUrl` is set
/// ([ADR-0040](../../../docs/adr/0040-typescript-tsconfig-semantics-where-tsconfig-paths-departs.md)).
#[test]
fn a_typescript_7_tsconfig_resolves_its_own_paths_and_no_implicit_base_url() {
    let found = run(
        "ts7-config",
        r#"{"tsConfig": {"fileName": "tsconfig.paths.json"}}"#,
        &["src/app.ts"],
    );
    let Ok(found) = found else {
        unreachable!("{found:?}");
    };
    assert_eq!(
        details(&found, "src/app.ts"),
        [
            (
                "@app/feature/feature.js".to_owned(),
                "src/feature/feature.ts".to_owned(),
                "cjs".to_owned(),
                types(&[
                    "aliased",
                    "aliased-tsconfig",
                    "aliased-tsconfig-paths",
                    "local",
                    "require"
                ]),
                true,
                false
            ),
            (
                "src/util.js".to_owned(),
                "src/util.js".to_owned(),
                "cjs".to_owned(),
                types(&["unknown"]),
                false,
                false
            ),
        ]
    );
    assert_eq!(resolved(&found, "src/feature/feature.ts"), ["src/util.ts"]);
}

#[test]
fn detect_process_builtin_module_calls_finds_both_spellings() {
    let edge = |options: &str| {
        run("process-builtin", options, &["src"])
            .ok()
            .map(|e| edges(&e, "src/index.js"))
    };
    assert_eq!(edge("{}"), Some(Vec::new()));
    let found = edge(r#"{"detectProcessBuiltinModuleCalls": true}"#);
    assert_eq!(
        found,
        Some(vec![
            (
                "fs".to_owned(),
                "fs".to_owned(),
                types(&["core", "process-get-builtin-module"])
            ),
            (
                "os".to_owned(),
                "os".to_owned(),
                types(&["core", "process-get-builtin-module"])
            ),
        ])
    );
}

#[test]
fn do_not_follow_by_path_and_by_dependency_type() {
    let by_path = run(
        "do-not-follow",
        r#"{"doNotFollow": {"path": "src/lib"}}"#,
        &["src/index.js"],
    );
    let Ok(by_path) = by_path else {
        unreachable!("{by_path:?}");
    };
    let lib = module(&by_path, "src/lib/a.js");
    assert_eq!(lib.and_then(|m| m.matches_do_not_follow), Some(true));
    assert_eq!(lib.and_then(|m| m.followable), Some(false));
    assert!(module(&by_path, "src/lib/b.js").is_none());
    let edge = module(&by_path, "src/index.js")
        .and_then(|m| m.dependencies.iter().find(|d| d.module == "./lib/a"));
    assert_eq!(
        edge.map(|d| (d.followable, d.matches_do_not_follow)),
        Some((false, Some(true)))
    );

    let by_type = run(
        "do-not-follow",
        r#"{"doNotFollow": {"dependencyTypes": ["npm"]}}"#,
        &["src/index.js"],
    );
    let Ok(by_type) = by_type else {
        unreachable!("{by_type:?}");
    };
    assert_eq!(
        sources(&by_type),
        [
            "src/index.js",
            "node_modules/dep/index.js",
            "src/lib/a.js",
            "src/lib/b.js"
        ]
    );
    assert_eq!(
        module(&by_type, "node_modules/dep/index.js").and_then(|m| m.dependency_types.clone()),
        Some(vec![
            rb_model::DependencyType::Npm,
            rb_model::DependencyType::Require
        ])
    );
    let followed = run("do-not-follow", "{}", &["src/index.js"]);
    assert!(
        followed.is_ok_and(|e| module(&e, "node_modules/dep/inner.js").is_some()),
        "without doNotFollow the npm package is followed"
    );
}

#[test]
fn enhanced_resolve_options_map_to_the_resolver() {
    let targets = |options: &str| {
        run("enhanced-resolve", options, &["src/index.js"])
            .ok()
            .map(|e| resolved(&e, "src/index.js"))
    };
    assert_eq!(
        targets("{}"),
        Some(types(&[
            "src/dir/index.js",
            "src/typed.ts",
            "node_modules/pkg/main.js",
            "node_modules/pkg2/node.js"
        ]))
    );
    assert_eq!(
        targets(
            r#"{"enhancedResolveOptions": {"exportsFields": ["exports"], "conditionNames": ["import"],
                "cachedInputFileSystem": {"cacheDuration": 1000}}}"#
        ),
        Some(types(&[
            "src/dir/index.js",
            "src/typed.ts",
            "node_modules/pkg/esm.js",
            "node_modules/pkg2/node.js"
        ]))
    );
    assert_eq!(
        targets(
            r#"{"enhancedResolveOptions": {"mainFields": ["module", "main"], "mainFiles": ["main"],
                "aliasFields": ["browser"], "extensions": [".js"]}}"#
        ),
        Some(types(&[
            "src/dir/main.js",
            "./typed",
            "node_modules/pkg/module.js",
            "node_modules/pkg2/browser.js"
        ]))
    );
}

#[test]
fn exclude_drops_paths_and_dynamic_imports() {
    let run_with = |options: &str| {
        run("exclude", options, &["src/index.js"])
            .ok()
            .map(|e| (sources(&e).join(","), resolved(&e, "src/index.js")))
    };
    assert_eq!(
        run_with("{}"),
        Some((
            "src/index.js,src/excluded/x.js,src/kept.js,src/lazy.js".to_owned(),
            types(&["src/excluded/x.js", "src/kept.js", "src/lazy.js"])
        ))
    );
    assert_eq!(
        run_with(r#"{"exclude": {"path": "excluded", "dynamic": true}}"#),
        Some((
            "src/index.js,src/kept.js,src/lazy.js".to_owned(),
            types(&["src/kept.js"])
        ))
    );
    assert_eq!(
        run_with(r#"{"exclude": "excluded"}"#).map(|(_, r)| r),
        Some(types(&["src/kept.js", "src/lazy.js"]))
    );
}

#[test]
fn exotic_require_strings_name_other_loaders() {
    let extraction = run(
        "exotic-require",
        r#"{"exoticRequireStrings": ["need", "window.require"]}"#,
        &["src/index.js"],
    );
    let Ok(extraction) = extraction else {
        unreachable!("{extraction:?}");
    };
    let exotic: Vec<(String, Option<String>)> = module(&extraction, "src/index.js")
        .map(|m| {
            m.dependencies
                .iter()
                .map(|d| (d.module.clone(), d.exotic_require.clone()))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        exotic,
        [
            ("./a".to_owned(), Some("need".to_owned())),
            ("./b".to_owned(), Some("window.require".to_owned())),
            ("./c".to_owned(), None)
        ]
    );
    let plain = run("exotic-require", "{}", &["src/index.js"]);
    assert_eq!(
        plain.ok().map(|e| resolved(&e, "src/index.js")),
        Some(types(&["src/c.js"]))
    );
}

#[test]
fn yarn_pnp_resolves_through_the_manifest() {
    let pnp = run(
        "yarn-pnp",
        r#"{"externalModuleResolutionStrategy": "yarn-pnp"}"#,
        &["src"],
    );
    let Ok(pnp) = pnp else {
        unreachable!("{pnp:?}");
    };
    assert_eq!(
        edges(&pnp, "src/index.js"),
        [(
            "left-pad".to_owned(),
            ".yarn/unplugged/left-pad-npm-1.3.0/node_modules/left-pad/index.js".to_owned(),
            types(&["npm", "require"])
        )]
    );
    let node_modules = run("yarn-pnp", "{}", &["src"]);
    assert_eq!(
        node_modules.ok().map(|e| resolved(&e, "src/index.js")),
        Some(types(&["left-pad"]))
    );
    // No manifest: a named reason, not a silent fall back to node_modules.
    let missing = run_in(
        &scratch_without_pnp(),
        r#"{"externalModuleResolutionStrategy": "yarn-pnp"}"#,
        &["src"],
    );
    assert!(
        matches!(missing, Err(ExtractError::UnsupportedFile { reason, .. }) if reason.contains(".pnp.cjs"))
    );
}

fn scratch_without_pnp() -> PathBuf {
    let root = scratch("yarn-pnp", "missing");
    let _ = std::fs::remove_file(root.join(".pnp.cjs"));
    root
}

#[test]
fn a_malformed_pnp_manifest_is_a_named_error() {
    let root = scratch("yarn-pnp", "malformed");
    let _ = std::fs::write(
        root.join(".pnp.cjs"),
        "const RAW_RUNTIME_STATE = '{\"packageRegistryData\": []}';",
    );
    let malformed = run_in(
        &root,
        r#"{"externalModuleResolutionStrategy": "yarn-pnp"}"#,
        &["src"],
    );
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        matches!(malformed, Err(ExtractError::UnsupportedFile { reason, .. }) if reason.contains("top-level package"))
    );
}

#[test]
fn extra_extensions_are_discovered_and_carry_no_dependencies() {
    let without = run("extra-extensions", "{}", &["src"]);
    assert_eq!(
        without.ok().map(|e| sources(&e).join(",")),
        Some("src/index.js,src/other.js".to_owned())
    );
    let with = run(
        "extra-extensions",
        r#"{"extraExtensionsToScan": [".md"]}"#,
        &["src"],
    );
    let Ok(with) = with else {
        unreachable!("{with:?}");
    };
    assert_eq!(
        sources(&with),
        ["src/index.js", "src/other.js", "src/notes.md"]
    );
    assert_eq!(
        module(&with, "src/notes.md").map(|m| m.dependencies.len()),
        Some(0)
    );
}

#[test]
fn markdown_fences_are_read_for_a_native_configuration() -> Result<(), Box<dyn std::error::Error>> {
    let options = r#"{"extraExtensionsToScan": [".md"]}"#;
    // dependency-cruiser's behaviour, which a dependency-cruiser configuration keeps.
    let upstream = run("markdown", options, &["src"])?;
    assert_eq!(
        module(&upstream, "src/usage.md").map(|m| m.dependencies.len()),
        Some(0)
    );
    let native = run_native("markdown", options, &["src"])?;
    assert_eq!(
        edges(&native, "src/usage.md"),
        [
            (
                "./index".to_owned(),
                "src/index.ts".to_owned(),
                vec!["local".to_owned(), "require".to_owned()]
            ),
            (
                "./index".to_owned(),
                "src/index.ts".to_owned(),
                vec!["local".to_owned(), "import".to_owned()]
            ),
            (
                "./missing".to_owned(),
                "./missing".to_owned(),
                vec!["unknown".to_owned()]
            ),
        ],
        "the type-only import is elided as in a .ts file; python and untagged fences are not read"
    );
    assert_eq!(
        positions(&native, "src/usage.md"),
        [
            ("./index".to_owned(), Some(12), Some(13)),
            ("./index".to_owned(), Some(5), Some(1)),
            ("./missing".to_owned(), Some(16), Some(13)),
        ]
    );
    // Without `.md` listed, a native configuration does not gather Markdown at all.
    let unlisted = run_native("markdown", "{}", &["src"])?;
    assert!(module(&unlisted, "src/usage.md").is_none());
    // Two runs serialise byte for byte.
    let again = run_native("markdown", options, &["src"])?;
    assert_eq!(
        serde_json::to_string(&native.modules)?,
        serde_json::to_string(&again.modules)?
    );
    Ok(())
}

#[test]
fn vue_and_svelte_scripts_are_extracted_where_they_stand() -> Result<(), Box<dyn std::error::Error>>
{
    let extraction = run("sfc", "{}", &["src"])?;
    let expected = |shared_line: u32, helper_line: u32, column: u32| {
        (
            vec![
                (
                    "./helper.js".to_owned(),
                    "src/helper.js".to_owned(),
                    vec!["local".to_owned(), "import".to_owned()],
                ),
                (
                    "./shared".to_owned(),
                    "src/shared.ts".to_owned(),
                    vec!["local".to_owned(), "import".to_owned()],
                ),
            ],
            vec![
                ("./helper.js".to_owned(), Some(helper_line), Some(column)),
                ("./shared".to_owned(), Some(shared_line), Some(column)),
            ],
        )
    };
    for (component, (edges_expected, positions_expected)) in [
        ("src/App.vue", expected(5, 9, 1)),
        ("src/Widget.svelte", expected(2, 7, 3)),
    ] {
        assert_eq!(edges(&extraction, component), edges_expected, "{component}");
        assert_eq!(
            positions(&extraction, component),
            positions_expected,
            "{component}"
        );
    }
    // tsc reads a Vue script as TypeScript and keeps the type-only import; a Svelte component is
    // compiled before it is read, so its type-only import is gone either way.
    let tsc = run("sfc", r#"{"tsPreCompilationDeps": true}"#, &["src"])?;
    let targets = |source: &str| -> Vec<String> {
        edges(&tsc, source).into_iter().map(|(m, _, _)| m).collect()
    };
    assert_eq!(
        targets("src/App.vue"),
        ["./helper.js", "./props", "./shared"]
    );
    assert_eq!(targets("src/Widget.svelte"), ["./helper.js", "./shared"]);
    Ok(())
}

#[test]
fn a_webpack_resolve_block_reaches_the_resolver() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = fixture("webpack-resolve");
    let (settings, mut config) = prepare(&TypeScriptOptions::default(), &cwd)?;
    let src = cwd.join("src").to_string_lossy().into_owned();
    let block = serde_json::json!({
        "alias": { "@": src, "gone": false },
        "modules": ["node_modules", "lib"],
        "extensions": [".ts", ".js"]
    });
    let unread = rb_extract_ts::resolve::apply_resolve_block(
        &mut config,
        block.as_object().ok_or("an object")?,
    );
    assert!(unread.is_empty());
    let extraction = extract_with(&[PathBuf::from("src/index.js")], &settings, &config)?;
    assert_eq!(
        edges(&extraction, "src/index.js"),
        [
            (
                "@/util".to_owned(),
                "src/util/index.ts".to_owned(),
                vec![
                    "aliased".to_owned(),
                    "aliased-webpack".to_owned(),
                    "local".to_owned(),
                    "import".to_owned()
                ]
            ),
            (
                "gone".to_owned(),
                "gone".to_owned(),
                vec!["unknown".to_owned()]
            ),
            (
                "thing".to_owned(),
                "lib/thing.js".to_owned(),
                vec!["localmodule".to_owned(), "import".to_owned()]
            ),
        ]
    );
    Ok(())
}

#[test]
fn include_only_keeps_matching_modules() {
    let extraction = run("include-only", r#"{"includeOnly": "^src/keep"}"#, &["src"]);
    assert_eq!(
        extraction.ok().map(|e| sources(&e).join(",")),
        Some("src/keep/a.js,src/keep/index.js".to_owned())
    );
    let all = run("include-only", "{}", &["src"]);
    assert_eq!(all.ok().map(|e| sources(&e).len()), Some(3));
}

#[test]
fn max_depth_bounds_the_walk() {
    let at = |depth: u8| {
        run(
            "max-depth",
            &format!(r#"{{"maxDepth": {depth}}}"#),
            &["src/a.js"],
        )
        .ok()
        .map(|e| {
            e.modules
                .iter()
                .map(|m| format!("{}:{}", m.source, m.dependencies.len()))
                .collect::<Vec<_>>()
                .join(",")
        })
    };
    assert_eq!(
        at(0).as_deref(),
        Some("src/a.js:1,src/b.js:1,src/c.js:1,src/d.js:0")
    );
    assert_eq!(at(1).as_deref(), Some("src/a.js:1,src/b.js:0"));
    assert_eq!(at(2).as_deref(), Some("src/a.js:1,src/b.js:1,src/c.js:0"));
}

#[test]
fn module_systems_select_the_forms_extracted() {
    let systems = |options: &str| {
        run("module-systems", options, &["src/index.js"])
            .ok()
            .map(|e| resolved(&e, "src/index.js"))
    };
    assert_eq!(
        systems("{}"),
        Some(types(&["src/amd.js", "src/cjs.js", "src/es.js"]))
    );
    assert_eq!(
        systems(r#"{"moduleSystems": ["cjs"]}"#),
        Some(types(&["src/cjs.js"]))
    );
    assert_eq!(
        systems(r#"{"moduleSystems": ["es6", "amd"]}"#),
        Some(types(&["src/amd.js", "src/es.js"]))
    );
}

#[test]
fn parser_picks_the_walker_upstream_would() {
    let found = |options: &str| {
        run("parser", options, &["src/index.ts"])
            .ok()
            .map(|e| edges(&e, "src/index.ts"))
    };
    let area = (
        "./area".to_owned(),
        "src/area.ts".to_owned(),
        types(&["local", "import"]),
    );
    // acorn reads the compiled output, where the type-only import is gone.
    assert_eq!(found("{}"), Some(vec![area.clone()]));
    assert_eq!(found(r#"{"parser": "acorn"}"#), Some(vec![area.clone()]));
    assert_eq!(
        found(r#"{"parser": "tsc"}"#),
        Some(vec![
            area.clone(),
            (
                "./shape".to_owned(),
                "src/shape.ts".to_owned(),
                types(&["local", "type-only", "import"])
            )
        ])
    );
    // swc reads the source, but does not mark type-only imports.
    assert_eq!(
        found(r#"{"parser": "swc"}"#),
        Some(vec![
            area,
            (
                "./shape".to_owned(),
                "src/shape.ts".to_owned(),
                types(&["local", "import"])
            )
        ])
    );
}

#[cfg(unix)]
#[test]
fn preserve_symlinks_keeps_the_link_path() {
    let root = scratch("preserve-symlinks", "links");
    let _ = std::fs::create_dir_all(root.join("node_modules"));
    let _ = std::os::unix::fs::symlink("../packages/lib", root.join("node_modules/lib"));
    let edge = |options: &str| {
        run_in(&root, options, &["src"])
            .ok()
            .map(|e| edges(&e, "src/index.js"))
    };
    let followed = edge("{}");
    let preserved = edge(r#"{"preserveSymlinks": true}"#);
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(
        followed,
        Some(vec![(
            "lib".to_owned(),
            "packages/lib/index.js".to_owned(),
            types(&["undetermined", "require"])
        )])
    );
    assert_eq!(
        preserved,
        Some(vec![(
            "lib".to_owned(),
            "node_modules/lib/index.js".to_owned(),
            types(&["npm", "require"])
        )])
    );
}

#[test]
fn ts_config_paths_base_url_extends_and_references() {
    let extraction = run(
        "ts-config",
        r#"{"tsConfig": {"fileName": "tsconfig.json"}}"#,
        &["src/index.ts"],
    );
    let Ok(extraction) = extraction else {
        unreachable!("{extraction:?}");
    };
    assert_eq!(
        edges(&extraction, "src/index.ts"),
        [
            (
                "../sub/lib/y".to_owned(),
                "sub/lib/y.ts".to_owned(),
                types(&["local", "import"])
            ),
            (
                "@sh/x".to_owned(),
                "shared/x.ts".to_owned(),
                types(&[
                    "aliased",
                    "aliased-tsconfig",
                    "aliased-tsconfig-paths",
                    "local",
                    "import"
                ])
            ),
            (
                "src/util".to_owned(),
                "src/util.ts".to_owned(),
                types(&[
                    "aliased",
                    "aliased-tsconfig",
                    "aliased-tsconfig-base-url",
                    "local",
                    "import"
                ])
            ),
        ]
    );
    // The referenced project's own `paths` resolve its files.
    assert_eq!(resolved(&extraction, "sub/lib/y.ts"), ["sub/lib/z.ts"]);
    let without = run("ts-config", "{}", &["src/index.ts"]);
    assert_eq!(
        without.ok().map(|e| resolved(&e, "src/index.ts")),
        Some(types(&["sub/lib/y.ts", "@sh/x", "src/util"]))
    );
    let missing = run(
        "ts-config",
        r#"{"tsConfig": {"fileName": "nope.json"}}"#,
        &["src/index.ts"],
    );
    assert!(
        matches!(missing, Err(ExtractError::UnsupportedFile { reason, .. }) if reason.contains("tsConfig"))
    );
}

#[test]
fn ts_pre_compilation_deps_false_true_and_specify() {
    let deps = |options: &str| {
        run("ts-pre-compilation-deps", options, &["src/index.ts"])
            .ok()
            .and_then(|e| module(&e, "src/index.ts").cloned())
            .map(|m| {
                m.dependencies
                    .into_iter()
                    .map(|d| {
                        (
                            d.module,
                            d.pre_compilation_only,
                            d.dependency_types
                                .iter()
                                .map(|t| t.as_str().to_owned())
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
    };
    assert_eq!(
        deps("{}"),
        Some(vec![(
            "./value".to_owned(),
            None,
            types(&["local", "import"])
        )])
    );
    assert_eq!(
        deps(r#"{"tsPreCompilationDeps": true}"#),
        Some(vec![
            (
                "./types".to_owned(),
                None,
                types(&["local", "type-only", "import"])
            ),
            ("./unused".to_owned(), None, types(&["local", "import"])),
            ("./value".to_owned(), None, types(&["local", "import"])),
        ])
    );
    assert_eq!(
        deps(r#"{"tsPreCompilationDeps": "specify"}"#),
        Some(vec![
            (
                "./types".to_owned(),
                Some(true),
                types(&["local", "type-only", "import", "pre-compilation-only"])
            ),
            (
                "./unused".to_owned(),
                Some(true),
                types(&["local", "import", "pre-compilation-only"])
            ),
            (
                "./value".to_owned(),
                Some(false),
                types(&["local", "import"])
            ),
        ])
    );
}

#[test]
fn every_edge_carries_its_line_and_column() {
    let extraction = run(
        "ts-pre-compilation-deps",
        r#"{"tsPreCompilationDeps": true}"#,
        &["src/index.ts"],
    );
    let positions: Vec<(Option<u32>, Option<u32>)> = extraction
        .ok()
        .and_then(|e| module(&e, "src/index.ts").cloned())
        .map(|m| m.dependencies.iter().map(|d| (d.line, d.column)).collect())
        .unwrap_or_default();
    assert_eq!(
        positions,
        [(Some(1), Some(1)), (Some(2), Some(1)), (Some(3), Some(1))]
    );
}

#[test]
fn a_sidecar_file_stops_the_run_with_a_named_reason() {
    let extraction = run("sidecar", "{}", &["src"]);
    let Err(ExtractError::UnsupportedFile { path, reason }) = extraction else {
        unreachable!("{extraction:?}");
    };
    assert_eq!(path, PathBuf::from("src/legacy.coffee"));
    assert!(reason.contains("ADR-0017") && reason.contains("--sidecar node"));
    // Reached only as an unfollowed dependency, it is never read, so it does not stop the run.
    let not_followed = run(
        "sidecar",
        r#"{"doNotFollow": {"path": "coffee$"}}"#,
        &["src/index.js"],
    );
    assert!(not_followed.is_ok());
}

#[test]
fn an_empty_input_finds_no_modules() {
    let root = std::env::temp_dir().join(format!("rb-options-empty-{}", std::process::id()));
    let _ = std::fs::create_dir_all(root.join("src"));
    let empty = run_in(&root, "{}", &["src"]);
    let no_roots = run_in(&root, "{}", &[]);
    let _ = std::fs::remove_dir_all(&root);
    assert!(matches!(empty, Err(ExtractError::NoModulesFound)));
    assert!(matches!(no_roots, Err(ExtractError::NoModulesFound)));
}

#[test]
fn two_runs_serialise_byte_for_byte() {
    let json = || {
        run(
            "ts-config",
            r#"{"tsConfig": {"fileName": "tsconfig.json"}}"#,
            &["src", "shared", "sub"],
        )
        .ok()
        .and_then(|e| serde_json::to_string(&e.modules).ok())
    };
    let first = json();
    assert!(first.as_ref().is_some_and(|j| j.contains("sub/lib/z.ts")));
    for _ in 0..5 {
        assert_eq!(json(), first);
    }
}

#[test]
fn the_extractor_trait_runs_from_the_working_directory() {
    // The trait entry point reads the process working directory, the crate folder under cargo.
    let root = PathBuf::from("tests/options/base-dir/project/src");
    let extraction = TypeScriptExtractor.extract(&[root], &TypeScriptOptions::default());
    let Ok(extraction) = extraction else {
        unreachable!("{extraction:?}");
    };
    assert_eq!(extraction.inspected.files, 2);
    assert_eq!(extraction.inspected.modules, 2);
}

/// A dotted name as long as the file (100,001 segments) run through the whole extraction on
/// rayon's 2 MiB workers: every walker and the code layer read it without recursing per segment,
/// so the tsc flavour finds the import; the default flavour's import elision is oxc's semantic
/// analysis, which does recurse, so it refuses the file by name instead of aborting.
#[test]
fn a_dotted_name_as_long_as_the_file_is_read_or_refused_by_name()
-> Result<(), Box<dyn std::error::Error>> {
    let cwd = Path::new(env!("CARGO_TARGET_TMPDIR")).join("deep-chain");
    std::fs::create_dir_all(cwd.join("src"))?;
    std::fs::write(cwd.join("src/a.ts"), "export default {};\n")?;
    let chain = format!("a{}", ".b".repeat(100_000));
    std::fs::write(
        cwd.join("src/deep.ts"),
        format!(
            "import a from './a';\nexport class X extends {chain} {{}}\nlet v: {chain};\n{chain}();\n"
        ),
    )?;
    let (mut settings, config) = prepare(
        &serde_json::from_str(r#"{"tsPreCompilationDeps": true}"#)?,
        &cwd,
    )?;
    settings.code_layer = true;
    let tsc = extract_with(&[PathBuf::from("src")], &settings, &config)?;
    let deep = module(&tsc, "src/deep.ts").ok_or("src/deep.ts")?;
    assert_eq!(
        deep.dependencies
            .iter()
            .map(|d| d.resolved.as_str())
            .collect::<Vec<_>>(),
        ["src/a.ts"]
    );
    assert!(
        tsc.code
            .as_ref()
            .is_some_and(|code| code.types.iter().any(|t| t.full_name == "src/deep.ts#X"))
    );
    let refused = run_in(&cwd, "{}", &["src"]);
    assert!(
        matches!(
            &refused,
            Err(ExtractError::UnsupportedFile { path, reason })
                if path.ends_with("src/deep.ts")
                    && reason.contains("a dotted name of 100001 segments")
        ),
        "expected the file refused by name, got {refused:?}"
    );
    Ok(())
}

/// Vue's `generic` attribute holds a `>` inside its quotes; the start tag ends at the `>` after
/// it, so both imports are found, as `@vue/compiler-sfc` hands them to upstream, under the
/// default (acorn) flavour and under tsc alike, at the lines they have in the component. Before,
/// the body began inside the attribute, and an import on the tag's line was lost.
#[test]
fn a_generic_script_setup_keeps_its_imports() -> Result<(), Box<dyn std::error::Error>> {
    let expected = vec![
        (
            "./format".to_owned(),
            "src/format.ts".to_owned(),
            vec!["local".to_owned(), "import".to_owned()],
        ),
        (
            "./item".to_owned(),
            "src/item.ts".to_owned(),
            vec!["local".to_owned(), "import".to_owned()],
        ),
    ];
    for options in ["{}", r#"{"tsPreCompilationDeps": true}"#] {
        let extraction = run("sfc-generic", options, &["src"])?;
        assert_eq!(edges(&extraction, "src/Generic.vue"), expected, "{options}");
        assert_eq!(
            positions(&extraction, "src/Generic.vue"),
            [
                ("./format".to_owned(), Some(2), Some(1)),
                ("./item".to_owned(), Some(3), Some(1)),
            ],
            "{options}"
        );
        // The first import on the start tag's own line, which blanking the line would lose.
        assert_eq!(
            positions(&extraction, "src/Inline.vue"),
            [
                ("./format".to_owned(), Some(1), Some(69)),
                ("./item".to_owned(), Some(2), Some(1)),
            ],
            "{options}"
        );
    }
    Ok(())
}

/// A `.vue` script with `lang="ts"` is TypeScript to the code layer: the class is abstract and
/// generic and located as typescript. The module keeps the `language` its extension gives, as
/// dependency-cruiser's module layer carries no language to follow.
#[test]
fn a_typescript_component_script_is_typescript_to_the_code_layer()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut settings, config) = prepare(&TypeScriptOptions::default(), &fixture("sfc-generic"))?;
    settings.code_layer = true;
    let extraction = extract_with(&[PathBuf::from("src")], &settings, &config)?;
    let shape = extraction
        .code
        .as_ref()
        .and_then(|code| {
            code.types
                .iter()
                .find(|t| t.full_name == "src/Typed.vue#Shape")
        })
        .ok_or("src/Typed.vue#Shape")?;
    assert_eq!(shape.location.language, rb_model::Language::Typescript);
    assert_eq!((shape.r#abstract, shape.generic), (Some(true), Some(true)));
    assert_eq!(
        module(&extraction, "src/Typed.vue").and_then(|m| m.language),
        Some(rb_model::Language::Javascript)
    );
    Ok(())
}

/// The earlier extraction of the `exclude` fixture with `src/index.js`'s dependencies emptied
/// and every file marked unchanged: what an incremental run reuses shows in its result.
fn tampered(options: &str, keep: bool) -> Result<(Extraction, Extraction), ExtractError> {
    let options: TypeScriptOptions =
        serde_json::from_str(options).map_err(|e| ExtractError::UnsupportedFile {
            path: PathBuf::from("options"),
            reason: e.to_string(),
        })?;
    let roots = [PathBuf::from("src/index.js")];
    let (mut settings, config) = prepare(&options, &fixture("exclude"))?;
    settings.keep_file_states = keep;
    let full = extract_with(&roots, &settings, &config)?;
    let mut previous = full.clone();
    for module in &mut previous.modules {
        if module.source == "src/index.js" {
            module.dependencies.clear();
            module.experimental_stats = Some(rb_model::ExperimentalStats {
                top_level_statement_count: 99,
                size: 1,
            });
        }
    }
    let unchanged = full
        .modules
        .iter()
        .filter(|m| m.language.is_some())
        .map(|m| PathBuf::from(&m.source))
        .collect();
    let (settings, config) = prepare(&options, &fixture("exclude"))?;
    let request = ExtractRequest {
        changed: Vec::new(),
        unchanged,
        previous,
        walk_unchanged: false,
    };
    Ok((
        full,
        extract_incremental(&roots, &settings, &config, &request)?,
    ))
}

#[test]
fn an_unchanged_file_is_taken_from_the_earlier_extraction_not_read() -> Result<(), ExtractError> {
    let (full, reused) = tampered(r#"{"experimentalStats": true}"#, true)?;
    assert_eq!(
        sources(&full),
        [
            "src/index.js",
            "src/excluded/x.js",
            "src/kept.js",
            "src/lazy.js"
        ]
    );
    // The emptied list is what the walk replays over, so nothing below the root is reached, and
    // the statistics are the earlier run's.
    assert_eq!(sources(&reused), ["src/index.js"]);
    let root = module(&reused, "src/index.js");
    assert_eq!(
        root.and_then(|m| m.experimental_stats)
            .map(|s| s.top_level_statement_count),
        Some(99)
    );
    Ok(())
}

#[test]
fn a_filter_the_document_cannot_undo_reads_every_file() -> Result<(), ExtractError> {
    // `exclude.dynamic` removed an edge the walk followed, so the kept list is not the file's
    // result: the tampered earlier result is ignored and the file is read.
    let options = r#"{"exclude": {"path": "excluded", "dynamic": true}}"#;
    let (full, again) = tampered(options, true)?;
    assert_eq!(again.modules, full.modules);
    assert_eq!(
        sources(&again),
        ["src/index.js", "src/kept.js", "src/lazy.js"]
    );
    let parsed: TypeScriptOptions = serde_json::from_str(options).unwrap_or_default();
    let (settings, _) = prepare(&parsed, &fixture("exclude"))?;
    assert!(rb_extract_ts::reuse_refused(&settings).is_some_and(|r| r.contains("exclude.dynamic")));
    let (plain, _) = prepare(&TypeScriptOptions::default(), &fixture("exclude"))?;
    assert_eq!(rb_extract_ts::reuse_refused(&plain), None);
    Ok(())
}

#[test]
fn a_file_whose_code_layer_was_not_kept_is_read() -> Result<(), ExtractError> {
    // The earlier run kept no file states, and the code layer is on: nothing can be reused, so
    // the tampered list is ignored.
    let (full, again) = tampered("{}", false)?;
    assert!(full.files.is_empty());
    assert_eq!(again.modules, full.modules);
    assert_eq!(again.code, full.code);
    Ok(())
}

#[test]
fn a_dependency_survives_the_trip_through_the_document() {
    let json = r#"{"module":"./a","protocol":"node:","mimeType":"text/x","resolved":"a.js",
        "coreModule":true,"dependencyTypes":["local","type-only"],"license":"MIT",
        "followable":true,"dynamic":true,"exoticallyRequired":true,"exoticRequire":"need",
        "matchesDoNotFollow":true,"couldNotResolve":true,"preCompilationOnly":true,
        "moduleSystem":"cjs","valid":true,"circular":false,"line":3,"column":7}"#;
    let dependency: rb_model::Dependency = serde_json::from_str(json).unwrap_or_else(|e| {
        unreachable!("{e}");
    });
    let back = rb_extract_ts::from_dependency(&dependency);
    assert_eq!(back.module, "./a");
    assert_eq!(back.resolved, "a.js");
    assert_eq!(back.protocol, dependency.protocol);
    assert_eq!(back.mime_type.as_deref(), Some("text/x"));
    assert!(back.core_module && back.followable && back.dynamic && back.exotically_required);
    assert!(back.matches_do_not_follow && back.could_not_resolve);
    assert_eq!(back.exotic_require.as_deref(), Some("need"));
    assert_eq!(back.pre_compilation_only, Some(true));
    assert_eq!(back.license.as_deref(), Some("MIT"));
    assert_eq!(back.dependency_types, dependency.dependency_types);
    assert_eq!(back.module_system, rb_model::ModuleSystem::Cjs);
    assert_eq!((back.line, back.column), (3, 7));
    let bare = rb_extract_ts::from_dependency(&rb_model::Dependency::new(
        "b",
        "b",
        rb_model::ModuleSystem::Es6,
    ));
    assert!(!bare.matches_do_not_follow);
    assert_eq!((bare.line, bare.column), (0, 0));
}

/// The configuration files a cache keys a reused TypeScript extraction on: the tsconfig, its
/// `extends` chain, its project references (with their own chains), and a Babel configuration.
#[test]
fn the_configuration_files_follow_extends_and_references() {
    let root = fixture("ts-config");
    let options: TypeScriptOptions = serde_json::from_str(
        r#"{"tsConfig": {"fileName": "tsconfig.json"}, "babelConfig": {"fileName": ".babelrc"}}"#,
    )
    .unwrap_or_default();
    let files: Vec<String> = rb_extract_ts::configuration_files(&options, &root)
        .iter()
        .map(|p| {
            p.strip_prefix(&root)
                .map_or_else(|_| p.display().to_string(), |r| r.display().to_string())
                .replace('\\', "/")
        })
        .collect();
    for expected in [".babelrc", "tsconfig.base.json", "tsconfig.json"] {
        assert!(
            files.contains(&expected.to_owned()),
            "{expected}: {files:?}"
        );
    }
    assert!(
        files.iter().any(|f| f.starts_with("sub/")
            && std::path::Path::new(f)
                .extension()
                .is_some_and(|e| e == "json")),
        "the referenced project's tsconfig: {files:?}"
    );
    assert!(rb_extract_ts::configuration_files(&TypeScriptOptions::default(), &root).is_empty());
}
