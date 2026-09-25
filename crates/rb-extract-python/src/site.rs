//! The installed-distributions index: which top-level names an environment's `site-packages`
//! provides, and under which licence, read from metadata files only.
//!
//! - Plan: [Wave 2 § 1.7](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#17-decisions-this-wave-must-make),
//!   rows "Installed distributions for Python" and "`to.license` for .NET and Python";
//!   resolution step four of
//!   [§ 1.4.4](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#144-python-resolution-order)
//! - Decision: [ADR-0013](../../../docs/adr/0013-ruff-parser-for-python.md) (no interpreter is
//!   ever executed)
//! - Requirement: [FR-EXT-PY-01](../../../docs/prd.md#fr-ext-py-01)
//!
//! The environment is `$VIRTUAL_ENV` when set, else the first of `.venv/`, `venv/` and `env/`
//! beside a root or the working directory that holds a `site-packages` folder. Each
//! `*.dist-info` folder contributes the names in `top_level.txt` and every dotted package and
//! module prefix of the paths in `RECORD`; its licence is `METADATA`'s `License-Expression`,
//! else its one-line `License`. A module is attributed through the longest prefix that names a
//! single distribution; a namespace several distributions share (`google`, which `google-auth`
//! and `protobuf` both install into) with nothing deeper to tell them apart is reported as
//! shared, with no distribution or licence, never as the first one found.
//! An absent licence stays absent, never guessed. When no environment is found, the receipt
//! says `site: none` and every non-local, non-stdlib import is `unresolved`.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

/// The folder names searched for an environment beside a root, in order.
pub const ENV_DIRS: &[&str] = &[".venv", "venv", "env"];

/// One installed distribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Distribution {
    /// The distribution name (`METADATA` `Name`, else the `dist-info` folder's).
    pub name: String,
    /// The licence, when `METADATA` states one.
    pub license: Option<String>,
}

/// The index of one `site-packages` folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SiteIndex {
    /// The `site-packages` folder, as the receipt shows it.
    pub path: String,
    /// Dotted names `top_level.txt` declares, to the distributions declaring them.
    pub declared: BTreeMap<String, Vec<Distribution>>,
    /// Every dotted package and module prefix a `RECORD` installs (`google/protobuf/x.py`
    /// gives `google`, `google.protobuf` and `google.protobuf.x`), to the distributions whose
    /// `RECORD` lists it.
    pub recorded: BTreeMap<String, Vec<Distribution>>,
}

/// Who provides a dotted module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provider<'a> {
    /// One distribution.
    One(&'a Distribution),
    /// Several distributions share the longest matching name (a namespace such as `google`
    /// that `google-auth` and `protobuf` both install into) and nothing deeper tells them
    /// apart; their names, sorted.
    Shared(Vec<&'a str>),
}

impl SiteIndex {
    /// The distribution that provides a dotted module: the longest dotted prefix of it that a
    /// distribution declares or records, a `top_level.txt` declaration beating a `RECORD` entry
    /// for the same name. A name several distributions share is [`Provider::Shared`], never
    /// the first of them.
    pub fn lookup(&self, dotted: &str) -> Option<Provider<'_>> {
        let parts: Vec<&str> = dotted.split('.').collect();
        (1..=parts.len()).rev().find_map(|n| {
            let prefix = parts[..n].join(".");
            let owners = self
                .declared
                .get(&prefix)
                .filter(|d| !d.is_empty())
                .or_else(|| self.recorded.get(&prefix))?;
            match owners.as_slice() {
                [] => None,
                [one] => Some(Provider::One(one)),
                many => {
                    let mut names: Vec<&str> = many.iter().map(|d| d.name.as_str()).collect();
                    names.sort_unstable();
                    names.dedup();
                    Some(Provider::Shared(names))
                }
            }
        })
    }

    fn add(map: &mut BTreeMap<String, Vec<Distribution>>, name: String, dist: &Distribution) {
        let owners = map.entry(name).or_default();
        if !owners.iter().any(|d| d.name == dist.name) {
            owners.push(dist.clone());
        }
    }
}

