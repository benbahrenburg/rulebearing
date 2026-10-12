//! Usage counts: off by default, counted on the machine with salted rule hashes, printed, and
//! never sent.
//!
//! - Architecture: [architecture § Security posture](../../../../docs/architecture.md#security-posture)
//! - Decisions: [ADR-0075](../../../../docs/adr/0075-usage-counts-are-counted-and-printed-but-not-sent.md)
//!   (the client without the send), [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//!   (adoption is measured)
//! - Plan: [plan 0004, Step 13](../../../../docs/plans/pending/0004-wave-4-reach.md#25-steps-for-sub-wave-4e-usage-counts-and-the-adoption-review),
//!   [§ 1.7](../../../../docs/plans/pending/0004-wave-4-reach.md#17-decisions-applied-and-decisions-this-wave-must-make)
//!   ("Usage counts transport and endpoint")
//! - Requirements: [FR-REACH-03](../../../../docs/prd.md#fr-reach-03), [NFR-SEC-01](../../../../docs/prd.md#nfr-sec-01)
//! - Reader's page: [docs/usage-counts.md](../../../../docs/usage-counts.md)
//!
//! | Module | Does |
//! | --- | --- |
//! | [`payload`] | the payload, the rule keys and the public library's rule names |
//! | [`store`] | `.graph/usage/`: the salt, the months, the monthly cadence |
//!
//! A command that loads a configuration tells the context's [`Recorder`] what `telemetry` says
//! ([`crate::configure::load`]); `cruise` tells it which rules fired. After the command,
//! [`finish`] counts the run when the configuration turned counting on and the CI guard ([`gate`])
//! allows it, and adds to stderr what the run must say: that counting started, or the monthly
//! report. Nothing here opens a connection: there is no send in this release, by decision, and a
//! test fails if the binary links a network call.

pub mod payload;
pub mod store;

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::fmt::Write as _;

use rb_config::Config;
use rb_config::model::Telemetry;

use crate::context::Context;
use crate::{Outcome, RunExit};

use self::payload::{Payload, period};
use self::store::{FOLDER, Recorded, Store};

/// What every message about the opt-in says about sending.
pub const NOT_SENT: &str =
    "Sending is not available in this release (ADR-0075): nothing leaves this machine.";

/// What one invocation learned about usage counting while it ran.
#[derive(Debug, Default)]
pub struct Recorder {
    /// The run is in a CI environment ([`ci`]).
    ci: bool,
    /// `telemetry` of the configuration the command loaded, once it loaded one.
    telemetry: Cell<Option<Telemetry>>,
    /// The rules that reported a violation in this run.
    fired: RefCell<BTreeSet<String>>,
}

impl Recorder {
    /// A recorder for a run whose CI environment is `ci`.
    pub fn new(ci: bool) -> Self {
        Self {
            ci,
            ..Self::default()
        }
    }

    /// A recorder for this process: in CI when the `CI` variable says so ([`ci`]).
    pub fn from_env() -> Self {
        Self::new(ci(std::env::var("CI").ok().as_deref()))
    }

    /// Whether the run is in a CI environment.
    pub fn in_ci(&self) -> bool {
        self.ci
    }

    /// The command loaded `config`.
    pub fn saw(&self, config: &Config) {
        self.telemetry.set(Some(config.telemetry));
    }

    /// `telemetry` of the configuration the command loaded, or `None` when it loaded none.
    pub fn telemetry(&self) -> Option<Telemetry> {
        self.telemetry.get()
    }

    /// The rules `names` reported at least one violation.
    pub fn fired(&self, names: impl IntoIterator<Item = String>) {
        self.fired.borrow_mut().extend(names);
    }

    /// The rules that fired in this run, by name.
    pub fn fired_rules(&self) -> BTreeSet<String> {
        self.fired.borrow().clone()
    }
}

