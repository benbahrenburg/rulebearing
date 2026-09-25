//! The module layer: each type-level dependency projected from a type pair to a file pair, and
//! classified with the .NET `dependencyTypes`.
//!
//! - Plan: [Wave 2, Step 3](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#23-step-3-attribution-edge-projection-the-net-code-layer-and-defaults-2a)
//!   (`project.rs`: the edge table of § 1.4.2) and § 1.7 (`to.license` read from the `.nuspec`)
//! - Source: [design § One engine, three languages](../../../docs/artifacts/design.md#one-engine-three-languages-one-monorepo)
//!   (the .NET `dependencyTypes`)
//! - Decisions: [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md),
//!   [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md) (a target outside
//!   the loaded code is a named module, never a guessed file)
//!
//! One dependency per source file, target and `dependencyKind`, located at the first reference;
//! `member` names the first member reference in sort order, so two runs agree.
//!
//! | `dependencyTypes` | When |
//! | --- | --- |
//! | `local` | the target type is defined in the same project |
//! | `project` | in another loaded project |
//! | `package` | in an assembly a `PackageReference` of the source project names, directly or through its `ProjectReference`s; or a non-framework assembly found beside the built output (a package arriving transitively) |
//! | `framework` | in a `System.*`, `Microsoft.*`, `mscorlib` or `netstandard` assembly the project does not reference as a package |
//! | `unresolved` | anywhere else outside the loaded set |
//! | `test-only` | added when the source project is a test project |
//! | `signature-only` | added when every edge between the pair is a `signature` edge |
//!
//! An external target is the module named by its assembly (`Newtonsoft.Json`), with
//! `coreModule` for the framework and `couldNotResolve` for `unresolved`, as dependency-cruiser
//! names an npm package by its name. Two projects may classify one assembly differently (one
//! references the package, the other does not); the module takes the most resolved answer
//! (`package`, then `framework`, then `unresolved`), with a licence over none, so its fields
//! never depend on which file sorts first.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rb_model::{Dependency, DependencyKind, DependencyType, Language, Module, ModuleSystem};

use crate::codelayer::TypeDependency;
use crate::discover::{PackageRef, Project};
use crate::names::{Resolved, Universe};

/// The assembly a primitive's type belongs to, for a target named only by `System.*` name.
pub const PRIMITIVE_ASSEMBLY: &str = "System.Runtime";

/// Per loaded assembly: its project and the file each of its types was attributed to.
#[derive(Debug)]
pub struct AssemblyFiles<'a> {
    /// The project the assembly was built from.
    pub project: &'a Project,
    /// The project file, repository-relative.
    pub project_path: String,
    /// Type full name to file.
    pub files: BTreeMap<String, String>,
}

/// Where a dependency lands.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Landing {
    File(String, bool),
    External(String, DependencyType, Option<String>),
}

/// Whether an assembly name is the framework's.
pub fn is_framework(assembly: &str) -> bool {
    let lower = assembly.to_ascii_lowercase();
    lower == "mscorlib"
        || lower == "netstandard"
        || lower == "system"
        || lower.starts_with("system.")
        || lower.starts_with("microsoft.")
        || lower.starts_with("windowsbase")
        || lower.starts_with("presentation")
}

/// The package reference that provides `assembly`: an id equal to it, or an id it extends
/// (`Serilog.Sinks.Console` is provided by a reference to `Serilog.Sinks.Console`, and
/// `AutoMapper.Extensions` by `AutoMapper.Extensions`, never by a mere prefix of the id).
pub fn providing_package<'p>(assembly: &str, packages: &'p [PackageRef]) -> Option<&'p PackageRef> {
    packages
        .iter()
        .find(|p| p.id.eq_ignore_ascii_case(assembly))
        .or_else(|| {
            packages
                .iter()
                .filter(|p| {
                    assembly
                        .get(..p.id.len())
                        .is_some_and(|head| head.eq_ignore_ascii_case(&p.id))
                        && assembly.as_bytes().get(p.id.len()) == Some(&b'.')
                })
                .max_by_key(|p| p.id.len())
        })
}

