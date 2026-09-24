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
//!    above the top-level package (a level greater than the package's parts, which includes any
//!    relative import from a module with no package) is `unresolved`, named as written;
//! 2. `from m import x` is the submodule `m.x` when one exists, else the module `m`;
//! 3. an absolute name is looked up as a regular module (a file, or a package's `__init__`)
//!    under each root in order and, for a file under no root only, among the other files under
//!    no root (named from the working directory); a relative name that is not found is
//!    `unresolved`, and so is an absolute one whose top-level name is a local regular module
//!    or package, after a namespace folder of that name has been tried;
//! 4. the standard library snapshot for the configured version, which beats a local
//!    namespace folder of the same top-level name and everything under it;
//! 5. the installed-distributions index, when there is one;
//! 6. a local namespace folder of that name;
//! 7. `unresolved`.
//!
//! The local lookup reads the discovered file list, never the file system, so the answer is a
//! function of the inputs. A `.py` file stands for its module ahead of a `.pyi` stub of the same
//! name; a folder without `__init__.py` is a namespace package whose identity is the folder
//! itself, and it ranks below every regular module, as Python's path finder records a namespace
//! portion and keeps searching.

use std::collections::{BTreeMap, BTreeSet};

use crate::discover::{module_name, split_root};
use crate::parse::{ImportSpec, Origin};
use crate::site::{Provider, SiteIndex};
use crate::stdlib::StdlibSet;

/// Whether a path is a `.pyi` stub.
pub fn is_stub(path: &str) -> bool {
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
    /// A warning the edge carries: a module under a namespace several installed distributions
    /// share, which is attributed to none of them.
    pub note: Option<String>,
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
    /// Whether the file lies under no root. Only such a file's absolute imports may resolve
    /// to the other files under no root; a file under a root sees the roots alone.
    pub unrooted: bool,
}

/// Dotted module names to repository files, per root.
///
/// Regular modules (a `.py` or `.pyi` file, a package's `__init__`) and namespace packages (a
/// folder without `__init__.py`) are kept apart because Python ranks them apart: a regular
/// module anywhere on the search path beats a namespace folder, so a namespace folder is
/// consulted only when nothing regular answers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleIndex {
    roots: Vec<String>,
    /// Regular modules: one map per root in order, then one for files under no root.
    maps: Vec<BTreeMap<String, String>>,
    /// Namespace-package folders, in the same slots.
    folders: Vec<BTreeMap<String, String>>,
    /// Top-level names that are regular modules or packages under a root.
    rooted_tops: BTreeSet<String>,
    /// Top-level names that are regular modules or packages under no root.
    unrooted_tops: BTreeSet<String>,
}

/// Whether a root-relative path is a package's `__init__` (`__init__.py`, `pkg/__init__.pyi`),
/// judged by the whole file name, not a suffix (`pkg/my__init__.py` is not one).
pub fn is_init(relative: &str) -> bool {
    let name = relative.rsplit('/').next().unwrap_or(relative);
    name == "__init__.py" || name == "__init__.pyi"
}

impl ModuleIndex {
    /// Indexes `files` (repository-relative) under `roots` (in lookup order).
    pub fn build(roots: &[String], files: &[String]) -> Self {
        let mut maps = vec![BTreeMap::new(); roots.len() + 1];
        let mut folders = vec![BTreeMap::new(); roots.len() + 1];
        let mut enclosing: Vec<(usize, String, String)> = Vec::new();
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
            // Every enclosing folder may be a namespace package; the ones that are regular
            // packages are answered by their `__init__` first.
            let root_prefix = &file[..file.len() - relative.len()];
            let parts: Vec<&str> = relative.split('/').collect();
            for depth in 1..parts.len() {
                enclosing.push((
                    slot,
                    parts[..depth].join("."),
                    format!("{root_prefix}{}", parts[..depth].join("/")),
                ));
            }
        }
        for (slot, dotted, folder) in enclosing {
            if dotted.split('.').all(crate::discover::is_identifier) {
                folders[slot].entry(dotted).or_insert(folder);
            }
        }
        let tops = |slots: &[BTreeMap<String, String>]| -> BTreeSet<String> {
            slots
                .iter()
                .flat_map(|m| m.keys())
                .filter(|k| !k.contains('.'))
                .cloned()
                .collect()
        };
        let rooted_tops = tops(&maps[..roots.len()]);
        let unrooted_tops = tops(&maps[roots.len()..]);
        Self {
            roots: roots.to_vec(),
            maps,
            folders,
            rooted_tops,
            unrooted_tops,
        }
    }

    /// The slots a file may resolve against: the roots, plus the files under no root for an
    /// unrooted importer.
    fn slots(&self, unrooted: bool) -> usize {
        if unrooted {
            self.maps.len()
        } else {
            self.roots.len()
        }
    }

    /// The repository path of a regular module (a file, or a package's `__init__`), first
    /// root first. `unrooted` also searches the files under no root.
    pub fn get(&self, dotted: &str, unrooted: bool) -> Option<&str> {
        self.maps[..self.slots(unrooted)]
            .iter()
            .find_map(|m| m.get(dotted))
            .map(String::as_str)
    }

    /// The folder of a namespace package, first root first, when no regular module has the
    /// name. `unrooted` also searches the files under no root.
    pub fn namespace(&self, dotted: &str, unrooted: bool) -> Option<&str> {
        self.folders[..self.slots(unrooted)]
            .iter()
            .find_map(|m| m.get(dotted))
            .map(String::as_str)
    }

    /// Whether a dotted name's top-level name is a local regular module or package, which
    /// shadows the standard library and the site. A top-level namespace folder does not.
    pub fn is_local_top(&self, dotted: &str, unrooted: bool) -> bool {
        let top = dotted.split('.').next().unwrap_or_default();
        self.rooted_tops.contains(top) || (unrooted && self.unrooted_tops.contains(top))
    }

    /// A file's module identity, or `None` for a path that names no module.
    pub fn identity(&self, file: &str) -> Option<Identity> {
        let (root, relative) = split_root(&self.roots, file);
        let dotted = module_name(relative)?;
        let init = is_init(relative);
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
            unrooted: root.is_none(),
        })
    }

    /// Whether a file lies under no root.
    pub fn is_unrooted(&self, file: &str) -> bool {
        split_root(&self.roots, file).0.is_none()
    }
}

