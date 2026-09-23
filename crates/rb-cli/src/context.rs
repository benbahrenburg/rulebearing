//! What a command needs from its environment, passed in rather than read from globals, so every
//! command runs the same way in a test as from a terminal.
//!
//! - Plan: [Wave 1, Step 13](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-13-rb-cli-cruise-fmt-exit-codes-flags-1d)
//! - Requirement: [FR-CORE-07](../../../docs/prd.md#fr-core-07) (hermetic, deterministic runs)

use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;

/// The environment of one invocation.
pub struct Context<'a> {
    /// The working directory; relative paths are resolved against it.
    pub cwd: PathBuf,
    /// Standard input, read only by the commands that take `-`.
    pub stdin: &'a mut dyn Read,
    /// Today, for `expires`.
    pub today: NaiveDate,
    /// Now, ISO 8601 without the trailing `Z`, for the `teamcity` reporter and receipts.
    pub timestamp: String,
    /// Whether stdout is a terminal that takes colour (`--color auto`).
    pub color_terminal: bool,
}

impl Context<'_> {
    /// Reads all of standard input.
    ///
    /// # Errors
    /// The I/O error.
    pub fn read_stdin(&mut self) -> std::io::Result<String> {
        let mut text = String::new();
        self.stdin.read_to_string(&mut text)?;
        Ok(text)
    }

    /// A path relative to the working directory.
    pub fn resolve(&self, path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref();
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        }
    }
}

/// Today and now from the clock, or from `SOURCE_DATE_EPOCH` when set, so a reproducible build
/// can pin them.
pub fn clock() -> (NaiveDate, String) {
    let now = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
        .unwrap_or_else(chrono::Utc::now);
    (
        now.date_naive(),
        now.format("%Y-%m-%dT%H:%M:%S%.3f").to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_resolve_against_the_working_directory() {
        let mut empty: &[u8] = b"in";
        let mut ctx = Context {
            cwd: PathBuf::from("/repo"),
            stdin: &mut empty,
            today: NaiveDate::default(),
            timestamp: String::new(),
            color_terminal: false,
        };
        assert_eq!(ctx.resolve("a/b"), PathBuf::from("/repo/a/b"));
        assert_eq!(ctx.resolve("/x"), PathBuf::from("/x"));
        assert_eq!(ctx.read_stdin().ok().as_deref(), Some("in"));
        let (_, stamp) = clock();
        assert_eq!(stamp.len(), 23);
    }
}