/// The licence a NuGet package declares in its `.nuspec` in the global packages folder
/// (`$NUGET_PACKAGES`, else `~/.nuget/packages`): `<license>`, else `<licenseUrl>`. Absent means
/// no licence field, never a guess (plan § 1.7).
pub fn package_license(package: &PackageRef, packages_root: Option<&Path>) -> Option<String> {
    let root = packages_root?;
    let id = package.id.to_ascii_lowercase();
    let version = package.version.as_deref()?.to_ascii_lowercase();
    let text =
        std::fs::read_to_string(root.join(&id).join(&version).join(format!("{id}.nuspec"))).ok()?;
    let document = roxmltree::Document::parse(&text).ok()?;
    let find = |tag: &str| {
        document
            .descendants()
            .find(|n| n.has_tag_name(tag))
            .and_then(|n| n.text())
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
    };
    find("license").or_else(|| find("licenseUrl"))
}

/// Package licences, each `.nuspec` read once per (id, version) however many edges name it.
#[derive(Debug)]
struct Licenses<'a> {
    root: Option<&'a Path>,
    cache: BTreeMap<(String, Option<String>), Option<String>>,
    reads: usize,
}

impl<'a> Licenses<'a> {
    fn new(root: Option<&'a Path>) -> Self {
        Self {
            root,
            cache: BTreeMap::new(),
            reads: 0,
        }
    }

    fn get(&mut self, package: &PackageRef) -> Option<String> {
        let key = (package.id.to_ascii_lowercase(), package.version.clone());
        if let Some(found) = self.cache.get(&key) {
            return found.clone();
        }
        self.reads += 1;
        let found = package_license(package, self.root);
        self.cache.insert(key, found.clone());
        found
    }
}

/// The global packages folder: `$NUGET_PACKAGES`, else `~/.nuget/packages`.
pub fn packages_root() -> Option<PathBuf> {
    std::env::var_os("NUGET_PACKAGES")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".nuget/packages"))
        })
}

/// How resolved an external classification is, lower first: the module keeps the lowest.
fn resolution_rank(kind: DependencyType) -> u8 {
    match kind {
        DependencyType::Package => 0,
        DependencyType::Framework => 1,
        _ => 2,
    }
}

/// The first reference of one grouped edge: line, column, member references, whether any was
/// dynamic, and the first target type in sort order.
type FirstReference = (Option<u32>, Option<u32>, BTreeSet<String>, bool, String);

