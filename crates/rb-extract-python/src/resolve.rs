//! Resolution: an [`ImportSpec`] to a local file, the standard library, an installed
//! distribution, or `unresolved`, in that order.
//!
//! - Plan: [Wave 2 § 1.4.4](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#144-python-resolution-order)
//!   (the order) and [Step 4](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#24-step-4-the-python-extractor-2b)
//!   (`resolve.rs`, the [`Resolution`] enum)
//! - Decision: [ADR-0013](../../../docs/adr/0013-ruff-parser-for-python.md);
//!   [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md) (no heuristic
//!   resolution: what is not found is `unresolved`)
//! - Source: [design § What each extractor has to get right](../../../docs/artifacts/design.md#what-each-extractor-has-to-get-right)
//! - Requirement: [FR-EXT-PY-01](../../../docs/prd.md#fr-ext-py-01)
//!
//! The order for one import:
//!
//! 1. a relative import is made absolute against the importing file's package; one that climbs
//!    above the top-level package is `unresolved`;
//! 2. `from m import x` is the submodule `m.x` when one exists, else the module `m`;
//! 3. an absolute name is looked up under each root in order, then among the files under no
//!    root (named from the working directory); a relative name that is not found is
//!    `unresolved`, and so is an absolute one whose top-level package is local;
//! 4. the standard library snapshot for the configured version;
//! 5. the installed-distributions index, when there is one;
//! 6. `unresolved`.
//!
//! The local lookup reads the discovered file list, never the file system, so the answer is a
//! function of the inputs. A `.py` file stands for its module ahead of a `.pyi` stub of the same
//! name; a folder under a root without `__init__.py` is a namespace package whose identity is
//! the folder itself.

use std::collections::{BTreeMap, BTreeSet};

use crate::discover::{module_name, split_root};
use crate::parse::{ImportSpec, Origin};
use crate::site::SiteIndex;
use crate::stdlib::StdlibSet;

fn is_stub(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|e| e == "pyi")
}

/// Where an import leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// A file (or a namespace package's folder) in the repository, repository-relative.
    Local(String),
    /// A standard-library module, by dotted name.
    Stdlib(String),
    /// A module of an installed distribution.
    Site {
        /// The distribution's name.
        dist: String,
        /// Its licence, when its metadata states one.
        license: Option<String>,
    },
    /// Nothing was found; the dotted name.
    Unresolved(String),
}

/// A resolved import: the module as written, the resolved name and how it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The module as written, dots kept (`..a.b`).
    pub module: String,
    /// The repository-relative path for a local target, the dotted name otherwise.
    pub resolved: String,
    /// How it was found.
    pub resolution: Resolution,
}

/// A file's place in the module tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The dotted module name (`pkg.sub.mod`; `pkg` for `pkg/__init__.py`).
    pub dotted: String,
    /// The package relative imports resolve against: the module itself for an `__init__`,
    /// else its parent (empty for a top-level module).
    pub package: String,
    /// Whether the file is a package's `__init__`.
    pub init: bool,
}

/// Dotted module names to repository files, per root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleIndex {
    roots: Vec<String>,
    /// One map per root in order, then one for files under no root.
    maps: Vec<BTreeMap<String, String>>,
    local_tops: BTreeSet<String>,
}

impl ModuleIndex {
    /// Indexes `files` (repository-relative) under `roots` (in lookup order).
    pub fn build(roots: &[String], files: &[String]) -> Self {
        let mut maps = vec![BTreeMap::new(); roots.len() + 1];
        let mut folders: Vec<(usize, String, String)> = Vec::new();
        for file in files {
            let (root, relative) = split_root(roots, file);
            let slot = root
                .and_then(|r| roots.iter().position(|x| x == r))
                .unwrap_or(roots.len());
            let Some(dotted) = module_name(relative) else {
                continue;
            };
            let map: &mut BTreeMap<String, String> = &mut maps[slot];
            let stub = is_stub(file);
            let keep_existing = map
                .get(&dotted)
                .is_some_and(|e: &String| !is_stub(e) || stub);
            if !keep_existing {
                map.insert(dotted.clone(), file.clone());
            }
            // Every enclosing folder is a package, with or without `__init__.py`.
            let root_prefix = &file[..file.len() - relative.len()];
            let parts: Vec<&str> = relative.split('/').collect();
            for depth in 1..parts.len() {
                folders.push((
                    slot,
                    parts[..depth].join("."),
                    format!("{root_prefix}{}", parts[..depth].join("/")),
                ));
            }
        }
        for (slot, dotted, folder) in folders {
            if dotted.split('.').all(crate::discover::is_identifier) {
                maps[slot].entry(dotted).or_insert(folder);
            }
        }
        let local_tops = maps
            .iter()
            .flat_map(|m| m.keys())
            .filter_map(|k| k.split('.').next())
            .map(str::to_owned)
            .collect();
        Self {
            roots: roots.to_vec(),
            maps,
            local_tops,
        }
    }

