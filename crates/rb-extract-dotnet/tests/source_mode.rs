//! `--mode source` end to end: a fixture solution with partial classes, global usings, aliases
//! and nested namespaces compared with a reviewed expectation; what is skipped; determinism; an
//! incremental run equal to a full one; and the precision and recall of source edges against
//! compiled edges on the fixtures that have both.
//!
//! - Plan: [Wave 3, Step 14](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)
//!   ("a fixture solution with partial classes, global usings, aliases and nested namespaces; and
//!   a precision test ... into `target/source-mode-precision.json`, asserted at or above 90%
//!   precision")
//! - Decision: [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md)
//! - Requirement: [FR-EXT-DN-04](../../../docs/prd.md#fr-ext-dn-04)
//!
//! `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-extract-dotnet --features source-mode --test source_mode`
//! rewrites the expectation.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rb_extract_dotnet::DotnetExtractor;
use rb_model::{
    Attribution, DotnetMode, DotnetOptions, ExtractError, ExtractRequest, Extraction, Extractor,
};
use serde_json::{Value, json};

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn fixture(name: &str) -> PathBuf {
    manifest().join("tests/fixtures").join(name)
}

fn source_options() -> DotnetOptions {
    DotnetOptions {
        mode: Some(DotnetMode::Source),
        ..DotnetOptions::default()
    }
}

fn source(root: &Path) -> Result<Extraction, ExtractError> {
    DotnetExtractor.extract(&[root.to_path_buf()], &source_options())
}

fn as_json(extraction: &Extraction) -> Result<Value, serde_json::Error> {
    Ok(json!({
        "inspected": serde_json::to_value(&extraction.inspected)?,
        "warnings": extraction.warnings.iter().map(|w| w.message.clone()).collect::<Vec<_>>(),
        "modules": serde_json::to_value(&extraction.modules)?,
    }))
}

/// (from, to) for every edge between two file modules.
fn file_edges(modules: &[rb_model::Module]) -> BTreeSet<(String, String)> {
    let files: BTreeSet<&str> = modules
        .iter()
        .filter(|m| m.followable == Some(true))
        .map(|m| m.source.as_str())
        .collect();
    modules
        .iter()
        .filter(|m| files.contains(m.source.as_str()))
        .flat_map(|m| {
            m.dependencies
                .iter()
                .filter(|d| files.contains(d.resolved.as_str()))
                .map(|d| (m.source.clone(), d.resolved.clone()))
        })
        .collect()
}

#[test]
fn the_fixture_solution_matches_its_expectation() -> Result<(), Box<dyn std::error::Error>> {
    let extraction = source(&fixture("source"))?;
    let actual = serde_json::to_string_pretty(&as_json(&extraction)?)? + "\n";
    let path = fixture("source.expected.json");
    if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &actual)?;
    }
    // Compared as JSON: a workspace build turns on serde_json's `preserve_order`, which changes
    // key order but not content.
    let expected: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    assert!(
        expected == serde_json::from_str::<Value>(&actual)?,
        "{} differs from the extraction; rerun with RB_UPDATE_SNAPSHOTS=1 and review the diff",
        path.display()
    );
    Ok(())
}

#[test]
fn every_edge_is_approximate_and_every_file_is_attributed_to_source()
-> Result<(), Box<dyn std::error::Error>> {
    let extraction = source(&fixture("source"))?;
    assert_eq!(extraction.inspected.mode, Some(DotnetMode::Source));
    assert_eq!(extraction.inspected.assemblies, 0);
    assert!(extraction.code.is_none());
    for module in &extraction.modules {
        if module.followable == Some(true) {
            assert_eq!(
                module.attribution,
                Some(Attribution::Source),
                "{}",
                module.source
            );
        }
        for dependency in &module.dependencies {
            assert_eq!(
                dependency.approximate,
                Some(true),
                "{} -> {}",
                module.source,
                dependency.resolved
            );
        }
    }
    Ok(())
}

