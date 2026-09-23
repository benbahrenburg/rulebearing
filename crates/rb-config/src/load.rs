//! Loading: one file in, one [`Config`] out.
//!
//! - Contract: [Wave 1 plan § 1.5](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#15-interfaces-and-contracts-frozen-by-this-wave)
//!   (`load(path, opts) -> Result<Config, ConfigError>`; every error is exit 3)
//! - Decisions: [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md),
//!   [ADR-0006](../../../docs/adr/0006-embedded-quickjs-config-evaluator.md),
//!   [ADR-0008](../../../docs/adr/0008-exit-code-contract.md)
//! - Plan: [Wave 1, Steps 1 to 3](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-1-config-model-and-the-two-front-ends-1a)
//! - Requirements: [FR-CFG-01](../../../docs/prd.md#fr-cfg-01) to [FR-CFG-06](../../../docs/prd.md#fr-cfg-06)
//!
//! The stages, in order: read the file; map a native file onto the canonical shape; resolve and
//! merge `extends`, depth first, as dependency-cruiser does; substitute `defines`; expand the
//! shorthands; check every key; normalise the rule set and compile its patterns; split the
//! options into the TypeScript extractor's block and the rest.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rb_model::{DotnetOptions, PythonOptions, TypeScriptOptions};
use serde_json::{Map, Value};

use crate::extends::{self, Target};
use crate::js::{self, Kind, Limits};
use crate::model::{
    CompatMode, Config, Define, KnownViolation, Languages, Options, Ratchet, Rules,
};
use crate::read::{self, Evaluation, Syntax};
use crate::{ConfigError, ConfigFormat, defines, native, normalize, shorthands};

/// How to load.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadOptions {
    /// The repository root the JavaScript sandbox may read under. Default: the nearest folder
    /// above the configuration holding `.git`, else the configuration's folder.
    pub root: Option<PathBuf>,
    /// The format, overriding detection by name (`--config-format`).
    pub format: Option<ConfigFormat>,
    /// Refuse what dependency-cruiser would refuse (`--strict-compat`).
    pub strict_compat: bool,
    /// Evaluate JavaScript with Node (`--config-via-node`).
    pub via_node: bool,
    /// Sandbox limits.
    pub limits: Limits,
}

/// The repository root for a configuration in `dir`.
pub fn repository_root(dir: &Path) -> PathBuf {
    dir.ancestors()
        .find(|d| d.join(".git").exists())
        .unwrap_or(dir)
        .to_path_buf()
}

fn evaluation(opts: &LoadOptions) -> Evaluation {
    Evaluation {
        via_node: opts.via_node,
        limits: opts.limits,
    }
}

fn as_object(value: Value, file: &Path) -> Result<Map<String, Value>, ConfigError> {
    match value {
        Value::Object(map) => Ok(map),
        other => Err(ConfigError::Invalid(format!(
            "{}: a configuration must be an object, not {}",
            file.display(),
            match other {
                Value::Array(_) => "an array",
                Value::Null => "null",
                _ => "a scalar",
            }
        ))),
    }
}

/// Maps a parsed file onto the canonical shape, deciding its format.
fn canonical_of(
    value: Map<String, Value>,
    format: Option<ConfigFormat>,
) -> Result<(Map<String, Value>, CompatMode), ConfigError> {
    let native = match format {
        Some(ConfigFormat::Native) => true,
        Some(ConfigFormat::DependencyCruiser) => false,
        None => native::looks_native(&value),
    };
    if native {
        Ok((native::to_canonical(&value)?, CompatMode::Native))
    } else {
        Ok((value, CompatMode::DependencyCruiser))
    }
}

/// Loads the configuration at `path`.
///
/// # Errors
/// Every [`ConfigError`]; the command line maps each to exit 3.
pub fn load(path: &Path, opts: &LoadOptions) -> Result<Config, ConfigError> {
    let file = read_canonical(path, opts)?;
    let mut files = file.files;
    let mut config = assemble(
        file.canonical,
        file.compat,
        &file.dir,
        &file.root,
        opts,
        &mut files,
    )?;
    config.origin = Some(file.path);
    config.files = files.into_iter().collect();
    config.via_node = opts.via_node;
    Ok(config)
}