/// Resolves a relative import (`from ..a import b`) against the importing module's dotted
/// package, per [design § What each extractor has to get right](../../../docs/artifacts/design.md#what-each-extractor-has-to-get-right).
/// Level 1 is the package itself, each further level one package up.
///
/// Returns `None` when the import climbs above the top-level package, as Python refuses it
/// ("attempted relative import beyond top-level package"): a level greater than the number of
/// the package's parts, which includes any relative import from a module with no package.
pub fn resolve_relative(package: &str, level: usize, name: &str) -> Option<String> {
    if level == 0 {
        return Some(name.to_owned());
    }
    let mut parts: Vec<&str> = if package.is_empty() {
        vec![]
    } else {
        package.split('.').collect()
    };
    if level > parts.len() {
        return None;
    }
    parts.truncate(parts.len() - (level - 1));
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

/// An import the installed distributions provide, when one does.
fn from_site(site: &SiteIndex, spec: &ImportSpec, base: &str) -> Option<Resolved> {
    let (resolution, note) = match site.lookup(base)? {
        Provider::One(dist) => (
            Resolution::Site {
                dist: dist.name.clone(),
                license: dist.license.clone(),
            },
            None,
        ),
        Provider::Shared(names) => (
            Resolution::Site {
                dist: names.join(" or "),
                license: None,
            },
            Some(format!(
                "`{base}` lies under a namespace several installed distributions share ({}) and no RECORD names the module, so the edge names no distribution and no licence. Check which distribution provides it and reinstall it so its RECORD lists the module",
                names.join(", ")
            )),
        ),
    };
    Some(Resolved {
        module: spec.written(),
        resolved: base.to_owned(),
        resolution,
        note,
    })
}

/// Resolves one import from the file whose identity is `from`. `None` means the import is not
/// an edge: an `__all__` name that is not a submodule, which is a re-export of something the
/// file already imports.
///
/// A regular local module answers first; a namespace folder answers only when nothing regular
/// does. Under a local regular top-level package that is at once (the standard library and the
/// site cannot provide its submodules); otherwise only after the standard library and the site
/// have failed, as Python's path finder records a namespace portion and keeps looking.
pub fn resolve(
    from: &Identity,
    spec: &ImportSpec,
    index: &ModuleIndex,
    stdlib: &StdlibSet,
    site: Option<&SiteIndex>,
) -> Option<Resolved> {
    let module = spec.module.as_deref().unwrap_or_default();
    let unrooted = from.unrooted;
    let unresolved = |written: String, dotted: String| Resolved {
        module: written,
        resolved: dotted.clone(),
        resolution: Resolution::Unresolved(dotted),
        note: None,
    };
    let level = usize::try_from(spec.level).unwrap_or(usize::MAX);
    let Some(base) = resolve_relative(&from.package, level, module) else {
        if spec.origin == Origin::All {
            return None;
        }
        let written = spec.written_with_member();
        return Some(unresolved(written.clone(), written));
    };
    let found = |path: &str, written: String| Resolved {
        module: written,
        resolved: path.to_owned(),
        resolution: Resolution::Local(path.to_owned()),
        note: None,
    };
    let local_first = spec.level > 0 || base.is_empty() || index.is_local_top(&base, unrooted);
    let candidate = spec.member.as_ref().map(|member| join(&base, member));
    let namespace_candidate = || {
        candidate
            .as_deref()
            .and_then(|c| index.namespace(c, unrooted))
            .map(|path| found(path, spec.written_with_member()))
    };
    let namespace_base = || {
        (!base.is_empty())
            .then(|| index.namespace(&base, unrooted))
            .flatten()
            .map(|path| found(path, spec.written()))
    };
    // The standard library's packages are regular, so one beats a local namespace folder of
    // the same top-level name and everything under it.
    if !local_first && spec.origin != Origin::All && stdlib.contains(&base) {
        return Some(Resolved {
            module: spec.written(),
            resolved: base.clone(),
            resolution: Resolution::Stdlib(base),
            note: None,
        });
    }
    if let Some(candidate) = &candidate {
        if let Some(path) = index.get(candidate, unrooted) {
            return Some(found(path, spec.written_with_member()));
        }
        if local_first && let Some(hit) = namespace_candidate() {
            return Some(hit);
        }
    }
    if spec.origin == Origin::All {
        return None;
    }
    if !base.is_empty()
        && let Some(path) = index.get(&base, unrooted)
    {
        return Some(found(path, spec.written()));
    }
    if local_first {
        if let Some(hit) = namespace_base() {
            return Some(hit);
        }
        let dotted = if base.is_empty() {
            spec.written()
        } else {
            base
        };
        return Some(unresolved(spec.written(), dotted));
    }
    if let Some(found) = site.and_then(|s| from_site(s, spec, &base)) {
        return Some(found);
    }
    if let Some(hit) = namespace_candidate().or_else(namespace_base) {
        return Some(hit);
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
            "tests/test_a.py",
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
        site.declared.insert(
            "fancy".into(),
            vec![Distribution {
                name: "fancy".into(),
                license: Some("MIT".into()),
            }],
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
        // Files under no root answer only imports made from files under no root.
        check(
            "tests/test_a.py",
            &spec(0, Some("tests.helpers"), None, S),
            Some(found("tests.helpers", "tests/helpers.py")),
        );
        let rooted = Resolution::Unresolved("tests.helpers".into());
        check(
            A,
            &spec(0, Some("tests.helpers"), None, S),
            Some(("tests.helpers", "tests.helpers", rooted)),
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
        // Climbing above the top-level package, or any relative import from a module with
        // no package, is refused as Python refuses it: unresolved, named as written.
        let above = Resolution::Unresolved("...x".into());
        check(
            A,
            &spec(3, None, Some("x"), S),
            Some(("...x", "...x", above)),
        );
        let one_above = Resolution::Unresolved("..json".into());
        check(
            A,
            &spec(2, None, Some("json"), S),
            Some(("..json", "..json", one_above)),
        );
        let above_module = Resolution::Unresolved("..other.thing".into());
        check(
            A,
            &spec(2, Some("other"), Some("thing"), S),
            Some(("..other.thing", "..other.thing", above_module)),
        );
        let top = Resolution::Unresolved(".x".into());
        check(
            "src/json.py",
            &spec(1, None, Some("x"), S),
            Some((".x", ".x", top)),
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
    fn a_namespace_folder_ranks_after_regular_modules_the_stdlib_and_the_site() {
        let files: Vec<String> = [
            "src/app/__init__.py",
            "src/app/main.py",
            "src/app/plugins/p.py",
            "src/email/extra.py",
            "src/fancy/plugin.py",
            "src/ownns/m.py",
            "src/shared/a.py",
            "lib/shared/__init__.py",
            "logging/handlers2.py",
            "scripts/tool.py",
            "scripts/util/x.py",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        let index = ModuleIndex::build(&["src".to_owned(), "lib".to_owned()], &files);
        let Some(stdlib) = StdlibSet::for_version("3.12") else {
            return;
        };
        let site = site();
        let at = |from: &str, spec: &ImportSpec| {
            index
                .identity(from)
                .and_then(|from| resolve(&from, spec, &index, &stdlib, Some(&site)))
                .map(|r| r.resolution)
        };
        let main = "src/app/main.py";
        // A regular package in a later root beats a namespace folder in an earlier one.
        assert_eq!(
            at(main, &spec(0, Some("shared"), None, S)),
            Some(local("lib/shared/__init__.py"))
        );
        // Under a local regular package, a namespace subfolder answers at once.
        assert_eq!(
            at(main, &spec(0, Some("app.plugins"), None, S)),
            Some(local("src/app/plugins"))
        );
        assert_eq!(
            at(main, &spec(0, Some("app"), Some("plugins"), S)),
            Some(local("src/app/plugins"))
        );
        assert_eq!(
            at(main, &spec(1, None, Some("plugins"), S)),
            Some(local("src/app/plugins"))
        );
        // A top-level namespace folder loses to the standard library, everything under it
        // included, and to the site.
        assert_eq!(
            at(main, &spec(0, Some("email"), None, S)),
            Some(Resolution::Stdlib("email".into()))
        );
        assert_eq!(
            at(main, &spec(0, Some("email.extra"), None, S)),
            Some(Resolution::Stdlib("email.extra".into()))
        );
        let fancy = Resolution::Site {
            dist: "fancy".into(),
            license: Some("MIT".into()),
        };
        assert_eq!(at(main, &spec(0, Some("fancy"), None, S)), Some(fancy));
        // Only when nothing else answers does the namespace folder.
        assert_eq!(
            at(main, &spec(0, Some("ownns"), None, S)),
            Some(local("src/ownns"))
        );
        assert_eq!(
            at(main, &spec(0, Some("ownns.m"), None, S)),
            Some(local("src/ownns/m.py"))
        );
        // A folder under no root is a namespace only to files under no root.
        assert_eq!(
            at(main, &spec(0, Some("logging"), None, S)),
            Some(Resolution::Stdlib("logging".into()))
        );
        assert_eq!(
            at("scripts/tool.py", &spec(0, Some("logging"), None, S)),
            Some(Resolution::Stdlib("logging".into()))
        );
        assert_eq!(
            at(main, &spec(0, Some("scripts.util"), None, S)),
            Some(Resolution::Unresolved("scripts.util".into()))
        );
        assert_eq!(
            at("scripts/tool.py", &spec(0, Some("scripts.util"), None, S)),
            Some(local("scripts/util"))
        );
        assert!(!index.is_local_top("ownns", false) && !index.is_local_top("scripts", true));
    }

    #[test]
    fn a_shared_site_namespace_names_no_distribution() {
        let index = index();
        let Some(stdlib) = StdlibSet::for_version("3.12") else {
            return;
        };
        let mut site = SiteIndex::default();
        let dist = |name: &str, license: &str| Distribution {
            name: name.into(),
            license: Some(license.into()),
        };
        site.declared.insert(
            "google".into(),
            vec![dist("google-auth", "Apache-2.0"), dist("protobuf", "BSD")],
        );
        site.recorded
            .insert("google.protobuf".into(), vec![dist("protobuf", "BSD")]);
        let from = index.identity(A);
        let at = |module: &str| {
            from.as_ref().and_then(|from| {
                resolve(
                    from,
                    &spec(0, Some(module), None, S),
                    &index,
                    &stdlib,
                    Some(&site),
                )
            })
        };
        let exact = at("google.protobuf");
        assert_eq!(
            exact.as_ref().map(|r| r.resolution.clone()),
            Some(Resolution::Site {
                dist: "protobuf".into(),
                license: Some("BSD".into())
            })
        );
        assert_eq!(exact.and_then(|r| r.note), None);
        let shared = at("google.auth");
        assert_eq!(
            shared.as_ref().map(|r| r.resolution.clone()),
            Some(Resolution::Site {
                dist: "google-auth or protobuf".into(),
                license: None
            })
        );
        assert!(
            shared
                .and_then(|r| r.note)
                .is_some_and(|n| n.contains("google.auth") && n.contains("google-auth, protobuf"))
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
        assert!(index.is_local_top("pkg.whatever", false) && !index.is_local_top("os", false));
        // `tests/` has no `__init__.py`: a namespace folder, which shadows nothing.
        assert!(!index.is_local_top("tests", true));
        assert_eq!(index.get("bad-dir.x", true), None);
        assert_eq!(id("src/pkg/my__init__.py").map(|i| i.2), Some(false));
        assert!(index.identity("src/pkg/a.py").is_some_and(|i| !i.unrooted));
        assert!(
            index
                .identity("tests/helpers.py")
                .is_some_and(|i| i.unrooted)
        );
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
        assert_eq!(resolve_relative("pkg", 2, "x"), None);
        assert_eq!(resolve_relative("pkg", 1, "").as_deref(), Some("pkg"));
        assert_eq!(resolve_relative("", 1, "x"), None);
        assert_eq!(resolve_relative("", 0, "x").as_deref(), Some("x"));
    }

    proptest! {
        /// A relative import never climbs above the top-level package: the result keeps
        /// exactly the package's first `len - (level - 1)` parts, and there is no result when
        /// the level exceeds the package's parts (so never an absolute name).
        #[test]
        fn relative_resolution_never_climbs_above_the_root(
            package in proptest::collection::vec("[a-z]{1,4}", 0..5),
            level in 1usize..7,
            name in proptest::collection::vec("[a-z]{1,4}", 0..3),
        ) {
            let dotted = package.join(".");
            let found = resolve_relative(&dotted, level, &name.join("."));
            if level > package.len() {
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