#[test]
fn the_edges_the_fixture_is_built_to_show_are_there() -> Result<(), Box<dyn std::error::Error>> {
    let modules = source(&fixture("source"))?.modules;
    let edges = file_edges(&modules);
    for (from, to, why) in [
        (
            "Domain/Orders/Order.cs",
            "Domain/Customers/Customer.cs",
            "a global using",
        ),
        (
            "Domain/Orders/Order.Lines.cs",
            "Domain/Values/Amount.cs",
            "a namespace-relative qualified name",
        ),
        ("Domain/Pricing.cs", "Domain/Values/Amount.cs", "an alias"),
        (
            "Domain/Pricing.cs",
            "Domain/Orders/Order.Lines.cs",
            "a using static's nested type",
        ),
        (
            "Domain/Pricing.cs",
            "Domain/Customers/Customer.cs",
            "a global using",
        ),
        (
            "App/Program.cs",
            "Domain/Orders/Order.cs",
            "a partial type lands in its constructor's part",
        ),
        (
            "App/Program.cs",
            "Domain/Customers/Customer.cs",
            "a fully qualified name in top-level statements",
        ),
        (
            "Tests/OrderTests.cs",
            "Domain/Orders/Order.cs",
            "a project reference",
        ),
    ] {
        assert!(
            edges.contains(&(from.to_owned(), to.to_owned())),
            "{from} -> {to} ({why}) missing from {edges:#?}"
        );
    }
    // Each skipped file is in the fixture (`obj/` is committed with `git add -f`), so its
    // absence below is the walk's doing.
    for present in [
        "App/obj/Generated.cs",
        "App/View.Designer.cs",
        "Outside/Stray.cs",
    ] {
        assert!(
            fixture("source").join(present).is_file(),
            "{present} is missing from the fixture"
        );
    }
    let sources: BTreeSet<&str> = modules.iter().map(|m| m.source.as_str()).collect();
    for skipped in [
        "App/obj/Generated.cs",
        "App/View.Designer.cs",
        "Outside/Stray.cs",
        "Domain/GlobalUsings.cs",
    ] {
        assert!(
            !sources.contains(skipped),
            "{skipped} should not be a module"
        );
    }
    let program = modules.iter().find(|m| m.source == "App/Program.cs");
    let serilog = program.and_then(|m| m.dependencies.iter().find(|d| d.resolved == "Serilog"));
    assert_eq!(
        serilog.map(|d| d.dependency_types.clone()),
        Some(vec![rb_model::DependencyType::Package])
    );
    let tests = modules.iter().find(|m| m.source == "Tests/OrderTests.cs");
    assert!(tests.is_some_and(|m| m.dependencies.iter().all(|d| {
        d.dependency_types
            .contains(&rb_model::DependencyType::TestOnly)
    })));
    Ok(())
}

#[test]
fn two_runs_serialise_byte_for_byte() -> Result<(), Box<dyn std::error::Error>> {
    let first = serde_json::to_string(&as_json(&source(&fixture("source"))?)?)?;
    let second = serde_json::to_string(&as_json(&source(&fixture("source"))?)?)?;
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn an_incremental_run_equals_a_full_one() -> Result<(), Box<dyn std::error::Error>> {
    let root = fixture("source");
    let full = rb_extract_dotnet::source::extract(&root, &source_options(), None, true)?;
    assert!(!full.files.is_empty());
    let sources: Vec<PathBuf> = full.files.keys().map(PathBuf::from).collect();
    for changed in &sources {
        let request = ExtractRequest {
            changed: vec![changed.clone()],
            unchanged: sources.iter().filter(|s| *s != changed).cloned().collect(),
            previous: full.clone(),
        };
        let again =
            rb_extract_dotnet::source::extract(&root, &source_options(), Some(&request), true)?;
        assert_eq!(again, full, "changing {} only", changed.display());
    }
    // Facts that cannot be read are parsed again rather than trusted.
    let mut corrupt = full.clone();
    for state in corrupt.files.values_mut() {
        state.code = Some(json!({"not": "facts"}));
    }
    let request = ExtractRequest {
        changed: Vec::new(),
        unchanged: sources,
        previous: corrupt,
    };
    let again = rb_extract_dotnet::source::extract(&root, &source_options(), Some(&request), true)?;
    assert_eq!(again, full);
    Ok(())
}

#[test]
fn loader_keys_are_refused_with_a_named_reason() {
    let options = DotnetOptions {
        mode: Some(DotnetMode::Source),
        assemblies: Some(vec!["built/*.dll".into()]),
        ..DotnetOptions::default()
    };
    let result = DotnetExtractor.extract(&[fixture("sample")], &options);
    assert!(
        matches!(&result, Err(ExtractError::UnsupportedFile { reason, .. }) if reason.starts_with("source-mode-reads-no-assemblies")),
        "{result:?}"
    );
}

#[test]
fn a_folder_with_no_csharp_finds_nothing() -> Result<(), Box<dyn std::error::Error>> {
    let empty = std::env::temp_dir().join(format!("rb-source-empty-{}", std::process::id()));
    std::fs::create_dir_all(&empty)?;
    let result = source(&empty);
    std::fs::remove_dir_all(&empty)?;
    assert!(
        matches!(result, Err(ExtractError::NoModulesFound)),
        "{result:?}"
    );
    Ok(())
}

#[test]
fn the_namespaces_option_keeps_the_listed_namespaces() -> Result<(), Box<dyn std::error::Error>> {
    let options = DotnetOptions {
        mode: Some(DotnetMode::Source),
        namespaces: Some(vec!["Shop.Domain.Orders".into()]),
        ..DotnetOptions::default()
    };
    let modules = DotnetExtractor
        .extract(&[fixture("source")], &options)?
        .modules;
    let files: BTreeSet<&str> = modules
        .iter()
        .filter(|m| m.followable == Some(true))
        .map(|m| m.source.as_str())
        .collect();
    assert_eq!(
        files,
        BTreeSet::from(["Domain/Orders/Order.Lines.cs", "Domain/Orders/Order.cs"])
    );
    for module in &modules {
        for d in &module.dependencies {
            assert!(
                !Path::new(&d.resolved)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("cs"))
                    || files.contains(d.resolved.as_str()),
                "{}",
                d.resolved
            );
        }
    }
    Ok(())
}

