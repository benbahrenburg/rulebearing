//! `rulebearing docs --format agents-md | contributing | skill [--out FILE] [--verify]`: the
//! documentation an agent or a contributor reads, rendered from the rules so it cannot drift.
//!
//! - Source: [design § Docs derived from the rules, never written beside them](../../../../docs/artifacts/design.md#docs-derived-from-the-rules-never-written-beside-them)
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md) decision 2
//! - Coverage: [`ArchUnitNET` coverage tab](../../../../docs/artifacts/archunitnet-0.13.4-coverage.md)
//!   (the "Stays" note: both rule styles are taught when both exist)
//! - Requirement: [FR-CLI-02](../../../../docs/prd.md#fr-cli-02)
//!
//! | Format | Renders |
//! | --- | --- |
//! | `agents-md` | the section of `AGENTS.md` or `CLAUDE.md` an agent reads: one line per rule with its sentence (`explain --plain`), severity, `fix` and decision links, grouped by the tree it fences |
//! | `contributing` | the "what does this error mean and how do I fix it" table: rule, sentence, `fix`, decision |
//! | `skill` | a Claude Code `SKILL.md`: the rule families, the commands, how to read the `agent` reporter, and the rules |
//!
//! `agents-md` and `contributing` are sections: with `--out FILE` they replace what lies between
//! their `<!-- rulebearing:<format>:begin -->` and `end` markers and keep the rest of the file,
//! or are appended when the file has no markers, so a hand-written `CLAUDE.md` keeps its prose.
//! `skill` is a whole file. `--verify` renders the same bytes and exits 1 when the file on disk
//! differs, which is how CI fails a stale copy. Decision links are relative to the output file,
//! whose path is normalised first (`--out ../AGENTS.md`). `--out -` prints, as no `--out` does. A
//! file holding one marker without the other is refused with exit 2, naming the file, since
//! appending would leave a second section and a later run would replace the prose between them.
//! Nothing time- or machine-dependent is written, so two runs agree byte for byte.

use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use clap::{Args, ValueEnum};
use rb_config::capability::{Applicability, Capability, applicability, capability};
use rb_config::elements::{Concept, Kind, Side, VOCABULARY, ValueKind, spellings, split_key};
use rb_model::Language;

use crate::cli::ConfigArgs;
use crate::cmd::catalogue::{self, Entry};
use crate::cmd::decisions::{self, ADR_DIR};
use crate::context::Context;
use crate::{Outcome, RunExit, configure};

/// What `docs` renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DocsFormat {
    /// The rules section of AGENTS.md or CLAUDE.md
    AgentsMd,
    /// The error-to-fix table of a contributing guide
    Contributing,
    /// A Claude Code SKILL.md
    Skill,
    /// The element-rule key reference, from the vocabulary and the capability table; needs no
    /// configuration
    Reference,
}

impl DocsFormat {
    /// The name `--format` takes.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AgentsMd => "agents-md",
            Self::Contributing => "contributing",
            Self::Skill => "skill",
            Self::Reference => "reference",
        }
    }
}

/// `docs`.
#[derive(Debug, Clone, Args)]
pub struct DocsArgs {
    /// What to render: agents-md, contributing, skill or reference
    #[arg(long, value_enum, value_name = "FORMAT")]
    pub format: DocsFormat,
    /// Write to FILE instead of stdout; agents-md and contributing replace their marked section
    /// and keep the rest of the file
    #[arg(long, value_name = "FILE")]
    pub out: Option<String>,
    /// Exit 1 when FILE differs from what would be written, without writing it
    #[arg(long, requires = "out")]
    pub verify: bool,
    /// The folder holding the decision records the links point at
    #[arg(long, value_name = "DIR", default_value = ADR_DIR)]
    pub adr_dir: String,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
}

/// A path's components with `.` dropped and each `..` taking out the folder before it, lexically:
/// `/r/sub/../A.md` is `/r/A.md`. A `..` above a root is dropped; one leading a relative path is
/// kept.
fn normal(path: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut anchored = 0;
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if out.len() > anchored && out.last().is_some_and(|l| l != "..") {
                    out.pop();
                } else if anchored == 0 {
                    out.push("..".to_owned());
                }
            }
            Component::Prefix(_) | Component::RootDir => {
                out.push(component.as_os_str().to_string_lossy().into_owned());
                anchored += 1;
            }
            Component::Normal(name) => out.push(name.to_string_lossy().into_owned()),
        }
    }
    out
}

