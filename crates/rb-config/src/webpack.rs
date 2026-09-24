//! `webpackConfig`: the `resolve` block of a webpack configuration, evaluated in the sandbox.
//!
//! - Plan: [Wave 2, Step 9](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#29-step-9-presets---init-presets-vue-svelte-markdown-webpackconfig-collapse-highlight-experimentalstats-2d)
//!   ("`webpackConfig.fileName`, `env`, `arguments`: evaluate in the wave 1 sandbox and read
//!   `resolve.alias`, `resolve.modules`, `resolve.extensions` into `oxc_resolver` options;
//!   `--webpack-config-json` accepts a pre-evaluated copy")
//! - Decisions: [ADR-0006](../../../docs/adr/0006-embedded-quickjs-config-evaluator.md),
//!   [ADR-0027](../../../docs/adr/0027-pure-path-and-url-modules-in-the-config-sandbox.md)
//! - Coverage: [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   row `webpackConfig.fileName`, `env`, `arguments`
//! - Requirement: [FR-CFG-06](../../../docs/prd.md#fr-cfg-06) (`webpackConfig` as an alias into
//!   `languages.typescript`)
//! - Specification: dependency-cruiser 18.2.0 `src/config-utl/extract-webpack-resolve-config.mjs`
//!
//! The file is evaluated as any JavaScript configuration is: in the QuickJS sandbox, or by the
//! local Node under `--config-via-node`. Its module is pried as upstream pries it
//! ([`crate::js::WEBPACK_PICK`]) and its `resolve` block returned; the TypeScript extractor maps
//! that block onto its resolver. A configuration the sandbox cannot run (one that loads webpack
//! plugins from `node_modules`, say) fails with the sandbox's reason, which names
//! `--config-via-node`; `--webpack-config-json` takes a copy evaluated elsewhere.

use std::path::{Path, PathBuf};

use rb_model::options::WebpackConfig;
use serde_json::Value;

use crate::ConfigError;
use crate::js::{self, WebpackCall};
use crate::read::Evaluation;

/// The extensions the sandbox evaluates; upstream loads others through `interpret` and
/// `rechoir`, which need a transpiler installed in Node.
const SANDBOX_EXTENSIONS: &[&str] = &["js", "cjs", "mjs", "json", "json5"];

/// An evaluated `resolve` block and the files read to produce it.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolveBlock {
    /// The `resolve` block, `{}` when the configuration has none.
    pub value: Value,
    /// Every file the evaluation read, for the `attest` receipt.
    pub files: Vec<PathBuf>,
}

/// Evaluates the webpack configuration `reference` names, relative to `cwd` (as upstream's
/// `makeAbsolute` takes it against the working directory), and returns its `resolve` block;
/// `None` when no `fileName` is set.
///
/// # Errors
/// [`ConfigError::Invalid`] for an extension only Node can load, run in the sandbox;
/// [`ConfigError::Js`] when the evaluation fails; [`ConfigError::Invalid`] when the `resolve`
/// block is not an object.
pub fn resolve_block(
    reference: &WebpackConfig,
    cwd: &Path,
    root: &Path,
    evaluation: Evaluation,
) -> Result<Option<ResolveBlock>, ConfigError> {
    let Some(name) = reference.file_name.as_deref() else {
        return Ok(None);
    };
    let file = cwd.join(name);
    let null = Value::Null;
    let call = WebpackCall {
        env: reference.env.as_ref().unwrap_or(&null),
        arguments: reference.arguments.as_ref().unwrap_or(&null),
    };
    let (value, files) = if evaluation.via_node {
        (
            js::via_node::evaluate_webpack(&file, call)?,
            vec![file.clone()],
        )
    } else {
        let extension = file.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !SANDBOX_EXTENSIONS.contains(&extension) {
            return Err(ConfigError::Invalid(format!(
                "webpackConfig.fileName {}: a .{extension} webpack configuration needs a \
                 transpiler; evaluate it with --config-via-node, or pass its resolve block \
                 with --webpack-config-json",
                file.display()
            )));
        }
        let evaluated = js::evaluate_webpack(&file, root, evaluation.limits, call)?;
        (evaluated.value, evaluated.files)
    };
    Ok(Some(ResolveBlock {
        value: object(value, &file.display().to_string())?,
        files,
    }))
}