/// The configuration at `path` in the canonical shape with `extends` merged and nothing else
/// applied: the stage dependency-cruiser's `assertRuleSetValid` checks against its schema, which
/// is what conformance layer 4 validates.
///
/// # Errors
/// See [`load`].
pub fn merged(path: &Path, opts: &LoadOptions) -> Result<Map<String, Value>, ConfigError> {
    let mut file = read_canonical(path, opts)?;
    resolve_extends(
        file.canonical,
        &file.dir,
        &file.root,
        opts,
        &mut Vec::new(),
        &mut file.files,
    )
}

/// A configuration file read and mapped onto the canonical shape, before `extends`.
struct CanonicalFile {
    path: PathBuf,
    dir: PathBuf,
    root: PathBuf,
    canonical: Map<String, Value>,
    compat: CompatMode,
    files: BTreeSet<PathBuf>,
}

fn read_canonical(path: &Path, opts: &LoadOptions) -> Result<CanonicalFile, ConfigError> {
    let path = path.canonicalize().map_err(|e| ConfigError::Read {
        file: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let root = opts.root.clone().unwrap_or_else(|| repository_root(&dir));
    let syntax = Syntax::of(&path).unwrap_or(Syntax::JavaScript);
    let read = read::read_file(&path, syntax, &root, evaluation(opts))?;
    let format = opts.format.or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .and_then(ConfigFormat::detect)
    });
    let (canonical, compat) = canonical_of(as_object(read.value, &path)?, format)?;
    Ok(CanonicalFile {
        path,
        dir,
        root,
        canonical,
        compat,
        files: read.files.into_iter().collect(),
    })
}

/// Loads configuration text read from somewhere other than a file (`--config -`).
///
/// # Errors
/// See [`load`].
pub fn load_text(
    text: &str,
    syntax: Syntax,
    base_dir: &Path,
    opts: &LoadOptions,
) -> Result<Config, ConfigError> {
    let root = opts
        .root
        .clone()
        .unwrap_or_else(|| repository_root(base_dir));
    let name = base_dir.join("<stdin>");
    let value = read::parse_text(text, syntax, &name, &root, opts.limits)?;
    let (canonical, compat) = canonical_of(as_object(value, &name)?, opts.format)?;
    let mut files = BTreeSet::new();
    assemble(canonical, compat, base_dir, &root, opts, &mut files)
}

/// Builds a configuration from a canonical value already in memory (the rule set a
/// dependency-cruiser specification hands to `rulebearing validate`).
///
/// # Errors
/// See [`load`].
pub fn from_canonical(
    canonical: Map<String, Value>,
    compat: CompatMode,
) -> Result<Config, ConfigError> {
    let dir = std::env::current_dir().unwrap_or_default();
    let mut files = BTreeSet::new();
    assemble(
        canonical,
        compat,
        &dir,
        &dir,
        &LoadOptions::default(),
        &mut files,
    )
}