    /// The repository path of a dotted module, first root first.
    pub fn get(&self, dotted: &str) -> Option<&str> {
        self.maps
            .iter()
            .find_map(|m| m.get(dotted))
            .map(String::as_str)
    }

    /// Whether a top-level name is a local module or package.
    pub fn is_local_top(&self, dotted: &str) -> bool {
        let top = dotted.split('.').next().unwrap_or_default();
        self.local_tops.contains(top)
    }

    /// A file's module identity, or `None` for a path that names no module.
    pub fn identity(&self, file: &str) -> Option<Identity> {
        let (_, relative) = split_root(&self.roots, file);
        let dotted = module_name(relative)?;
        let init = relative.ends_with("__init__.py") || relative.ends_with("__init__.pyi");
        let package = if init {
            dotted.clone()
        } else {
            dotted
                .rsplit_once('.')
                .map(|(p, _)| p.to_owned())
                .unwrap_or_default()
        };
        Some(Identity {
            dotted,
            package,
            init,
        })
    }
}

/// Resolves a relative import (`from ..a import b`) against the importing module's dotted
/// package, per [design § What each extractor has to get right](../../../docs/artifacts/design.md#what-each-extractor-has-to-get-right).
/// Level 1 is the package itself, each further level one package up.
///
/// Returns `None` when the import climbs above the top-level package.
pub fn resolve_relative(package: &str, level: usize, name: &str) -> Option<String> {
    let mut parts: Vec<&str> = if package.is_empty() {
        vec![]
    } else {
        package.split('.').collect()
    };
    if level == 0 {
        return Some(name.to_owned());
    }
    for _ in 1..level {
        parts.pop()?;
    }
    if !name.is_empty() {
        parts.extend(name.split('.'));
    }
    Some(parts.join("."))
}

fn join(base: &str, member: &str) -> String {
    if base.is_empty() {
        member.to_owned()
    } else {
        format!("{base}.{member}")
    }
}

