//! The extractor end to end: both layers over two committed assemblies, compared with reviewed
//! expectations, and the attribution, determinism and failure cases.
//!
//! - Plan: [Wave 2, Step 3](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#23-step-3-attribution-edge-projection-the-net-code-layer-and-defaults-2a)
//!   ("`tests/extract.rs` runs the extractor over the fixture assembly and compares the module
//!   layer and the code layer against a committed JSON expectation"; the partial-class, no-PDB
//!   and repeated-run cases)
//! - Decisions: [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md),
//!   [ADR-0008](../../../docs/adr/0008-exit-code-contract.md) (a Windows PDB is untrustworthy)
//! - Fixtures: `conformance/archunitnet/fixtures/TestAssembly.{dll,pdb}`;
//!   `tests/fixtures/sample/` (see its `PROVENANCE.md`)
//!
//! `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-extract-dotnet --test extract` rewrites the
//! expectations; the diff is what a reviewer reads.

use std::path::{Path, PathBuf};

use rb_extract_dotnet::DotnetExtractor;
use rb_model::{
    Attribution, DependencyKind, DependencyType, DotnetOptions, ExtractError, Extraction, Extractor,
};
use serde_json::{Value, json};

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn sample() -> PathBuf {
    manifest().join("tests/fixtures/sample")
}

fn test_assembly() -> PathBuf {
    manifest().join("../../conformance/archunitnet/fixtures")
}

fn loader(glob: &str) -> DotnetOptions {
    DotnetOptions {
        assemblies: Some(vec![glob.to_owned()]),
        ..DotnetOptions::default()
    }
}

fn extract(root: &Path, options: &DotnetOptions) -> Result<Extraction, ExtractError> {
    DotnetExtractor.extract(&[root.to_path_buf()], options)
}

fn as_json(extraction: &Extraction) -> Result<Value, serde_json::Error> {
    Ok(json!({
        "inspected": serde_json::to_value(&extraction.inspected)?,
        "warnings": extraction.warnings.iter().map(|w| w.message.clone()).collect::<Vec<_>>(),
        "modules": serde_json::to_value(&extraction.modules)?,
        "code": serde_json::to_value(&extraction.code)?,
    }))
}

fn matches_expectation(
    name: &str,
    extraction: &Extraction,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = manifest()
        .join("tests/fixtures")
        .join(format!("{name}.expected.json"));
    let actual = format!("{}\n", serde_json::to_string_pretty(&as_json(extraction)?)?);
    if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &actual)?;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        expected == actual,
        "{} differs from the extraction; regenerate with RB_UPDATE_SNAPSHOTS=1 and review the diff",
        path.display()
    );
    Ok(())
}

#[test]
fn test_assembly_matches_its_expectation() -> Result<(), Box<dyn std::error::Error>> {
    let extraction = extract(&test_assembly(), &loader("TestAssembly.dll"))?;
    assert_eq!(
        extraction.inspected.attribution.map(|a| (a.pdb, a.none)),
        Some((45, 0))
    );
    matches_expectation("test-assembly", &extraction)
}

#[test]
fn sample_matches_its_expectation() -> Result<(), Box<dyn std::error::Error>> {
    matches_expectation("sample", &extract(&sample(), &loader("built/Sample.dll"))?)
}