/// `to` relative to the folder `from`, `/`-separated; both absolute or both relative. Both are
/// normalised first, so `--out ../A.md` links from the folder the file lands in.
pub fn relative(from: &Path, to: &Path) -> String {
    let (from, to) = (normal(from), normal(to));
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut out: Vec<String> = vec!["..".to_owned(); from.len() - common];
    out.extend(to[common..].iter().cloned());
    out.join("/")
}

/// What a rendering needs besides the configuration.
struct Place<'a> {
    /// The working directory.
    base: &'a Path,
    /// The folder the output's links are relative to.
    folder: PathBuf,
    /// `--adr-dir`.
    adr_dir: &'a str,
    /// The configuration's name as the reader should type it.
    config_name: String,
}

impl Place<'_> {
    /// The decision links of an entry, as Markdown.
    fn links(&self, entry: &Entry) -> Vec<String> {
        entry
            .tokens()
            .into_iter()
            .map(
                |token| match decisions::resolve(self.base, self.adr_dir, &token) {
                    Some(path) => format!("[{token}]({})", relative(&self.folder, &path)),
                    None => format!("`{token}`"),
                },
            )
            .collect()
    }

    /// One rule as a Markdown list item.
    fn line(&self, entry: &Entry) -> String {
        let mut text = format!("- **{}**", entry.name);
        match entry.severity {
            Some(severity) => {
                let _ = write!(text, " ({}, {})", entry.family, severity.as_str());
            }
            None => {
                let _ = write!(text, " ({})", entry.family);
            }
        }
        let _ = write!(text, ": {}", entry.sentence);
        if let Some(fix) = &entry.fix {
            let _ = write!(text, " Fix: {}", fix.trim());
        }
        let links = self.links(entry);
        if !links.is_empty() {
            let _ = write!(text, " Decision: {}.", links.join(", "));
        }
        text
    }

    /// Every rule, grouped by what it fences.
    fn grouped_lines(&self, entries: &[Entry], level: &str) -> String {
        let mut out = String::new();
        for (group, members) in catalogue::grouped(entries) {
            let _ = writeln!(out, "{level} {}\n", group.heading());
            for entry in members {
                let _ = writeln!(out, "{}", self.line(entry));
            }
            out.push('\n');
        }
        out
    }
}

fn begin(format: DocsFormat) -> String {
    format!("<!-- rulebearing:{}:begin -->", format.as_str())
}

fn end(format: DocsFormat) -> String {
    format!("<!-- rulebearing:{}:end -->", format.as_str())
}

fn agents_md(place: &Place<'_>, entries: &[Entry]) -> String {
    let mut out = format!("{}\n## Architecture rules\n\n", begin(DocsFormat::AgentsMd));
    let _ = writeln!(
        out,
        "Generated from `{}` by `rulebearing docs --format agents-md`; change the rules, not this section. Each line is one rule: what it forbids or requires, its severity, what to do when it fires, and the decision it serves. Ask `rulebearing can-import <from> <to>` before writing an import, and `rulebearing explain <rule>` when one fires.\n",
        place.config_name
    );
    if entries.is_empty() {
        out.push_str("The configuration holds no rules.\n\n");
    } else {
        out.push_str(&place.grouped_lines(entries, "###"));
    }
    out.push_str(&end(DocsFormat::AgentsMd));
    out.push('\n');
    out
}

fn cell(text: &str) -> String {
    text.trim().replace('|', "\\|").replace('\n', " ")
}

fn contributing(place: &Place<'_>, entries: &[Entry]) -> String {
    let mut out = format!(
        "{}\n## What a rulebearing error means and how to fix it\n\n",
        begin(DocsFormat::Contributing)
    );
    let _ = writeln!(
        out,
        "Generated from `{}` by `rulebearing docs --format contributing`. A failing check names the rule; find it here.\n",
        place.config_name
    );
    out.push_str(
        "| Rule | What it means | How to fix it | Decision |\n| --- | --- | --- | --- |\n",
    );
    for entry in entries {
        let links = place.links(entry);
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} |",
            cell(&entry.name),
            cell(&entry.sentence),
            entry.fix.as_deref().map_or_else(|| "-".to_owned(), cell),
            if links.is_empty() {
                "-".to_owned()
            } else {
                links.join(", ")
            }
        );
    }
    out.push('\n');
    out.push_str(&end(DocsFormat::Contributing));
    out.push('\n');
    out
}