/// The `site-packages` folder to index, if any: `virtual_env` first, then the environment
/// folders beside each root and the working directory. Within an environment, the
/// `lib/pythonX.Y/site-packages` matching `version` wins, then the highest version, then the
/// Windows `Lib/site-packages`.
pub fn find(
    base: &Path,
    roots: &[String],
    virtual_env: Option<&Path>,
    version: &str,
) -> Option<PathBuf> {
    if let Some(env) = virtual_env
        && let Some(found) = site_packages_in(env, version)
    {
        return Some(found);
    }
    let mut dirs: Vec<PathBuf> = roots
        .iter()
        .filter(|r| r.as_str() != ".")
        .map(|r| base.join(r))
        .collect();
    dirs.push(base.to_path_buf());
    dirs.iter()
        .flat_map(|dir| ENV_DIRS.iter().map(move |name| dir.join(name)))
        .find_map(|env| site_packages_in(&env, version))
}

fn site_packages_in(env: &Path, version: &str) -> Option<PathBuf> {
    let lib = env.join("lib");
    let mut candidates: Vec<(Option<(u32, u32)>, PathBuf)> = std::fs::read_dir(&lib)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let rest = name.strip_prefix("python")?;
            let site = entry.path().join("site-packages");
            site.is_dir().then(|| (parse_version(rest), site))
        })
        .collect();
    let wanted = parse_version(version);
    candidates.sort();
    if let Some((_, exact)) = candidates.iter().find(|(v, _)| v.is_some() && *v == wanted) {
        return Some(exact.clone());
    }
    if let Some((_, highest)) = candidates.pop() {
        return Some(highest);
    }
    let windows = env.join("Lib").join("site-packages");
    windows.is_dir().then_some(windows)
}

fn parse_version(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts
        .next()?
        .trim_end_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .ok()?;
    Some((major, minor))
}

/// Indexes a `site-packages` folder. `shown` is the path the receipt records.
///
/// # Errors
/// The folder cannot be listed. A single unreadable `dist-info` file is skipped, not an error.
pub fn index(site_packages: &Path, shown: impl Into<String>) -> io::Result<SiteIndex> {
    let mut dist_infos: Vec<PathBuf> = std::fs::read_dir(site_packages)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.extension().and_then(|e| e.to_str()) == Some("dist-info"))
        .collect();
    dist_infos.sort();
    let mut site = SiteIndex {
        path: shown.into(),
        ..SiteIndex::default()
    };
    for info in dist_infos {
        let folder = info
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let metadata = std::fs::read_to_string(info.join("METADATA")).unwrap_or_default();
        let headers = metadata_headers(&metadata);
        let name = headers
            .get("Name")
            .cloned()
            .unwrap_or_else(|| folder.split('-').next().unwrap_or_default().to_owned());
        let distribution = Distribution {
            name,
            license: license(&headers),
        };
        if let Ok(text) = std::fs::read_to_string(info.join("top_level.txt")) {
            for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
                SiteIndex::add(&mut site.declared, line.replace('/', "."), &distribution);
            }
        }
        if let Ok(text) = std::fs::read_to_string(info.join("RECORD")) {
            for prefix in text.lines().flat_map(record_prefixes) {
                SiteIndex::add(&mut site.recorded, prefix, &distribution);
            }
        }
    }
    Ok(site)
}

/// Every dotted prefix a `RECORD` line installs, shortest first: `google/protobuf/x.py` gives
/// `google`, `google.protobuf`, `google.protobuf.x`; `pkg/__init__.py` gives `pkg`; a data file
/// gives its folders. The walk stops at the first path component that is not an identifier.
pub fn record_prefixes(line: &str) -> Vec<String> {
    let Some(top) = record_top_level(line) else {
        return Vec::new();
    };
    let path = line
        .split(',')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches('"');
    let parts: Vec<&str> = path.split('/').collect();
    let mut prefixes = vec![top.clone()];
    let mut current = top;
    let Some((last, folders)) = parts.split_last() else {
        return prefixes;
    };
    for folder in folders.iter().skip(1) {
        if *folder == "__pycache__" || !crate::discover::is_identifier(folder) {
            return prefixes;
        }
        current = format!("{current}.{folder}");
        prefixes.push(current.clone());
    }
    if folders.is_empty() {
        return prefixes;
    }
    let extension = Path::new(last).extension().and_then(|e| e.to_str());
    let stem = match extension {
        Some("py" | "pyi") => last.rsplit_once('.').map(|(s, _)| s),
        Some("so" | "pyd") => last.split('.').next(),
        _ => None,
    };
    if let Some(stem) = stem.filter(|s| *s != "__init__" && crate::discover::is_identifier(s)) {
        prefixes.push(format!("{current}.{stem}"));
    }
    prefixes
}