/// A copy evaluated elsewhere (`--webpack-config-json`): the webpack configuration as JSON (an
/// object with a `resolve` key, or an array whose first element is one) or its `resolve` block
/// itself. A `resolve` block never has a `resolve` key of its own, so the two cannot be confused.
///
/// # Errors
/// [`ConfigError::Invalid`] when the value is not an object, or its `resolve` is not one.
pub fn from_json(value: Value, file: &str) -> Result<Value, ConfigError> {
    let config = match value {
        Value::Array(mut items) if !items.is_empty() => items.swap_remove(0),
        other => other,
    };
    let block = match config {
        Value::Object(mut map) => match map.remove("resolve") {
            Some(resolve) => resolve,
            None => Value::Object(map),
        },
        other => other,
    };
    object(block, file)
}

fn object(value: Value, file: &str) -> Result<Value, ConfigError> {
    if value.is_object() {
        Ok(value)
    } else {
        Err(ConfigError::Invalid(format!(
            "{file}: the webpack resolve block must be an object"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dir(name: &str, files: &[(&str, &str)]) -> Result<PathBuf, std::io::Error> {
        let dir = std::env::temp_dir().join(format!("rb-webpack-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        for (file, text) in files {
            std::fs::write(dir.join(file), text)?;
        }
        Ok(dir)
    }

    #[test]
    fn a_function_config_is_called_with_env_and_arguments() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = dir(
            "function",
            &[(
                "webpack.config.js",
                "const path = require('path');\nmodule.exports = (env, argv) => ({ resolve: { alias: { '@': path.resolve(__dirname, env.src) }, extensions: argv } });",
            )],
        )?;
        let reference = WebpackConfig {
            file_name: Some("webpack.config.js".into()),
            env: Some(json!({ "src": "app" })),
            arguments: Some(json!([".ts"])),
        };
        let block = resolve_block(&reference, &dir, &dir, Evaluation::default())?;
        let block = block.ok_or("no block")?;
        assert_eq!(block.value["extensions"], json!([".ts"]));
        assert!(
            block.value["alias"]["@"]
                .as_str()
                .is_some_and(|a| a.ends_with("/app"))
        );
        assert_eq!(block.files.len(), 1);
        assert_eq!(
            resolve_block(&WebpackConfig::default(), &dir, &dir, Evaluation::default())?,
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn a_typescript_config_is_refused_in_the_sandbox() -> Result<(), Box<dyn std::error::Error>> {
        let dir = dir("ts", &[("webpack.config.ts", "export default {};")])?;
        let reference = WebpackConfig {
            file_name: Some("webpack.config.ts".into()),
            ..WebpackConfig::default()
        };
        let error = resolve_block(&reference, &dir, &dir, Evaluation::default())
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("--config-via-node") && error.contains("--webpack-config-json"),
            "{error}"
        );
        let array = dir.join("array.cjs");
        std::fs::write(&array, "module.exports = { resolve: [1] };")?;
        let error = resolve_block(
            &WebpackConfig {
                file_name: Some("array.cjs".into()),
                ..WebpackConfig::default()
            },
            &dir,
            &dir,
            Evaluation::default(),
        )
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
        assert!(
            error.contains("not a JSON-shaped object (array)"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn a_pre_evaluated_copy_is_a_config_or_its_resolve_block() -> Result<(), ConfigError> {
        let block = json!({ "alias": { "@": "src" } });
        assert_eq!(from_json(block.clone(), "w.json")?, block);
        assert_eq!(
            from_json(json!({ "resolve": block, "entry": "x" }), "w.json")?,
            block
        );
        assert_eq!(
            from_json(json!([{ "resolve": block }, { "resolve": {} }]), "w.json")?,
            block
        );
        assert!(from_json(json!([]), "w.json").is_err());
        assert!(from_json(json!({ "resolve": "x" }), "w.json").is_err());
        assert!(from_json(json!(3), "w.json").is_err());
        Ok(())
    }
}
