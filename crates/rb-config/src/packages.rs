//! Where an `extends` entry naming a package is found beyond `node_modules/`: the NuGet global
//! packages folder and a Python environment's `site-packages`, so `extends: rulebearing-rules/nextjs`
//! reads the rule library from whichever registry the repository installed it from.
//!
//! - Plan: [Wave 3, Step 23](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#27-steps-for-sub-wave-3g-the-rule-library-the-scale-table-adoption)
//!   ("`extends` resolution in `rb-config` learns to find the package in `node_modules/`, the NuGet
//!   global packages folder and `site-packages`, in that order")
//! - Decisions: [ADR-0020](../../../docs/adr/0020-single-name-across-registries.md) (one name on
//!   every registry: `rulebearing-rules` on npm and PyPI, `Rulebearing.Rules` on NuGet)
//! - Requirement: [FR-REACH-04](../../../docs/prd.md#fr-reach-04)
//!
//! A spec is `<package>/<path>`. The package keeps its files in a folder named for it:
//!
//! | Registry | Where `<path>` is looked for |
//! | --- | --- |
//! | NuGet | `<global packages>/<id>/<version>/<package>/<path>`, where `<id>` is the package name with `-` read as `.`, lower-cased as NuGet stores it (`rulebearing.rules`); the global packages folder is `$NUGET_PACKAGES`, else `.nuget/packages` in the home folder |
//! | PyPI | `<site-packages>/<module>/<path>`, where `<module>` is the package name with `-` read as `_` (`rulebearing_rules`), in `$VIRTUAL_ENV`, else a `.venv` or `venv` folder in the configuration's folder or one above it |
//!
//! NuGet keeps every version it has restored side by side, so the version is the one installed
//! when there is one, else the one the repository's projects reference (`PackageReference` or
//! `PackageVersion` in a `.csproj`, `Directory.Packages.props` or `Directory.Build.props` under the
//! repository root); several installed and none referenced is an error naming them, never a
//! guess.

use std::path::{Path, PathBuf};

/// What the lookups read from the process environment, so tests can give their own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    /// `$NUGET_PACKAGES`, or `<home>/.nuget/packages`.
    pub nuget_packages: Option<PathBuf>,
    /// `$VIRTUAL_ENV`.
    pub virtual_env: Option<PathBuf>,
}

impl Environment {
    /// The running process's environment.
    pub fn current() -> Self {
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let home = var("HOME").or_else(|| var("USERPROFILE"));
        Self {
            nuget_packages: var("NUGET_PACKAGES")
                .or_else(|| home.map(|h| h.join(".nuget").join("packages"))),
            virtual_env: var("VIRTUAL_ENV"),
        }
    }
}

/// The package name and the path within it, for a spec `<package>/<path>`; a scoped npm name
/// (`@scope/name/...`) has no NuGet or PyPI counterpart.
fn split(spec: &str) -> Option<(&str, &str)> {
    if spec.starts_with('@') {
        return None;
    }
    let (package, rest) = spec.split_once('/')?;
    (!package.is_empty() && !rest.is_empty()).then_some((package, rest))
}

/// The candidates (without extension) for `spec` in the NuGet global packages folder.
///
/// # Errors
/// A reason, when the package is installed in several versions and the repository references none
/// of them.
pub fn nuget(spec: &str, root: &Path, env: &Environment) -> Result<Vec<PathBuf>, String> {
    let (Some((package, rest)), Some(packages)) = (split(spec), env.nuget_packages.as_ref()) else {
        return Ok(Vec::new());
    };
    let id = package.replace('-', ".").to_lowercase();
    let folder = packages.join(&id);
    let mut versions: Vec<String> = std::fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .collect();
    versions.sort();
    let version = match versions.as_slice() {
        [] => return Ok(Vec::new()),
        [only] => only.clone(),
        several => match referenced_version(root, &id) {
            Some(v) if several.iter().any(|s| s.eq_ignore_ascii_case(&v)) => v.to_lowercase(),
            _ => {
                return Err(format!(
                    "NuGet has {} {} installed and no project under {} references one of them; reference the version to use",
                    id,
                    several.join(", "),
                    root.display()
                ));
            }
        },
    };
    Ok(vec![folder.join(version).join(package).join(rest)])
}

/// The version of NuGet package `id` (lower case) the repository's project files reference.
fn referenced_version(root: &Path, id: &str) -> Option<String> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                if !matches!(
                    name,
                    "bin" | "obj" | "node_modules" | ".git" | "target" | ".venv" | "venv"
                ) {
                    stack.push(path);
                }
            } else if (name.ends_with(".csproj")
                || name == "Directory.Packages.props"
                || name == "Directory.Build.props")
                && let Ok(text) = std::fs::read_to_string(&path)
                && let Some(version) = version_in(&text, id)
            {
                return Some(version);
            }
        }
    }
    None
}

/// The `Version` of a `PackageReference` or `PackageVersion` element whose `Include` is `id`,
/// compared without case, in an MSBuild file's text.
fn version_in(text: &str, id: &str) -> Option<String> {
    for element in text.split('<').skip(1) {
        let element = element.split('>').next().unwrap_or("");
        if !(element.starts_with("PackageReference") || element.starts_with("PackageVersion")) {
            continue;
        }
        let attribute = |name: &str| -> Option<String> {
            let start = element.find(&format!("{name}=\""))? + name.len() + 2;
            let end = element[start..].find('"')? + start;
            Some(element[start..end].to_owned())
        };
        if attribute("Include").is_some_and(|i| i.eq_ignore_ascii_case(id))
            && let Some(version) = attribute("Version")
        {
            return Some(version);
        }
    }
    None
}

