//! `.graph/usage/`: the salt, one payload per calendar month, and the month of the last monthly
//! report.
//!
//! - Decision: [ADR-0075](../../../../docs/adr/0075-usage-counts-are-counted-and-printed-but-not-sent.md)
//!   (storage, cadence, retention)
//! - Plan: [plan 0004, Step 13](../../../../docs/plans/pending/0004-wave-4-reach.md#25-steps-for-sub-wave-4e-usage-counts-and-the-adoption-review),
//!   [§ 1.7](../../../../docs/plans/pending/0004-wave-4-reach.md#17-decisions-applied-and-decisions-this-wave-must-make)
//!   (at most once per calendar month)
//! - Requirements: [FR-REACH-03](../../../../docs/prd.md#fr-reach-03), [NFR-SEC-01](../../../../docs/prd.md#nfr-sec-01)
//! - Retention: [docs/usage-counts.md](../../../../docs/usage-counts.md#retention)
//!
//! | File | Holds |
//! | --- | --- |
//! | `salt` | 64 hex characters, written once |
//! | `<YYYY-MM>.json` | that month's [`Payload`], already hashed |
//! | `state.json` | `{ "lastReport": "YYYY-MM" }`, the month the last monthly report was made in |
//!
//! Every file is written whole to a temporary name and renamed over the old one, so a reader never
//! sees half a payload. Two runs that count at the same moment can lose one increment; a count is
//! a signal, not a ledger.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::payload::{Payload, is_period, rule_key};

/// The folder under the repository root.
pub const FOLDER: &str = ".graph/usage";

/// The salt's file name.
pub const SALT_FILE: &str = "salt";

/// The cadence state's file name.
pub const STATE_FILE: &str = "state.json";

/// Why the usage folder could not be read or written.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("{}: {reason}", file.display())]
pub struct UsageError {
    /// The file.
    pub file: PathBuf,
    /// What went wrong, and what to do.
    pub reason: String,
}

impl UsageError {
    fn at(file: &Path, reason: impl Into<String>) -> Self {
        Self {
            file: file.to_path_buf(),
            reason: reason.into(),
        }
    }
}

/// The cadence state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    /// The month the last monthly report was made in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_report: Option<String>,
}

/// What one counted run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// This run created the salt: counting started in this repository now.
    pub started: bool,
    /// The closed month this run reported, at most once per calendar month.
    pub report: Option<Payload>,
}