/// The families present, with counts, in the order the engine evaluates them.
fn families(entries: &[Entry]) -> Vec<(&'static str, usize)> {
    [
        "forbidden",
        "allowed",
        "required",
        "ratchets",
        "elements",
        "slices",
        "diagrams",
    ]
    .into_iter()
    .map(|f| (f, entries.iter().filter(|e| e.family == f).count()))
    .filter(|(_, n)| *n > 0)
    .collect()
}

fn family_meaning(family: &str) -> &'static str {
    match family {
        "forbidden" => {
            "dependency-cruiser style: an import these rules match is a violation on that import's line"
        }
        "allowed" => {
            "dependency-cruiser style: every import must match one of these, else it is `not-in-allowed`"
        }
        "required" => {
            "dependency-cruiser style: every module a rule selects must import (or reach) what it names"
        }
        "ratchets" => {
            "a count of matching imports that may fall and never rise; `rulebearing count --write` lowers the ceiling"
        }
        "elements" => {
            "ArchUnitNET style: every selected type, member or module must satisfy the rule's `should`"
        }
        "slices" => {
            "ArchUnitNET style: slices of the code may not depend on each other or form a cycle"
        }
        _ => "ArchUnitNET style: the code must follow the dependencies a PlantUML diagram draws",
    }
}

fn skill(place: &Place<'_>, entries: &[Entry]) -> String {
    let mut out = String::from(
        "---\nname: rulebearing-architecture\ndescription: The architecture rules this repository enforces with rulebearing, and the commands that check an import, a file or a new rule before CI does. Use before adding an import, creating or moving a file, or changing the rule file.\n---\n\n# Architecture rules (rulebearing)\n\n",
    );
    let _ = writeln!(
        out,
        "The rules live in `{}` and CI fails on any error-severity violation. This skill was generated from them by `rulebearing docs --format skill`; regenerate it when they change.\n",
        place.config_name
    );
    out.push_str("## Rule families\n\n");
    let present = families(entries);
    for (family, count) in &present {
        let _ = writeln!(out, "- `{family}` ({count}): {}.", family_meaning(family));
    }
    if present.is_empty() {
        out.push_str("- none yet: the configuration holds no rules.\n");
    }
    let dependency_style = present
        .iter()
        .any(|(f, _)| matches!(*f, "forbidden" | "allowed" | "required"));
    let element_style = present
        .iter()
        .any(|(f, _)| matches!(*f, "elements" | "slices" | "diagrams"));
    if dependency_style && element_style {
        out.push_str("\nBoth styles are in force: dependency-cruiser-style rules judge imports between files, ArchUnitNET-style rules judge types and members. A change must satisfy both, and a violation names which rule it broke.\n");
    }
    out.push_str(
        "\n## Commands\n\n\
| Command | Use it to |\n\
| --- | --- |\n\
| `rulebearing cruise --output-type agent` | check the repository; exit 0 means no error-severity violation |\n\
| `rulebearing can-import <from> <to>` | ask before writing an import: `yes`, or `no` with the rule and its `fix` |\n\
| `rulebearing explain <rule>` | read a rule as a sentence, why it exists, its `fix` and the edges it matches |\n\
| `rulebearing impact <file>` | see the rules that mention a file, its dependents, cycles and ratchets before editing it |\n\
| `rulebearing place --imports a,b --imported-by c --language <language>` | find the directories where a new module with those imports would be legal |\n\
| `rulebearing test` | prove every rule's `examples`: forbidden ones flagged, allowed ones not |\n\
\n## Reading the `agent` reporter\n\n\
`cruise --output-type agent` prints JSON. `rules[]` holds one entry per rule that fired, cheapest to fix first, each with `name`, `severity`, `count` (all violations), `shown` (those listed), `fix` (do this), `decision` (the record the rule serves) and `violations[]`. Each violation has a stable `id`, `from`, `to`, `line` and `column` (edit that import), and `cost` (`edgesToMove`, `targetFanIn`, `score`). `inspected` counts what was read, so an empty run is visible; `vacuousRules` lists rules that matched nothing; `budget.truncated` says whether `--max-findings` cut the list. Fix the cheapest violations first, follow `fix` rather than widening a rule, and never raise a ratchet's ceiling.\n\n## The rules\n\n",
    );
    if entries.is_empty() {
        out.push_str("The configuration holds no rules.\n");
    } else {
        let lines = place.grouped_lines(entries, "###");
        out.push_str(lines.trim_end());
        out.push('\n');
    }
    out
}

