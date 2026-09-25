//! `package.json`: finding the manifest that classifies a dependency, and what it says about it.
//!
//! - Plan: [Wave 0, Step 8](../../../docs/plans/pending/0000-wave-0-spike.md#step-8-spike-a-rb-extract-ts-0c)
//!   (`npm.rs`: "nearest `package.json` walk-up to classify `npm`, `npm-dev`, `npm-peer`,
//!   `npm-optional`, `npm-bundled`, `npm-no-pkg`, `npm-unknown`")
//! - Source: [coverage § Extraction and resolution](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution),
//!   rows npm classification and `combinedDependencies`
//! - Specification: dependency-cruiser 18.2.0 `src/extract/resolve/{get-manifest,merge-manifests,
//!   determine-dependency-types,external-module-helpers}.mjs`
//!
//! Key order matters: dependency-cruiser reports the dependency types in the order the keys appear
//! in the manifest, so a manifest is kept as an ordered list of `(key, value)` pairs rather than a
//! map.

use std::path::{Path, PathBuf};

use rb_model::DependencyType;
use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::Value;

/// A JSON object with its keys in file order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Manifest(pub Vec<(String, Value)>);

impl<'de> Deserialize<'de> for Manifest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Ordered;
        impl<'de> Visitor<'de> for Ordered {
            type Value = Manifest;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Manifest, A::Error> {
                let mut entries = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, Value>()? {
                    entries.retain(|(k, _): &(String, Value)| *k != key);
                    entries.push((key, value));
                }
                Ok(Manifest(entries))
            }
        }
        deserializer.deserialize_map(Ordered)
    }
}