/// The usage folder of one repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// The folder under the repository at `root`.
    pub fn of(root: &Path) -> Self {
        Self {
            dir: root.join(FOLDER),
        }
    }

    /// The folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file holding `period`'s payload.
    pub fn month_file(&self, period: &str) -> PathBuf {
        self.dir.join(format!("{period}.json"))
    }

    /// Counts one run of `command`, with the rules that fired in it, into `period`, after the
    /// monthly report when one is due.
    ///
    /// # Errors
    /// [`UsageError`] naming the file that could not be read or written.
    pub fn record(
        &self,
        period: &str,
        command: &str,
        fired: &BTreeSet<String>,
    ) -> Result<Recorded, UsageError> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| UsageError::at(&self.dir, format!("cannot create the folder: {e}")))?;
        let (salt, started) = self.salt()?;
        let report = self.report(period)?;
        let mut payload = self.read(period)?.unwrap_or_else(|| Payload::empty(period));
        env!("CARGO_PKG_VERSION").clone_into(&mut payload.version);
        payload.count_command(command);
        for rule in fired {
            payload.count_rule(rule_key(rule, &salt));
        }
        write(&self.month_file(period), &payload.to_text())?;
        Ok(Recorded { started, report })
    }

    /// The salt, created when there is none; `true` when this call created it.
    ///
    /// # Errors
    /// [`UsageError`] when the salt cannot be read or written, or is not 64 hex characters.
    pub fn salt(&self) -> Result<(String, bool), UsageError> {
        let file = self.dir.join(SALT_FILE);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
        {
            Ok(mut handle) => {
                let salt = fresh_salt();
                handle
                    .write_all(format!("{salt}\n").as_bytes())
                    .map_err(|e| UsageError::at(&file, format!("cannot write the salt: {e}")))?;
                Ok((salt, true))
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let text = std::fs::read_to_string(&file)
                    .map_err(|e| UsageError::at(&file, format!("cannot read the salt: {e}")))?;
                let salt = text.trim();
                if is_salt(salt) {
                    Ok((salt.to_owned(), false))
                } else {
                    Err(UsageError::at(
                        &file,
                        format!(
                            "is not a usage-counts salt (64 hex characters); delete {FOLDER} to start counting again"
                        ),
                    ))
                }
            }
            Err(e) => Err(UsageError::at(
                &file,
                format!("cannot create the salt: {e}"),
            )),
        }
    }

    /// `period`'s payload, or `None` when nothing was counted in it.
    ///
    /// # Errors
    /// [`UsageError`] when the file exists but cannot be read or is not a payload of `period`.
    pub fn read(&self, period: &str) -> Result<Option<Payload>, UsageError> {
        let file = self.month_file(period);
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(UsageError::at(&file, format!("cannot read it: {e}"))),
        };
        let payload = Payload::parse(&text).map_err(|reason| {
            UsageError::at(
                &file,
                format!(
                    "is not a usage-counts payload ({reason}); delete it to start the month again"
                ),
            )
        })?;
        if payload.period != period {
            return Err(UsageError::at(
                &file,
                format!(
                    "holds the counts of {}, not {period}; delete it to start the month again",
                    payload.period
                ),
            ));
        }
        Ok(Some(payload))
    }

    /// The months before `period` that have a payload file, oldest first.
    ///
    /// # Errors
    /// [`UsageError`] when the folder cannot be listed.
    pub fn closed(&self, period: &str) -> Result<Vec<String>, UsageError> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(UsageError::at(&self.dir, format!("cannot list it: {e}"))),
        };
        let months: BTreeSet<String> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
            .filter_map(|name| name.strip_suffix(".json").map(str::to_owned))
            .filter(|month| is_period(month) && month.as_str() < period)
            .collect();
        Ok(months.into_iter().collect())
    }

    /// The cadence state, empty when there is none.
    ///
    /// # Errors
    /// [`UsageError`] when the file exists but cannot be read or parsed.
    pub fn state(&self) -> Result<State, UsageError> {
        let file = self.dir.join(STATE_FILE);
        match std::fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| {
                UsageError::at(
                    &file,
                    format!("is not the usage-counts state ({e}); delete it"),
                )
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
            Err(e) => Err(UsageError::at(&file, format!("cannot read it: {e}"))),
        }
    }

    /// The monthly report, when one is due in `period`: the latest closed month's payload. The
    /// closed months' files are deleted and `period` is recorded, so the next report is due in
    /// the next calendar month at the earliest.
    fn report(&self, period: &str) -> Result<Option<Payload>, UsageError> {
        if self.state()?.last_report.as_deref() == Some(period) {
            return Ok(None);
        }
        let closed = self.closed(period)?;
        let Some(latest) = closed.last() else {
            return Ok(None);
        };
        let payload = self.read(latest)?;
        for month in &closed {
            let file = self.month_file(month);
            std::fs::remove_file(&file)
                .map_err(|e| UsageError::at(&file, format!("cannot delete it: {e}")))?;
        }
        let state = State {
            last_report: Some(period.to_owned()),
        };
        let text = serde_json::to_string_pretty(&state).unwrap_or_default();
        write(&self.dir.join(STATE_FILE), &format!("{text}\n"))?;
        Ok(payload)
    }
}