/// Whether the `CI` variable's value marks a CI environment: set to anything but empty, `false`
/// or `0` (any case). Most CI services set `CI=true`.
pub fn ci(value: Option<&str>) -> bool {
    value
        .map(str::trim)
        .is_some_and(|v| !v.is_empty() && !v.eq_ignore_ascii_case("false") && v != "0")
}

/// Whether a run is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// No configuration, or `telemetry.enabled` is not true: nothing is counted or written.
    Off,
    /// Turned on, but the run is in CI and `telemetry.ciAllowed` is not true: nothing is
    /// counted or written.
    Ci,
    /// Counted.
    On,
}

/// The CI guard: whether a run with `telemetry` (`None` without a configuration) is counted.
pub fn gate(telemetry: Option<Telemetry>, in_ci: bool) -> Gate {
    match telemetry {
        Some(t) if t.enabled && (!in_ci || t.ci_allowed) => Gate::On,
        Some(t) if t.enabled => Gate::Ci,
        _ => Gate::Off,
    }
}

/// After `command` ran: counts it when the gate allows, and adds to `outcome`'s stderr what the
/// run must say. A failure to count is a warning; it never changes the exit code.
pub fn finish(ctx: &Context<'_>, command: &str, outcome: &mut Outcome) {
    if gate(ctx.usage.telemetry(), ctx.usage.in_ci()) != Gate::On {
        return;
    }
    let store = Store::of(&rb_config::load::repository_root(&ctx.cwd));
    match store.record(&period(ctx.today), command, &ctx.usage.fired_rules()) {
        Ok(recorded) => outcome.stderr.push_str(&notices(&recorded)),
        Err(e) => {
            let _ = writeln!(
                outcome.stderr,
                "warning: usage counts were not recorded: {e}"
            );
        }
    }
}

/// What a counted run says on stderr: that counting started, and the monthly report when one
/// was due.
pub fn notices(recorded: &Recorded) -> String {
    let mut out = String::new();
    if recorded.started {
        let _ = writeln!(
            out,
            "rulebearing: usage counts are on for this repository (telemetry.enabled): each command and each rule that fires is counted per calendar month in {FOLDER}/, a rule outside the public library only as a salted hash; `rulebearing --usage-counts print` shows them. {NOT_SENT}"
        );
    }
    if let Some(report) = &recorded.report {
        let _ = write!(
            out,
            "rulebearing: usage counts for {}, as a send would carry them:\n{}",
            report.period,
            report.to_text()
        );
        let _ = writeln!(
            out,
            "rulebearing: {NOT_SENT} Nothing was sent, and the counts for {} are deleted from {FOLDER}/ now that they have been shown.",
            report.period
        );
    }
    out
}