/// The file's new content: the section put between its markers, appended when the file has none,
/// or the whole text for a format that is not a section.
///
/// # Errors
/// What is wrong with the markers, when the file has one without the other: appending would
/// leave two sections, and a later run would replace the hand-written text between them.
pub fn merge(format: DocsFormat, existing: Option<&str>, rendered: &str) -> Result<String, String> {
    if matches!(format, DocsFormat::Skill | DocsFormat::Reference) {
        return Ok(rendered.to_owned());
    }
    let Some(existing) = existing.filter(|e| !e.trim().is_empty()) else {
        return Ok(rendered.to_owned());
    };
    let (open, close) = (begin(format), end(format));
    if let Some(start) = existing.find(&open) {
        let Some(stop) = existing[start..].find(&close).map(|i| start + i) else {
            return Err(format!(
                "has `{open}` but no `{close}` after it; add the end marker where the generated section stops, or delete the begin marker"
            ));
        };
        let mut rest = &existing[stop + close.len()..];
        rest = rest.strip_prefix('\n').unwrap_or(rest);
        return Ok(format!("{}{rendered}{rest}", &existing[..start]));
    }
    if existing.contains(&close) {
        return Err(format!(
            "has `{close}` but no `{open}` before it; add the begin marker where the generated section starts, or delete the end marker"
        ));
    }
    Ok(format!("{}\n\n{rendered}", existing.trim_end()))
}

/// What each language says about `concept`, for one table cell.
fn answer(concept: Concept, language: Language) -> String {
    match capability(concept, language) {
        Capability::Answerable => "yes".to_owned(),
        Capability::Mapped(how) => cell(how),
        Capability::Unanswerable(why) => format!("no: {}", cell(why)),
    }
}

/// The `select.kind`s a concept applies to, for one table cell.
fn kinds(concept: Concept) -> String {
    let list: Vec<String> = Kind::ALL
        .into_iter()
        .filter(|k| applicability(concept, *k) == Applicability::Applies)
        .map(|k| format!("`{}`", k.as_str()))
        .collect();
    if list.len() == Kind::ALL.len() {
        "every kind".to_owned()
    } else {
        list.join(", ")
    }
}

/// The value a key of `kind` takes, in words.
const fn value_words(kind: ValueKind) -> &'static str {
    match kind {
        ValueKind::Flag => "`true` or `false`",
        ValueKind::Names => "a name or a list",
        ValueKind::Pattern => "a regular expression",
        ValueKind::Objects => "full names, or a nested selector",
        ValueKind::AttributeArguments => "`{ attribute, arguments: [...] }`",
        ValueKind::AttributeNamedArguments => "`{ attribute, arguments: { name: value } }`",
        ValueKind::ArgumentValues => "a list of argument values",
        ValueKind::NamedArgumentValues => "`{ name: value }`",
        ValueKind::Diagram => "a `.puml` file",
    }
}

/// The element-rule reference: one row per concept of the vocabulary, with its spellings on each
/// side, the value it takes and how each language answers it. It depends on this build only, so
/// `--verify` in CI holds the committed file to the code
/// ([plan 0002, section 2.16](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#216-documentation-to-update)).
pub fn reference() -> String {
    let mut keys: Vec<(Concept, Side, String)> = Vec::new();
    for side in [Side::Where, Side::Should] {
        for spelling in spellings(side) {
            let (base, _, _) = split_key(&spelling, side);
            if let Some((_, concept, ..)) = VOCABULARY.iter().find(|(n, ..)| *n == base) {
                keys.push((*concept, side, spelling));
            }
        }
    }
    let spelled = |concept: Concept, side: Side| -> String {
        let list: Vec<String> = keys
            .iter()
            .filter(|(c, s, _)| *c == concept && *s == side)
            .map(|(_, _, k)| format!("`{k}`"))
            .collect();
        if list.is_empty() {
            "(condition only)".to_owned()
        } else {
            list.join(", ")
        }
    };
    let mut out = String::from(
        "# Element rule keys\n\nGenerated by `rulebearing docs --format reference --out docs/reference/element-rules.md` from the element-rule vocabulary and the capability table; edit those, not this file. `yes` means the language answers the key as `ArchUnitNET` defines it; a `no` is exit 3 unless `select.language` leaves the language out. A key used on a `select.kind` (or nested selector kind) outside its Kinds cell is exit 3 as well. `getterVisibility` and `setterVisibility` take an access level and mean the `havePublicGetter` ... `havePrivateProtectedSetter` rows.\n\n| Concept | `select.where` | `should` | Value | Kinds | .NET | TypeScript | JavaScript | Python |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for (name, concept, kind, _) in VOCABULARY {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            if name.is_empty() {
                "`are` / `be`"
            } else {
                name
            },
            spelled(*concept, Side::Where),
            spelled(*concept, Side::Should),
            value_words(*kind),
            kinds(*concept),
            answer(*concept, Language::Dotnet),
            answer(*concept, Language::Typescript),
            answer(*concept, Language::Javascript),
            answer(*concept, Language::Python),
        );
    }
    out
}

