//! `decisions [--json]` and `decisions new --title T --rules r1,r2`.
//!
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//! - Source: [design § The agentic engineering hat](../../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two)

use crate::common::{CONFIG, Result, clean, code, json, read, run, stderr, stdout, tree, write};

#[test]
fn decisions_lists_the_links_and_fails_a_dangling_adr() -> Result {
    let dir = tree("decisions")?;
    let output = run(&dir, &["decisions"])?;
    assert_eq!(code(&output), Some(0), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "rule               family     decision     record
domain-not-to-web  forbidden  adr:0010     docs/adr/0010-domain-independent.md
domain-web-edges   ratchets   plan:wave-1  not written
"
    );
    let listed = json(&run(&dir, &["decisions", "--json"])?)?;
    assert_eq!(listed["dangling"], 0);
    assert_eq!(
        listed["decisions"][0]["file"],
        "docs/adr/0010-domain-independent.md"
    );

    write(
        &dir,
        "rulebearing.yaml",
        &CONFIG.replace("adr:0010\"", "adr:0010 adr:0099\""),
    )?;
    let dangling = run(&dir, &["decisions"])?;
    assert_eq!(code(&dangling), Some(1));
    assert!(
        stdout(&dangling).contains("adr:0099     MISSING under docs/adr"),
        "{}",
        stdout(&dangling)
    );
    assert!(stderr(&dangling).contains("rule `domain-not-to-web` cites adr:0099"));
    let as_json = run(&dir, &["decisions", "--json"])?;
    assert_eq!(code(&as_json), Some(1));
    assert_eq!(json(&as_json)?["dangling"], 1);
    let elsewhere = run(&dir, &["decisions", "--adr-dir", "decisions"])?;
    assert_eq!(code(&elsewhere), Some(1), "no records there at all");
    clean(&dir);
    Ok(())
}

#[test]
fn decisions_new_scaffolds_the_next_record_from_the_template() -> Result {
    let dir = tree("decisions-new")?;
    let output = run(
        &dir,
        &[
            "decisions",
            "new",
            "--title",
            "Keep the web layer out of the domain",
            "--rules",
            "domain-not-to-web,domain-web-edges",
        ],
    )?;
    assert_eq!(code(&output), Some(0), "{}", stderr(&output));
    let path = "docs/adr/0011-keep-the-web-layer-out-of-the-domain.md";
    assert_eq!(
        stdout(&output),
        format!(
            "wrote {path}\nadd adr:0011 to the comment of domain-not-to-web, domain-web-edges\n"
        )
    );
    let record = read(&dir, path)?;
    assert!(record.starts_with("# ADR-0011: Keep the web layer out of the domain\n\n- **Status:** Proposed\n- **Date:** 2026-09-24\n"));
    assert!(record.contains(
        "- **Enforced by:** `domain-not-to-web`, `domain-web-edges` in `rulebearing.yaml`, each citing `adr:0011` in its `comment`\n"
    ));
    for heading in [
        "## Context",
        "## Decision",
        "## Consequences",
        "## Alternatives considered",
    ] {
        assert!(record.contains(heading), "{heading}");
    }
    let next = run(
        &dir,
        &[
            "decisions",
            "new",
            "--title",
            "Second",
            "--rules",
            "domain-not-to-web",
        ],
    )?;
    assert!(stdout(&next).starts_with("wrote docs/adr/0012-second.md\n"));

    let unknown = run(
        &dir,
        &["decisions", "new", "--title", "T", "--rules", "nope"],
    )?;
    assert_eq!(code(&unknown), Some(3));
    assert!(stderr(&unknown).contains("no rule `nope`"));
    let untitled = run(
        &dir,
        &[
            "decisions",
            "new",
            "--title",
            "!!!",
            "--rules",
            "domain-not-to-web",
        ],
    )?;
    assert_eq!(code(&untitled), Some(3));
    let no_rules = run(&dir, &["decisions", "new", "--title", "T"])?;
    assert_eq!(code(&no_rules), Some(3), "--rules is required");
    clean(&dir);
    Ok(())
}