/// The top-level importable name a `RECORD` line installs, if any: `pkg/x.py` gives `pkg`,
/// `mod.py` gives `mod`, `ext.cpython-312-darwin.so` gives `ext`. Metadata folders, scripts
/// outside `site-packages`, bytecode caches and `.pth` files give nothing.
pub fn record_top_level(line: &str) -> Option<String> {
    let path = line.split(',').next()?.trim().trim_matches('"');
    if path.is_empty() || path.starts_with("..") || path.starts_with('/') {
        return None;
    }
    let (first, rest) = match path.split_once('/') {
        Some((first, rest)) => (first, Some(rest)),
        None => (path, None),
    };
    let extension = Path::new(first).extension().and_then(|e| e.to_str());
    if matches!(extension, Some("dist-info" | "data")) || first == "__pycache__" {
        return None;
    }
    let name = if rest.is_some() {
        first
    } else if let Some(stem) = first.strip_suffix(".py") {
        stem
    } else if matches!(extension, Some("so" | "pyd")) {
        first.split('.').next()?
    } else {
        return None;
    };
    crate::discover::is_identifier(name).then(|| name.to_owned())
}

/// The header block of a core-metadata file (`Key: value` lines up to the first blank line),
/// first value per key; continuation lines are not joined.
pub fn metadata_headers(text: &str) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            headers
                .entry(key.trim().to_owned())
                .or_insert_with(|| value.trim().to_owned());
        }
    }
    headers
}