/// Whether `text` is a salt [`fresh_salt`] could have written.
pub fn is_salt(text: &str) -> bool {
    text.len() == 64
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// 32 bytes from the operating system's randomness, as 64 hex characters: the keys `std` seeds
/// each thread's `RandomState` with, drawn through four hashers, mixed through SHA-256 with the
/// clock and the process id.
pub fn fresh_salt() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};
    let mut digest = Sha256::new();
    for round in 0_u8..4 {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u8(round);
        digest.update(hasher.finish().to_le_bytes());
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    digest.update(nanos.to_le_bytes());
    digest.update(std::process::id().to_le_bytes());
    digest
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Writes `text` to `file` whole: to a temporary name beside it, then renamed over it.
fn write(file: &Path, text: &str) -> Result<(), UsageError> {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary = file.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, text)
        .map_err(|e| UsageError::at(file, format!("cannot write it: {e}")))?;
    std::fs::rename(&temporary, file).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        UsageError::at(file, format!("cannot write it: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("rb-usage-store-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(&dir);
            Self(dir)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fired(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn the_first_count_creates_the_salt_and_the_month() -> Result<(), UsageError> {
        let dir = Dir::new("first");
        let store = Store::of(&dir.0);
        assert_eq!(store.dir(), dir.0.join(".graph/usage"));
        assert_eq!(store.read("2026-10")?, None);
        let first = store.record("2026-10", "cruise", &fired(&["no-circular", "mine"]))?;
        assert_eq!(
            first,
            Recorded {
                started: true,
                report: None
            }
        );
        let second = store.record("2026-10", "cruise", &fired(&["mine"]))?;
        assert!(!second.started, "the salt is made once");
        let payload = store
            .read("2026-10")?
            .unwrap_or_else(|| Payload::empty("x"));
        assert_eq!(payload.commands["cruise"], 2);
        assert_eq!(payload.rules["no-circular"], 1);
        let (salt, created) = store.salt()?;
        assert!(!created && is_salt(&salt));
        assert_eq!(payload.rules[&rule_key("mine", &salt)], 2);
        let on_disk = std::fs::read_to_string(store.month_file("2026-10")).unwrap_or_default();
        assert!(
            !on_disk.contains("mine"),
            "no rule name outside the library on disk"
        );
        assert_eq!(on_disk, payload.to_text());
        Ok(())
    }

    #[test]
    fn the_monthly_report_comes_once_with_the_latest_closed_month() -> Result<(), UsageError> {
        let dir = Dir::new("cadence");
        let store = Store::of(&dir.0);
        store.record("2026-08", "cruise", &BTreeSet::new())?;
        store.record("2026-09", "cruise", &BTreeSet::new())?;
        store.record("2026-09", "rules", &BTreeSet::new())?;
        // 2026-09's first run reported 2026-08 already.
        assert_eq!(store.state()?.last_report.as_deref(), Some("2026-09"));
        assert_eq!(store.closed("2026-10")?, ["2026-09"]);
        let october = store.record("2026-10", "cruise", &BTreeSet::new())?;
        let report = october.report.unwrap_or_else(|| Payload::empty("none"));
        assert_eq!(report.period, "2026-09");
        assert_eq!(report.commands.get("rules"), Some(&1));
        assert!(!store.month_file("2026-09").exists(), "deleted once shown");
        assert_eq!(store.state()?.last_report.as_deref(), Some("2026-10"));
        let again = store.record("2026-10", "cruise", &BTreeSet::new())?;
        assert_eq!(again.report, None, "at most once per calendar month");
        // A month with nothing closed reports nothing and records nothing.
        let fresh = Dir::new("cadence-none");
        let store = Store::of(&fresh.0);
        assert_eq!(
            store.record("2026-10", "cruise", &BTreeSet::new())?.report,
            None
        );
        assert_eq!(store.state()?, State::default());
        Ok(())
    }

    #[test]
    fn several_closed_months_report_the_latest_and_delete_them_all() -> Result<(), UsageError> {
        let dir = Dir::new("skipped");
        let store = Store::of(&dir.0);
        std::fs::create_dir_all(store.dir())
            .map_err(|e| UsageError::at(store.dir(), e.to_string()))?;
        for month in ["2026-05", "2026-07"] {
            let mut payload = Payload::empty(month);
            payload.count_command(month);
            write(&store.month_file(month), &payload.to_text())?;
        }
        // Neither a future month nor a stray file is a closed month.
        write(
            &store.month_file("2027-01"),
            &Payload::empty("2027-01").to_text(),
        )?;
        write(&store.dir().join("notes.json"), "{}")?;
        let recorded = store.record("2026-10", "cruise", &BTreeSet::new())?;
        assert_eq!(
            recorded.report.map(|p| p.period),
            Some("2026-07".to_owned())
        );
        assert!(!store.month_file("2026-05").exists());
        assert!(store.month_file("2027-01").exists());
        assert!(store.dir().join("notes.json").exists());
        Ok(())
    }

    #[test]
    fn a_damaged_file_is_named_with_the_way_out() -> Result<(), UsageError> {
        let dir = Dir::new("damaged");
        let store = Store::of(&dir.0);
        std::fs::create_dir_all(store.dir())
            .map_err(|e| UsageError::at(store.dir(), e.to_string()))?;
        write(&store.month_file("2026-10"), "not json")?;
        let error = store.record("2026-10", "cruise", &BTreeSet::new()).err();
        assert!(
            error.as_ref().is_some_and(|e| e.file.ends_with("2026-10.json")
                && e.reason.contains("delete it")),
            "{error:?}"
        );
        write(
            &store.month_file("2026-10"),
            &Payload::empty("2026-09").to_text(),
        )?;
        assert!(
            store
                .read("2026-10")
                .is_err_and(|e| e.reason.contains("holds the counts of 2026-09"))
        );
        write(&store.dir().join(SALT_FILE), "short\n")?;
        assert!(store.salt().is_err_and(|e| e.reason.contains("64 hex")));
        write(&store.dir().join(STATE_FILE), "{ \"lastReport\": 1 }")?;
        assert!(store.state().is_err_and(|e| e.reason.contains("state")));
        let message = UsageError::at(Path::new("a/b"), "why").to_string();
        assert_eq!(message, "a/b: why");
        Ok(())
    }

    #[test]
    fn a_folder_that_cannot_be_made_is_an_error_not_a_panic() {
        let dir = Dir::new("blocked");
        let _ = std::fs::write(dir.0.join(".graph"), "a file where the folder goes");
        let store = Store::of(&dir.0);
        assert!(store.record("2026-10", "cruise", &BTreeSet::new()).is_err());
        // Windows reports a path through a file as not found, which lists as no closed month.
        #[cfg(unix)]
        assert!(
            store
                .closed("2026-10")
                .is_err_and(|e| e.reason.contains("cannot list"))
        );
    }

    #[test]
    fn salts_are_hex_and_differ() {
        let a = fresh_salt();
        let b = fresh_salt();
        assert!(is_salt(&a) && is_salt(&b));
        assert_ne!(a, b);
        assert!(!is_salt(&"A".repeat(64)));
        assert!(!is_salt(&"a".repeat(63)));
        assert!(!is_salt(&"g".repeat(64)));
        assert!(is_salt(&"0".repeat(64)));
    }
}