/// Resolves one import from the file whose identity is `from`. `None` means the import is not
/// an edge: an `__all__` name that is not a submodule, which is a re-export of something the
/// file already imports.
pub fn resolve(
    from: &Identity,
    spec: &ImportSpec,
    index: &ModuleIndex,
    stdlib: &StdlibSet,
    site: Option<&SiteIndex>,
) -> Option<Resolved> {
    let module = spec.module.as_deref().unwrap_or_default();
    let unresolved = |written: String, dotted: String| Resolved {
        module: written,
        resolved: dotted.clone(),
        resolution: Resolution::Unresolved(dotted),
    };
    let level = usize::try_from(spec.level).unwrap_or(usize::MAX);
    let Some(base) = resolve_relative(&from.package, level, module) else {
        if spec.origin == Origin::All {
            return None;
        }
        return Some(unresolved(spec.written(), spec.written()));
    };
    let local = |dotted: &str, written: String| {
        index.get(dotted).map(|path| Resolved {
            module: written,
            resolved: path.to_owned(),
            resolution: Resolution::Local(path.to_owned()),
        })
    };
    if let Some(member) = &spec.member {
        let candidate = join(&base, member);
        if let Some(found) = local(&candidate, spec.written_with_member()) {
            return Some(found);
        }
    }
    if spec.origin == Origin::All {
        return None;
    }
    if !base.is_empty()
        && let Some(found) = local(&base, spec.written())
    {
        return Some(found);
    }
    if spec.level > 0 || base.is_empty() || index.is_local_top(&base) {
        let dotted = if base.is_empty() {
            spec.written()
        } else {
            base
        };
        return Some(unresolved(spec.written(), dotted));
    }
    if stdlib.contains(&base) {
        return Some(Resolved {
            module: spec.written(),
            resolved: base.clone(),
            resolution: Resolution::Stdlib(base),
        });
    }
    if let Some(dist) = site.and_then(|s| s.lookup(&base)) {
        return Some(Resolved {
            module: spec.written(),
            resolved: base,
            resolution: Resolution::Site {
                dist: dist.name.clone(),
                license: dist.license.clone(),
            },
        });
    }
    Some(unresolved(spec.written(), base))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site::Distribution;
    use proptest::prelude::*;

    fn spec(level: u32, module: Option<&str>, member: Option<&str>, origin: Origin) -> ImportSpec {
        ImportSpec {
            level,
            module: module.map(str::to_owned),
            member: member.map(str::to_owned),
            line: 1,
            column: 1,
            type_only: false,
            origin,
        }
    }

    fn index() -> ModuleIndex {
        let files: Vec<String> = [
            "src/pkg/__init__.py",
            "src/pkg/a.py",
            "src/pkg/sub/__init__.py",
            "src/pkg/sub/b.py",
            "src/pkg/ns/c.py",
            "src/pkg/stubbed.pyi",
            "src/pkg/both.py",
            "src/pkg/both.pyi",
            "src/json.py",
            "tests/helpers.py",
            "bad-dir/x.py",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        ModuleIndex::build(&["src".to_owned()], &files)
    }

    fn site() -> SiteIndex {
        let mut site = SiteIndex {
            path: ".venv/lib/python3.12/site-packages".into(),
            ..SiteIndex::default()
        };
        site.top_level.insert(
            "fancy".into(),
            Distribution {
                name: "fancy".into(),
                license: Some("MIT".into()),
            },
        );
        site
    }

    fn run(from_file: &str, spec: &ImportSpec) -> Option<(String, String, Resolution)> {
        let index = index();
        let stdlib = StdlibSet::for_version("3.12")?;
        let from = index.identity(from_file)?;
        let site = site();
        resolve(&from, spec, &index, &stdlib, Some(&site))
            .map(|r| (r.module, r.resolved, r.resolution))
    }

    fn local(path: &str) -> Resolution {
        Resolution::Local(path.to_owned())
    }

    type Expected = Option<(&'static str, &'static str, Resolution)>;

    fn check(from: &str, spec: &ImportSpec, expected: Expected) {
        let expected = expected.map(|(m, r, res)| (m.to_owned(), r.to_owned(), res));
        assert_eq!(run(from, spec), expected, "{from} {spec:?}");
    }

    fn found(module: &'static str, path: &'static str) -> (&'static str, &'static str, Resolution) {
        (module, path, local(path))
    }

    const A: &str = "src/pkg/a.py";
    const B: &str = "src/pkg/sub/b.py";
    const S: Origin = Origin::Statement;

    #[test]
    fn absolute_imports_resolve_against_the_roots() {
        check(
            A,
            &spec(0, Some("pkg.sub.b"), None, S),
            Some(found("pkg.sub.b", B)),
        );
        check(
            A,
            &spec(0, Some("pkg"), Some("sub"), S),
            Some(found("pkg.sub", "src/pkg/sub/__init__.py")),
        );
        check(
            A,
            &spec(0, Some("pkg.a"), Some("thing"), S),
            Some(found("pkg.a", A)),
        );
        check(
            A,
            &spec(0, Some("pkg.ns.c"), None, S),
            Some(found("pkg.ns.c", "src/pkg/ns/c.py")),
        );
        check(
            A,
            &spec(0, Some("pkg.ns"), None, S),
            Some(found("pkg.ns", "src/pkg/ns")),
        );
        check(
            A,
            &spec(0, Some("pkg.stubbed"), None, S),
            Some(found("pkg.stubbed", "src/pkg/stubbed.pyi")),
        );
        check(
            A,
            &spec(0, Some("pkg.both"), None, S),
            Some(found("pkg.both", "src/pkg/both.py")),
        );
        check(
            A,
            &spec(0, Some("json"), None, S),
            Some(found("json", "src/json.py")),
        );
        check(
            A,
            &spec(0, Some("tests.helpers"), None, S),
            Some(found("tests.helpers", "tests/helpers.py")),
        );
        let missing = Resolution::Unresolved("pkg.missing".into());
        check(
            A,
            &spec(0, Some("pkg.missing"), None, S),
            Some(("pkg.missing", "pkg.missing", missing)),
        );
    }

    #[test]
    fn relative_imports_resolve_against_the_package() {
        check(B, &spec(1, None, Some("b"), S), Some(found(".b", B)));
        check(B, &spec(2, None, Some("a"), S), Some(found("..a", A)));
        check(
            B,
            &spec(2, Some("sub"), Some("b"), S),
            Some(found("..sub.b", B)),
        );
        check(
            "src/pkg/sub/__init__.py",
            &spec(1, None, Some("b"), S),
            Some(found(".b", B)),
        );
        check(
            B,
            &spec(1, None, Some("nothing"), S),
            Some(found(".", "src/pkg/sub/__init__.py")),
        );
        let missing = Resolution::Unresolved("pkg.missing".into());
        check(
            A,
            &spec(1, Some("missing"), None, S),
            Some((".missing", "pkg.missing", missing)),
        );
        let above = Resolution::Unresolved("...".into());
        check(A, &spec(3, None, Some("x"), S), Some(("...", "...", above)));
        let top = Resolution::Unresolved(".".into());
        check(
            "src/json.py",
            &spec(1, None, Some("x"), S),
            Some((".", ".", top)),
        );
    }

    #[test]
    fn then_the_stdlib_then_the_site_then_unresolved() {
        let stdlib = |name: &str| Resolution::Stdlib(name.into());
        check(
            A,
            &spec(0, Some("os.path"), None, S),
            Some(("os.path", "os.path", stdlib("os.path"))),
        );
        check(
            A,
            &spec(0, Some("os"), Some("path"), S),
            Some(("os", "os", stdlib("os"))),
        );
        let gone = Resolution::Unresolved("distutils".into());
        check(
            A,
            &spec(0, Some("distutils"), None, S),
            Some(("distutils", "distutils", gone)),
        );
        let site = Resolution::Site {
            dist: "fancy".into(),
            license: Some("MIT".into()),
        };
        check(
            A,
            &spec(0, Some("fancy.x"), None, S),
            Some(("fancy.x", "fancy.x", site)),
        );
        let nowhere = Resolution::Unresolved("nowhere".into());
        check(
            A,
            &spec(0, Some("nowhere"), None, S),
            Some(("nowhere", "nowhere", nowhere)),
        );
    }

    #[test]
    fn all_names_are_edges_only_to_submodules() {
        let init = "src/pkg/__init__.py";
        check(
            init,
            &spec(1, None, Some("a"), Origin::All),
            Some(found(".a", A)),
        );
        check(init, &spec(1, None, Some("helper"), Origin::All), None);
        check("src/json.py", &spec(3, None, Some("a"), Origin::All), None);
    }

    #[test]
    fn without_a_site_index_third_party_is_unresolved() {
        let index = index();
        let stdlib = StdlibSet::for_version("3.12");
        let from = index.identity("src/pkg/a.py");
        let found = stdlib.zip(from).and_then(|(stdlib, from)| {
            resolve(
                &from,
                &spec(0, Some("fancy"), None, Origin::Statement),
                &index,
                &stdlib,
                None,
            )
        });
        assert_eq!(
            found.map(|r| r.resolution),
            Some(Resolution::Unresolved("fancy".into()))
        );
    }

    #[test]
    fn identities() {
        let index = index();
        let id = |f: &str| index.identity(f).map(|i| (i.dotted, i.package, i.init));
        assert_eq!(
            id("src/pkg/__init__.py"),
            Some(("pkg".into(), "pkg".into(), true))
        );
        assert_eq!(
            id("src/pkg/sub/b.py"),
            Some(("pkg.sub.b".into(), "pkg.sub".into(), false))
        );
        assert_eq!(
            id("src/json.py"),
            Some(("json".into(), String::new(), false))
        );
        assert_eq!(
            id("tests/helpers.py"),
            Some(("tests.helpers".into(), "tests".into(), false))
        );
        assert_eq!(id("bad-dir/x.py"), None);
        assert!(index.is_local_top("pkg.whatever") && !index.is_local_top("os"));
        assert_eq!(index.get("bad-dir.x"), None);
    }

    #[test]
    fn resolves_current_and_parent_packages() {
        assert_eq!(
            resolve_relative("pkg.sub", 1, "mod").as_deref(),
            Some("pkg.sub.mod")
        );
        assert_eq!(
            resolve_relative("pkg.sub", 2, "mod").as_deref(),
            Some("pkg.mod")
        );
        assert_eq!(resolve_relative("pkg.sub", 2, "").as_deref(), Some("pkg"));
        assert_eq!(
            resolve_relative("pkg", 0, "os.path").as_deref(),
            Some("os.path")
        );
        assert_eq!(resolve_relative("pkg", 3, "x"), None);
        assert_eq!(resolve_relative("", 1, "x").as_deref(), Some("x"));
    }

    proptest! {
        /// A relative import never climbs above the root: the result keeps exactly the
        /// package's first `len - (level - 1)` parts, or there is no result.
        #[test]
        fn relative_resolution_never_climbs_above_the_root(
            package in proptest::collection::vec("[a-z]{1,4}", 0..5),
            level in 1usize..7,
            name in proptest::collection::vec("[a-z]{1,4}", 0..3),
        ) {
            let dotted = package.join(".");
            let found = resolve_relative(&dotted, level, &name.join("."));
            if level - 1 > package.len() {
                prop_assert!(found.is_none());
            } else {
                let kept = &package[..package.len() - (level - 1)];
                let mut expected: Vec<String> = kept.to_vec();
                expected.extend(name.iter().cloned());
                prop_assert_eq!(found, Some(expected.join(".")));
            }
        }
    }
}
