//! `rulebearing init`: a first configuration that describes the repository it found, and passes.
//!
//! - Source: [design § The developer relations hat](../../../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo)
//!   ("reads the repo before it asks anything ... every rule already passing or baselined")
//! - Plan: [Wave 1, Step 16](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-16-init-1f)
//! - Requirement: [FR-CLI-03](../../../../docs/prd.md#fr-cli-03)
//!
//! Discovery reads the tree: TypeScript (`tsconfig.json`) or JavaScript (`package.json`), an
//! `apps/` and `packages/` split, `src/features/*` layouts, and the entry files and conventions a
//! framework implies, which `no-orphans` must not report. The proposal extends
//! `rulebearing:recommended` and adds one fence per boundary found, each with a `comment` and a
//! `fix`. A cruise then runs over it: a rule that would be vacuous is dropped, and every current
//! finding is written into the baseline ([`crate::cmd::adopt::baseline`]), so the file is written
//! only when a second cruise with it exits 0. .NET and Python discovery arrive with their
//! extractors in wave 2.

use std::fmt::Write as _;
use std::path::Path;

use clap::Args;
use rb_config::extends::{self, Target};
use rb_config::load::{self, LoadOptions};
use rb_config::read::Syntax;
use serde_json::Value;

use crate::cmd::adopt::{self, BaselineArgs};
use crate::context::Context;
use crate::pipeline::{self, RunOptions};
use crate::progress::Progress;
use crate::{Outcome, RunExit};

/// `init`.
#[derive(Debug, Clone, Default, Args)]
pub struct InitArgs {
    /// Print the proposal and do not write it
    #[arg(long)]
    pub dry_run: bool,
    /// Replace an existing configuration file
    #[arg(long)]
    pub force: bool,
    /// Where to write the configuration
    #[arg(long, default_value = "rulebearing.yaml", value_name = "FILE")]
    pub output: String,
    /// Baseline entries for current findings
    #[command(flatten)]
    pub baseline: BaselineArgs,
}

/// What `init` found in the tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Discovery {
    /// `tsconfig.json` at the root.
    pub typescript: bool,
    /// `package.json` at the root.
    pub package_json: bool,
    /// Folders under `apps/`.
    pub apps: Vec<String>,
    /// Folders under `packages/`.
    pub packages: Vec<String>,
    /// `src/features` folders (relative) with at least two features.
    pub features: Vec<String>,
    /// Frameworks and tools whose conventions add entry points.
    pub frameworks: Vec<&'static str>,
    /// The roots to cruise.
    pub roots: Vec<String>,
}

fn folders(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with('.') && n != "node_modules")
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

fn any_file(dir: &Path, names: &[&str]) -> bool {
    names.iter().any(|n| dir.join(n).is_file())
}

const FRAMEWORKS: &[(&str, &[&str])] = &[
    (
        "next",
        &["next.config.js", "next.config.mjs", "next.config.ts"],
    ),
    (
        "vite",
        &["vite.config.ts", "vite.config.js", "vite.config.mts"],
    ),
    (
        "vitest",
        &["vitest.config.ts", "vitest.config.js", "vitest.config.mts"],
    ),
    (
        "jest",
        &["jest.config.js", "jest.config.ts", "jest.config.cjs"],
    ),
    ("storybook", &[".storybook/main.ts", ".storybook/main.js"]),
];

/// Reads the tree under `root`.
pub fn discover(root: &Path) -> Discovery {
    let apps = folders(&root.join("apps"));
    let packages = folders(&root.join("packages"));
    let mut projects: Vec<String> = vec![String::new()];
    projects.extend(apps.iter().map(|a| format!("apps/{a}/")));
    projects.extend(packages.iter().map(|p| format!("packages/{p}/")));
    let features = projects
        .iter()
        .map(|p| format!("{p}src/features"))
        .filter(|f| folders(&root.join(f)).len() >= 2)
        .collect();
    let mut frameworks = Vec::new();
    for (name, files) in FRAMEWORKS {
        if projects.iter().any(|p| any_file(&root.join(p), files)) {
            frameworks.push(*name);
        }
    }
    let mut roots: Vec<String> = ["apps", "packages", "libs"]
        .iter()
        .filter(|r| root.join(r).is_dir())
        .map(|r| (*r).to_owned())
        .collect();
    if roots.is_empty() {
        roots.push(
            if root.join("src").is_dir() {
                "src"
            } else {
                "."
            }
            .to_owned(),
        );
    }
    Discovery {
        typescript: root.join("tsconfig.json").is_file(),
        package_json: root.join("package.json").is_file(),
        apps,
        packages,
        features,
        frameworks,
        roots,
    }
}

