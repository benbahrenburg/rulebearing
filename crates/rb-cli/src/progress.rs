//! `--progress`: what a run is doing, on stderr.
//!
//! - Coverage: [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   row `progress` (`none`, `cli-feedback`, `performance-log`, `ndjson`)
//! - Plan: [Wave 1, Step 13](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-13-rb-cli-cruise-fmt-exit-codes-flags-1d),
//!   [Step 20](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-20-performance-measurement-for-nfr-perf-01-1g-started-in-1c)
//!   (the stage split a performance miss is attributed to)
//! - Requirement: [NFR-PERF-01](../../../docs/prd.md#nfr-perf-01)

use std::fmt::Write as _;
use std::time::Instant;

use crate::cli::ProgressType;

/// Collects stage timings and renders them in the chosen format.
#[derive(Debug)]
pub struct Progress {
    kind: ProgressType,
    started: Instant,
    last: Instant,
    out: String,
}

impl Progress {
    /// A progress writer; `None` writes nothing.
    pub fn new(kind: Option<ProgressType>) -> Self {
        let now = Instant::now();
        Self {
            kind: kind.unwrap_or(ProgressType::None),
            started: now,
            last: now,
            out: String::new(),
        }
    }

    /// Marks the end of `stage`.
    pub fn stage(&mut self, stage: &str) {
        let now = Instant::now();
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        let (took, total) = (ms(now - self.last), ms(now - self.started));
        self.last = now;
        let _ = match self.kind {
            ProgressType::None => Ok(()),
            ProgressType::CliFeedback => writeln!(self.out, "  {stage} ..."),
            ProgressType::PerformanceLog => {
                writeln!(self.out, "{took:>9.1}ms {total:>9.1}ms  {stage}")
            }
            ProgressType::Ndjson => writeln!(
                self.out,
                "{}",
                serde_json::json!({ "stage": stage, "elapsedMs": (took * 10.0).round() / 10.0, "totalMs": (total * 10.0).round() / 10.0 })
            ),
        };
    }

    /// What was written, for stderr; `performance-log` gets a header.
    pub fn finish(self) -> String {
        match self.kind {
            ProgressType::PerformanceLog if !self.out.is_empty() => {
                format!("  elapsed      total  stage\n{}", self.out)
            }
            _ => self.out,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_format() {
        for (kind, needle) in [
            (Some(ProgressType::CliFeedback), "  extract ...\n"),
            (Some(ProgressType::PerformanceLog), "ms  extract\n"),
            (Some(ProgressType::Ndjson), "\"stage\":\"extract\""),
        ] {
            let mut p = Progress::new(kind);
            p.stage("extract");
            let out = p.finish();
            assert!(out.contains(needle), "{kind:?}: {out}");
        }
        let mut silent = Progress::new(None);
        silent.stage("x");
        assert_eq!(silent.finish(), "");
        assert!(
            Progress::new(Some(ProgressType::PerformanceLog))
                .finish()
                .is_empty()
        );
    }
}
