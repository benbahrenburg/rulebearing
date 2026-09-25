//! The wave 2 options `cruise` applies end to end: `collapse` and `highlight` (configuration and
//! `--collapse`, `--highlight`), `webpackConfig` (`fileName`, `env`, `arguments`,
//! `--webpack-config`, `--webpack-config-json`) and Markdown fences by configuration format.
//!
//! - Plan: [Wave 2, Step 9](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#29-step-9-presets---init-presets-vue-svelte-markdown-webpackconfig-collapse-highlight-experimentalstats-2d)
//! - Coverage: [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options)
//!   (`collapse`, `highlight.path`, `webpackConfig`), [coverage § Command line](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line)
//! - Decisions: [ADR-0006](../../../docs/adr/0006-embedded-quickjs-config-evaluator.md),
//!   [ADR-0036](../../../docs/adr/0036-markdown-fences-follow-the-configuration-format.md)
//! - Requirements: [FR-CLI-08](../../../docs/prd.md#fr-cli-08), [FR-EXT-TS-04](../../../docs/prd.md#fr-ext-ts-04)

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

fn tree(name: &str, files: &[(&str, &str)]) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-options-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (file, text) in files {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    // The sandbox reads under the repository root; a `.git` folder makes this tree one.
    std::fs::create_dir_all(dir.join(".git"))?;
    Ok(dir)
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .env("SOURCE_DATE_EPOCH", "1790000000")
        .output()?)
}

fn json(output: &Output) -> Result<Value, Box<dyn Error>> {
    serde_json::from_slice(&output.stdout).map_err(|e| {
        format!(
            "{e}: stdout {} stderr {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into()
    })
}

fn sources(result: &Value) -> Vec<String> {
    result["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["source"].as_str().map(str::to_owned))
        .collect()
}

fn module<'r>(result: &'r Value, source: &str) -> Option<&'r Value> {
    result["modules"]
        .as_array()?
        .iter()
        .find(|m| m["source"] == source)
}

const LAYERED: &[(&str, &str)] = &[
    (
        "src/domain/model.ts",
        "import { w } from \"../web/view\";\nexport const d = w;\n",
    ),
    (
        "src/web/view.ts",
        "import { h } from \"./helper\";\nexport const w = h;\n",
    ),
    ("src/web/helper.ts", "export const h = 1;\n"),
    (
        "src/main.ts",
        "import { d } from \"./domain/model\";\nconsole.log(d);\n",
    ),
    (
        ".dependency-cruiser.json",
        r#"{ "forbidden": [], "options": { "tsPreCompilationDeps": true } }"#,
    ),
];