/// Runs `docs`.
pub fn run(ctx: &mut Context<'_>, args: &DocsArgs) -> Outcome {
    if args.format == DocsFormat::Reference {
        return deliver(ctx, args, &reference());
    }
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let out_path = out_file(args).map(|o| ctx.resolve(o));
    let folder = out_path
        .as_deref()
        .and_then(Path::parent)
        .map_or_else(|| ctx.cwd.clone(), Path::to_path_buf);
    let place = Place {
        base: &ctx.cwd,
        folder,
        adr_dir: &args.adr_dir,
        config_name: config.origin.as_deref().map_or_else(
            || "the configuration".to_owned(),
            |p| {
                let base = ctx.cwd.canonicalize().unwrap_or_else(|_| ctx.cwd.clone());
                let path = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
                decisions::shown(&base, &path)
            },
        ),
    };
    let entries = catalogue::entries(&config);
    let rendered = match args.format {
        DocsFormat::AgentsMd => agents_md(&place, &entries),
        DocsFormat::Contributing => contributing(&place, &entries),
        DocsFormat::Skill => skill(&place, &entries),
        DocsFormat::Reference => reference(),
    };
    deliver(ctx, args, &rendered)
}

/// `--out`, unless it is `-`, which is standard output as it is for every other command.
fn out_file(args: &DocsArgs) -> Option<&str> {
    args.out.as_deref().filter(|o| *o != "-")
}