/// The candidates (without extension) for `spec` in a Python environment's `site-packages`.
pub fn site_packages(spec: &str, base_dir: &Path, env: &Environment) -> Vec<PathBuf> {
    let Some((package, rest)) = split(spec) else {
        return Vec::new();
    };
    let module = package.replace('-', "_");
    let mut environments: Vec<PathBuf> = env.virtual_env.iter().cloned().collect();
    for dir in base_dir.ancestors() {
        for name in [".venv", "venv"] {
            let candidate = dir.join(name);
            if candidate.is_dir() {
                environments.push(candidate);
            }
        }
    }
    let mut found = Vec::new();
    for environment in environments {
        // Windows: Lib/site-packages; elsewhere lib/python3.X/site-packages.
        let mut sites = vec![environment.join("Lib").join("site-packages")];
        let mut pythons: Vec<PathBuf> = std::fs::read_dir(environment.join("lib"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("python"))
            })
            .collect();
        pythons.sort();
        sites.extend(pythons.into_iter().map(|p| p.join("site-packages")));
        for site in sites {
            let module_dir = site.join(&module);
            if module_dir.is_dir() {
                found.push(module_dir.join(rest));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rb-config-packages-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_spec_splits_into_package_and_path() {
        assert_eq!(
            split("rulebearing-rules/nextjs"),
            Some(("rulebearing-rules", "nextjs"))
        );
        assert_eq!(
            split("rulebearing-rules/sub/x"),
            Some(("rulebearing-rules", "sub/x"))
        );
        assert_eq!(split("@scope/name/x"), None);
        assert_eq!(split("plain"), None);
        assert_eq!(split("trailing/"), None);
    }

    #[test]
    fn a_version_is_read_from_msbuild_text() {
        let text = r#"<Project><ItemGroup>
            <PackageReference Include="Other" Version="9.0.0" />
            <PackageVersion Include="Rulebearing.Rules" Version="1.2.3" />
        </ItemGroup></Project>"#;
        assert_eq!(
            version_in(text, "rulebearing.rules").as_deref(),
            Some("1.2.3")
        );
        assert_eq!(version_in(text, "missing"), None);
        assert_eq!(
            version_in(
                "<PackageReference Include=\"Rulebearing.Rules\" />",
                "rulebearing.rules"
            ),
            None
        );
    }

    #[test]
    fn nuget_takes_the_only_version_or_the_referenced_one() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = scratch("nuget");
        let packages = dir.join("packages");
        let repo = dir.join("repo");
        std::fs::create_dir_all(packages.join("rulebearing.rules/1.0.0/rulebearing-rules"))?;
        std::fs::create_dir_all(&repo)?;
        let env = Environment {
            nuget_packages: Some(packages.clone()),
            virtual_env: None,
        };
        assert_eq!(
            nuget("rulebearing-rules/nextjs", &repo, &env)?,
            vec![packages.join("rulebearing.rules/1.0.0/rulebearing-rules/nextjs")]
        );
        std::fs::create_dir_all(packages.join("rulebearing.rules/2.0.0"))?;
        let ambiguous = nuget("rulebearing-rules/nextjs", &repo, &env);
        assert!(ambiguous.is_err_and(|e| e.contains("1.0.0, 2.0.0")));
        std::fs::create_dir_all(repo.join("src/App"))?;
        std::fs::create_dir_all(repo.join("src/App/obj"))?;
        std::fs::write(
            repo.join("src/App/obj/stale.csproj"),
            r#"<PackageReference Include="Rulebearing.Rules" Version="1.0.0" />"#,
        )?;
        std::fs::write(
            repo.join("src/App/App.csproj"),
            r#"<Project><ItemGroup><PackageReference Include="Rulebearing.Rules" Version="2.0.0" /></ItemGroup></Project>"#,
        )?;
        assert_eq!(
            nuget("rulebearing-rules/nextjs", &repo, &env)?,
            vec![packages.join("rulebearing.rules/2.0.0/rulebearing-rules/nextjs")]
        );
        assert!(nuget("other-package/x", &repo, &env)?.is_empty());
        assert!(nuget("rulebearing-rules/x", &repo, &Environment::default())?.is_empty());
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn site_packages_are_found_in_the_virtual_env_and_in_local_environments()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("site");
        let repo = dir.join("repo");
        let unix = repo.join(".venv/lib/python3.12/site-packages/rulebearing_rules");
        std::fs::create_dir_all(&unix)?;
        std::fs::create_dir_all(repo.join("config"))?;
        let elsewhere = dir.join("env");
        let windows = elsewhere.join("Lib/site-packages/rulebearing_rules");
        std::fs::create_dir_all(&windows)?;
        let env = Environment {
            nuget_packages: None,
            virtual_env: Some(elsewhere),
        };
        assert_eq!(
            site_packages("rulebearing-rules/django", &repo.join("config"), &env),
            vec![windows.join("django"), unix.join("django")]
        );
        assert!(site_packages("absent-package/x", &repo, &env).is_empty());
        assert!(site_packages("plain", &repo, &env).is_empty());
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn the_environment_reads_the_process() {
        let env = Environment::current();
        assert!(env.nuget_packages.is_some() || std::env::var_os("HOME").is_none());
    }
}
