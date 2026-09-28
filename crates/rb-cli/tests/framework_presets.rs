//! The framework presets: each one's examples pass `rulebearing test`, each is clean under
//! `config lint --require-comment-token`, `extends: rulebearing:<name>` resolves, and
//! `init --preset <name>` on a small fixture writes a configuration that passes, byte-compared.
//! What each preset file must carry (tokens, fixes, examples, no options) is
//! `crates/rb-config/tests/presets.rs`.
//!
//! - Plan: [Wave 3, Step 11](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#23-steps-for-sub-wave-3c-presets-lifecycle-fields-snapshot-and-changelog)
//!   ("`rulebearing test` on each preset's examples"; "`init --preset <name>` and
//!   `extends: rulebearing:<name>` resolve them; they are off by default")
//! - Source: [design § The developer relations hat](../../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo)
//!   ("off by default, each a documented opinion")
//! - Coverage: [coverage § Command line](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line),
//!   row `--init`
//! - Requirements: [FR-CLI-08](../../../docs/prd.md#fr-cli-08), [FR-REACH-04](../../../docs/prd.md#fr-reach-04)
//! - Fixtures: [`fixtures/presets`](fixtures/presets/README.md), and
//!   [`fixtures/init-layers`](fixtures/init-layers/PROVENANCE.md) for `clean-architecture`
//!
//! Each fixture's expected proposal is `fixtures/presets/<name>/rulebearing.yaml`; regenerate
//! deliberately with `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-cli --test framework_presets` and
//! explain the diff in review.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

