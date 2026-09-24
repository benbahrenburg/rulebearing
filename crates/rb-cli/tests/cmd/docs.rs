//! `docs --format agents-md | contributing | skill`, `--out`, `--verify`.
//!
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//! - Source: [design § Docs derived from the rules](../../../../docs/artifacts/design.md#docs-derived-from-the-rules-never-written-beside-them)

use crate::common::{Result, clean, code, read, run, stderr, stdout, tree, write};

const AGENTS_LINE: &str = "- **domain-not-to-web** (forbidden, error): Files matching `src/domain/` may not import files matching `src/web/`. Fix: Move the shared type into src/domain Decision: [adr:0010](docs/adr/0010-domain-independent.md).";

#[test]
fn agents_md_renders_one_line_per_rule_grouped_and_deterministic() -> Result {
    let dir = tree("docs-agents")?;
    let first = run(&dir, &["docs", "--format", "agents-md"])?;
    assert_eq!(code(&first), Some(0), "{}", stderr(&first));
    let text = stdout(&first);
    assert!(text.starts_with("<!-- rulebearing:agents-md:begin -->\n## Architecture rules\n"));
    assert!(text.ends_with("<!-- rulebearing:agents-md:end -->\n"));
    assert!(text.contains("### `src/domain/`\n\n"), "{text}");
    assert!(text.contains(AGENTS_LINE), "{text}");
    assert!(
        text.contains("- **domain-web-edges** (ratchets): Imports from `src/domain/` to `src/web/` may not outnumber the ceiling in `budgets/domain-web.json`, which only falls. Decision: `plan:wave-1`."),
        "an unresolved plan token is named, not linked: {text}"
    );
    assert!(text.contains("Generated from `rulebearing.yaml`"));
    let second = run(&dir, &["docs", "--format", "agents-md"])?;
    assert_eq!(first.stdout, second.stdout, "byte for byte");
    clean(&dir);
    Ok(())
}

#[test]
fn verify_passes_a_fresh_copy_and_fails_a_stale_or_missing_one() -> Result {
    let dir = tree("docs-verify")?;
    let verify = [
        "docs",
        "--format",
        "agents-md",
        "--verify",
        "--out",
        "AGENTS.md",
    ];
    let missing = run(&dir, &verify)?;
    assert_eq!(code(&missing), Some(1));
    assert!(stderr(&missing).contains("AGENTS.md does not exist"));

    let written = run(
        &dir,
        &["docs", "--format", "agents-md", "--out", "AGENTS.md"],
    )?;
    assert_eq!(code(&written), Some(0));
    assert_eq!(stdout(&written), "wrote AGENTS.md\n");
    let fresh = run(&dir, &verify)?;
    assert_eq!(code(&fresh), Some(0), "{}", stderr(&fresh));
    assert_eq!(stdout(&fresh), "AGENTS.md is up to date\n");

    let config = read(&dir, "rulebearing.yaml")?;
    write(
        &dir,
        "rulebearing.yaml",
        &config.replace("Move the shared type", "Move the type"),
    )?;
    let stale = run(&dir, &verify)?;
    assert_eq!(code(&stale), Some(1));
    assert!(stderr(&stale).contains("AGENTS.md is stale"));
    assert!(stderr(&stale).contains("rulebearing docs --format agents-md --out AGENTS.md"));

    let no_out = run(&dir, &["docs", "--format", "agents-md", "--verify"])?;
    assert_eq!(code(&no_out), Some(3), "--verify needs --out");
    clean(&dir);
    Ok(())
}

#[test]
fn a_hand_written_file_keeps_its_prose_around_the_section() -> Result {
    let dir = tree("docs-section")?;
    write(&dir, "CLAUDE.md", "# Working agreement\n\nHand-written.\n")?;
    let args = ["docs", "--format", "agents-md", "--out", "CLAUDE.md"];
    assert_eq!(code(&run(&dir, &args)?), Some(0));
    let once = read(&dir, "CLAUDE.md")?;
    assert!(once.starts_with(
        "# Working agreement\n\nHand-written.\n\n<!-- rulebearing:agents-md:begin -->"
    ));
    write(
        &dir,
        "CLAUDE.md",
        &format!("{once}\n## After\n\nMore prose.\n"),
    )?;
    let config = read(&dir, "rulebearing.yaml")?;
    write(
        &dir,
        "rulebearing.yaml",
        &config.replace("Move the shared type", "Move the type"),
    )?;
    assert_eq!(code(&run(&dir, &args)?), Some(0));
    let twice = read(&dir, "CLAUDE.md")?;
    assert!(twice.starts_with("# Working agreement\n\nHand-written.\n\n"));
    assert!(twice.ends_with("<!-- rulebearing:agents-md:end -->\n\n## After\n\nMore prose.\n"));
    assert!(twice.contains("Fix: Move the type") && !twice.contains("Move the shared type"));
    let verify = run(
        &dir,
        &[
            "docs",
            "--format",
            "agents-md",
            "--out",
            "CLAUDE.md",
            "--verify",
        ],
    )?;
    assert_eq!(code(&verify), Some(0));
    clean(&dir);
    Ok(())
}