/// The licence the headers state: `License-Expression`, else `License` unless it is empty or
/// `UNKNOWN`.
pub fn license(headers: &BTreeMap<String, String>) -> Option<String> {
    ["License-Expression", "License"]
        .iter()
        .filter_map(|key| headers.get(*key))
        .map(|value| value.trim())
        .find(|value| !value.is_empty() && !value.eq_ignore_ascii_case("UNKNOWN"))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rb-py-site-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn write(dir: &Path, file: &str, text: &str) {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, text);
    }

    #[test]
    fn record_lines_name_their_top_level_package() {
        let table: &[(&str, Option<&str>)] = &[
            ("requests/__init__.py,sha256=x,12", Some("requests")),
            ("six.py,sha256=x,1", Some("six")),
            (
                "_cffi_backend.cpython-312-darwin.so,,",
                Some("_cffi_backend"),
            ),
            ("ext.pyd,,", Some("ext")),
            ("requests-2.0.dist-info/RECORD,,", None),
            ("pkg-1.0.data/scripts/x,,", None),
            ("../../../bin/tool,,", None),
            ("__pycache__/six.cpython-312.pyc,,", None),
            ("distutils-precedence.pth,,", None),
            ("\"quoted/x.py\",,", Some("quoted")),
            ("bad-name/x.py,,", None),
            ("", None),
        ];
        for &(line, expected) in table {
            assert_eq!(record_top_level(line).as_deref(), expected, "{line}");
        }
    }

    #[test]
    fn metadata_licence_prefers_the_expression() {
        let headers = metadata_headers(
            "Metadata-Version: 2.4\nName: fancy\nLicense: MIT License\n  continued\nLicense-Expression: MIT\n\nLicense: body\n",
        );
        assert_eq!(headers.get("Name").map(String::as_str), Some("fancy"));
        assert_eq!(license(&headers).as_deref(), Some("MIT"));
        let plain = metadata_headers("Name: x\nLicense: BSD-3-Clause\n");
        assert_eq!(license(&plain).as_deref(), Some("BSD-3-Clause"));
        let unknown = metadata_headers("Name: x\nLicense: UNKNOWN\n");
        assert_eq!(license(&unknown), None);
        assert_eq!(license(&metadata_headers("Name: x\nLicense:\n")), None);
        assert_eq!(license(&BTreeMap::new()), None);
    }

    #[test]
    fn an_environment_is_found_and_indexed() -> io::Result<()> {
        let dir = scratch("find");
        assert_eq!(find(&dir, &[".".to_owned()], None, "3.12"), None);
        let site = "src/.venv/lib/python3.11/site-packages";
        write(
            &dir,
            &format!("{site}/fancy-1.0.dist-info/top_level.txt"),
            "fancy\n\n",
        );
        write(
            &dir,
            &format!("{site}/fancy-1.0.dist-info/RECORD"),
            "fancy/__init__.py,,\nfancy_extra.py,,\nfancy-1.0.dist-info/RECORD,,\n",
        );
        write(
            &dir,
            &format!("{site}/fancy-1.0.dist-info/METADATA"),
            "Name: Fancy\nLicense-Expression: Apache-2.0\n",
        );
        write(
            &dir,
            &format!("{site}/bare-2.0.dist-info/RECORD"),
            "bare.py,,\nfancy/x.py,,\n",
        );
        write(&dir, "src/.venv/lib/python3.12/site-packages/.keep", "");
        write(&dir, "src/.venv/lib/pythonX/site-packages/.keep", "");
        let roots = ["src".to_owned()];
        let found = find(&dir, &roots, None, "3.11");
        assert_eq!(found, Some(dir.join(site)));
        let highest = find(&dir, &roots, None, "3.13");
        assert_eq!(
            highest,
            Some(dir.join("src/.venv/lib/python3.12/site-packages"))
        );
        let index = index(&dir.join(site), site)?;
        assert_eq!(index.path, site);
        let names: Vec<&str> = index.recorded.keys().map(String::as_str).collect();
        assert_eq!(names, ["bare", "fancy", "fancy.x", "fancy_extra"]);
        let one = |dotted: &str| match index.lookup(dotted) {
            Some(Provider::One(d)) => Some((d.name.clone(), d.license.clone())),
            _ => None,
        };
        let apache = Some("Apache-2.0".to_owned());
        assert_eq!(one("fancy.sub"), Some(("Fancy".to_owned(), apache)));
        // `top_level.txt` beats a `RECORD` for the same name; a deeper `RECORD` path is exact.
        assert_eq!(one("fancy.x"), Some(("bare".to_owned(), None)));
        assert_eq!(one("bare"), Some(("bare".to_owned(), None)));
        assert!(index.lookup("missing").is_none());
        assert!(super::index(&dir.join("nowhere"), "x").is_err());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn record_prefixes_name_every_package_and_module() {
        let table: &[(&str, &[&str])] = &[
            (
                "google/protobuf/internal/x.py,,",
                &[
                    "google",
                    "google.protobuf",
                    "google.protobuf.internal",
                    "google.protobuf.internal.x",
                ],
            ),
            ("pkg/__init__.py,,", &["pkg"]),
            ("pkg/_c.cpython-312-darwin.so,,", &["pkg", "pkg._c"]),
            ("pkg/py.typed,,", &["pkg"]),
            ("pkg/bad-dir/x.py,,", &["pkg"]),
            ("pkg/__pycache__/x.cpython-312.pyc,,", &["pkg"]),
            ("six.py,,", &["six"]),
            ("x-1.0.dist-info/RECORD,,", &[]),
            ("", &[]),
        ];
        for &(line, expected) in table {
            assert_eq!(record_prefixes(line), expected, "{line}");
        }
    }

    #[test]
    fn a_shared_namespace_is_attributed_by_its_longest_prefix() -> io::Result<()> {
        let dir = scratch("shared");
        let dist = |folder: &str, top: &str, record: &str, metadata: &str| {
            write(&dir, &format!("{folder}.dist-info/top_level.txt"), top);
            if !record.is_empty() {
                write(&dir, &format!("{folder}.dist-info/RECORD"), record);
            }
            write(&dir, &format!("{folder}.dist-info/METADATA"), metadata);
        };
        dist(
            "google_auth-2.0",
            "google\n",
            "",
            "Name: google-auth\nLicense: Apache-2.0\n",
        );
        dist(
            "protobuf-4.0",
            "google\n",
            "google/protobuf/__init__.py,,\n",
            "Name: protobuf\nLicense: BSD-3-Clause\n",
        );
        let index = index(&dir, "site")?;
        let protobuf = Distribution {
            name: "protobuf".into(),
            license: Some("BSD-3-Clause".into()),
        };
        assert_eq!(
            index.lookup("google.protobuf.message"),
            Some(Provider::One(&protobuf))
        );
        assert_eq!(
            index.lookup("google.auth"),
            Some(Provider::Shared(vec!["google-auth", "protobuf"]))
        );
        assert_eq!(
            index.lookup("google"),
            Some(Provider::Shared(vec!["google-auth", "protobuf"]))
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn virtual_env_wins_and_windows_layouts_are_read() {
        let dir = scratch("venv");
        write(&dir, "active/Lib/site-packages/.keep", "");
        write(&dir, ".venv/lib/python3.12/site-packages/.keep", "");
        let active = dir.join("active");
        assert_eq!(
            find(&dir, &[], Some(&active), "3.12"),
            Some(active.join("Lib/site-packages"))
        );
        assert_eq!(
            find(&dir, &[], Some(&dir.join("missing")), "3.12"),
            Some(dir.join(".venv/lib/python3.12/site-packages"))
        );
        assert_eq!(parse_version("3.13t"), Some((3, 13)));
        assert_eq!(parse_version("3"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
