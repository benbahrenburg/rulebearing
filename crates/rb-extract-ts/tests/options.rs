//! One fixture per Wave 1 option row: a mini-repository under `tests/options/<option>/`, extracted
//! through the same entry points `TypeScriptExtractor::extract` uses, and the modules and
//! dependencies it yields asserted.
//!
//! - Plan: [Wave 1, Step 10](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-10-rb-extract-ts-to-100-and-the-option-set-1c)
//!   ("every new option gets a fixture under `rb-extract-ts/tests/options/`") and
//!   [Wave 1C](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#wave-1c-rb-extract-ts-completion)
//! - Source: [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   the Wave 1 rows; [coverage § Extraction and resolution](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution)
//! - Decisions: [ADR-0012](../../../docs/adr/0012-oxc-for-typescript.md),
//!   [ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md) (the sidecar failure)
//! - Requirements: [FR-EXT-TS-02](../../../docs/prd.md#fr-ext-ts-02), [FR-EXT-TS-03](../../../docs/prd.md#fr-ext-ts-03)
//!
//! Each fixture is extracted with its own folder as the working directory, passed to
//! [`rb_extract_ts::prepare`] rather than set on the process, so the tests run in parallel.

use std::path::{Path, PathBuf};

use rb_extract_ts::{TypeScriptExtractor, extract_with, prepare};
use rb_model::{ExtractError, Extraction, Extractor, Module, TypeScriptOptions};

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
    let (settings, config) = prepare(&options, cwd)?;
    let roots: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
    extract_with(&roots, &settings, &config)
}

fn run(name: &str, options: &str, roots: &[&str]) -> Result<Extraction, ExtractError> {
    run_in(&fixture(name), options, roots)
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