/// Precision and recall of source edges against compiled edges, over the files both modes
/// know.
fn agreement(compiled: &[rb_model::Module], source: &[rb_model::Module]) -> (usize, usize, usize) {
    let known: BTreeSet<&str> = compiled
        .iter()
        .filter(|m| m.followable == Some(true))
        .map(|m| m.source.as_str())
        .collect();
    let within = |edges: BTreeSet<(String, String)>| -> BTreeSet<(String, String)> {
        edges
            .into_iter()
            .filter(|(f, t)| known.contains(f.as_str()) && known.contains(t.as_str()))
            .collect()
    };
    let compiled = within(file_edges(compiled));
    let source = within(file_edges(source));
    let both = compiled.intersection(&source).count();
    (source.len(), compiled.len(), both)
}

/// One fixture's agreement: its name, the source and compiled edge counts, how many agree.
fn measure(name: &str, dll: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let root = fixture(name);
    let compiled = DotnetExtractor.extract(
        std::slice::from_ref(&root),
        &DotnetOptions {
            assemblies: Some(vec![dll.into()]),
            ..DotnetOptions::default()
        },
    )?;
    let source = source(&root)?;
    let (source_count, compiled_count, both) = agreement(&compiled.modules, &source.modules);
    let ratio = |part: usize, whole: usize| {
        let (part, whole) = (
            u32::try_from(part).unwrap_or(u32::MAX),
            u32::try_from(whole).unwrap_or(u32::MAX),
        );
        if whole == 0 {
            1.0
        } else {
            f64::from(part) / f64::from(whole)
        }
    };
    Ok(json!({
        "name": format!("rb-extract-dotnet/tests/fixtures/{name}"),
        "sourceEdges": source_count,
        "compiledEdges": compiled_count,
        "agreeing": both,
        "precision": ratio(both, source_count),
        "recall": ratio(both, compiled_count),
    }))
}

#[test]
fn source_edges_agree_with_compiled_edges_on_the_built_fixtures()
-> Result<(), Box<dyn std::error::Error>> {
    let fixtures = vec![
        measure("sample", "built/Sample.dll")?,
        measure("toplevel", "built/TopLevel.dll")?,
    ];
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| manifest().join("../../target"), PathBuf::from);
    std::fs::create_dir_all(&target)?;
    std::fs::write(
        target.join("source-mode-precision.json"),
        serde_json::to_string_pretty(&json!({ "fixtures": fixtures }))? + "\n",
    )?;
    for fixture in &fixtures {
        assert!(fixture["compiledEdges"].as_u64() > Some(0), "{fixture}");
        assert!(fixture["precision"].as_f64() >= Some(0.9), "{fixture}");
        assert!(fixture["recall"].as_f64() >= Some(0.9), "{fixture}");
    }
    Ok(())
}