/// Each framework preset and the number of rules it declares.
const PRESETS: &[(&str, usize)] = &[
    ("nextjs", 3),
    ("clean-architecture", 3),
    ("django", 3),
    ("fastapi", 4),
    ("vertical-slices", 2),
];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn scratch(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!(
        "rb-cli-framework-presets-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".git"))?;
    Ok(dir)
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// The init-layers solution in `dir`, each assembly where `dotnet build` puts it.
fn layered_solution(dir: &Path) -> Result<(), Box<dyn Error>> {
    let source = fixtures().join("init-layers");
    std::fs::copy(source.join("Shop.slnx"), dir.join("Shop.slnx"))?;
    copy_tree(&source.join("src"), &dir.join("src"))?;
    for project in [
        "Shop.Domain",
        "Shop.Application",
        "Shop.Infrastructure",
        "Shop.Web",
    ] {
        let bin = dir.join("src").join(project).join("bin/Debug/net10.0");
        std::fs::create_dir_all(&bin)?;
        for extension in ["dll", "pdb"] {
            let file = format!("{project}.{extension}");
            std::fs::copy(source.join("built").join(&file), bin.join(&file))?;
        }
    }
    Ok(())
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new(BIN);
    for (key, _) in std::env::vars_os() {
        let key_text = key.to_string_lossy();
        if key_text.starts_with("GIT_") || key_text == "VIRTUAL_ENV" {
            command.env_remove(key);
        }
    }
    Ok(command
        .args(args)
        .current_dir(dir)
        .env("SOURCE_DATE_EPOCH", "1790000000")
        .output()?)
}

fn ok(output: &Output) -> Result<String, Box<dyn Error>> {
    if output.status.code() == Some(0) {
        Ok(String::from_utf8(output.stdout.clone())?)
    } else {
        Err(format!(
            "exit {:?}: {}{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}

#[test]
fn every_preset_s_examples_pass_rulebearing_test() -> Result<(), Box<dyn Error>> {
    let dir = scratch("test")?;
    for (name, rules) in PRESETS {
        let config = format!("{name}.yaml");
        std::fs::write(dir.join(&config), format!("extends: rulebearing:{name}\n"))?;
        let report = ok(&run(&dir, &["test", "-c", &config])?)?;
        assert!(
            report.ends_with(&format!(
                "\n{rules} rule(s) with examples, 0 failing; 0 rule(s) without examples\n"
            )),
            "{name}: {report}"
        );
        assert!(!report.contains("FAIL"), "{name}: {report}");
        // Every rule has examples of both kinds: at least one edge it flags and one it allows.
        let flags = report.matches("  ok   flags ").count();
        let allows = report.matches("  ok   allows ").count();
        assert!(flags >= *rules && allows >= *rules, "{name}: {report}");
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn every_preset_is_clean_under_config_lint_with_decision_tokens() -> Result<(), Box<dyn Error>> {
    let dir = scratch("lint")?;
    for (name, _) in PRESETS {
        let config = format!("{name}.yaml");
        std::fs::write(dir.join(&config), format!("extends: rulebearing:{name}\n"))?;
        let lint = ok(&run(
            &dir,
            &["config", "lint", "-c", &config, "--require-comment-token"],
        )?)?;
        assert_eq!(lint, "config lint: no findings\n", "{name}");
    }
    // The control: a rule of the repository's own without a token is reported beside them.
    std::fs::write(
        dir.join("bare.yaml"),
        "extends: rulebearing:nextjs\nrules:\n  dependencies:\n    forbidden:\n      - name: bare\n        comment: \"No token here.\"\n        fix: \"Import b through its index.\"\n        severity: error\n        from: { path: \"^a/\" }\n        to: { path: \"^b/\" }\n",
    )?;
    let bare = run(
        &dir,
        &[
            "config",
            "lint",
            "-c",
            "bare.yaml",
            "--require-comment-token",
        ],
    )?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&bare.stdout),
        String::from_utf8_lossy(&bare.stderr)
    );
    assert!(
        text.contains("bare") && text.contains("decision token") && !text.contains("nextjs-"),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn an_unknown_preset_is_refused_and_named() -> Result<(), Box<dyn Error>> {
    let dir = scratch("unknown")?;
    let refused = run(&dir, &["init", "--dry-run", "--preset", "rails"])?;
    assert_eq!(refused.status.code(), Some(3), "clap refuses the value");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    for name in PRESETS.iter().map(|(n, _)| n) {
        assert!(stderr.contains(name), "{name}: {stderr}");
    }
    std::fs::write(dir.join("rulebearing.yaml"), "extends: rulebearing:rails\n")?;
    let unresolved = run(&dir, &["test"])?;
    assert_eq!(unresolved.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&unresolved.stderr).contains("rulebearing:vertical-slices"),
        "{}",
        String::from_utf8_lossy(&unresolved.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// `init --dry-run --preset <name>` over the fixture for `name`, compared with the committed
/// proposal; the proposal is then written and a cruise with it exits 0.
fn proposal_matches(name: &str, extra: &[&str]) -> Result<String, Box<dyn Error>> {
    let dir = scratch(name)?;
    if name == "clean-architecture" {
        layered_solution(&dir)?;
    } else {
        copy_tree(&fixtures().join("presets").join(name).join("repo"), &dir)?;
    }
    let mut args = vec!["init", "--dry-run", "--owner", "@fixture", "--preset", name];
    args.extend_from_slice(extra);
    let output = run(&dir, &args)?;
    let proposal = ok(&output)?;
    let again = ok(&run(&dir, &args)?)?;
    assert_eq!(proposal, again, "{name}: two runs write the same proposal");
    let expected = fixtures()
        .join("presets")
        .join(name)
        .join("rulebearing.yaml");
    if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&expected, &proposal)?;
    } else {
        let committed = std::fs::read_to_string(&expected).map_err(|e| {
            format!(
                "{}: {e}; run with RB_UPDATE_SNAPSHOTS=1",
                expected.display()
            )
        })?;
        assert_eq!(
            proposal, committed,
            "{name}: the proposal changed; if that was intended, regenerate with RB_UPDATE_SNAPSHOTS=1"
        );
    }
    std::fs::write(dir.join("rulebearing.yaml"), &proposal)?;
    let summary = String::from_utf8_lossy(&output.stderr).into_owned();
    let roots: Vec<String> = summary
        .lines()
        .find_map(|l| l.strip_prefix("a cruise of "))
        .and_then(|l| l.split(" with this configuration").next())
        .map(|r| r.split(' ').map(str::to_owned).collect())
        .unwrap_or_default();
    let mut cruise = vec!["cruise".to_owned()];
    cruise.extend(roots);
    let cruise: Vec<&str> = cruise.iter().map(String::as_str).collect();
    ok(&run(&dir, &cruise)?)?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(proposal)
}

#[test]
fn init_preset_nextjs_extends_it_beside_the_language() -> Result<(), Box<dyn Error>> {
    let proposal = proposal_matches("nextjs", &[])?;
    assert!(
        proposal.contains(
            "extends: [rulebearing:typescript, rulebearing:recommended, rulebearing:nextjs]\n"
        ),
        "{proposal}"
    );
    // src/lib/links.ts calls the route handler: the three rules each find it.
    for rule in [
        "nextjs-no-import-of-api-routes",
        "nextjs-no-import-of-route-entries",
        "nextjs-shared-code-not-to-routes",
    ] {
        let edge = format!(
            "\"from\":\"src/lib/links.ts\",\"to\":\"src/app/api/items/route.ts\",\"rule\":{{\"name\":\"{rule}\""
        );
        assert!(proposal.contains(&edge), "{rule}: {proposal}");
    }
    // Not findings: the component's `import type` of the handler (erased), its import of
    // server/ (a Server Component under the App Router), and the dashboard route's colocated
    // components/ and lib/ importing each other.
    for from in [
        "src/components/ItemCount.tsx",
        "src/app/dashboard/components/Chart.tsx",
        "src/app/dashboard/lib/data.ts",
    ] {
        assert!(
            !proposal.contains(&format!("\"type\":\"dependency\",\"from\":\"{from}\"")),
            "{from}: {proposal}"
        );
    }
    Ok(())
}

#[test]
fn init_preset_clean_architecture_agrees_with_the_namespace_layers() -> Result<(), Box<dyn Error>> {
    let proposal = proposal_matches("clean-architecture", &[])?;
    assert!(
        proposal.contains(
            "extends: [rulebearing:dotnet, rulebearing:recommended, rulebearing:clean-architecture]\n"
        ),
        "{proposal}"
    );
    // Shop.Application's use of Shop.Infrastructure is the one finding, by both rules.
    for rule in [
        "application-not-to-outer-layers",
        "clean-application-not-to-outer-layers",
    ] {
        assert!(
            proposal.contains(&format!("\"rule\":{{\"name\":\"{rule}\"")),
            "{rule}: {proposal}"
        );
    }
    Ok(())
}

#[test]
fn init_preset_django_with_python_named() -> Result<(), Box<dyn Error>> {
    let proposal = proposal_matches("django", &["--preset", "python"])?;
    assert!(
        proposal.contains(
            "extends: [rulebearing:python, rulebearing:recommended, rulebearing:django]\n"
        ),
        "{proposal}"
    );
    assert!(
        proposal.contains("\"rule\":{\"name\":\"django-views-only-from-urls\""),
        "forms.py importing a view is baselined: {proposal}"
    );
    Ok(())
}

#[test]
fn init_preset_fastapi_leaves_out_what_matches_nothing() -> Result<(), Box<dyn Error>> {
    let proposal = proposal_matches("fastapi", &[])?;
    // There is no repository layer, so its rule is left out by name, not dropped silently.
    assert!(
        proposal.contains(
            "      - name: fastapi-repositories-not-to-services-or-routers\n        severity: ignore\n"
        ),
        "{proposal}"
    );
    assert!(
        proposal.contains("\"rule\":{\"name\":\"fastapi-services-not-to-routers\""),
        "{proposal}"
    );
    Ok(())
}

#[test]
fn init_preset_vertical_slices_fences_the_slices() -> Result<(), Box<dyn Error>> {
    let proposal = proposal_matches("vertical-slices", &[])?;
    assert!(
        proposal.contains("\"rule\":{\"name\":\"slices-are-independent\""),
        "{proposal}"
    );
    assert!(
        proposal.contains("# Framework presets named with --preset, each an opinion that is off unless named: rulebearing:vertical-slices.\n"),
        "{proposal}"
    );
    Ok(())
}

#[test]
fn no_framework_preset_is_proposed_unless_named() -> Result<(), Box<dyn Error>> {
    // The Next.js fixture has a next.config.mjs; init finds Next.js and still proposes no
    // framework preset, since each is an opinion.
    let dir = scratch("unnamed")?;
    copy_tree(&fixtures().join("presets/nextjs/repo"), &dir)?;
    let proposal = ok(&run(&dir, &["init", "--dry-run", "--owner", "@fixture"])?)?;
    assert!(
        proposal.contains("extends: [rulebearing:typescript, rulebearing:recommended]\n"),
        "{proposal}"
    );
    assert!(!proposal.contains("nextjs"), "{proposal}");
    // `cruise --init --preset` takes the framework presets too.
    ok(&run(&dir, &["cruise", "--init", "--preset", "nextjs"])?)?;
    let written = std::fs::read_to_string(dir.join("rulebearing.yaml"))?;
    assert!(
        written.contains(
            "extends: [rulebearing:typescript, rulebearing:recommended, rulebearing:nextjs]\n"
        ),
        "{written}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