/// `rulebearing --usage-counts print`: this month's payload on stdout, exactly as a send would
/// carry it. Writes nothing.
pub fn print(ctx: &Context<'_>) -> Outcome {
    let store = Store::of(&rb_config::load::repository_root(&ctx.cwd));
    let month = period(ctx.today);
    match store.read(&month) {
        Ok(Some(payload)) => Outcome::printed(payload.to_text()),
        Ok(None) => Outcome {
            stdout: Payload::empty(&month).to_text(),
            stderr: format!(
                "rulebearing: nothing counted for {month} in {FOLDER}/: usage counts are off unless the repository's configuration sets `telemetry: {{ enabled: true }}` (docs/usage-counts.md). {NOT_SENT}\n"
            ),
            code: RunExit::Violations(0).code(),
        },
        Err(e) => Outcome::failed(
            RunExit::Untrustworthy,
            format!("rulebearing --usage-counts print: {e}\n"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use chrono::NaiveDate;

    const ON: Telemetry = Telemetry {
        enabled: true,
        ci_allowed: false,
    };
    const CI_TOO: Telemetry = Telemetry {
        enabled: true,
        ci_allowed: true,
    };

    #[test]
    fn the_ci_variable() {
        for (value, expected) in [
            (None, false),
            (Some(""), false),
            (Some("  "), false),
            (Some("false"), false),
            (Some("FALSE"), false),
            (Some("0"), false),
            (Some("true"), true),
            (Some("True"), true),
            (Some("1"), true),
            (Some("yes"), true),
        ] {
            assert_eq!(ci(value), expected, "{value:?}");
        }
    }

    #[test]
    fn the_gate_is_off_by_default_and_ci_needs_its_own_switch() {
        for (telemetry, in_ci, expected) in [
            (None, false, Gate::Off),
            (None, true, Gate::Off),
            (Some(Telemetry::default()), false, Gate::Off),
            (Some(Telemetry::default()), true, Gate::Off),
            (
                Some(Telemetry {
                    enabled: false,
                    ci_allowed: true,
                }),
                true,
                Gate::Off,
            ),
            (Some(ON), false, Gate::On),
            (Some(ON), true, Gate::Ci),
            (Some(CI_TOO), true, Gate::On),
            (Some(CI_TOO), false, Gate::On),
        ] {
            assert_eq!(gate(telemetry, in_ci), expected, "{telemetry:?} ci={in_ci}");
        }
    }

    #[test]
    fn the_recorder_keeps_what_the_run_saw() {
        let recorder = Recorder::new(true);
        assert!(recorder.in_ci());
        assert!(!Recorder::default().in_ci());
        assert_eq!(recorder.telemetry(), None);
        let config = Config {
            telemetry: ON,
            ..Config::default()
        };
        recorder.saw(&config);
        assert_eq!(recorder.telemetry(), Some(ON));
        recorder.fired(["b".to_owned(), "a".to_owned()]);
        recorder.fired(["a".to_owned()]);
        assert_eq!(
            recorder.fired_rules().into_iter().collect::<Vec<_>>(),
            ["a", "b"]
        );
        let _ = Recorder::from_env();
    }

    #[test]
    fn notices_say_nothing_is_sent() {
        assert_eq!(
            notices(&Recorded {
                started: false,
                report: None
            }),
            ""
        );
        let started = notices(&Recorded {
            started: true,
            report: None,
        });
        assert!(started.contains("usage counts are on"), "{started}");
        assert!(started.contains("--usage-counts print"), "{started}");
        assert!(started.contains(NOT_SENT), "{started}");
        let mut report = Payload::empty("2026-09");
        report.count_command("cruise");
        let text = notices(&Recorded {
            started: false,
            report: Some(report.clone()),
        });
        assert!(text.contains(&report.to_text()), "{text}");
        assert!(text.contains("Nothing was sent"), "{text}");
        assert!(text.ends_with(".\n"));
    }

    struct Repo(PathBuf);

    impl Repo {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("rb-usage-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(dir.join(".git"));
            Self(dir)
        }

        fn run(&self, recorder: Recorder, today: NaiveDate, command: &str) -> Outcome {
            let mut empty: &[u8] = &[];
            let ctx = Context {
                cwd: self.0.clone(),
                stdin: &mut empty,
                today,
                timestamp: String::new(),
                color_terminal: false,
                warm: None,
                usage: recorder,
            };
            let mut outcome = Outcome::printed("report\n");
            finish(&ctx, command, &mut outcome);
            outcome
        }

        fn print(&self, today: NaiveDate) -> Outcome {
            let mut empty: &[u8] = &[];
            let ctx = Context {
                cwd: self.0.clone(),
                stdin: &mut empty,
                today,
                timestamp: String::new(),
                color_terminal: false,
                warm: None,
                usage: Recorder::default(),
            };
            print(&ctx)
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default()
    }

    fn with(telemetry: Telemetry, in_ci: bool) -> Recorder {
        let recorder = Recorder::new(in_ci);
        recorder.saw(&Config {
            telemetry,
            ..Config::default()
        });
        recorder
    }

    #[test]
    fn off_writes_nothing_and_ci_writes_nothing_without_its_switch() {
        let repo = Repo::new("off");
        let outcome = repo.run(Recorder::default(), day(2026, 10, 1), "cruise");
        assert_eq!(outcome, Outcome::printed("report\n"));
        let outcome = repo.run(
            with(Telemetry::default(), false),
            day(2026, 10, 1),
            "cruise",
        );
        assert_eq!(outcome, Outcome::printed("report\n"));
        let outcome = repo.run(with(ON, true), day(2026, 10, 1), "cruise");
        assert_eq!(outcome, Outcome::printed("report\n"));
        assert!(!repo.0.join(FOLDER).exists(), "no file without the opt-in");
        let counted = repo.run(with(CI_TOO, true), day(2026, 10, 1), "cruise");
        assert!(
            counted.stderr.contains("usage counts are on"),
            "{}",
            counted.stderr
        );
        assert_eq!((counted.stdout.as_str(), counted.code), ("report\n", 0));
    }

    #[test]
    fn a_counted_month_prints_and_reports_once_in_the_next() {
        let repo = Repo::new("months");
        let first = repo.run(with(ON, false), day(2026, 9, 3), "cruise");
        assert!(first.stderr.contains("usage counts are on"));
        let recorder = with(ON, false);
        recorder.fired(["no-circular".to_owned(), "team-rule".to_owned()]);
        let second = repo.run(recorder, day(2026, 9, 30), "cruise");
        assert_eq!(second.stderr, "", "said once");
        let printed = repo.print(day(2026, 9, 30));
        assert_eq!(printed.code, 0);
        assert_eq!(printed.stderr, "");
        let payload = Payload::parse(&printed.stdout).unwrap_or_else(|_| Payload::empty("x"));
        assert_eq!(payload.commands["cruise"], 2);
        assert_eq!(payload.rules["no-circular"], 1);
        assert!(!printed.stdout.contains("team-rule"));
        let october = repo.run(with(ON, false), day(2026, 10, 1), "rules");
        assert!(
            october.stderr.contains("usage counts for 2026-09"),
            "{}",
            october.stderr
        );
        assert!(
            october.stderr.contains(&printed.stdout),
            "the report is what print showed"
        );
        assert!(october.stderr.contains("Nothing was sent"));
        let later = repo.run(with(ON, false), day(2026, 10, 2), "rules");
        assert_eq!(later.stderr, "");
    }

    #[test]
    fn print_with_nothing_counted_shows_the_empty_payload_and_writes_nothing() {
        let repo = Repo::new("print-empty");
        let printed = repo.print(day(2026, 11, 5));
        assert_eq!(printed.code, 0);
        assert_eq!(printed.stdout, Payload::empty("2026-11").to_text());
        assert!(
            printed.stderr.contains("telemetry: { enabled: true }"),
            "{}",
            printed.stderr
        );
        assert!(printed.stderr.contains(NOT_SENT));
        assert!(!repo.0.join(FOLDER).exists());
    }

    #[test]
    fn a_damaged_month_is_a_warning_for_a_run_and_exit_2_for_print() {
        let repo = Repo::new("damaged");
        let folder = repo.0.join(FOLDER);
        let _ = std::fs::create_dir_all(&folder);
        let _ = std::fs::write(folder.join("2026-10.json"), "{");
        let outcome = repo.run(with(ON, false), day(2026, 10, 9), "cruise");
        assert_eq!(outcome.code, 0, "never changes the exit code");
        assert!(
            outcome
                .stderr
                .starts_with("warning: usage counts were not recorded: "),
            "{}",
            outcome.stderr
        );
        let printed = repo.print(day(2026, 10, 9));
        assert_eq!(printed.code, 2);
        assert!(
            printed.stderr.contains("2026-10.json"),
            "{}",
            printed.stderr
        );
    }
}