impl Manifest {
    /// The value under `key`.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_str())
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

fn read_manifest(folder: &Path) -> Option<Result<Manifest, ()>> {
    let text = std::fs::read_to_string(folder.join("package.json")).ok()?;
    Some(serde_json::from_str(&text).map_err(|_| ()))
}

/// The nearest `package.json` from `folder` upwards. A manifest that is not valid JSON stops the
/// walk and counts as none, as upstream's does.
pub fn nearest(folder: &Path) -> Option<Manifest> {
    let mut current = Some(folder);
    while let Some(dir) = current {
        match read_manifest(dir) {
            Some(Ok(manifest)) => return Some(manifest),
            Some(Err(())) => return None,
            None => {
                current = dir
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .or_else(|| {
                        (dir != Path::new(".") && dir.is_relative()).then_some(Path::new("."))
                    });
            }
        }
        if current == Some(dir) {
            break;
        }
    }
    None
}

fn is_interesting(key: &str) -> bool {
    key.ends_with("ependencies") || key == "workspaces" || key == "imports"
}

fn is_array_key(key: &str) -> bool {
    key.starts_with("bundle") || key == "workspaces"
}

fn normalise_keys(manifest: Manifest) -> Manifest {
    Manifest(
        manifest
            .0
            .into_iter()
            .map(|(k, v)| {
                if k == "bundleDependencies" {
                    ("bundledDependencies".to_owned(), v)
                } else {
                    (k, v)
                }
            })
            .collect(),
    )
}

/// Merges two manifests, the closer one winning, as `combinedDependencies` does.
pub fn merge(closest: Manifest, further: Manifest) -> Manifest {
    let closest = normalise_keys(closest);
    let further = normalise_keys(further);
    let mut keys: Vec<String> = Vec::new();
    for key in closest
        .keys()
        .chain(further.keys())
        .filter(|k| is_interesting(k))
    {
        if !keys.iter().any(|k| k == key) {
            keys.push(key.to_owned());
        }
    }
    Manifest(
        keys.into_iter()
            .map(|key| {
                let near = closest.get(&key);
                let far = further.get(&key);
                let value = if is_array_key(&key) {
                    let mut items: Vec<Value> = Vec::new();
                    for item in near
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .chain(far.and_then(Value::as_array).into_iter().flatten())
                    {
                        if !items.contains(item) {
                            items.push(item.clone());
                        }
                    }
                    Value::Array(items)
                } else {
                    let mut object = far.and_then(Value::as_object).cloned().unwrap_or_default();
                    for (k, v) in near.and_then(Value::as_object).into_iter().flatten() {
                        object.insert(k.clone(), v.clone());
                    }
                    Value::Object(object)
                };
                (key, value)
            })
            .collect(),
    )
}

/// The manifests from `file_dir` up to `base_dir`, merged closest first; `None` when there are
/// none. Upstream refuses a `file_dir` outside `base_dir`, so that is `None` too.
pub fn combined(file_dir: &Path, base_dir: &Path) -> Option<Manifest> {
    if !file_dir.starts_with(base_dir) {
        return None;
    }
    let mut folders: Vec<PathBuf> = Vec::new();
    let mut current = file_dir.to_path_buf();
    while current != base_dir && current.parent().is_some_and(|p| p != current) {
        folders.push(current.clone());
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent.to_path_buf();
    }
    folders.push(base_dir.to_path_buf());
    let merged = folders.iter().fold(Manifest::default(), |all, folder| {
        let next = read_manifest(folder)
            .and_then(Result::ok)
            .unwrap_or_default();
        merge(all, next)
    });
    (!merged.is_empty()).then_some(merged)
}

/// The package a specifier names: `lodash/fp` is `lodash`, `@scope/pkg/x` is `@scope/pkg`.
pub fn package_root(module: &str) -> &str {
    if module.is_empty() || crate::resolve::is_relative(module) {
        return module;
    }
    let mut parts = module.splitn(3, '/');
    let first = parts.next().unwrap_or(module);
    if module.starts_with('@') {
        match parts.next() {
            Some(second) => &module[..first.len() + 1 + second.len()],
            None => module,
        }
    } else {
        first
    }
}

fn dependency_type_of(key: &str) -> DependencyType {
    match key {
        "dependencies" => DependencyType::Npm,
        "devDependencies" => DependencyType::NpmDev,
        "optionalDependencies" => DependencyType::NpmOptional,
        "peerDependencies" => DependencyType::NpmPeer,
        _ => DependencyType::NpmNoPkg,
    }
}

fn find_in(manifest: &Manifest, name: &str) -> Vec<DependencyType> {
    manifest
        .0
        .iter()
        .filter(|(key, value)| {
            key.contains("ependencies") && value.as_object().is_some_and(|o| o.contains_key(name))
        })
        .map(|(key, _)| dependency_type_of(key))
        .collect()
}

/// The npm dependency types for a package, from the manifest (`npm-unknown` without one).
pub fn manifest_dependency_types(
    package: &str,
    manifest: Option<&Manifest>,
    modules: &[String],
) -> Vec<DependencyType> {
    let Some(manifest) = manifest else {
        return vec![DependencyType::NpmUnknown];
    };
    let mut found = find_in(manifest, package);
    if found.is_empty() && modules.iter().any(|m| m.contains("@types")) {
        found = find_in(manifest, &format!("@types/{package}"));
    }
    if found.is_empty() {
        found.push(DependencyType::NpmNoPkg);
    }
    found
}

/// Whether the manifest lists `module` in `bundledDependencies`.
pub fn is_bundled(module: &str, manifest: Option<&Manifest>) -> bool {
    manifest
        .and_then(|m| {
            m.get("bundledDependencies")
                .or_else(|| m.get("bundleDependencies"))
        })
        .and_then(Value::as_array)
        .is_some_and(|list| list.iter().any(|v| v.as_str() == Some(module)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(json: &str) -> Manifest {
        serde_json::from_str(json).unwrap_or_default()
    }

    #[test]
    fn keeps_keys_in_file_order() {
        let m = manifest(r#"{"devDependencies": {"a": "1"}, "dependencies": {"a": "1"}}"#);
        assert_eq!(
            manifest_dependency_types("a", Some(&m), &[]),
            [DependencyType::NpmDev, DependencyType::Npm]
        );
    }

    #[test]
    fn classifies_like_dependency_cruiser() {
        let m = manifest(
            r#"{"dependencies": {"x": "1", "@types/y": "1"}, "peerDependencies": {"p": "1"}, "optionalDependencies": {"o": "1"}, "bundledDependencies": ["x"]}"#,
        );
        let types = |name: &str, modules: &[&str]| {
            let modules: Vec<String> = modules.iter().map(|s| (*s).to_owned()).collect();
            manifest_dependency_types(name, Some(&m), &modules)
        };
        assert_eq!(types("x", &[]), [DependencyType::Npm]);
        assert_eq!(types("p", &[]), [DependencyType::NpmPeer]);
        assert_eq!(types("o", &[]), [DependencyType::NpmOptional]);
        assert_eq!(types("nope", &[]), [DependencyType::NpmNoPkg]);
        assert_eq!(
            types("y", &["node_modules", "node_modules/@types"]),
            [DependencyType::Npm]
        );
        assert_eq!(types("y", &["node_modules"]), [DependencyType::NpmNoPkg]);
        assert_eq!(
            manifest_dependency_types("x", None, &[]),
            [DependencyType::NpmUnknown]
        );
        assert!(is_bundled("x", Some(&m)));
        assert!(!is_bundled("p", Some(&m)));
        assert!(!is_bundled("x", None));
    }

    #[test]
    fn package_roots_handle_scopes_and_relatives() {
        assert_eq!(package_root("lodash/fp"), "lodash");
        assert_eq!(package_root("@scope/pkg/deep/x"), "@scope/pkg");
        assert_eq!(package_root("@scope"), "@scope");
        assert_eq!(package_root("./local"), "./local");
        assert_eq!(package_root(""), "");
    }

    #[test]
    fn merging_prefers_the_closest_and_unions_arrays() {
        let near = manifest(
            r#"{"name": "near", "dependencies": {"a": "2"}, "workspaces": ["x"], "bundleDependencies": ["b"]}"#,
        );
        let far = manifest(
            r#"{"dependencies": {"a": "1", "c": "1"}, "workspaces": ["x", "y"], "devDependencies": {"d": "1"}}"#,
        );
        let merged = merge(near, far);
        let keys: Vec<&str> = merged.keys().collect();
        assert_eq!(
            keys,
            [
                "dependencies",
                "workspaces",
                "bundledDependencies",
                "devDependencies"
            ]
        );
        assert_eq!(
            merged.get("dependencies").and_then(|d| d.get("a")),
            Some(&Value::from("2"))
        );
        assert_eq!(
            merged.get("dependencies").and_then(|d| d.get("c")),
            Some(&Value::from("1"))
        );
        assert_eq!(
            merged.get("workspaces"),
            Some(&serde_json::json!(["x", "y"]))
        );
    }

    #[test]
    fn finds_nearest_and_combined_manifests_on_disk() {
        let root = std::env::temp_dir().join(format!("rb-npm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(root.join("a/b"));
        let _ = std::fs::write(
            root.join("package.json"),
            r#"{"dependencies": {"top": "1"}}"#,
        );
        let _ = std::fs::write(
            root.join("a/package.json"),
            r#"{"dependencies": {"mid": "1"}}"#,
        );
        let near = nearest(&root.join("a/b"));
        assert_eq!(near.as_ref().map(|m| find_in(m, "mid").len()), Some(1));
        let all = combined(&root.join("a/b"), &root);
        assert_eq!(all.as_ref().map(|m| find_in(m, "top").len()), Some(1));
        assert_eq!(all.as_ref().map(|m| find_in(m, "mid").len()), Some(1));
        assert_eq!(combined(&root, &root.join("a")), None);
        let _ = std::fs::write(root.join("a/package.json"), "not json");
        assert_eq!(nearest(&root.join("a/b")), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