#[test]
fn links_are_relative_to_the_output_file() -> Result {
    let dir = tree("docs-relative")?;
    let out = "docs/guide/AGENTS.md";
    assert_eq!(
        code(&run(
            &dir,
            &["docs", "--format", "agents-md", "--out", out]
        )?),
        Some(0)
    );
    assert!(read(&dir, out)?.contains("[adr:0010](../adr/0010-domain-independent.md)"));
    let elsewhere = run(
        &dir,
        &["docs", "--format", "agents-md", "--adr-dir", "decisions"],
    )?;
    assert!(
        stdout(&elsewhere).contains("Decision: `adr:0010`."),
        "no record there, no link"
    );
    clean(&dir);
    Ok(())
}

#[test]
fn contributing_is_a_table_of_rule_sentence_fix_and_decision() -> Result {
    let dir = tree("docs-contributing")?;
    let config = read(&dir, "rulebearing.yaml")?;
    write(
        &dir,
        "rulebearing.yaml",
        &config.replace(
            "fix: Move the shared type into src/domain",
            "fix: \"Move the shared type into src/domain | or invert the edge\"",
        ),
    )?;
    let output = run(&dir, &["docs", "--format", "contributing"])?;
    assert_eq!(code(&output), Some(0));
    let text = stdout(&output);
    assert!(text.starts_with("<!-- rulebearing:contributing:begin -->\n## What a rulebearing error means and how to fix it\n"));
    assert!(text.contains(
        "| Rule | What it means | How to fix it | Decision |\n| --- | --- | --- | --- |\n"
    ));
    assert!(text.contains(
        "| `domain-not-to-web` | Files matching `src/domain/` may not import files matching `src/web/`. | Move the shared type into src/domain \\| or invert the edge | [adr:0010](docs/adr/0010-domain-independent.md) |"
    ), "{text}");
    assert!(text.contains("| `domain-web-edges` |"));
    clean(&dir);
    Ok(())
}

#[test]
fn skill_teaches_the_families_the_commands_and_the_reporter() -> Result {
    let dir = tree("docs-skill")?;
    let output = run(&dir, &["docs", "--format", "skill"])?;
    assert_eq!(code(&output), Some(0));
    let text = stdout(&output);
    assert!(text.starts_with("---\nname: rulebearing-architecture\ndescription: "));
    for needle in [
        "- `forbidden` (1): dependency-cruiser style",
        "- `ratchets` (1):",
        "`rulebearing cruise --output-type agent`",
        "`rulebearing can-import <from> <to>`",
        "`rulebearing explain <rule>`",
        "`rulebearing impact <file>`",
        "`rulebearing place --imports a,b --imported-by c --language <language>`",
        "`rulebearing test`",
        "## Reading the `agent` reporter",
        AGENTS_LINE,
    ] {
        assert!(text.contains(needle), "{needle}\n{text}");
    }
    assert!(
        !text.contains("Both styles are in force"),
        "only one style here"
    );

    // With element rules beside the dependency rules, both styles are taught (the "Stays" note).
    write(
        &dir,
        "rulebearing.yaml",
        &format!(
            "{}  elements:\n    - name: services-are-sealed\n      comment: adr:0010\n      select: {{ kind: class, where: {{ haveNameEndingWith: Service }} }}\n      should: {{ beSealed: true }}\n",
            crate::common::CONFIG
        ),
    )?;
    let skill = run(
        &dir,
        &[
            "docs",
            "--format",
            "skill",
            "--out",
            ".claude/skills/rulebearing/SKILL.md",
        ],
    )?;
    assert_eq!(code(&skill), Some(0), "{}", stderr(&skill));
    let file = read(&dir, ".claude/skills/rulebearing/SKILL.md")?;
    assert!(file.contains("- `elements` (1): ArchUnitNET style"));
    assert!(file.contains("Both styles are in force"));
    assert!(file.contains("### Element rules over `class`"));
    assert!(file.contains(
        "- **services-are-sealed** (elements, error): Every `class` where `haveNameEndingWith \"Service\"` must satisfy `beSealed`. Decision: [adr:0010](../../../docs/adr/0010-domain-independent.md)."
    ), "{file}");
    clean(&dir);
    Ok(())
}