/// The `no-orphans` exclusions: `rulebearing:recommended`'s, then the conventions found.
fn orphan_exclusions(found: &Discovery) -> Vec<String> {
    let mut out: Vec<String> = match extends::resolve("rulebearing:recommended", Path::new(".")) {
        Ok(Target::NativePreset(_, text)) => serde_yaml::from_str::<Value>(text)
            .ok()
            .and_then(|preset| {
                preset
                    .pointer("/rules/dependencies/forbidden")
                    .and_then(Value::as_array)
                    .and_then(|rules| rules.iter().find(|r| r["name"] == "no-orphans"))
                    .and_then(|r| r.pointer("/from/pathNot"))
                    .and_then(Value::as_array)
                    .map(|p| {
                        p.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    out.push(r"(^|/)(src/)?(main|index|cli|server|app)\.[cm]?[jt]sx?$".into());
    out.push(r"(^|/)(scripts|tools|bin)/".into());
    if found
        .frameworks
        .iter()
        .any(|f| matches!(*f, "vitest" | "jest"))
    {
        out.push(r"\.(spec|test)\.[cm]?[jt]sx?$".into());
        out.push(r"(^|/)__tests__/".into());
    }
    if found.frameworks.contains(&"storybook") {
        out.push(r"\.stories\.[cm]?[jt]sx?$".into());
    }
    if found.frameworks.contains(&"next") {
        out.push(r"(^|/)app/(.+/)?(page|layout|route|loading|error|not-found|template|default)\.[jt]sx?$".into());
        out.push(r"(^|/)pages/".into());
        out.push(r"(^|/)middleware\.[jt]s$".into());
    }
    out
}

/// A string as a YAML double-quoted scalar (JSON's escaping is valid YAML).
fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

/// One proposed rule, in the order it is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposed {
    /// The rule name.
    pub name: String,
    /// The YAML for the rule, indented for `forbidden:`.
    pub yaml: String,
}

fn fence(name: &str, comment: &str, fix: &str, folder: &str) -> Proposed {
    let escaped = regex_escape(folder);
    let mut yaml = String::new();
    let _ = writeln!(yaml, "      - name: {name}");
    let _ = writeln!(yaml, "        comment: {}", quoted(comment));
    let _ = writeln!(yaml, "        fix: {}", quoted(fix));
    let _ = writeln!(yaml, "        severity: error");
    let _ = writeln!(
        yaml,
        "        from: {{ path: {} }}",
        quoted(&format!("^({escaped}/[^/]+)/"))
    );
    let _ = writeln!(
        yaml,
        "        to: {{ path: {}, pathNot: {} }}",
        quoted(&format!("^{escaped}/[^/]+/")),
        quoted("^$1/")
    );
    Proposed {
        name: name.to_owned(),
        yaml,
    }
}

fn regex_escape(text: &str) -> String {
    text.chars().fold(String::new(), |mut out, c| {
        if ".^$*+?()[]{}|\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
        out
    })
}

/// The rules the discovery calls for, beyond the preset.
pub fn proposed_rules(found: &Discovery) -> Vec<Proposed> {
    let mut out = Vec::new();
    let mut orphans = String::from(
        "      - name: no-orphans\n        from:\n          orphan: true\n          pathNot:\n",
    );
    for p in orphan_exclusions(found) {
        let _ = writeln!(orphans, "            - {}", quoted(&p));
    }
    out.push(Proposed {
        name: "no-orphans".into(),
        yaml: orphans,
    });
    if found.apps.len() >= 2 {
        out.push(fence(
            "apps-are-independent",
            "An app never imports another app; what two apps share lives in a package.",
            "Move the shared code into a package under packages/ and import it from both apps.",
            "apps",
        ));
    }
    if !found.apps.is_empty() && !found.packages.is_empty() {
        let mut yaml = String::new();
        let _ = writeln!(yaml, "      - name: packages-not-to-apps");
        let _ = writeln!(
            yaml,
            "        comment: {}",
            quoted(
                "Packages are shared by apps; a package that imports an app ties every other app to it."
            )
        );
        let _ = writeln!(
            yaml,
            "        fix: {}",
            quoted(
                "Move what the package needs out of the app and into the package, or into a new package both can import."
            )
        );
        let _ = writeln!(yaml, "        severity: error");
        let _ = writeln!(yaml, "        from: {{ path: {} }}", quoted("^packages/"));
        let _ = writeln!(yaml, "        to: {{ path: {} }}", quoted("^apps/"));
        out.push(Proposed {
            name: "packages-not-to-apps".into(),
            yaml,
        });
    }
    for folder in &found.features {
        let owner = folder
            .strip_suffix("src/features")
            .unwrap_or_default()
            .trim_end_matches('/');
        let name = if owner.is_empty() {
            "features-are-independent".to_owned()
        } else {
            format!("features-are-independent-in-{}", owner.replace('/', "-"))
        };
        out.push(fence(
            &name,
            "A feature is changed and removed on its own; it never imports another feature.",
            "Move what both features need into a shared module outside the features folder, or pass it in from the page that composes them.",
            folder,
        ));
    }
    out
}

/// The configuration text for `found`, with `rules` and the baseline `entries`.
pub fn render(found: &Discovery, rules: &[Proposed], entries: &[Value], today: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# rulebearing.yaml, written by `rulebearing init` on {today}."
    );
    let mut seen = Vec::new();
    if found.typescript {
        seen.push("TypeScript".to_owned());
    } else if found.package_json {
        seen.push("JavaScript".to_owned());
    }
    if !found.apps.is_empty() {
        seen.push(format!("{} apps", found.apps.len()));
    }
    if !found.packages.is_empty() {
        seen.push(format!("{} packages", found.packages.len()));
    }
    for f in &found.features {
        seen.push(f.clone());
    }
    for f in &found.frameworks {
        seen.push((*f).to_owned());
    }
    let _ = writeln!(out, "# Found: {}.", seen.join(", "));
    let _ = writeln!(
        out,
        "# Every rule says why it exists (comment) and what to do when it fires (fix);"
    );
    let _ = writeln!(
        out,
        "# `rulebearing explain <rule>` prints both. The rules of rulebearing:recommended apply too."
    );
    let _ = writeln!(out, "extends: rulebearing:recommended");
    if found.typescript {
        out.push_str("languages:\n  typescript:\n    tsConfig: { fileName: tsconfig.json }\n    tsPreCompilationDeps: true\n");
    }
    out.push_str("rules:\n  dependencies:\n    forbidden:\n");
    for rule in rules {
        out.push_str(&rule.yaml);
    }
    if !entries.is_empty() {
        let _ = writeln!(
            out,
            "options:\n  # Findings present when this file was written. Each expires; fix it before then."
        );
        let _ = writeln!(out, "  knownViolations:");
        for entry in entries {
            let _ = writeln!(
                out,
                "    - {}",
                serde_json::to_string(entry).unwrap_or_default()
            );
        }
    }
    out
}

fn load_text(ctx: &Context<'_>, text: &str) -> Result<rb_config::Config, String> {
    load::load_text(text, Syntax::Yaml, &ctx.cwd, &LoadOptions::default())
        .map_err(|e| e.to_string())
}

/// Cruises the proposal, dropping vacuous rules and baselining findings, until a run exits 0.
///
/// # Errors
/// An [`Outcome`] naming what could not be made to pass.
pub fn converge(
    ctx: &Context<'_>,
    found: &Discovery,
    baseline: &BaselineArgs,
) -> Result<(String, Vec<String>, usize), Outcome> {
    let mut rules = proposed_rules(found);
    let today = ctx.today.to_string();
    let failed =
        |m: String| Outcome::failed(RunExit::Untrustworthy, format!("rulebearing init: {m}\n"));
    let options = RunOptions {
        liveness: true,
        options_used: serde_json::Map::new(),
        paths: found.roots.clone(),
    };
    let mut dropped = Vec::new();
    for _ in 0..=rules.len() {
        let text = render(found, &rules, &[], &today);
        let config = load_text(ctx, &text).map_err(|m| {
            Outcome::failed(
                RunExit::InvalidConfig,
                format!("rulebearing init: the proposal does not load: {m}\n"),
            )
        })?;
        let run = pipeline::run(ctx, &config, &options, &mut Progress::new(None))
            .map_err(|e| failed(e.to_string()))?;
        if !run.evaluation.vacuous.is_empty() {
            let vacuous: Vec<String> = run
                .evaluation
                .vacuous
                .iter()
                .map(|v| v.name.clone())
                .collect();
            let before = rules.len();
            rules.retain(|r| !vacuous.contains(&r.name));
            if rules.len() == before {
                return Err(failed(format!(
                    "rules {} match nothing and are not ones init proposed",
                    vacuous.join(", ")
                )));
            }
            dropped.extend(vacuous);
            continue;
        }
        let entries = adopt::baseline(ctx, &run.evaluation.document.summary.violations, baseline)?;
        let text = render(found, &rules, &entries, &today);
        let config = load_text(ctx, &text).map_err(|m| {
            Outcome::failed(RunExit::InvalidConfig, format!("rulebearing init: {m}\n"))
        })?;
        let check = pipeline::run(ctx, &config, &options, &mut Progress::new(None))
            .map_err(|e| failed(e.to_string()))?;
        let errors = check.evaluation.error_count();
        if errors != 0 || !check.evaluation.vacuous.is_empty() {
            return Err(failed(format!(
                "the proposal with its baseline still has {errors} errors; please report this"
            )));
        }
        return Ok((text, dropped, entries.len()));
    }
    Err(failed("no proposal converged".into()))
}

/// Runs `init`.
pub fn run(ctx: &mut Context<'_>, args: &InitArgs) -> Outcome {
    let found = discover(&ctx.cwd);
    if !found.typescript && !found.package_json {
        return Outcome::failed(
            RunExit::Untrustworthy,
            "rulebearing init: no package.json or tsconfig.json here. Wave 1 initialises TypeScript and JavaScript repositories; run it at the repository root (.NET and Python arrive in wave 2)\n",
        );
    }
    let target = ctx.resolve(&args.output);
    let existing = rb_config::DEFAULT_NAMES
        .iter()
        .find(|n| ctx.resolve(n).is_file());
    if !args.force
        && !args.dry_run
        && let Some(name) = existing
    {
        return Outcome::failed(
            RunExit::InvalidConfig,
            format!(
                "rulebearing init: {name} already exists. For a repository already on dependency-cruiser, run `rulebearing adopt`; to replace it, pass --force\n"
            ),
        );
    }
    let (text, dropped, baselined) = match converge(ctx, &found, &args.baseline) {
        Ok(v) => v,
        Err(o) => return o,
    };
    let mut report = String::new();
    if !dropped.is_empty() {
        let _ = writeln!(
            report,
            "left out, because they match nothing yet: {}",
            dropped.join(", ")
        );
    }
    let _ = writeln!(
        report,
        "a cruise of {} with this configuration exits 0 ({baselined} findings baselined)",
        found.roots.join(" ")
    );
    if args.dry_run {
        return Outcome {
            stdout: text,
            stderr: report,
            code: 0,
        };
    }
    if let Err(e) = std::fs::write(&target, &text) {
        return Outcome::failed(
            RunExit::Untrustworthy,
            format!("rulebearing init: cannot write {}: {e}\n", target.display()),
        );
    }
    let _ = writeln!(
        report,
        "wrote {}; next: rulebearing cruise {}",
        args.output,
        found.roots.join(" ")
    );
    Outcome::printed(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_reads_the_layout() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("rb-init-discover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in [
            "apps/web/src/features/a",
            "apps/web/src/features/b",
            "apps/admin",
            "packages/ui",
            "node_modules/x",
        ] {
            std::fs::create_dir_all(dir.join(d))?;
        }
        std::fs::write(dir.join("tsconfig.json"), "{}")?;
        std::fs::write(dir.join("apps/web/next.config.mjs"), "")?;
        std::fs::write(dir.join("vitest.config.ts"), "")?;
        let found = discover(&dir);
        assert!(found.typescript && !found.package_json);
        assert_eq!(found.apps, ["admin", "web"]);
        assert_eq!(found.packages, ["ui"]);
        assert_eq!(found.features, ["apps/web/src/features"]);
        assert_eq!(found.frameworks, ["next", "vitest"]);
        assert_eq!(found.roots, ["apps", "packages"]);
        let names: Vec<String> = proposed_rules(&found).into_iter().map(|r| r.name).collect();
        assert_eq!(
            names,
            [
                "no-orphans",
                "apps-are-independent",
                "packages-not-to-apps",
                "features-are-independent-in-apps-web"
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
        let empty = discover(&dir);
        assert_eq!(empty.roots, ["."]);
        Ok(())
    }

    #[test]
    fn exclusions_start_from_the_preset() {
        let plain = orphan_exclusions(&Discovery::default());
        assert!(plain.iter().any(|p| p.contains("tsconfig")), "{plain:?}");
        let next = orphan_exclusions(&Discovery {
            frameworks: vec!["next", "storybook", "jest"],
            ..Discovery::default()
        });
        assert_eq!(next.len(), plain.len() + 6);
        assert_eq!(regex_escape("a.b/c"), r"a\.b/c");
    }

    #[test]
    fn rendered_text_loads() -> Result<(), Box<dyn std::error::Error>> {
        let found = Discovery {
            typescript: true,
            apps: vec!["a".into(), "b".into()],
            packages: vec!["p".into()],
            features: vec!["src/features".into()],
            roots: vec!["apps".into()],
            ..Discovery::default()
        };
        let entry = serde_json::json!({ "id": "RB-00000000", "from": "a", "to": "b", "rule": { "name": "r", "severity": "error" }, "expires": "2026-12-22", "owner": "me" });
        let text = render(&found, &proposed_rules(&found), &[entry], "2026-09-23");
        let config = load::load_text(&text, Syntax::Yaml, Path::new("."), &LoadOptions::default())?;
        assert_eq!(config.known_violations.len(), 1);
        let names: Vec<&str> = config
            .rules
            .dependencies
            .forbidden
            .iter()
            .map(rb_config::Rule::name)
            .collect();
        assert!(
            names.contains(&"features-are-independent") && names.contains(&"no-circular"),
            "{names:?}"
        );
        assert!(text.contains("# Found: TypeScript, 2 apps, 1 packages, src/features."));
        Ok(())
    }
}