/// Projects the type-level dependencies onto the modules, which must be sorted by source.
#[expect(
    clippy::too_many_lines,
    reason = "group, classify, then attach: one projection kept in one place so the grouping key is visible"
)]
///
/// `beside` holds the simple names (lowercase) of the assemblies found beside the built output:
/// a target in one of them that no package reference names is a `package` (a package arriving
/// transitively), never `unresolved`.
pub fn project(
    universe: &Universe<'_>,
    assemblies: &[AssemblyFiles<'_>],
    dependencies: &[TypeDependency],
    modules: &mut Vec<Module>,
    packages_root: Option<&Path>,
    beside: &BTreeSet<String>,
) {
    let mut licenses = Licenses::new(packages_root);
    // (from file, landing, kind) -> first reference.
    let mut grouped: BTreeMap<(String, Landing, DependencyKind), FirstReference> = BTreeMap::new();
    let mut from_test: BTreeMap<String, bool> = BTreeMap::new();
    for dep in dependencies {
        let Some(from) = &dep.file else {
            continue;
        };
        let Some(source) = assemblies.get(dep.assembly) else {
            continue;
        };
        let target_name = universe.full_name(&dep.target);
        let landing = match &dep.target {
            Resolved::Def { assembly, .. } => {
                let Some(files) = assemblies.get(*assembly) else {
                    continue;
                };
                let Some(file) = files.files.get(&target_name) else {
                    continue;
                };
                if file == from {
                    continue;
                }
                Landing::File(file.clone(), *assembly == dep.assembly)
            }
            Resolved::External { assembly, .. } => {
                let name = assembly
                    .clone()
                    .unwrap_or_else(|| PRIMITIVE_ASSEMBLY.to_owned());
                match providing_package(&name, &source.project.package_refs) {
                    Some(package) => {
                        let license = licenses.get(package);
                        Landing::External(name, DependencyType::Package, license)
                    }
                    None if is_framework(&name) => {
                        Landing::External(name, DependencyType::Framework, None)
                    }
                    None if beside.contains(&name.to_ascii_lowercase()) => {
                        Landing::External(name, DependencyType::Package, None)
                    }
                    None => Landing::External(name, DependencyType::Unresolved, None),
                }
            }
        };
        from_test.insert(from.clone(), source.project.is_test);
        let entry = grouped
            .entry((from.clone(), landing, dep.kind))
            .or_insert_with(|| {
                (
                    dep.line,
                    dep.column,
                    BTreeSet::new(),
                    false,
                    target_name.clone(),
                )
            });
        if let Some(member) = &dep.member {
            entry.2.insert(format!("{target_name}.{member}"));
        }
        entry.3 |= dep.dynamic;
        if target_name < entry.4 && !target_name.is_empty() {
            entry.4.clone_from(&target_name);
        }
    }
    // Pairs whose every edge is a signature edge.
    let mut kinds_by_pair: BTreeMap<(String, Landing), BTreeSet<DependencyKind>> = BTreeMap::new();
    for (from, landing, kind) in grouped.keys() {
        kinds_by_pair
            .entry((from.clone(), landing.clone()))
            .or_default()
            .insert(*kind);
    }
    let mut externals: BTreeMap<String, (DependencyType, Option<String>)> = BTreeMap::new();
    let mut by_source: BTreeMap<String, Vec<Dependency>> = BTreeMap::new();
    for ((from, landing, kind), (line, column, members, dynamic, first_target)) in grouped {
        let signature_only = kinds_by_pair
            .get(&(from.clone(), landing.clone()))
            .is_some_and(|kinds| kinds.iter().all(|k| *k == DependencyKind::Signature));
        let (resolved, mut types, core, unresolved, license) = match &landing {
            Landing::File(file, same) => (
                file.clone(),
                vec![if *same {
                    DependencyType::Local
                } else {
                    DependencyType::Project
                }],
                false,
                false,
                None,
            ),
            Landing::External(name, kind, license) => {
                let candidate = (*kind, license.clone());
                externals
                    .entry(name.clone())
                    .and_modify(|current| {
                        let better = (resolution_rank(candidate.0), candidate.1.is_none())
                            < (resolution_rank(current.0), current.1.is_none());
                        if better {
                            current.clone_from(&candidate);
                        }
                    })
                    .or_insert(candidate);
                (
                    name.clone(),
                    vec![*kind],
                    *kind == DependencyType::Framework,
                    *kind == DependencyType::Unresolved,
                    license.clone(),
                )
            }
        };
        if from_test.get(&from).copied().unwrap_or(false) {
            types.push(DependencyType::TestOnly);
        }
        if signature_only {
            types.push(DependencyType::SignatureOnly);
        }
        let mut dependency = Dependency::new(first_target, resolved, ModuleSystem::Clr);
        dependency.dependency_types = types;
        dependency.core_module = core;
        dependency.could_not_resolve = unresolved;
        dependency.followable = matches!(landing, Landing::File(..));
        dependency.dynamic = dynamic;
        dependency.license = license;
        dependency.line = line;
        dependency.column = column;
        dependency.dependency_kind = Some(kind);
        dependency.member = members.into_iter().next();
        by_source.entry(from).or_default().push(dependency);
    }
    for module in modules.iter_mut() {
        if let Some(mut deps) = by_source.remove(&module.source) {
            deps.sort_by(|a, b| {
                (&a.resolved, a.dependency_kind, &a.module).cmp(&(
                    &b.resolved,
                    b.dependency_kind,
                    &b.module,
                ))
            });
            module.dependencies = deps;
        }
    }
    for (name, (kind, license)) in externals {
        if modules.iter().any(|m| m.source == name) {
            continue;
        }
        let mut module = Module::new(name);
        module.language = Some(Language::Dotnet);
        module.followable = Some(false);
        module.core_module = Some(kind == DependencyType::Framework);
        module.could_not_resolve = Some(kind == DependencyType::Unresolved);
        module.dependency_types = Some(vec![kind]);
        module.license = license;
        modules.push(module);
    }
    modules.sort_by(|a, b| a.source.cmp(&b.source));
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rb_model::Module;

    fn package(id: &str, version: Option<&str>) -> PackageRef {
        PackageRef {
            id: id.to_owned(),
            version: version.map(str::to_owned),
        }
    }

    #[test]
    fn framework_assemblies_are_recognised_by_name() {
        for (name, framework) in [
            ("System.Runtime", true),
            ("system.collections", true),
            ("Microsoft.Extensions.Logging", true),
            ("mscorlib", true),
            ("netstandard", true),
            ("System", true),
            ("Newtonsoft.Json", false),
            ("SystemX", false),
        ] {
            assert_eq!(is_framework(name), framework, "{name}");
        }
    }

    #[test]
    fn a_package_provides_its_own_assembly_and_its_dotted_extensions() {
        let packages = [
            package("Serilog", Some("3.1.1")),
            package("Serilog.Sinks", None),
        ];
        assert_eq!(
            providing_package("serilog", &packages).map(|p| p.id.as_str()),
            Some("Serilog")
        );
        assert_eq!(
            providing_package("Serilog.Sinks.Console", &packages).map(|p| p.id.as_str()),
            Some("Serilog.Sinks"),
            "the longest dotted prefix wins"
        );
        assert_eq!(providing_package("SerilogX", &packages), None);
        assert_eq!(providing_package("Other", &packages), None);
    }

    #[test]
    fn a_licence_comes_from_the_nuspec_or_nowhere() {
        let root = std::env::temp_dir().join(format!("rb-nuspec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("newtonsoft.json/13.0.3");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            dir.join("newtonsoft.json.nuspec"),
            r#"<package><metadata><id>Newtonsoft.Json</id><license type="expression">MIT</license></metadata></package>"#,
        );
        let old = root.join("old/1.0.0");
        let _ = std::fs::create_dir_all(&old);
        let _ = std::fs::write(
            old.join("old.nuspec"),
            "<package><metadata><licenseUrl>https://example.org/l</licenseUrl></metadata></package>",
        );
        assert_eq!(
            package_license(&package("Newtonsoft.Json", Some("13.0.3")), Some(&root)).as_deref(),
            Some("MIT")
        );
        assert_eq!(
            package_license(&package("Old", Some("1.0.0")), Some(&root)).as_deref(),
            Some("https://example.org/l")
        );
        assert_eq!(
            package_license(&package("Newtonsoft.Json", None), Some(&root)),
            None
        );
        assert_eq!(
            package_license(&package("Missing", Some("1")), Some(&root)),
            None
        );
        assert_eq!(package_license(&package("Old", Some("1.0.0")), None), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn dep(file: &str, target: Resolved, kind: DependencyKind) -> TypeDependency {
        TypeDependency {
            assembly: 0,
            from_type: "App.A".into(),
            file: Some(file.into()),
            target,
            kind,
            member: None,
            line: Some(3),
            column: Some(1),
            dynamic: false,
        }
    }

    fn external(name: &str, assembly: Option<&str>) -> Resolved {
        Resolved::External {
            full_name: name.into(),
            assembly: assembly.map(str::to_owned),
        }
    }

    /// A nuspec for Newtonsoft.Json 13.0.3 under `root`, and the dependencies of one test file.
    fn projection_inputs(root: &Path) -> Vec<TypeDependency> {
        let dir = root.join("newtonsoft.json/13.0.3");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            dir.join("newtonsoft.json.nuspec"),
            "<package><metadata><license>MIT</license></metadata></package>",
        );
        let mut dynamic = dep(
            "tests/A.cs",
            external("", Some("Plugins")),
            DependencyKind::Body,
        );
        dynamic.dynamic = true;
        vec![
            dep(
                "tests/A.cs",
                external("Newtonsoft.Json.JsonConvert", Some("Newtonsoft.Json")),
                DependencyKind::Body,
            ),
            dep(
                "tests/A.cs",
                external("System.String", None),
                DependencyKind::Signature,
            ),
            dep(
                "tests/A.cs",
                external("Other.Thing", Some("Other")),
                DependencyKind::Field,
            ),
            dynamic,
            TypeDependency {
                file: None,
                ..dep("x", external("System.Int32", None), DependencyKind::Body)
            },
        ]
    }

    #[test]
    fn projection_classifies_every_dotnet_dependency_type() {
        let root = std::env::temp_dir().join(format!("rb-edges-nuspec-{}", std::process::id()));
        let dependencies = projection_inputs(&root);
        let mut test_project = crate::discover::Project::loose(Path::new("tests/App.Tests.dll"));
        test_project.is_test = true;
        test_project.package_refs = vec![package("Newtonsoft.Json", Some("13.0.3"))];
        let assemblies = [AssemblyFiles {
            project: &test_project,
            project_path: "tests/App.Tests.csproj".into(),
            files: BTreeMap::new(),
        }];
        let universe = Universe::new(Vec::new());
        let mut modules = vec![Module::new("tests/A.cs")];
        project(
            &universe,
            &assemblies,
            &dependencies,
            &mut modules,
            Some(&root),
            &BTreeSet::new(),
        );
        let _ = std::fs::remove_dir_all(&root);
        let sources: Vec<&str> = modules.iter().map(|m| m.source.as_str()).collect();
        assert_eq!(
            sources,
            [
                "Newtonsoft.Json",
                "Other",
                "Plugins",
                "System.Runtime",
                "tests/A.cs"
            ]
        );
        let a = &modules[4];
        let edge = |resolved: &str| a.dependencies.iter().find(|d| d.resolved == resolved);
        let newtonsoft = edge("Newtonsoft.Json");
        assert_eq!(
            newtonsoft.map(|d| d.dependency_types.clone()),
            Some(vec![DependencyType::Package, DependencyType::TestOnly])
        );
        assert_eq!(newtonsoft.and_then(|d| d.license.as_deref()), Some("MIT"));
        assert_eq!(modules[0].license.as_deref(), Some("MIT"));
        let runtime = edge("System.Runtime");
        assert_eq!(
            runtime.map(|d| d.dependency_types.clone()),
            Some(vec![
                DependencyType::Framework,
                DependencyType::TestOnly,
                DependencyType::SignatureOnly
            ]),
            "only signature edges between the pair"
        );
        assert_eq!(
            runtime.map(|d| (d.core_module, d.followable)),
            Some((true, false))
        );
        let other = edge("Other");
        assert_eq!(other.map(|d| d.could_not_resolve), Some(true));
        assert_eq!(
            other.map(|d| d.dependency_types[0]),
            Some(DependencyType::Unresolved)
        );
        assert_eq!(edge("Plugins").map(|d| d.dynamic), Some(true));
        assert_eq!(modules[3].core_module, Some(true));
        assert_eq!(modules[1].could_not_resolve, Some(true));
        assert!(
            a.dependencies
                .iter()
                .all(|d| d.module_system == ModuleSystem::Clr && d.line == Some(3))
        );
        assert_eq!(
            a.dependencies.len(),
            4,
            "the dependency without a file is dropped"
        );
    }

    #[test]
    fn a_non_ascii_assembly_name_never_splits_a_character() {
        let packages = [package("Ab", None), package("A", None)];
        assert_eq!(
            providing_package("Aé.X", &packages).map(|p| p.id.as_str()),
            None
        );
        assert_eq!(
            providing_package("A.é", &packages).map(|p| p.id.as_str()),
            Some("A")
        );
        assert_eq!(providing_package("é", &[package("éé", None)]), None);
    }

    #[test]
    fn a_nuspec_is_read_once_per_package_version() {
        let root = std::env::temp_dir().join(format!("rb-licence-cache-{}", std::process::id()));
        let dir = root.join("p/1.0.0");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            dir.join("p.nuspec"),
            "<package><metadata><license>MIT</license></metadata></package>",
        );
        let mut licenses = Licenses::new(Some(&root));
        for _ in 0..5 {
            assert_eq!(
                licenses.get(&package("P", Some("1.0.0"))).as_deref(),
                Some("MIT")
            );
            assert_eq!(
                licenses.get(&package("p", Some("1.0.0"))).as_deref(),
                Some("MIT")
            );
        }
        assert_eq!(licenses.get(&package("P", Some("2.0.0"))), None);
        assert_eq!(licenses.reads, 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_assembly_beside_the_output_is_a_package_and_modules_take_the_most_resolved_answer() {
        // Two projects: one references Lib as a package, the other gets it transitively (it is
        // beside its output); a third assembly, Gone, is neither.
        let mut direct = crate::discover::Project::loose(Path::new("a/A.dll"));
        direct.package_refs = vec![package("Lib", Some("1.0.0"))];
        let transitive = crate::discover::Project::loose(Path::new("b/B.dll"));
        let assemblies = [
            AssemblyFiles {
                project: &transitive,
                project_path: "b/B.csproj".into(),
                files: BTreeMap::new(),
            },
            AssemblyFiles {
                project: &direct,
                project_path: "a/A.csproj".into(),
                files: BTreeMap::new(),
            },
        ];
        let lib = external("Lib.T", Some("Lib"));
        let gone = external("Gone.T", Some("Gone"));
        let dependencies = vec![
            dep("a.cs", lib.clone(), DependencyKind::Body),
            TypeDependency {
                assembly: 1,
                ..dep("b.cs", lib, DependencyKind::Body)
            },
            dep("a.cs", gone, DependencyKind::Body),
        ];
        let universe = Universe::new(Vec::new());
        let classify = |beside: &BTreeSet<String>| {
            let mut modules = vec![Module::new("a.cs"), Module::new("b.cs")];
            project(
                &universe,
                &assemblies,
                &dependencies,
                &mut modules,
                None,
                beside,
            );
            modules
        };
        let modules = classify(&BTreeSet::new());
        let module = |modules: &[Module], name: &str| {
            modules
                .iter()
                .find(|m| m.source == name)
                .map(|m| (m.dependency_types.clone(), m.could_not_resolve))
        };
        assert_eq!(
            module(&modules, "Lib"),
            Some((Some(vec![DependencyType::Package]), Some(false))),
            "the project that references the package wins over the one that does not"
        );
        let edge = |modules: &[Module], from: &str| {
            modules
                .iter()
                .find(|m| m.source == from)
                .and_then(|m| m.dependencies.iter().find(|d| d.resolved == "Lib"))
                .map(|d| (d.dependency_types[0], d.could_not_resolve))
        };
        assert_eq!(
            edge(&modules, "a.cs"),
            Some((DependencyType::Unresolved, true))
        );
        let beside = BTreeSet::from(["lib".to_owned()]);
        let modules = classify(&beside);
        assert_eq!(
            edge(&modules, "a.cs"),
            Some((DependencyType::Package, false))
        );
        assert_eq!(
            module(&modules, "Gone"),
            Some((Some(vec![DependencyType::Unresolved]), Some(true)))
        );
    }

    proptest! {
        #[test]
        fn a_prefix_that_is_not_dotted_never_provides(id in "[A-Za-z]{1,8}", rest in "[A-Za-z]{1,8}") {
            let packages = [package(&id, None)];
            let joined = format!("{id}{rest}");
            prop_assert!(providing_package(&joined, &packages).is_none());
        }

        #[test]
        fn any_names_are_matched_without_a_panic(id in "\\PC{0,6}", assembly in "\\PC{0,10}") {
            let packages = [package(&id, None)];
            let found = providing_package(&assembly, &packages);
            if found.is_some() {
                prop_assert!(assembly.to_ascii_lowercase().starts_with(&id.to_ascii_lowercase()));
            }
        }
    }
}