/// Resolves `extends` in `canonical`, depth first.
fn resolve_extends(
    mut canonical: Map<String, Value>,
    base_dir: &Path,
    root: &Path,
    opts: &LoadOptions,
    visiting: &mut Vec<String>,
    files: &mut BTreeSet<PathBuf>,
) -> Result<Map<String, Value>, ConfigError> {
    let entries = extends::entries(&canonical)?;
    canonical.remove("extends");
    for entry in entries {
        let target = extends::resolve(&entry, base_dir)?;
        let key = target.key();
        if visiting.contains(&key) {
            return Err(ConfigError::Extends {
                spec: entry,
                reason: format!("the chain is circular: {} -> {key}", visiting.join(" -> ")),
            });
        }
        visiting.push(key);
        let (value, dir) = match &target {
            Target::File(path) => {
                let syntax = Syntax::of(path).unwrap_or(Syntax::JavaScript);
                let read = read::read_file(path, syntax, root, evaluation(opts))?;
                files.extend(read.files);
                (read.value, path.parent().unwrap_or(base_dir).to_path_buf())
            }
            Target::DependencyCruiserPreset(name) => {
                let virtual_name = PathBuf::from(format!("rb:dc-preset/{name}"));
                let evaluated =
                    js::evaluate_text(&virtual_name, "", Kind::CommonJs, root, opts.limits)?;
                (evaluated.value, base_dir.to_path_buf())
            }
            Target::NativePreset(name, text) => {
                let value = serde_yaml::from_str(text).map_err(|e| ConfigError::Parse {
                    file: PathBuf::from(format!("rulebearing:{name}")),
                    reason: e.to_string(),
                })?;
                (value, base_dir.to_path_buf())
            }
        };
        let (loaded, _) = canonical_of(as_object(value, Path::new(&entry))?, None)?;
        let loaded = resolve_extends(loaded, &dir, root, opts, visiting, files)?;
        visiting.pop();
        canonical = extends::merge(&canonical, &loaded);
    }
    Ok(canonical)
}

/// The option keys that belong to the TypeScript extractor's block.
fn typescript_keys() -> Vec<String> {
    let schema = schemars::schema_for!(TypeScriptOptions);
    schema
        .as_value()
        .get("properties")
        .and_then(Value::as_object)
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default()
}

/// dependency-cruiser's `normalizeFilterOption` for `focus`, `reaches` and `highlight`.
fn filter_option(value: &Value) -> Option<Value> {
    let mut filter = match value {
        Value::String(_) | Value::Array(_) => {
            let mut map = Map::new();
            map.insert("path".into(), value.clone());
            map
        }
        Value::Object(map) => map.clone(),
        _ => return None,
    };
    if let Some(Value::Array(paths)) = filter.get("path") {
        let joined = paths
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("|");
        filter.insert("path".into(), Value::String(joined));
    }
    Some(Value::Object(filter))
}