#[test]
fn collapse_folds_modules_to_a_depth_or_a_pattern() -> Result<(), Box<dyn Error>> {
    let dir = tree("collapse", LAYERED)?;
    let by_depth = run(&dir, &["cruise", "-T", "json", "--collapse", "2", "src"])?;
    assert_eq!(by_depth.status.code(), Some(0));
    let result = json(&by_depth)?;
    assert_eq!(sources(&result), ["src/domain/", "src/main.ts", "src/web/"]);
    let web = module(&result, "src/web/").ok_or("src/web/")?;
    assert_eq!(web["consolidated"], true);
    assert_eq!(
        web["dependencies"],
        serde_json::json!([]),
        "the edge inside the folder is a self edge and goes"
    );
    let domain = module(&result, "src/domain/").ok_or("src/domain/")?;
    assert_eq!(domain["dependencies"][0]["resolved"], "src/web/");
    assert_eq!(result["summary"]["optionsUsed"]["collapse"], "2");
    // The same from the configuration, as a regex; two runs serialise byte for byte.
    std::fs::write(
        dir.join(".dependency-cruiser.json"),
        r#"{ "forbidden": [], "options": { "tsPreCompilationDeps": true, "collapse": "^src/[^/]+" } }"#,
    )?;
    let first = run(&dir, &["cruise", "-T", "json", "src"])?;
    let second = run(&dir, &["cruise", "-T", "json", "src"])?;
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(
        sources(&json(&first)?),
        ["src/domain", "src/main.ts", "src/web"]
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn highlight_marks_every_module() -> Result<(), Box<dyn Error>> {
    let dir = tree("highlight", LAYERED)?;
    let output = run(&dir, &["cruise", "-T", "json", "-H", "^src/web/", "src"])?;
    let result = json(&output)?;
    for source in sources(&result) {
        let marked = module(&result, &source).map(|m| m["matchesHighlight"].clone());
        assert_eq!(
            marked,
            Some(Value::Bool(source.starts_with("src/web/"))),
            "{source}"
        );
    }
    let plain = json(&run(&dir, &["cruise", "-T", "json", "src"])?)?;
    assert!(
        module(&plain, "src/main.ts").is_some_and(|m| m.get("matchesHighlight").is_none()),
        "without highlight, no module carries the key"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

const WEBPACK: &[(&str, &str)] = &[
    (
        "src/index.js",
        "import util from \"@/util\";\nexport default util;\n",
    ),
    ("src/lib/util.js", "export default 1;\n"),
    ("other/util.js", "export default 2;\n"),
    (
        "webpack.config.js",
        "const path = require('path');\nmodule.exports = (env, argv) => ({\n  entry: './src/index.js',\n  resolve: { alias: { '@': path.resolve(__dirname, env.lib) }, extensions: argv.extensions }\n});\n",
    ),
    (
        "webpack.other.cjs",
        "module.exports = { resolve: { alias: { '@': require('path').join(__dirname, 'other') } } };\n",
    ),
    (
        "webpack.fs.cjs",
        "const fs = require('fs');\nmodule.exports = { resolve: {} };\n",
    ),
    (
        ".dependency-cruiser.json",
        r#"{ "forbidden": [], "options": { "webpackConfig": { "fileName": "webpack.config.js", "env": { "lib": "src/lib" }, "arguments": { "extensions": [".js"] } } } }"#,
    ),
];

fn util_edge(result: &Value) -> (String, Vec<String>) {
    let dependency = &result["modules"]
        .as_array()
        .and_then(|m| m.iter().find(|m| m["source"] == "src/index.js"))
        .map(|m| m["dependencies"][0].clone())
        .unwrap_or_default();
    (
        dependency["resolved"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        dependency["dependencyTypes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_str().map(str::to_owned))
            .collect(),
    )
}

#[test]
fn webpack_config_resolves_as_webpack_would() -> Result<(), Box<dyn Error>> {
    let dir = tree("webpack", WEBPACK)?;
    let configured = json(&run(&dir, &["cruise", "-T", "json", "src"])?)?;
    let (resolved, types) = util_edge(&configured);
    assert_eq!(resolved, "src/lib/util.js");
    assert!(types.contains(&"aliased-webpack".to_owned()), "{types:?}");
    assert_eq!(
        configured["summary"]["optionsUsed"]["webpackConfig"]["fileName"], "webpack.config.js",
        "optionsUsed reports webpackConfig as written"
    );
    assert!(
        configured["summary"]["optionsUsed"]
            .get("webpackConfigJson")
            .is_none()
    );
    let flag = json(&run(
        &dir,
        &[
            "cruise",
            "-T",
            "json",
            "--webpack-config",
            "webpack.other.cjs",
            "src",
        ],
    )?)?;
    assert_eq!(util_edge(&flag).0, "other/util.js");
    let copied = serde_json::json!({
        "resolve": { "alias": { "@": dir.join("other") }, "fallback": {} }
    });
    std::fs::write(dir.join("resolve.json"), copied.to_string())?;
    let copy = run(
        &dir,
        &[
            "cruise",
            "-T",
            "json",
            "--webpack-config-json",
            "resolve.json",
            "src",
        ],
    )?;
    assert_eq!(util_edge(&json(&copy)?).0, "other/util.js");
    assert!(
        String::from_utf8_lossy(&copy.stderr).contains("fallback"),
        "a key the resolver does not read is named: {}",
        String::from_utf8_lossy(&copy.stderr)
    );
    let refused = run(
        &dir,
        &[
            "cruise",
            "-T",
            "json",
            "--webpack-config",
            "webpack.fs.cjs",
            "src",
        ],
    )?;
    assert_eq!(refused.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--config-via-node"));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn markdown_fences_follow_the_configuration_format() -> Result<(), Box<dyn Error>> {
    let files: &[(&str, &str)] = &[
        ("src/index.js", "export default 1;\n"),
        (
            "src/README.md",
            "# Use\n\n```js\nimport x from \"./index.js\";\n```\n",
        ),
        (
            ".dependency-cruiser.json",
            r#"{ "forbidden": [], "options": { "extraExtensionsToScan": [".md"] } }"#,
        ),
        (
            "rulebearing.yaml",
            "options:\n  extraExtensionsToScan: [\".md\"]\n",
        ),
    ];
    let dir = tree("markdown", files)?;
    let cruiser = json(&run(
        &dir,
        &[
            "cruise",
            "-T",
            "json",
            "-c",
            ".dependency-cruiser.json",
            "src",
        ],
    )?)?;
    assert_eq!(
        module(&cruiser, "src/README.md").map(|m| m["dependencies"].clone()),
        Some(serde_json::json!([]))
    );
    let native = json(&run(
        &dir,
        &["cruise", "-T", "json", "-c", "rulebearing.yaml", "src"],
    )?)?;
    let readme = module(&native, "src/README.md").ok_or("src/README.md")?;
    assert_eq!(readme["dependencies"][0]["resolved"], "src/index.js");
    assert_eq!(readme["dependencies"][0]["line"], 4);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn experimental_stats_are_recorded_on_modules_and_summed_on_folders() -> Result<(), Box<dyn Error>>
{
    let files: &[(&str, &str)] = &[
        ("src/index.js", "import \"./a/b\";\nexport const x = 1;\n"),
        ("src/a/b.js", "export const y = 2;\nexport const z = 3;\n"),
        (
            ".dependency-cruiser.json",
            r#"{ "forbidden": [], "options": { "experimentalStats": true } }"#,
        ),
    ];
    let dir = tree("stats", files)?;
    let result = json(&run(&dir, &["cruise", "-T", "json", "--metrics", "src"])?)?;
    let stats = |source: &str| module(&result, source).map(|m| m["experimentalStats"].clone());
    assert_eq!(
        stats("src/a/b.js"),
        Some(serde_json::json!({ "topLevelStatementCount": 2, "size": 40 }))
    );
    assert_eq!(
        stats("src/index.js"),
        Some(serde_json::json!({ "topLevelStatementCount": 2, "size": 36 }))
    );
    let folder = result["folders"]
        .as_array()
        .and_then(|f| f.iter().find(|f| f["name"] == "src"))
        .map(|f| f["experimentalStats"].clone());
    assert_eq!(
        folder,
        Some(serde_json::json!({ "topLevelStatementCount": 4, "size": 76 }))
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_highlight_or_collapse_that_does_not_compile_is_a_configuration_error()
-> Result<(), Box<dyn Error>> {
    let dir = tree("bad-patterns", LAYERED)?;
    for flag in ["--highlight", "--collapse"] {
        let output = run(&dir, &["cruise", "-T", "json", flag, "(", "src"])?;
        assert_eq!(output.status.code(), Some(3), "{flag}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(&flag[2..]), "{flag}: {stderr}");
    }
    std::fs::write(
        dir.join(".dependency-cruiser.json"),
        r#"{ "forbidden": [], "options": { "collapse": "(" } }"#,
    )?;
    let output = run(&dir, &["cruise", "-T", "json", "src"])?;
    assert_eq!(output.status.code(), Some(3));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