/// Prints `rendered`, or writes or verifies it against `--out`.
fn deliver(ctx: &Context<'_>, args: &DocsArgs, rendered: &str) -> Outcome {
    let Some(name) = out_file(args) else {
        if args.verify {
            return Outcome::failed(
                RunExit::InvalidConfig,
                "rulebearing docs: --verify compares a file with what would be written, and `--out -` is standard output; name the file to verify\n",
            );
        }
        return Outcome::printed(rendered.to_owned());
    };
    let path = ctx.resolve(name);
    let existing = std::fs::read_to_string(&path).ok();
    let content = match merge(args.format, existing.as_deref(), rendered) {
        Ok(content) => content,
        Err(reason) => {
            return Outcome::failed(
                RunExit::Untrustworthy,
                format!("rulebearing docs: {name} {reason}\n"),
            );
        }
    };
    if args.verify {
        return if existing.as_deref() == Some(content.as_str()) {
            Outcome::printed(format!("{name} is up to date\n"))
        } else {
            let state = if existing.is_some() {
                "is stale"
            } else {
                "does not exist"
            };
            Outcome {
                stdout: String::new(),
                stderr: format!(
                    "rulebearing docs: {name} {state}; regenerate it with `rulebearing docs --format {} --out {name}` and commit it\n",
                    args.format.as_str()
                ),
                code: RunExit::Violations(1).code(),
            }
        };
    }
    let mut ignored = String::new();
    match crate::write_output(ctx, name, &content, &mut ignored) {
        Ok(()) => Outcome::printed(format!("wrote {name}\n")),
        Err(e) => Outcome::failed(RunExit::Untrustworthy, format!("rulebearing docs: {e}\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reference_has_a_row_per_concept_and_the_committed_file_is_current() {
        let text = reference();
        assert_eq!(text, reference(), "deterministic");
        let rows = text
            .lines()
            .filter(|l| {
                l.starts_with("| ") && !l.starts_with("| Concept") && !l.starts_with("| ---")
            })
            .count();
        assert_eq!(rows, VOCABULARY.len());
        let sealed = text
            .lines()
            .find(|l| l.starts_with("| sealed |"))
            .unwrap_or_default();
        assert!(
            sealed.contains("`areSealed`") && sealed.contains("`beSealed`"),
            "{sealed}"
        );
        assert!(
            sealed.contains("| `type`, `class`, `attribute` | yes | no: the language cannot forbid subclassing |"),
            "{sealed}"
        );
        let name = text
            .lines()
            .find(|l| l.starts_with("| haveName |"))
            .unwrap_or_default();
        assert!(name.contains("| every kind | yes |"), "{name}");
        let committed = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/element-rules.md"),
        )
        .unwrap_or_default();
        assert!(
            committed == text,
            "docs/reference/element-rules.md is stale; run `rulebearing docs --format reference --out docs/reference/element-rules.md`"
        );
    }

    #[test]
    fn links_are_relative_to_the_output_folder() {
        assert_eq!(
            relative(Path::new("/r"), Path::new("/r/docs/adr/1.md")),
            "docs/adr/1.md"
        );
        assert_eq!(
            relative(
                Path::new("/r/.claude/skills/x"),
                Path::new("/r/docs/adr/1.md")
            ),
            "../../../docs/adr/1.md"
        );
        assert_eq!(relative(Path::new("./a"), Path::new("a/b.md")), "b.md");
        assert_eq!(
            relative(Path::new("/r/sub/.."), Path::new("/r/sub/docs/adr/1.md")),
            "sub/docs/adr/1.md",
            "`--out ../A.md` from /r/sub"
        );
        assert_eq!(
            relative(Path::new("/r/a/./b/../c"), Path::new("/r/a/c/x.md")),
            "x.md"
        );
        assert_eq!(relative(Path::new("/.."), Path::new("/x.md")), "x.md");
        assert_eq!(normal(Path::new("../a/../../b")), ["..", "..", "b"]);
    }

    #[test]
    fn a_section_replaces_its_markers_or_is_appended() {
        let section =
            "<!-- rulebearing:agents-md:begin -->\nNEW\n<!-- rulebearing:agents-md:end -->\n";
        let f = DocsFormat::AgentsMd;
        let merge = |f: DocsFormat, existing: Option<&str>, rendered: &str| {
            merge(f, existing, rendered).unwrap_or_default()
        };
        assert_eq!(merge(f, None, section), section);
        assert_eq!(merge(f, Some("  \n"), section), section);
        let hand = "# Mine\n\nprose\n";
        let appended = merge(f, Some(hand), section);
        assert_eq!(appended, format!("# Mine\n\nprose\n\n{section}"));
        let old = appended
            .replace("NEW", "OLD")
            .replace("prose", "kept prose")
            + "tail\n";
        assert_eq!(
            merge(f, Some(&old), section),
            format!("# Mine\n\nkept prose\n\n{section}tail\n")
        );
        assert_eq!(merge(f, Some(&appended), section), appended, "idempotent");
        assert_eq!(merge(DocsFormat::Skill, Some(hand), "S"), "S");
        let other = merge(DocsFormat::Contributing, Some(&appended), "C\n");
        assert!(other.ends_with("C\n") && other.contains("NEW"));
    }

    #[test]
    fn a_marker_without_its_pair_is_refused() {
        let f = DocsFormat::AgentsMd;
        let section =
            "<!-- rulebearing:agents-md:begin -->\nNEW\n<!-- rulebearing:agents-md:end -->\n";
        let open_only = "# Mine\n\n<!-- rulebearing:agents-md:begin -->\nOLD\n\n## Hand-written\n";
        let err = merge(f, Some(open_only), section).err().unwrap_or_default();
        assert!(
            err.contains("no `<!-- rulebearing:agents-md:end -->` after it"),
            "{err}"
        );
        let close_only = "# Mine\n\n<!-- rulebearing:agents-md:end -->\n";
        let err = merge(f, Some(close_only), section)
            .err()
            .unwrap_or_default();
        assert!(
            err.contains("no `<!-- rulebearing:agents-md:begin -->` before it"),
            "{err}"
        );
        let reversed = "<!-- rulebearing:agents-md:end -->\n<!-- rulebearing:agents-md:begin -->\n";
        assert!(merge(f, Some(reversed), section).is_err());
        assert!(merge(DocsFormat::Skill, Some(open_only), "S").is_ok());
    }

    #[test]
    fn formats_and_cells() {
        for f in DocsFormat::value_variants() {
            assert_eq!(DocsFormat::from_str(f.as_str(), false).ok(), Some(*f));
        }
        assert_eq!(cell(" a|b\nc "), "a\\|b c");
        for family in [
            "forbidden",
            "allowed",
            "required",
            "ratchets",
            "elements",
            "slices",
            "diagrams",
        ] {
            assert!(!family_meaning(family).is_empty());
        }
    }
}
