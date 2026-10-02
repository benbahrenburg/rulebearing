//! The security review's findings on the plugin sandbox, each pinned by a test
//! ([Wave 3, Step 7](../../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)).

use super::*;
use std::error::Error;
use std::fs;

use serde_json::json;

/// A temporary folder holding a repository (`repo/`) and a sibling outside it (`outside/`),
/// removed on drop.
struct Layout(PathBuf);

impl Layout {
    fn new(repo: &[(&str, &str)], outside: &[(&str, &str)]) -> Result<Self, Box<dyn Error>> {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("rb-config-review-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("repo"))?;
        fs::create_dir_all(path.join("outside"))?;
        let layout = Self(rb_model::without_verbatim(&path.canonicalize()?));
        for (folder, files) in [("repo", repo), ("outside", outside)] {
            for (name, text) in files {
                let file = layout.0.join(folder).join(name);
                if let Some(parent) = file.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(file, text)?;
            }
        }
        Ok(layout)
    }

    fn repo(&self) -> PathBuf {
        self.0.join("repo")
    }

    fn sandbox(&self) -> Sandbox {
        Sandbox::new(&self.repo(), &self.repo(), Limits::plugin()).with_home(None)
    }

    fn render(&self, name: &str) -> Result<PluginOutput, PluginError> {
        let sandbox = self.sandbox();
        sandbox.report(&sandbox.resolve(name)?, &result())
    }
}

impl Drop for Layout {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn result() -> serde_json::Value {
    json!({ "modules": [{ "source": "a.ts", "dependencies": [] }], "summary": { "error": 0, "totalCruised": 1 } })
}

/// A package outside the repository that names itself `leakname` and exports `./x.js`.
const LEAK: &[(&str, &str)] = &[
    (
        "pkgdir/package.json",
        r#"{ "name": "leakname", "exports": { ".": "./x.js" }, "main": "x.js" }"#,
    ),
    (
        "pkgdir/x.js",
        "module.exports = () => ({ output: 'outside', exitCode: 0 });",
    ),
];

/// Asks the host resolver directly, with a `from` that climbs out of the repository, and
/// reports what it answered.
const ORACLE: &str = r"const r = [];
const t = (n, f) => { try { r.push(n + ' => ' + f()); } catch (e) { r.push(n + ' ! ' + e.message); } };
t('lexical', () => __rb_resolve(__rb_cwd + '/../outside/pkgdir/x.js', 'leakname'));
t('deep', () => __rb_resolve(__rb_cwd + '/a/../../outside/pkgdir/x.js', './x.js'));
t('guess', () => __rb_resolve(__rb_cwd + '/../outside/pkgdir/x.js', 'othername'));
t('inside', () => __rb_resolve(__rb_cwd + '/p/x.js', 'leakname'));
module.exports = () => ({ output: r.join('\n'), exitCode: 0 });";

#[test]
fn a_from_outside_the_repository_is_refused_before_any_lookup() -> Result<(), Box<dyn Error>> {
    let layout = Layout::new(&[("oracle.cjs", ORACLE)], LEAK)?;
    let output = layout.render("./oracle.cjs")?.output;
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 4, "{output}");
    // A refusal that names the repository, the same whether the package exists or not, so the
    // answer says nothing about a manifest outside it.
    for (line, label) in lines.iter().zip(["lexical", "deep", "guess"]) {
        assert!(
            line.starts_with(&format!("{label} ! ")) && line.contains("is outside the repository"),
            "{line}"
        );
    }
    assert!(
        lines[3].starts_with("inside ! cannot find `leakname`"),
        "{}",
        lines[3]
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_package_symlinked_out_of_the_repository_is_not_read() -> Result<(), Box<dyn Error>> {
    let layout = Layout::new(
        &[(
            "uses.cjs",
            "let r; try { r = require('leakname'); } catch (e) { r = e.message; } module.exports = () => ({ output: String(typeof r === 'function' ? 'loaded' : r), exitCode: 0 });",
        )],
        LEAK,
    )?;
    fs::create_dir_all(layout.repo().join("node_modules"))?;
    std::os::unix::fs::symlink(
        layout.0.join("outside/pkgdir"),
        layout.repo().join("node_modules/leakname"),
    )?;
    let output = layout.render("./uses.cjs")?.output;
    assert!(output.starts_with("cannot find `leakname`"), "{output}");
    // Nor by naming it as the plugin.
    assert!(matches!(
        layout.sandbox().resolve("leakname"),
        Err(PluginError::NotFound { .. })
    ));
    Ok(())
}

#[test]
fn a_caught_refusal_does_not_take_the_blame_for_a_later_error() -> Result<(), Box<dyn Error>> {
    let layout = Layout::new(
        &[
            (
                "optfs.cjs",
                "let fs = null; try { fs = require('fs'); } catch (e) {}\nmodule.exports = (r) => { if (r.modules.length > 0) throw new TypeError('my own bug on line 3'); return { output: '', exitCode: 0 }; };",
            ),
            (
                "twice.cjs",
                "try { require('fs'); } catch (e) {}\nmodule.exports = (r) => { if (r.modules.length > 0) require('http'); return { output: '', exitCode: 0 }; };",
            ),
        ],
        &[],
    )?;
    assert_eq!(
        layout.render("./optfs.cjs"),
        Err(PluginError::Thrown {
            name: "./optfs.cjs".into(),
            message: "my own bug on line 3".into()
        })
    );
    // An uncaught refusal after a caught one is named by what was refused last.
    let twice = layout.render("./twice.cjs");
    assert!(
        matches!(&twice, Err(PluginError::Sandbox { reason, .. }) if reason.starts_with("`http`")),
        "{twice:?}"
    );
    Ok(())
}

#[test]
fn exports_targets_and_patterns_cannot_step_out_of_their_package() -> Result<(), Box<dyn Error>> {
    let ok = "module.exports = () => ({ output: 'ok', exitCode: 0 });";
    let layout = Layout::new(
        &[
            ("p/ok.cjs", ok),
            (
                "node_modules/evstar/package.json",
                r#"{ "name": "evstar", "exports": { "./*": "./*", "./up": "./../../p/ok.cjs", "./nm": "./node_modules/x.cjs" } }"#,
            ),
            ("node_modules/evstar/lib/a.cjs", ok),
            ("node_modules/evstar/node_modules/x.cjs", ok),
        ],
        &[],
    )?;
    let sandbox = layout.sandbox();
    assert_eq!(
        sandbox.resolve("evstar/lib/a.cjs")?.file(),
        layout.repo().join("node_modules/evstar/lib/a.cjs")
    );
    for name in [
        "evstar/../../p/ok.cjs",
        "evstar/lib/../../../p/ok.cjs",
        "evstar/%2e%2e/%2E%2E/p/ok.cjs",
        "evstar/up",
        "evstar/nm",
        "evstar/node_modules/x.cjs",
        "evstar/lib//a.cjs",
    ] {
        let resolved = sandbox.resolve(name);
        assert!(
            matches!(resolved, Err(PluginError::NotFound { .. })),
            "{name}: {resolved:?}"
        );
    }
    Ok(())
}

#[test]
fn invalid_segments_are_those_node_refuses() {
    use super::super::has_invalid_segment;
    for (path, invalid) in [
        ("lib/a.cjs", false),
        ("a.b/c..d.js", false),
        ("node_modules_x/a.js", false),
        ("../a.js", true),
        ("lib/./a.js", true),
        ("lib//a.js", true),
        ("", true),
        ("lib/", true),
        ("%2e%2e/a.js", true),
        ("%2E/a.js", true),
        ("lib\\..\\a.js", true),
        ("node_modules/a.js", true),
        ("Node_Modules/a.js", true),
        ("%6Eode%5Fmodules/a.js", true),
        ("100%/a.js", false),
    ] {
        assert_eq!(has_invalid_segment(path), invalid, "{path}");
    }
}

#[test]
fn a_lone_surrogate_is_written_as_the_replacement_character() -> Result<(), Box<dyn Error>> {
    let layout = Layout::new(
        &[(
            "badutf.cjs",
            "module.exports = () => ({ output: \"a\\u0000b\\ud800c\\udc00d\\ud83d\\ude00\\n\", exitCode: 0 });",
        )],
        &[],
    )?;
    assert_eq!(
        layout.render("./badutf.cjs")?.output,
        "a\u{0}b\u{FFFD}c\u{FFFD}d\u{1F600}\n"
    );
    Ok(())
}

#[test]
fn a_getter_is_read_once_so_the_verdict_is_coherent() -> Result<(), Box<dyn Error>> {
    let layout = Layout::new(
        &[
            // Every read flips the type.
            (
                "flip-code.cjs",
                "let n = 0; module.exports = () => ({ output: 'x', get exitCode() { return n++ % 2 === 0 ? 0 : {}; } });",
            ),
            (
                "flip-output.cjs",
                "let n = 0; module.exports = () => ({ get output() { return n++ % 2 === 0 ? 's' : 42; }, exitCode: 0 });",
            ),
            (
                "throwing-getter.cjs",
                "let n = 0; module.exports = () => ({ output: 'x', get exitCode() { if (n++ > 0) throw new Error('getter gave up'); return 0; } });",
            ),
        ],
        &[],
    )?;
    // The probe read 0 once, the report read {} once.
    assert_eq!(
        layout.render("./flip-code.cjs"),
        Err(PluginError::Invalid {
            name: "./flip-code.cjs".into(),
            reason: "called with the cruise result, it returned an `exitCode` that is an object, not a number".into()
        })
    );
    // The probe does not read `output`; the report read 's' once.
    assert_eq!(layout.render("./flip-output.cjs")?.output, "s");
    assert_eq!(
        layout.render("./throwing-getter.cjs"),
        Err(PluginError::Thrown {
            name: "./throwing-getter.cjs".into(),
            message: "getter gave up".into()
        })
    );
    Ok(())
}

#[test]
fn a_sandbox_rooted_at_the_filesystem_root_or_home_is_refused() -> Result<(), Box<dyn Error>> {
    let layout = Layout::new(
        &[(
            "ok.cjs",
            "module.exports = () => ({ output: 'ok', exitCode: 0 });",
        )],
        &[],
    )?;
    let root = Path::new("/");
    let at_root = Sandbox::new(root, root, Limits::plugin()).resolve("./tmp/ok.cjs");
    assert!(
        matches!(&at_root, Err(PluginError::Sandbox { reason, .. }) if reason.contains("the filesystem root") && reason.contains("git init")),
        "{at_root:?}"
    );
    // The repository is the home directory: refused, whether resolving or running.
    let home = layout.repo();
    let at_home = Sandbox::new(&home, &home, Limits::plugin()).with_home(Some(&home));
    let refused = at_home.resolve("./ok.cjs");
    assert!(
        matches!(&refused, Err(PluginError::Sandbox { reason, .. }) if reason.contains("your home directory")),
        "{refused:?}"
    );
    let plugin = layout.sandbox().resolve("./ok.cjs")?;
    assert!(matches!(
        at_home.report(&plugin, &result()),
        Err(PluginError::Sandbox { .. })
    ));
    // A project inside the home directory is fine.
    let inside = Sandbox::new(&home, &home, Limits::plugin()).with_home(Some(&layout.0));
    assert_eq!(inside.report(&plugin, &result())?.output, "ok");
    Ok(())
}
