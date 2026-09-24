//! `rulebearing init`: a first configuration that describes the repository it found, and passes.
//!
//! - Source: [design § The developer relations hat](../../../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo)
//!   ("reads the repo before it asks anything ... every rule already passing or baselined")
//! - Plans: [Wave 1, Step 16](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-16-init-1f),
//!   [Wave 2, Step 9](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#29-step-9-presets---init-presets-vue-svelte-markdown-webpackconfig-collapse-highlight-experimentalstats-2d)
//!   ("`init` (wave 1) selects presets from the languages it detects")
//! - Source: [design § What stays honest across the boundary](../../../../docs/artifacts/design.md#what-stays-honest-across-the-boundary)
//!   (per-language presets, composed by `rulebearing:recommended`)
//! - Requirements: [FR-CLI-03](../../../../docs/prd.md#fr-cli-03), [FR-CFG-06](../../../../docs/prd.md#fr-cfg-06)
//!
//! Discovery reads the tree: TypeScript (`tsconfig.json`) or JavaScript (`package.json`), .NET (a
//! solution, a project file or `Directory.Build.props` at the root), Python (`pyproject.toml`,
//! `setup.py` or `setup.cfg`), an `apps/` and `packages/` split, `src/features/*` layouts, and the
//! entry files and conventions a framework implies, which `no-orphans` must not report. The
//! languages found (or named with `--preset`) choose the presets: one language extends its own
//! preset and `rulebearing:recommended`, its own first so that its exclusions win and no other
//! language's are carried; several extend `rulebearing:recommended`, which composes all three.
//! The proposal adds one fence per boundary found, each with a `comment` and a `fix`. A cruise
//! then runs over it: a rule that would be vacuous is dropped, and every current finding is
//! written into the baseline ([`crate::cmd::adopt::baseline`]), so the file is written only when
//! a second cruise with it exits 0.

use std::fmt::Write as _;
use std::path::Path;

use clap::{Args, ValueEnum};
use rb_config::extends::{self, Target};
use rb_config::load::{self, LoadOptions};
use rb_config::read::Syntax;
use serde_json::Value;

use crate::cmd::adopt::{self, BaselineArgs};
use crate::context::Context;
use crate::pipeline::{self, RunOptions};
use crate::progress::Progress;
use crate::{Outcome, RunExit};

/// A language whose defaults are a bundled preset (`rulebearing:<name>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum Preset {
    /// TypeScript and JavaScript: `rulebearing:typescript`.
    Typescript,
    /// .NET: `rulebearing:dotnet`.
    Dotnet,
    /// Python: `rulebearing:python`.
    Python,
}

impl Preset {
    /// The preset's name after `rulebearing:`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Typescript => "typescript",
            Self::Dotnet => "dotnet",
            Self::Python => "python",
        }
    }

    /// The language as the proposal's header names it.
    const fn label(self) -> &'static str {
        match self {
            Self::Typescript => "TypeScript",
            Self::Dotnet => ".NET",
            Self::Python => "Python",
        }
    }
}

/// The `extends` a proposal for `languages` writes: one language's preset before
/// `rulebearing:recommended`, so its exclusions win; otherwise `rulebearing:recommended`, the
/// composition of all three.
pub fn extends_for(languages: &[Preset]) -> String {
    match languages {
        [one] => format!("[rulebearing:{}, rulebearing:recommended]", one.name()),
        _ => "rulebearing:recommended".to_owned(),
    }
}

/// `init`.
#[derive(Debug, Clone, Default, Args)]
pub struct InitArgs {
    /// Use these languages' presets instead of the ones found: typescript, dotnet, python
    /// (repeat, or separate with commas)
    #[arg(long, value_enum, value_delimiter = ',', value_name = "LANGUAGE")]
    pub preset: Vec<Preset>,
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
    /// The languages whose presets the proposal extends: those found (TypeScript for a
    /// `tsconfig.json` or `package.json`, .NET for a `.sln`, `.slnx` or `.csproj`, or
    /// `Directory.Build.props`, Python for a `pyproject.toml`, `setup.py` or `setup.cfg`, each at
    /// the root), or those `--preset` names.
    pub languages: Vec<Preset>,
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
    let typescript = root.join("tsconfig.json").is_file();
    let package_json = root.join("package.json").is_file();
    let dotnet = any_file(root, &["Directory.Build.props"])
        || std::fs::read_dir(root).is_ok_and(|entries| {
            entries.flatten().any(|e| {
                e.path().is_file()
                    && e.path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|x| {
                            ["sln", "slnx", "csproj"].contains(&x.to_ascii_lowercase().as_str())
                        })
            })
        });
    let python = any_file(root, &["pyproject.toml", "setup.py", "setup.cfg"]);
    let languages = [
        (typescript || package_json, Preset::Typescript),
        (dotnet, Preset::Dotnet),
        (python, Preset::Python),
    ]
    .into_iter()
    .filter_map(|(found, preset)| found.then_some(preset))
    .collect();
    Discovery {
        typescript,
        package_json,
        languages,
        apps,
        packages,
        features,
        frameworks,
        roots,
    }
}