#[test]
fn two_runs_serialise_byte_for_byte() -> Result<(), Box<dyn std::error::Error>> {
    let first = as_json(&extract(&sample(), &loader("built/Sample.dll"))?)?.to_string();
    let second = as_json(&extract(&sample(), &loader("built/Sample.dll"))?)?.to_string();
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn a_partial_type_lists_every_file_and_edges_leave_the_file_that_declares_them()
-> Result<(), Box<dyn std::error::Error>> {
    let extraction = extract(&sample(), &loader("built/Sample.dll"))?;
    let code = extraction.code.unwrap_or_default();
    let order = code
        .types
        .iter()
        .find(|t| t.full_name == "Sample.Orders.Order")
        .ok_or("no Order")?;
    assert_eq!(
        order.location.file.as_deref(),
        Some("src/Order.cs"),
        "the first constructor's file"
    );
    assert_eq!(order.files, ["src/Order.Lines.cs"]);
    assert_eq!(order.attribution, Some(Attribution::Pdb));
    let lines = extraction
        .modules
        .iter()
        .find(|m| m.source == "src/Order.Lines.cs")
        .ok_or("no Order.Lines.cs module")?;
    assert!(
        lines
            .dependencies
            .iter()
            .any(|d| d.resolved == "src/Customer.cs"),
        "the async method in Order.Lines.cs creates a Customer: {:?}",
        lines
            .dependencies
            .iter()
            .map(|d| &d.resolved)
            .collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn types_mirror_archunitnet() -> Result<(), Box<dyn std::error::Error>> {
    let extraction = extract(&sample(), &loader("built/Sample.dll"))?;
    let code = extraction.code.unwrap_or_default();
    let ty = |name: &str| code.types.iter().find(|t| t.full_name == name);
    assert_eq!(
        ty("Sample.Customers.Customer").and_then(|t| t.record),
        Some(true)
    );
    assert_eq!(
        ty("Sample.Customers.Money").map(|t| t.kind.as_str()),
        Some("struct")
    );
    assert_eq!(
        ty("Sample.Customers.Tier").map(|t| t.kind.as_str()),
        Some("enum")
    );
    assert_eq!(
        ty("Sample.IEntity").map(|t| t.kind.as_str()),
        Some("interface")
    );
    assert_eq!(
        ty("Sample.AuditAttribute").map(|t| t.kind.as_str()),
        Some("attribute")
    );
    assert_eq!(
        ty("Sample.Customers.Constants").and_then(|t| t.r#static),
        Some(true)
    );
    assert_eq!(
        ty("Sample.Customers.Repository`1").and_then(|t| t.generic),
        Some(true)
    );
    assert_eq!(
        ty("Sample.Marker").and_then(|t| t.visibility.as_deref()),
        Some("internal")
    );
    let line = ty("Sample.Orders.Order+Line").ok_or("no nested Line")?;
    assert_eq!(line.nested_in.as_deref(), Some("Sample.Orders.Order"));
    assert_eq!(line.sealed, Some(true));
    assert!(
        code.types.iter().all(|t| !t.full_name.contains('<')),
        "no compiler-generated types"
    );
    assert!(
        // `Owner+<T>` is ArchUnitNET's name for a generic parameter; `Owner+<Positive>d__12` is
        // a compiler-generated type.
        code.types
            .iter()
            .flat_map(|t| &t.dependencies)
            .all(|d| !d.target.contains("+<") || d.target.ends_with('>')),
        "no dependency on a compiler-generated type, not even through typeof in an attribute"
    );
    Ok(())
}

#[test]
fn members_attributes_and_calls_mirror_archunitnet() -> Result<(), Box<dyn std::error::Error>> {
    let extraction = extract(&sample(), &loader("built/Sample.dll"))?;
    let code = extraction.code.unwrap_or_default();
    let member = |name: &str| {
        code.members
            .iter()
            .find(|m| m.full_name.as_deref() == Some(name))
    };
    let id = member("System.Int32 Sample.Orders.Order::Id()").ok_or("no Id")?;
    assert!(id.init_setter.is_some() && id.setter.is_none());
    let quantity =
        member("System.Int32 Sample.Orders.Order/Line::Quantity()").ok_or("no Quantity")?;
    assert_eq!(
        quantity.setter.as_ref().map(|s| s.visibility.as_str()),
        Some("private")
    );
    assert!(
        code.members
            .iter()
            .all(|m| !m.name.contains("k__BackingField"))
    );
    let total = member("System.Decimal Sample.Orders.Order::Total()").ok_or("no Total")?;
    assert!(
        total
            .dependencies
            .iter()
            .any(|d| d.target == "System.Linq.Enumerable" && d.kind == "body"),
        "the lambda's LINQ calls are followed into: {:?}",
        total.dependencies
    );
    // A Release build makes an async state machine a struct, created without `newobj`, so
    // ArchUnitNET's HandleAsync does not follow it and neither do we: the result type is still a
    // generic argument of the signature.
    let load = member("System.Threading.Tasks.Task`1<Sample.Customers.Customer> Sample.Orders.Order::LoadOwnerAsync()")
        .ok_or("no LoadOwnerAsync")?;
    assert!(
        load.dependencies
            .iter()
            .any(|d| d.target == "Sample.Customers.Customer" && d.kind == "generic-argument")
    );
    // An iterator's state machine is always a class: its MoveNext is read, so the property
    // getter it calls on each line is a body dependency of Positive().
    let positive = member("System.Collections.Generic.IEnumerable`1<Sample.Orders.Order/Line> Sample.Orders.Order::Positive()")
        .ok_or("no Positive")?;
    assert!(
        positive
            .dependencies
            .iter()
            .any(|d| d.target == "Sample.Orders.Order+Line"
                && d.member.as_deref() == Some("get_Quantity")),
        "{:?}",
        positive.dependencies
    );
    assert_eq!(
        positive.location.file.as_deref(),
        Some("src/Order.Lines.cs"),
        "located by its state machine's statements"
    );
    let marker_attributes: Vec<&str> = code
        .attributes
        .iter()
        .filter(|a| a.target == "Sample.Orders.Order")
        .map(|a| a.attribute_type.as_str())
        .collect();
    assert_eq!(marker_attributes, ["Sample.AuditAttribute"]);
    let audit = code
        .attributes
        .iter()
        .find(|a| a.target == "Sample.Orders.Order")
        .ok_or("no audit")?;
    assert_eq!(audit.arguments, ["orders"]);
    assert_eq!(
        audit
            .named_arguments
            .first()
            .map(|n| (n.name.as_str(), n.value.as_str())),
        Some(("Level", "2"))
    );
    assert!(
        code.calls.iter().any(|c| c.from
            == "System.Void Sample.Orders.Order::Add(Sample.Orders.Order/Line)"
            && c.to.contains("List`1")),
        "{:?}",
        code.calls
    );
    Ok(())
}

#[test]
fn edges_carry_kinds_types_and_the_dynamic_flag() -> Result<(), Box<dyn std::error::Error>> {
    let extraction = extract(&sample(), &loader("built/Sample.dll"))?;
    let lines = extraction
        .modules
        .iter()
        .find(|m| m.source == "src/Order.Lines.cs")
        .ok_or("no Order.Lines.cs")?;
    let dynamic = lines
        .dependencies
        .iter()
        .find(|d| d.dynamic)
        .ok_or("no dynamic edge for Type.GetType(\"Sample.Customers.Customer\")")?;
    assert_eq!(dynamic.resolved, "src/Customer.cs");
    assert_eq!(dynamic.dependency_types, [DependencyType::Local]);
    let framework = extraction
        .modules
        .iter()
        .find(|m| m.source == "System.Runtime")
        .ok_or("no System.Runtime")?;
    assert_eq!(framework.core_module, Some(true));
    assert_eq!(framework.followable, Some(false));
    let order = extraction
        .modules
        .iter()
        .find(|m| m.source == "src/Order.cs")
        .ok_or("no Order.cs")?;
    let kinds: Vec<DependencyKind> = order
        .dependencies
        .iter()
        .filter_map(|d| d.dependency_kind)
        .collect();
    assert!(kinds.contains(&DependencyKind::Implements), "{kinds:?}");
    assert!(kinds.contains(&DependencyKind::Attribute), "{kinds:?}");
    assert!(
        order
            .dependencies
            .iter()
            .all(|d| d.line.is_none_or(|l| l > 0))
    );
    Ok(())
}

fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(from, to).map(|_| ())
}

#[test]
fn a_missing_pdb_gives_attribution_none_and_a_warning() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join(format!("rb-nopdb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(
        &test_assembly().join("TestAssembly.dll"),
        &dir.join("TestAssembly.dll"),
    )?;
    let extraction = extract(&dir, &loader("*.dll"))?;
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        extraction
            .inspected
            .attribution
            .map(|a| (a.pdb, a.inferred)),
        Some((0, 0))
    );
    assert!(
        extraction
            .warnings
            .iter()
            .any(|w| w.message.contains("DebugType=portable"))
    );
    let code = extraction.code.unwrap_or_default();
    assert!(!code.types.is_empty(), "element rules still see the types");
    assert!(
        code.types
            .iter()
            .all(|t| t.attribution == Some(Attribution::None))
    );
    Ok(())
}

#[test]
fn a_windows_pdb_is_untrustworthy() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join(format!("rb-winpdb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(
        &test_assembly().join("TestAssembly.dll"),
        &dir.join("TestAssembly.dll"),
    )?;
    std::fs::write(
        dir.join("TestAssembly.pdb"),
        b"Microsoft C/C++ MSF 7.00\r\n",
    )?;
    let result = extract(&dir, &loader("*.dll"));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        matches!(result, Err(ExtractError::NonPortablePdb { .. })),
        "{result:?}"
    );
    Ok(())
}

#[test]
fn a_solution_with_nothing_built_is_untrustworthy() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join(format!("rb-unbuilt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/App"))?;
    std::fs::write(
        dir.join("App.slnx"),
        r#"<Solution><Project Path="src/App/App.csproj"/></Solution>"#,
    )?;
    std::fs::write(dir.join("src/App/App.csproj"), "<Project/>")?;
    let result = extract(&dir, &DotnetOptions::default());
    let _ = std::fs::remove_dir_all(&dir);
    let message = result
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    assert!(
        matches!(result, Err(ExtractError::NoBuiltAssemblies { .. })),
        "{result:?}"
    );
    assert!(message.contains("dotnet build"), "{message}");
    assert!(extract(Path::new("/nonexistent-rb"), &DotnetOptions::default()).is_err());
    assert!(matches!(
        DotnetExtractor.extract(&[], &DotnetOptions::default()),
        Err(ExtractError::NoModulesFound)
    ));
    Ok(())
}

#[test]
fn a_corrupt_assembly_is_a_named_error() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join(format!("rb-corrupt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let bytes = std::fs::read(test_assembly().join("TestAssembly.dll"))?;
    std::fs::write(dir.join("Broken.dll"), &bytes[..bytes.len() / 3])?;
    let result = extract(&dir, &loader("*.dll"));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        matches!(result, Err(ExtractError::UnsupportedFile { .. })),
        "{result:?}"
    );
    Ok(())
}