/// Splits `options` into the TypeScript extractor's block, the rest, and `knownViolations`.
fn split_options(
    canonical: &Map<String, Value>,
) -> Result<(TypeScriptOptions, Options, Vec<KnownViolation>), ConfigError> {
    let mut options_map = canonical
        .get("options")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let invalid = |what: &str, e: serde_json::Error| ConfigError::Invalid(format!("`{what}`: {e}"));
    let known_violations: Vec<KnownViolation> = options_map
        .get("knownViolations")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| invalid("options.knownViolations", e))?
        .unwrap_or_default();
    let typescript_keys = typescript_keys();
    let mut typescript = Map::new();
    for key in &typescript_keys {
        if let Some(value) = options_map.get(key) {
            typescript.insert(key.clone(), value.clone());
        }
    }
    for key in ["focus", "reaches", "highlight"] {
        if let Some(value) = options_map.get(key).and_then(filter_option) {
            options_map.insert(key.into(), value);
        }
    }
    let typescript: TypeScriptOptions = serde_json::from_value(Value::Object(typescript))
        .map_err(|e| invalid("options (TypeScript)", e))?;
    let rest: Map<String, Value> = options_map
        .iter()
        .filter(|(k, _)| {
            !typescript_keys.contains(k)
                && !matches!(
                    k.as_str(),
                    "knownViolations" | "rulesFile" | "validate" | "args"
                )
                && normalize::OPTION_KEYS.contains(&k.as_str())
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let options: Options =
        serde_json::from_value(Value::Object(rest)).map_err(|e| invalid("options", e))?;

    Ok((typescript, options, known_violations))
}

fn assemble(
    canonical: Map<String, Value>,
    compat: CompatMode,
    base_dir: &Path,
    root: &Path,
    opts: &LoadOptions,
    files: &mut BTreeSet<PathBuf>,
) -> Result<Config, ConfigError> {
    let written_extends = extends::entries(&canonical)?;
    let schema = canonical
        .get("$schema")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut canonical = resolve_extends(canonical, base_dir, root, opts, &mut Vec::new(), files)?;
    defines::apply_defines(&mut canonical, base_dir)?;
    let merged = canonical.clone();
    let expanded = shorthands::expand(&mut canonical)?;
    let keys = normalize::check_keys(&canonical, compat, opts.strict_compat)?;
    let rules = normalize::rule_set(&canonical)?;
    let mut warnings = keys.warnings;
    warnings.extend(normalize::check_patterns(&rules, opts.strict_compat)?);

    let (typescript, options, known_violations) = split_options(&canonical)?;
    let invalid = |what: &str, e: serde_json::Error| ConfigError::Invalid(format!("`{what}`: {e}"));
    let languages = canonical
        .get("languages")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let dotnet: Option<DotnetOptions> = languages
        .get("dotnet")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| invalid("languages.dotnet", e))?;
    let python: Option<PythonOptions> = languages
        .get("python")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| invalid("languages.python", e))?;
    let ratchets: Vec<Ratchet> = canonical
        .get("ratchets")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| invalid("rules.ratchets", e))?
        .unwrap_or_default();
    let defines = canonical
        .get("defines")
        .cloned()
        .map(serde_json::from_value::<std::collections::BTreeMap<String, Define>>)
        .transpose()
        .map_err(|e| invalid("defines", e))?
        .unwrap_or_default();
    Ok(Config {
        schema,
        extends: written_extends,
        defines,
        languages: Languages {
            typescript,
            dotnet,
            python,
        },
        options,
        rules: Rules {
            dependencies: rules,
            ratchets,
            layers: expanded.layers,
            independence: expanded.independence,
        },
        known_violations,
        compat,
        origin: None,
        canonical: merged,
        warnings,
        files: Vec::new(),
        via_node: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_model::Severity;
    use std::error::Error;

    struct Repo(PathBuf);

    impl Repo {
        fn new(name: &str, files: &[(&str, &str)]) -> Result<Self, Box<dyn Error>> {
            let dir = std::env::temp_dir().join(format!("rb-load-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join(".git"))?;
            for (file, text) in files {
                let path = dir.join(file);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(path, text)?;
            }
            Ok(Self(dir.canonicalize()?))
        }

        fn load(&self, file: &str) -> Result<Config, ConfigError> {
            load(&self.0.join(file), &LoadOptions::default())
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn merged_is_the_file_with_extends_and_nothing_else() -> Result<(), Box<dyn Error>> {
        let repo = Repo::new(
            "merged",
            &[
                (
                    ".dependency-cruiser.cjs",
                    r#"module.exports = {
  extends: "./base.json",
  forbidden: [{ name: "shared", severity: "error" }],
};"#,
                ),
                (
                    "base.json",
                    r#"{ "forbidden": [{ "name": "shared", "from": {}, "to": { "circular": true } }], "options": { "maxDepth": 2 } }"#,
                ),
            ],
        )?;
        let merged = merged(
            &repo.0.join(".dependency-cruiser.cjs"),
            &LoadOptions::default(),
        )?;
        assert!(merged.get("extends").is_none());
        assert_eq!(merged["options"]["maxDepth"], 2);
        // The rule is merged by name and not normalised: no `scope` is added.
        assert_eq!(
            merged["forbidden"],
            serde_json::json!([{ "name": "shared", "severity": "error", "from": {}, "to": { "circular": true } }])
        );
        assert!(super::merged(&repo.0.join("missing.json"), &LoadOptions::default()).is_err());
        Ok(())
    }

    #[test]
    fn a_dependency_cruiser_config_with_extends_and_presets() -> Result<(), Box<dyn Error>> {
        let repo = Repo::new(
            "dc",
            &[
                (
                    ".dependency-cruiser.cjs",
                    r#"module.exports = {
  extends: ["dependency-cruiser/configs/recommended-strict", "./base.json"],
  forbidden: [{ name: "no-circular", severity: "warn" }, { name: "mine", from: { path: ["^a", "^b"] }, to: { path: "^c" } }],
  options: { tsPreCompilationDeps: true, focus: "^src", exclude: "node_modules" },
};"#,
                ),
                (
                    "base.json",
                    r#"{ "options": { "maxDepth": 2, "prefix": "x" } }"#,
                ),
            ],
        )?;
        let config = repo.load(".dependency-cruiser.cjs")?;
        assert_eq!(config.compat, CompatMode::DependencyCruiser);
        let names: Vec<&str> = config
            .rules
            .dependencies
            .forbidden
            .iter()
            .map(crate::model::Rule::name)
            .collect();
        assert!(
            names.contains(&"mine") && names.contains(&"no-orphans"),
            "{names:?}"
        );
        let circular = config
            .rules
            .dependencies
            .forbidden
            .iter()
            .find(|r| r.name() == "no-circular");
        assert_eq!(
            circular.map(crate::model::Rule::severity),
            Some(Severity::Warn),
            "the extender wins"
        );
        assert!(config.languages.typescript.keeps_pre_compilation_deps());
        assert_eq!(config.languages.typescript.max_depth(), 2);
        assert_eq!(config.options.prefix.as_deref(), Some("x"));
        assert_eq!(
            config
                .options
                .focus
                .as_ref()
                .and_then(|f| f.path.as_deref()),
            Some("^src")
        );
        assert_eq!(config.extends.len(), 2);
        assert!(config.files.len() >= 2);
        Ok(())
    }

    #[test]
    fn a_native_config_with_every_addition() -> Result<(), Box<dyn Error>> {
        let repo = Repo::new(
            "native",
            &[
                (
                    "rulebearing.yaml",
                    r#"$schema: https://benbahrenburg.github.io/rulebearing/schema/v1.json
extends: [rulebearing:recommended]
defines:
  legacyApps: { fromJson: exceptions.json, select: "[*].app", joinWith: "|" }
languages:
  typescript: { tsPreCompilationDeps: specify }
rules:
  dependencies:
    forbidden:
      - name: no-cross-app-imports
        comment: "adr:0003"
        fix: "Call the other app over its API."
        severity: error
        from: { path: "^apps/([^/]+)/", pathNot: "^apps/(${legacyApps})/" }
        to: { path: "^apps/([^/]+)/", pathNot: "^apps/$1/" }
        examples: { forbidden: ["apps/web/x.ts -> apps/api/y.ts"] }
  ratchets:
    - { name: routes, from: { path: "^apps/" }, to: { path: "^lib/" }, budget: budget.json }
  layers:
    - { name: clean, layers: ["^web/", "^domain/"] }
  independence:
    - { name: features, pattern: "^features/([^/]+)/" }
"#,
                ),
                ("exceptions.json", r#"[{"app": "old"}]"#),
            ],
        )?;
        let config = repo.load("rulebearing.yaml")?;
        assert_eq!(config.compat, CompatMode::Native);
        let rule = config
            .rules
            .dependencies
            .forbidden
            .iter()
            .find(|r| r.name() == "no-cross-app-imports");
        assert_eq!(
            rule.and_then(|r| r.from.path_not.clone())
                .map(|p| p.joined()),
            Some("^apps/(old)/".to_owned())
        );
        assert!(rule.and_then(|r| r.meta.fix.clone()).is_some());
        assert_eq!(config.rules.ratchets.len(), 1);
        assert_eq!(config.rules.layers.len(), 1);
        assert_eq!(config.rules.independence.len(), 1);
        let names: Vec<&str> = config
            .rules
            .dependencies
            .forbidden
            .iter()
            .map(crate::model::Rule::name)
            .collect();
        assert!(
            names.contains(&"clean:2-to-1")
                && names.contains(&"features")
                && names.contains(&"no-circular"),
            "{names:?}"
        );
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        assert_eq!(config.defines.len(), 1);
        assert!(config.schema.is_some());
        Ok(())
    }

    #[test]
    fn every_native_syntax_loads_to_the_same_rules() -> Result<(), Box<dyn Error>> {
        let repo = Repo::new(
            "syntaxes",
            &[
                (
                    "rulebearing.yaml",
                    "rules: { dependencies: { forbidden: [{ name: r, from: { path: '^a' }, to: {} }] } }",
                ),
                (
                    "rulebearing.json",
                    r#"{"rules": {"dependencies": {"forbidden": [{"name": "r", "from": {"path": "^a"}, "to": {}}]}}}"#,
                ),
                (
                    "rulebearing.jsonc",
                    "// c\n{\"rules\": {\"dependencies\": {\"forbidden\": [{\"name\": \"r\", \"from\": {\"path\": \"^a\"}, \"to\": {},},]}}}",
                ),
                (
                    "rulebearing.toml",
                    "[[rules.dependencies.forbidden]]\nname = \"r\"\nfrom = { path = \"^a\" }\nto = {}\n",
                ),
            ],
        )?;
        let first = repo.load("rulebearing.yaml")?.rules;
        for file in ["rulebearing.json", "rulebearing.jsonc", "rulebearing.toml"] {
            assert_eq!(repo.load(file)?.rules, first, "{file}");
        }
        Ok(())
    }

    #[test]
    fn stdin_text_loads() -> Result<(), ConfigError> {
        let dir = std::env::temp_dir();
        let config = load_text(
            "forbidden: [{ name: r, from: {}, to: { circular: true } }]",
            Syntax::Yaml,
            &dir,
            &LoadOptions {
                format: Some(ConfigFormat::DependencyCruiser),
                root: Some(dir.clone()),
                ..LoadOptions::default()
            },
        )?;
        assert_eq!(config.rules.dependencies.forbidden.len(), 1);
        Ok(())
    }

    #[test]
    fn errors_are_named() -> Result<(), Box<dyn Error>> {
        let repo = Repo::new(
            "errors",
            &[
                ("a.json", r#"{ "extends": "./b.json" }"#),
                ("b.json", r#"{ "extends": "./a.json" }"#),
                ("array.json", "[]"),
                ("bad-extends.json", r#"{ "extends": "./nope" }"#),
                (
                    "bad-rule.json",
                    r#"{ "forbidden": [{ "from": { "path": "(?<=x)" }, "to": {} }] }"#,
                ),
                ("ts.json", r#"{ "options": { "moduleSystems": ["nope"] } }"#),
            ],
        )?;
        let message = |file: &str| {
            repo.load(file)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default()
        };
        assert!(message("a.json").contains("circular"));
        assert!(message("array.json").contains("an array"));
        assert!(message("bad-extends.json").contains("./nope"));
        assert!(message("bad-rule.json").contains("lookbehind"));
        assert!(message("ts.json").contains("nope"));
        assert!(
            load(
                Path::new("/definitely/missing.json"),
                &LoadOptions::default()
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn filter_options_normalise() {
        assert_eq!(
            filter_option(&serde_json::json!(["a", "b"])),
            Some(serde_json::json!({ "path": "a|b" }))
        );
        assert_eq!(
            filter_option(&serde_json::json!({ "path": ["a"], "depth": 2 })),
            Some(serde_json::json!({ "path": "a", "depth": 2 }))
        );
        assert_eq!(filter_option(&serde_json::json!(3)), None);
    }

    #[test]
    fn from_canonical_normalises_a_rule_set() -> Result<(), ConfigError> {
        let map = match serde_json::json!({ "forbidden": [{ "from": {}, "to": {} }] }) {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        let config = from_canonical(map, CompatMode::DependencyCruiser)?;
        assert_eq!(config.rules.dependencies.forbidden[0].name(), "unnamed");
        Ok(())
    }

    #[test]
    fn the_repository_root_is_the_git_folder() -> Result<(), Box<dyn Error>> {
        let repo = Repo::new("root", &[("a/b/c.json", "{}")])?;
        assert_eq!(repository_root(&repo.0.join("a/b")), repo.0);
        Ok(())
    }
}
