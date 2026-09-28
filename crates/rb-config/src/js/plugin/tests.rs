use super::*;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;

use serde_json::json;

/// A temporary repository, removed on drop; std only, so no extra dependency.
struct Repo(PathBuf);

impl Repo {
    fn new(files: &[(&str, &str)]) -> Result<Self, Box<dyn Error>> {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("rb-config-plugin-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path)?;
        let repo = Self(path.canonicalize()?);
        for (name, text) in files {
            let file = repo.0.join(name);
            if let Some(parent) = file.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(file, text)?;
        }
        Ok(repo)
    }

    fn sandbox(&self) -> Sandbox {
        Sandbox::new(&self.0, &self.0, Limits::plugin())
    }

    fn render(&self, name: &str, result: &serde_json::Value) -> Result<PluginOutput, PluginError> {
        let sandbox = self.sandbox();
        sandbox.report(&sandbox.resolve(name)?, result)
    }

    fn refused(&self, name: &str) -> PluginError {
        match self.render(name, &cruise_result()) {
            Ok(output) => PluginError::Thrown {
                name: name.into(),
                message: format!("unexpectedly rendered {output:?}"),
            },
            Err(error) => error,
        }
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A small real cruise result: two modules, one violation.
fn cruise_result() -> serde_json::Value {
    json!({
        "modules": [
            { "source": "src/a.ts", "valid": false, "dependencies": [
                { "resolved": "src/b.ts", "module": "./b", "valid": false, "dependencyTypes": ["local"],
                  "rules": [{ "name": "no-b", "severity": "error" }] } ] },
            { "source": "src/b.ts", "valid": true, "dependencies": [] }
        ],
        "summary": {
            "violations": [{ "from": "src/a.ts", "to": "src/b.ts", "rule": { "name": "no-b", "severity": "error" } }],
            "error": 1, "warn": 0, "info": 0, "ignore": 0,
            "totalCruised": 2, "totalDependenciesCruised": 1, "optionsUsed": {}
        }
    })
}

/// dependency-cruiser 18.2.0's `test/report/plugins/__fixtures__`, verbatim.
const UPSTREAM_FIXTURES: &[(&str, &str)] = &[
    (
        "invalid-no-exit-code-plugin.cjs",
        "module.exports = (_pCruiseResult) => ({\n  output: \"dummy\",\n});\n",
    ),
    (
        "invalid-no-output-plugin.cjs",
        "module.exports = (_pCruiseResult) => ({\n  exitCode: 0,\n});\n",
    ),
    (
        "invalid-non-number-exit-code-plugin.cjs",
        "module.exports = (pCruiseResult) => ({\n  output: \"dummy\",\n  exitCode: \"not a number\",\n});\n",
    ),
    (
        "invalid-not-a-function-plugin.cjs",
        "module.exports = {\n  \"not-a-function\": \"really not\",\n};\n",
    ),
    (
        "valid-non-functional-plugin.cjs",
        "module.exports = (_pCruiseResult) => ({\n  output: \"some string\",\n  exitCode: 42,\n});\n",
    ),
    (
        "valid-plugin.cjs",
        "function samplePluginReporter(pCruiseResult) {\n  return {\n    moduleCount: pCruiseResult.summary.totalCruised,\n    dependencyCount: pCruiseResult.summary.totalDependenciesCruised,\n  };\n}\n\nmodule.exports = (pCruiseResult) => ({\n  output: JSON.stringify(samplePluginReporter(pCruiseResult), null, 2),\n  exitCode: 0,\n});\n",
    ),
];

#[test]
fn upstreams_fixtures_are_valid_or_not_as_is_valid_plugin_says() -> Result<(), Box<dyn Error>> {
    let repo = Repo::new(UPSTREAM_FIXTURES)?;
    let sandbox = repo.sandbox();
    for (file, valid) in [
        ("invalid-no-exit-code-plugin.cjs", false),
        ("invalid-no-output-plugin.cjs", false),
        ("invalid-non-number-exit-code-plugin.cjs", false),
        ("invalid-not-a-function-plugin.cjs", false),
        ("valid-non-functional-plugin.cjs", true),
        ("valid-plugin.cjs", true),
    ] {
        let plugin = sandbox.resolve(&format!("./{file}"))?;
        assert_eq!(sandbox.is_valid(&plugin), Ok(valid), "{file}");
        assert_eq!(sandbox.check(&plugin).is_ok(), valid, "{file}");
    }
    let bad = sandbox.resolve("./invalid-no-output-plugin.cjs")?;
    assert_eq!(
        sandbox.check(&bad),
        Err(PluginError::Invalid {
            name: "./invalid-no-output-plugin.cjs".into(),
            reason: "called with a minimal cruise result, it returned no own `output`".into()
        })
    );
    // Upstream's message names the plugin as given; the reason says what is missing.
    let error = repo
        .refused("./invalid-no-exit-code-plugin.cjs")
        .to_string();
    assert!(
        error.starts_with(
            "./invalid-no-exit-code-plugin.cjs is not a valid plugin: called with a minimal cruise result, it returned no own `exitCode`."
        ),
        "{error}"
    );
    let error = repo.refused("./invalid-no-output-plugin.cjs").to_string();
    assert!(error.contains("no own `output`"), "{error}");
    let error = repo
        .refused("./invalid-non-number-exit-code-plugin.cjs")
        .to_string();
    assert!(error.contains("a string, not a number"), "{error}");
    let error = repo
        .refused("./invalid-not-a-function-plugin.cjs")
        .to_string();
    assert!(error.contains("its default export is object"), "{error}");
    Ok(())
}

#[test]
fn a_plugin_renders_a_real_cruise_result_deterministically() -> Result<(), Box<dyn Error>> {
    let repo = Repo::new(&[
        ("helpers/count.json", r#"{ "label": "violations" }"#),
        (
            "reporters/summary.cjs",
            r"const { label } = require('../helpers/count.json');
const path = require('path');
module.exports = (result) => ({
  output: result.modules.map((m) => path.basename(m.source) + ':' + m.dependencies.length).join(',')
    + ' ' + label + '=' + result.summary.violations.length + '\n',
  exitCode: result.summary.error,
});",
        ),
        ("valid-plugin.cjs", UPSTREAM_FIXTURES[5].1),
    ])?;
    let first = repo.render("./reporters/summary.cjs", &cruise_result())?;
    assert_eq!(
        first,
        PluginOutput {
            output: "a.ts:1,b.ts:0 violations=1\n".into(),
            exit_code: 1.0
        }
    );
    let second = repo.render("./reporters/summary.cjs", &cruise_result())?;
    assert_eq!(first.output.as_bytes(), second.output.as_bytes());
    assert_eq!(first.whole_exit_code(), Some(1));
    // Upstream's sample fixture over the same result.
    let sample = repo.render("./valid-plugin.cjs", &cruise_result())?;
    assert_eq!(
        sample.output,
        "{\n  \"moduleCount\": 2,\n  \"dependencyCount\": 1\n}"
    );
    Ok(())
}

#[test]
fn an_es_module_plugin_imports_inside_the_repository() -> Result<(), Box<dyn Error>> {
    let repo = Repo::new(&[
        (
            "lib/format.mjs",
            "export const line = (m) => `- ${m.source}`;",
        ),
        (
            "lib/legacy.cjs",
            "module.exports = { header: '# modules' };",
        ),
        (
            "reporter.mjs",
            "import { line } from './lib/format.mjs';\nimport legacy from './lib/legacy.cjs';\nimport { dirname } from 'node:path';\nexport default (r) => ({ output: [legacy.header, ...r.modules.map(line), dirname(import.meta.filename) !== '' ? 'ok' : ''].join('\\n'), exitCode: 0 });",
        ),
    ])?;
    let rendered = repo.render("./reporter.mjs", &cruise_result())?;
    assert_eq!(rendered.output, "# modules\n- src/a.ts\n- src/b.ts\nok");
    assert_eq!(rendered.whole_exit_code(), Some(0));
    Ok(())
}

#[test]
fn plugins_resolve_as_upstreams_import_would_inside_the_repository() -> Result<(), Box<dyn Error>> {
    let plugin = "module.exports = () => ({ output: 'pkg', exitCode: 0 });";
    let repo = Repo::new(&[
        ("with space/reporter.cjs", plugin),
        ("node_modules/plain/index.js", plugin),
        (
            "node_modules/@scope/exported/package.json",
            r#"{ "name": "@scope/exported", "exports": { ".": { "types": "./x.d.ts", "import": "./dist/main.cjs" }, "./reporters/*": "./lib/*.cjs" } }"#,
        ),
        ("node_modules/@scope/exported/dist/main.cjs", plugin),
        ("node_modules/@scope/exported/lib/stats.cjs", plugin),
        (
            "package.json",
            r#"{ "name": "my-repo", "exports": { "./sample-reporter-plugin": { "import": "./configs/sample.mjs" } } }"#,
        ),
        (
            "configs/sample.mjs",
            "export default () => ({ output: 'self', exitCode: 0 });",
        ),
    ])?;
    let sandbox = repo.sandbox();
    let root = repo.0.clone();
    let url = format!(
        "file://{}/with%20space/reporter.cjs",
        root.to_string_lossy()
    );
    for (name, file) in [
        ("./with space/reporter.cjs", "with space/reporter.cjs"),
        (url.as_str(), "with space/reporter.cjs"),
        ("plain", "node_modules/plain/index.js"),
        (
            "@scope/exported",
            "node_modules/@scope/exported/dist/main.cjs",
        ),
        (
            "@scope/exported/reporters/stats",
            "node_modules/@scope/exported/lib/stats.cjs",
        ),
        ("my-repo/sample-reporter-plugin", "configs/sample.mjs"),
    ] {
        let found = sandbox.resolve(name)?;
        assert_eq!(found.file(), root.join(file), "{name}");
        assert_eq!(found.name(), name);
        assert!(sandbox.is_valid(&found)?, "{name}");
    }
    assert_eq!(sandbox.root(), root.as_path());
    // A relative path starts from the working directory, not the repository root.
    let nested = Sandbox::new(&root, &root.join("configs"), Limits::plugin());
    assert_eq!(
        nested.resolve("./sample.mjs")?.file(),
        root.join("configs/sample.mjs")
    );
    let missing = sandbox.resolve("this-plugin-does-not-exist");
    assert!(
        matches!(&missing, Err(PluginError::NotFound { name, .. }) if name == "this-plugin-does-not-exist"),
        "{missing:?}"
    );
    let message = missing.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(
        message.starts_with(
            "Could not find reporter plugin 'this-plugin-does-not-exist' (or it isn't valid)"
        ),
        "{message}"
    );
    assert!(matches!(
        sandbox.resolve("./nope.cjs"),
        Err(PluginError::NotFound { .. })
    ));
    assert!(matches!(
        sandbox.resolve("path"),
        Err(PluginError::Invalid { .. })
    ));
    Ok(())
}

fn is_sandbox(error: &PluginError, needle: &str) -> bool {
    matches!(error, PluginError::Sandbox { reason, .. } if reason.contains(needle))
        && error
            .to_string()
            .contains("the reporter sandbox refused it")
}

#[test]
fn sandbox_escape_attempts_fail_with_a_named_sandbox_error() -> Result<(), Box<dyn Error>> {
    let returns = "return { output: 'leaked', exitCode: 0 };";
    let repo = Repo::new(&[
        (
            "fs-at-load.cjs",
            "const fs = require('fs'); module.exports = () => ({ output: '', exitCode: 0 });",
        ),
        (
            "fs-at-call.cjs",
            &format!(
                "module.exports = () => {{ require('fs').readFileSync('/etc/passwd'); {returns} }};"
            ),
        ),
        (
            "fs-import.mjs",
            "import { readFileSync } from 'node:fs'; export default () => ({ output: readFileSync('/etc/passwd', 'utf8'), exitCode: 0 });",
        ),
        (
            "http.cjs",
            &format!(
                "module.exports = () => {{ require('http').get('http://example.com'); {returns} }};"
            ),
        ),
        (
            "fetch.cjs",
            &format!("module.exports = () => {{ fetch('http://example.com'); {returns} }};"),
        ),
        (
            "xhr.cjs",
            &format!("module.exports = () => {{ new XMLHttpRequest(); {returns} }};"),
        ),
        (
            "passwd.cjs",
            "module.exports = () => ({ output: String(require('../../etc/passwd')), exitCode: 0 });",
        ),
        (
            "constructor.cjs",
            "module.exports = (r) => ({ output: r.constructor.constructor('return process')().env.HOME, exitCode: 0 });",
        ),
        (
            "process.cjs",
            "module.exports = () => ({ output: process.env.HOME, exitCode: 0 });",
        ),
    ])?;
    let checks: &[(&str, &str)] = &[
        (
            "./fs-at-load.cjs",
            "`fs` is not available in the reporter sandbox",
        ),
        (
            "./fs-at-call.cjs",
            "`fs` is not available in the reporter sandbox",
        ),
        (
            "./fs-import.mjs",
            "`node:fs` is not available in the reporter sandbox",
        ),
        (
            "./http.cjs",
            "`http` is not available in the reporter sandbox",
        ),
        (
            "./fetch.cjs",
            "`fetch` is not available in the reporter sandbox",
        ),
        (
            "./xhr.cjs",
            "`XMLHttpRequest` is not available in the reporter sandbox",
        ),
        ("./passwd.cjs", "is outside the repository"),
        (
            "./process.cjs",
            "`process` is not available in the reporter sandbox",
        ),
        (
            "./constructor.cjs",
            "`process` is not available in the reporter sandbox",
        ),
    ];
    for (name, needle) in checks {
        let error = repo.refused(name);
        assert!(is_sandbox(&error, needle), "{name}: {error}");
        assert!(!error.to_string().contains("--config-via-node"), "{name}");
    }
    Ok(())
}

#[test]
fn time_memory_and_handled_refusals() -> Result<(), Box<dyn Error>> {
    let repo = Repo::new(&[
        ("loop.cjs", "module.exports = () => { for (;;) {} };"),
        (
            "memory.cjs",
            "module.exports = () => { const a = []; for (;;) { a.push('x'.repeat(1 << 20)); } };",
        ),
        (
            "caught.cjs",
            "let fs; try { fs = require('fs'); } catch (e) { fs = null; }\nmodule.exports = () => ({ output: fs === null ? 'without fs' : 'with fs', exitCode: 0 });",
        ),
    ])?;
    // Bounded time and memory: a named sandbox error, never a hang or a panic.
    let tight = Sandbox::new(
        &repo.0,
        &repo.0,
        Limits {
            time: Duration::from_millis(200),
            ..Limits::plugin()
        },
    );
    let looped = tight.report(&tight.resolve("./loop.cjs")?, &cruise_result());
    assert!(
        looped
            .as_ref()
            .is_err_and(|e| is_sandbox(e, "ran past the 200 ms time limit")),
        "{looped:?}"
    );
    // Its own sandbox, so a slow machine cannot reach the time limit first.
    let small = Sandbox::new(
        &repo.0,
        &repo.0,
        Limits {
            memory: 16 * 1024 * 1024,
            ..Limits::plugin()
        },
    );
    let grew = small.report(&small.resolve("./memory.cjs")?, &cruise_result());
    assert!(
        grew.as_ref()
            .is_err_and(|e| is_sandbox(e, "ran past the 16 MiB memory limit")),
        "{grew:?}"
    );
    // A refusal the plugin handles itself is no failure: nothing was read.
    assert_eq!(
        repo.render("./caught.cjs", &cruise_result())?.output,
        "without fs"
    );
    Ok(())
}

#[test]
fn a_path_outside_the_repository_is_refused_before_it_is_read() -> Result<(), Box<dyn Error>> {
    let outside = Repo::new(&[(
        "evil.cjs",
        "module.exports = () => ({ output: 'outside', exitCode: 0 });",
    )])?;
    let repo = Repo::new(&[])?;
    let sandbox = repo.sandbox();
    let absolute = outside.0.join("evil.cjs");
    let absolute = absolute.to_string_lossy();
    for name in [
        absolute.to_string(),
        format!("file://{absolute}"),
        "../../../../../../../../etc/passwd".to_owned(),
    ] {
        let error = sandbox.resolve(&name).err();
        assert!(
            error
                .as_ref()
                .is_some_and(|e| is_sandbox(e, "is outside the repository")),
            "{name}: {error:?}"
        );
    }
    let builtin = sandbox.resolve("fs").err();
    assert!(
        builtin
            .as_ref()
            .is_some_and(|e| is_sandbox(e, "`fs` is not available")),
        "{builtin:?}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_symlink_out_of_the_repository_is_not_followed() -> Result<(), Box<dyn Error>> {
    let outside = Repo::new(&[(
        "evil.cjs",
        "module.exports = () => ({ output: 'outside', exitCode: 0 });",
    )])?;
    let repo = Repo::new(&[])?;
    std::os::unix::fs::symlink(outside.0.join("evil.cjs"), repo.0.join("link.cjs"))?;
    assert!(matches!(
        repo.sandbox().resolve("./link.cjs"),
        Err(PluginError::NotFound { .. })
    ));
    Ok(())
}

#[test]
fn a_plugin_that_throws_or_returns_the_wrong_shape_is_named() -> Result<(), Box<dyn Error>> {
    let repo = Repo::new(&[
        ("syntax.cjs", "module.exports = ("),
        ("throws-at-load.cjs", "throw new Error('broken at load');"),
        (
            "throws-on-result.cjs",
            "module.exports = (r) => { if (r.modules.length > 0) throw new Error('boom'); return { output: '', exitCode: 0 }; };",
        ),
        (
            "number-output.cjs",
            "module.exports = (r) => ({ output: r.modules.length > 0 ? 42 : '', exitCode: 0 });",
        ),
        (
            "string-exit.cjs",
            "module.exports = (r) => ({ output: '', exitCode: r.modules.length > 0 ? 'one' : 0 });",
        ),
        (
            "no-report.cjs",
            "module.exports = (r) => r.modules.length > 0 ? undefined : { output: '', exitCode: 0 };",
        ),
        (
            "fraction.cjs",
            "module.exports = (r) => ({ output: '', exitCode: r.modules.length > 0 ? 1.5 : 0 });",
        ),
        ("undefined-probe.cjs", "module.exports = () => undefined;"),
    ])?;
    for name in ["./syntax.cjs", "./throws-at-load.cjs"] {
        let error = repo.refused(name);
        assert!(
            matches!(error, PluginError::NotFound { .. }),
            "{name}: {error}"
        );
    }
    assert!(
        repo.refused("./throws-at-load.cjs")
            .to_string()
            .contains("broken at load")
    );
    let thrown = repo.refused("./throws-on-result.cjs");
    assert_eq!(
        thrown,
        PluginError::Thrown {
            name: "./throws-on-result.cjs".into(),
            message: "boom".into()
        }
    );
    assert!(
        thrown
            .to_string()
            .starts_with("plugin:./throws-on-result.cjs threw: boom.")
    );
    let expectations = [
        (
            "./number-output.cjs",
            "an `output` that is a number, not a string",
        ),
        (
            "./string-exit.cjs",
            "an `exitCode` that is a string, not a number",
        ),
        (
            "./no-report.cjs",
            "an `output` that is undefined, not a string",
        ),
    ];
    for (name, needle) in expectations {
        let error = repo.refused(name);
        assert!(
            matches!(&error, PluginError::Invalid { reason, .. } if reason.contains(needle)),
            "{name}: {error}"
        );
    }
    let fraction = repo.render("./fraction.cjs", &cruise_result())?;
    assert_eq!(fraction.whole_exit_code(), None);
    // As upstream: `Object.hasOwn(undefined, ...)` throws; the plugin's error, not a verdict.
    assert!(matches!(
        repo.refused("./undefined-probe.cjs"),
        PluginError::Thrown { .. }
    ));
    Ok(())
}

#[test]
fn plugin_names_follow_upstreams_pattern() {
    for (output_type, name) in [
        ("plugin:./x.cjs", Some("./x.cjs")),
        ("plugin:file:///a/b.mjs", Some("file:///a/b.mjs")),
        (
            "plugin:dependency-cruiser/sample-reporter-plugin",
            Some("dependency-cruiser/sample-reporter-plugin"),
        ),
        ("plugin:", None),
        ("err", None),
        ("whatever-just-not-a-plugin", None),
        ("", None),
    ] {
        assert_eq!(plugin_name(output_type), name, "{output_type}");
    }
}

#[test]
fn whole_exit_codes() {
    for (code, whole) in [
        (0.0, Some(0)),
        (42.0, Some(42)),
        (1e30, Some(u64::MAX)),
        (-1.0, None),
        (1.5, None),
        (f64::NAN, None),
        (f64::INFINITY, None),
    ] {
        let output = PluginOutput {
            output: String::new(),
            exit_code: code,
        };
        assert_eq!(output.whole_exit_code(), whole, "{code}");
    }
}

#[test]
fn helpers() {
    assert_eq!(percent_decode("/a%20b/c%2Fd"), "/a b/c/d");
    assert_eq!(percent_decode("/100%/x%zz"), "/100%/x%zz");
    assert_eq!(percent_decode("/C:/repo/x.cjs"), "C:/repo/x.cjs");
    assert_eq!(percent_decode("/x"), "/x");
    assert_eq!(host_global("process is not defined"), Some("process"));
    assert_eq!(host_global("'fetch' is not defined"), Some("fetch"));
    assert_eq!(host_global("require is not defined"), None);
    assert_eq!(host_global("boom"), None);
    assert_eq!(duration(Duration::from_secs(30)), "30-second");
    assert_eq!(duration(Duration::from_millis(200)), "200 ms");
    assert_eq!(duration(Duration::from_millis(1500)), "1500 ms");
    assert_eq!(article("number"), "a number");
    assert_eq!(article("object"), "an object");
    assert_eq!(article("undefined"), "undefined");
    assert_eq!(
        Limits::plugin(),
        Limits {
            time: Duration::from_secs(30),
            memory: 512 * 1024 * 1024
        }
    );
}

#[test]
fn package_specifiers_split_and_exports_resolve() {
    use super::super::{exports_target, split_package};
    for (specifier, name, subpath) in [
        ("watskeburt", "watskeburt", "."),
        (
            "dependency-cruiser/sample-reporter-plugin",
            "dependency-cruiser",
            "./sample-reporter-plugin",
        ),
        ("@scope/pkg", "@scope/pkg", "."),
        ("@scope/pkg/a/b", "@scope/pkg", "./a/b"),
    ] {
        assert_eq!(
            split_package(specifier),
            (name, subpath.to_owned()),
            "{specifier}"
        );
    }
    let exports = json!({
        ".": [{ "types": "./t.d.ts", "import": "./dist/main.js" }, "./dist/main.js"],
        "./plugin": { "types": "./t.d.ts", "require": "./cjs/plugin.cjs", "default": "./esm/plugin.mjs" },
        "./reporters/*": "./lib/*.mjs",
        "./reporters/special/*": { "node": "./special/*.cjs" },
        "./hidden": null,
        "./escape": "../outside.js"
    });
    for (subpath, target) in [
        (".", Some("./dist/main.js")),
        ("./plugin", Some("./cjs/plugin.cjs")),
        ("./reporters/stats", Some("./lib/stats.mjs")),
        ("./reporters/special/x", Some("./special/x.cjs")),
        ("./hidden", None),
        ("./escape", None),
        ("./absent", None),
    ] {
        assert_eq!(
            exports_target(&exports, subpath).as_deref(),
            target,
            "{subpath}"
        );
    }
    assert_eq!(
        exports_target(&json!("./index.js"), ".").as_deref(),
        Some("./index.js")
    );
    assert_eq!(exports_target(&json!("./index.js"), "./x"), None);
    assert_eq!(
        exports_target(&json!({ "import": "./m.mjs", "types": "./t.d.ts" }), ".").as_deref(),
        Some("./m.mjs")
    );
    assert_eq!(exports_target(&json!({ "types": "./t.d.ts" }), "."), None);
}

proptest::proptest! {
    #[test]
    fn percent_decoding_undoes_percent_encoding(text in "[a-zA-Z0-9 ./_%é-]{0,40}") {
        let encoded = text.bytes().fold(String::new(), |mut out, b| {
            let _ = write!(out, "%{b:02X}");
            out
        });
        // A decoded path that starts like `/C:/` is read as a Windows drive; skip those.
        proptest::prop_assume!(!(text.len() > 3 && text.as_bytes()[0] == b'/' && text.as_bytes()[2] == b':'));
        proptest::prop_assert_eq!(percent_decode(&encoded), text);
    }

    #[test]
    fn a_split_specifier_joins_back(name in "(@[a-z]{1,6}/)?[a-z][a-z0-9-]{0,8}", rest in "(/[a-z0-9-]{1,6}){0,3}") {
        let specifier = format!("{name}{rest}");
        let (package, subpath) = split_package_for_test(&specifier);
        proptest::prop_assert_eq!(package, name.as_str());
        let joined = if subpath == "." { package.to_owned() } else { format!("{package}{}", &subpath[1..]) };
        proptest::prop_assert_eq!(joined, specifier);
    }
}

fn split_package_for_test(specifier: &str) -> (&str, String) {
    super::super::split_package(specifier)
}