/// The `no-orphans` exclusions: the chosen preset's (one language's own, else
/// `rulebearing:recommended`'s), then the conventions found.
fn orphan_exclusions(found: &Discovery) -> Vec<String> {
    let preset = match found.languages.as_slice() {
        [one] => one.name(),
        _ => "recommended",
    };
    let mut out: Vec<String> =
        match extends::resolve(&format!("rulebearing:{preset}"), Path::new(".")) {
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
    for language in &found.languages {
        seen.push(match language {
            Preset::Typescript if !found.typescript && found.package_json => {
                "JavaScript".to_owned()
            }
            other => other.label().to_owned(),
        });
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
    let _ = writeln!(out, "extends: {}", extends_for(&found.languages));
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
    let mut found = discover(&ctx.cwd);
    if !args.preset.is_empty() {
        let mut named = args.preset.clone();
        named.sort_unstable();
        named.dedup();
        found.languages = named;
    }
    if found.languages.is_empty() {
        return Outcome::failed(
            RunExit::Untrustworthy,
            "rulebearing init: no package.json, tsconfig.json, .sln, .slnx, .csproj, Directory.Build.props, pyproject.toml, setup.py or setup.cfg here; run it at the repository root, or name the languages with --preset typescript,dotnet,python\n",
        );
    }
    let target = ctx.resolve(&args.output);
    // The file it would write counts, as well as any configuration found by the default names.
    let existing = std::iter::once(args.output.as_str())
        .filter(|_| target.is_file())
        .chain(
            rb_config::DEFAULT_NAMES
                .iter()
                .copied()
                .filter(|n| ctx.resolve(n).is_file()),
        )
        .next();
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

/// The run scripts `--init x-scripts` adds to `package.json`: dependency-cruiser's
/// `compileRunScripts`, with `rulebearing cruise` for `depcruise`, over the roots found. Its
/// `depcruise:graph`, `depcruise:graph:dev`, `depcruise:graph:archi` and `depcruise:html` scripts
/// pipe the `dot`, `archi` and `err-html` reporters through `depcruise-wrap-stream-in-html`,
/// which this build does not have, so they are not written rather than written broken.
pub fn run_scripts(roots: &[String]) -> Vec<(String, String)> {
    let roots = roots.join(" ");
    vec![
        (
            "rulebearing".to_owned(),
            format!("rulebearing cruise {roots}"),
        ),
        (
            "rulebearing:text".to_owned(),
            format!("rulebearing cruise {roots} --progress --output-type text"),
        ),
        (
            "rulebearing:focus".to_owned(),
            format!("rulebearing cruise {roots} --progress --output-type text --focus"),
        ),
    ]
}

/// dependency-cruiser's `addRunScriptsToManifest`: each script whose name the manifest's
/// `scripts` does not have yet is added after the existing ones, which stay as they are. Returns
/// the names added.
pub fn add_run_scripts(
    manifest: &mut serde_json::Map<String, Value>,
    scripts: &[(String, String)],
) -> Vec<String> {
    let existing = manifest
        .get("scripts")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut merged = existing.clone();
    let mut added = Vec::new();
    for (name, command) in scripts {
        if !existing.contains_key(name) {
            merged.insert(name.clone(), Value::String(command.clone()));
            added.push(name.clone());
        }
    }
    manifest.insert("scripts".to_owned(), Value::Object(merged));
    added
}

fn write_run_scripts(ctx: &Context<'_>, roots: &[String]) -> Result<Vec<String>, String> {
    let path = ctx.resolve("package.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut manifest = match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => map,
        Ok(_) => return Err(format!("{} is not a JSON object", path.display())),
        Err(e) => return Err(format!("{} does not parse: {e}", path.display())),
    };
    let added = add_run_scripts(&mut manifest, &run_scripts(roots));
    let mut out = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    if text.ends_with('\n') {
        out.push('\n');
    }
    std::fs::write(&path, out).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(added)
}

/// `cruise --init [oneshot]`: dependency-cruiser's `depcruise --init`, without the questions.
/// Every one-shot name writes the configuration `init` writes, to `--config FILE` or
/// `rulebearing.yaml`, with `--preset` choosing the languages; as upstream, a name other than
/// `x-scripts` is `yes`. `x-scripts` also adds [`run_scripts`] to `package.json` when there is
/// one, and, as upstream, leaves an existing configuration be and still adds them. A bare
/// `--init`, which asks questions upstream, is `yes`: the proposal is read from the repository.
pub fn oneshot(ctx: &mut Context<'_>, oneshot: &str, args: &crate::cli::CruiseArgs) -> Outcome {
    let output = args
        .config
        .config
        .as_deref()
        .filter(|c| !c.is_empty() && *c != "-")
        .unwrap_or("rulebearing.yaml")
        .to_owned();
    let scripts = oneshot == "x-scripts" && ctx.resolve("package.json").is_file();
    let mut report = String::new();
    if !(scripts && ctx.resolve(&output).is_file()) {
        let init = InitArgs {
            preset: args.preset.clone(),
            output,
            ..InitArgs::default()
        };
        let outcome = run(ctx, &init);
        if outcome.code != 0 {
            return outcome;
        }
        report.push_str(&outcome.stdout);
    }
    if scripts {
        match write_run_scripts(ctx, &discover(&ctx.cwd).roots) {
            Ok(added) if added.is_empty() => {
                report.push_str("package.json already has the rulebearing run scripts\n");
            }
            Ok(added) => {
                let _ = writeln!(
                    report,
                    "added run scripts to package.json: {}",
                    added.join(", ")
                );
            }
            Err(message) => {
                return Outcome::failed(
                    RunExit::Untrustworthy,
                    format!("rulebearing cruise --init: {message}\n"),
                );
            }
        }
    }
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
    fn languages_choose_the_presets() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            extends_for(&[Preset::Dotnet]),
            "[rulebearing:dotnet, rulebearing:recommended]"
        );
        assert_eq!(
            extends_for(&[Preset::Typescript, Preset::Python]),
            "rulebearing:recommended"
        );
        assert_eq!(extends_for(&[]), "rulebearing:recommended");
        let names: Vec<&str> = [Preset::Typescript, Preset::Dotnet, Preset::Python]
            .iter()
            .map(|p| p.name())
            .collect();
        assert_eq!(names, ["typescript", "dotnet", "python"]);
        let dir = std::env::temp_dir().join(format!("rb-init-languages-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("App.SLN"), "")?;
        std::fs::write(dir.join("setup.cfg"), "")?;
        let found = discover(&dir);
        assert!(!found.typescript && !found.package_json);
        assert_eq!(found.languages, [Preset::Dotnet, Preset::Python]);
        std::fs::remove_file(dir.join("App.SLN"))?;
        std::fs::write(dir.join("Directory.Build.props"), "")?;
        assert_eq!(discover(&dir).languages, [Preset::Dotnet, Preset::Python]);
        std::fs::remove_file(dir.join("Directory.Build.props"))?;
        assert_eq!(discover(&dir).languages, [Preset::Python]);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn one_language_s_exclusions_are_its_own() {
        let dotnet = orphan_exclusions(&Discovery {
            languages: vec![Preset::Dotnet],
            ..Discovery::default()
        });
        assert!(dotnet.iter().any(|p| p.contains("Program")), "{dotnet:?}");
        assert!(
            !dotnet
                .iter()
                .any(|p| p.contains("tsconfig") || p.contains("__main__"))
        );
        let both = orphan_exclusions(&Discovery {
            languages: vec![Preset::Dotnet, Preset::Python],
            ..Discovery::default()
        });
        assert!(
            both.iter().any(|p| p.contains("Program"))
                && both.iter().any(|p| p.contains("__main__"))
        );
        let found = Discovery {
            package_json: true,
            languages: vec![Preset::Typescript, Preset::Dotnet],
            roots: vec![".".into()],
            ..Discovery::default()
        };
        let text = render(&found, &[], &[], "2026-09-24");
        assert!(text.contains("# Found: JavaScript, .NET.\n"), "{text}");
        assert!(text.contains("extends: rulebearing:recommended\n"));
    }

    #[test]
    fn run_scripts_are_added_after_the_existing_ones() {
        let scripts = run_scripts(&["src".to_owned(), "test".to_owned()]);
        assert_eq!(
            scripts[0],
            (
                "rulebearing".to_owned(),
                "rulebearing cruise src test".to_owned()
            )
        );
        let mut manifest = serde_json::Map::new();
        manifest.insert("name".into(), Value::String("x".into()));
        manifest.insert(
            "scripts".into(),
            serde_json::json!({ "rulebearing": "mine", "build": "tsc" }),
        );
        let added = add_run_scripts(&mut manifest, &scripts);
        assert_eq!(added, ["rulebearing:text", "rulebearing:focus"]);
        assert_eq!(
            manifest["scripts"]["rulebearing"], "mine",
            "an existing script is kept"
        );
        let order: Vec<&String> = manifest["scripts"]
            .as_object()
            .map(|s| s.keys().collect())
            .unwrap_or_default();
        assert_eq!(
            order,
            [
                "rulebearing",
                "build",
                "rulebearing:text",
                "rulebearing:focus"
            ]
        );
        let mut bare = serde_json::Map::new();
        assert_eq!(add_run_scripts(&mut bare, &scripts).len(), 3);
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
            languages: vec![Preset::Typescript],
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
