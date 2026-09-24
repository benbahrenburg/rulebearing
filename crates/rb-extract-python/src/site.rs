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
//! `*.dist-info` folder contributes the names in `top_level.txt` and the top-level entries of
//! `RECORD`; its licence is `METADATA`'s `License-Expression`, else its one-line `License`.
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
    /// Top-level importable name to the distribution that provides it.
    pub top_level: BTreeMap<String, Distribution>,
}

impl SiteIndex {
    /// The distribution that provides a dotted module, judged by its top-level name.
    pub fn lookup(&self, dotted: &str) -> Option<&Distribution> {
        let top = dotted.split('.').next().unwrap_or_default();
        self.top_level.get(top)
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
    let mut declared = Vec::new();
    let mut recorded = Vec::new();
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
            declared.extend(
                text.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(|l| (l.replace('/', "."), distribution.clone())),
            );
        }
        if let Ok(text) = std::fs::read_to_string(info.join("RECORD")) {
            recorded.extend(
                text.lines()
                    .filter_map(record_top_level)
                    .map(|top| (top, distribution.clone())),
            );
        }
    }
    // A name `top_level.txt` declares beats one only a `RECORD` lists; among equals, the first
    // distribution in sorted folder order keeps it.
    let mut top_level = BTreeMap::new();
    for (top, distribution) in declared.into_iter().chain(recorded) {
        top_level.entry(top).or_insert(distribution);
    }
    Ok(SiteIndex {
        path: shown.into(),
        top_level,
    })
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
        let names: Vec<&str> = index.top_level.keys().map(String::as_str).collect();
        assert_eq!(names, ["bare", "fancy", "fancy_extra"]);
        let fancy = index.lookup("fancy.sub");
        assert_eq!(fancy.map(|d| d.name.as_str()), Some("Fancy"));
        assert_eq!(fancy.and_then(|d| d.license.as_deref()), Some("Apache-2.0"));
        let bare = index.lookup("bare");
        assert_eq!(bare.map(|d| d.name.as_str()), Some("bare"));
        assert_eq!(bare.and_then(|d| d.license.clone()), None);
        assert!(index.lookup("missing").is_none());
        assert!(super::index(&dir.join("nowhere"), "x").is_err());
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
