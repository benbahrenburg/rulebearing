//! The ratchets a `cruise` enforces: each `rules.ratchets` entry counted against its budget file.
//!
//! - Decision: [ADR-0029](../../../docs/adr/0029-ratchets-enforced-by-cruise-and-reported-in-the-summary.md)
//!   (`summary.ratchets[]`; an exceeded ratchet is one error; a missing budget exits 2)
//! - Source: [design § The run and its consumers](../../../docs/artifacts/design.md#the-run-and-its-consumers)
//! - Plan: [Wave 1, Step 7](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-7-liveness-severity-ids-receipts-expires-ratchets-1b)
//! - Requirement: [FR-RULE-06](../../../docs/prd.md#fr-rule-06)
//!
//! The count and the comparison are `rb_rules::ratchet`'s; this module reads the budget files,
//! which only the command line may do.

use std::fmt::Write as _;

use rb_config::Config;
use rb_model::{GraphDocument, RatchetResult, RatchetStatus, VacuousRule};
use rb_rules::matchers::pattern;
use rb_rules::ratchet::{self, Verdict};

use crate::cmd::count::read_budget;
use crate::context::Context;

/// What the ratchets say about a run.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Ratchets {
    /// One result per ratchet, in configuration order.
    pub results: Vec<RatchetResult>,
    /// Ratchets whose `from` matched no module (only when liveness is on).
    pub vacuous: Vec<VacuousRule>,
}

impl Ratchets {
    /// Exceeded ratchets, each one error in the exit code.
    pub fn exceeded(&self) -> u64 {
        self.results
            .iter()
            .filter(|r| r.status == RatchetStatus::Exceeded)
            .count() as u64
    }

    /// Whether a ratchet's budget cannot be read, which makes the run untrustworthy.
    pub fn no_budget(&self) -> bool {
        self.results
            .iter()
            .any(|r| r.status == RatchetStatus::NoBudget)
    }
}

/// Counts every ratchet in `config` over `document` and reads its budget.
pub fn evaluate(
    ctx: &Context<'_>,
    config: &Config,
    document: &GraphDocument,
    liveness: bool,
) -> Ratchets {
    let mut out = Ratchets::default();
    for r in &config.rules.ratchets {
        let count = ratchet::edges(document, &r.from, &r.to).len() as u64;
        let (ceiling, status) = match read_budget(ctx, &r.budget) {
            Ok(budget) => match ratchet::verdict(count, Some(budget)) {
                Verdict::Over { .. } => (Some(budget.ceiling), RatchetStatus::Exceeded),
                _ => (Some(budget.ceiling), RatchetStatus::Held),
            },
            Err(_) => (None, RatchetStatus::NoBudget),
        };
        if liveness
            && !config.allow_empty.contains(&r.name)
            && let Some(from) = pattern(r.from.path.as_ref())
            && !document
                .modules
                .iter()
                .any(|m| rb_rules::patterns::test(&from, &m.source))
        {
            out.vacuous.push(VacuousRule::new(r.name.clone(), "from"));
        }
        out.results.push(RatchetResult {
            name: r.name.clone(),
            budget: r.budget.clone(),
            count,
            ceiling,
            status,
        });
    }
    out
}

/// The stderr lines for the ratchets that fail a run, each with its `fix`.
pub fn messages(config: &Config, ratchets: &Ratchets, strict: bool) -> String {
    let fix = |name: &str| {
        config
            .rules
            .ratchets
            .iter()
            .find(|r| r.name == name)
            .and_then(|r| r.fix.as_deref())
            .map_or_else(String::new, |f| format!(" Fix: {f}"))
    };
    let mut out = String::new();
    for r in &ratchets.results {
        match (r.status, r.ceiling) {
            (RatchetStatus::Exceeded, Some(ceiling)) => {
                let _ = writeln!(
                    out,
                    "error ratchet `{}`: {} edges, over the ceiling of {ceiling} in {}. Remove the new edges; the ceiling may only fall.{}",
                    r.name,
                    r.count,
                    r.budget,
                    fix(&r.name)
                );
            }
            (RatchetStatus::NoBudget, _) => {
                let _ = writeln!(
                    out,
                    "error ratchet `{}`: the budget {} cannot be read, so the ratchet checks nothing. Create it with `rulebearing count --from <from> --to <to> --budget {} --write` (ADR-0029)",
                    r.name, r.budget, r.budget
                );
            }
            _ => {}
        }
    }
    for v in &ratchets.vacuous {
        let _ = writeln!(
            out,
            "{}: ratchet `{}` is vacuous: its from side matched no module, so it counts nothing. Fix the pattern, delete the ratchet, or excuse it with allowEmpty (ADR-0007, ADR-0032)",
            if strict { "error" } else { "warning" },
            v.name
        );
    }
    out
}

/// Reads `summary.ratchets[]` from a saved result: the exceeded count and whether a budget was
/// missing, for `fmt --exit-code`.
pub fn from_summary(results: Option<&[RatchetResult]>) -> (u64, bool) {
    let results = results.unwrap_or_default();
    let exceeded = results
        .iter()
        .filter(|r| r.status == RatchetStatus::Exceeded)
        .count() as u64;
    (
        exceeded,
        results.iter().any(|r| r.status == RatchetStatus::NoBudget),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(status: RatchetStatus) -> RatchetResult {
        RatchetResult {
            name: "r".into(),
            budget: "b.json".into(),
            count: 2,
            ceiling: Some(1),
            status,
        }
    }

    #[test]
    fn exit_contributions_by_status() {
        let table = [
            (RatchetStatus::Held, 0, false),
            (RatchetStatus::Exceeded, 1, false),
            (RatchetStatus::NoBudget, 0, true),
        ];
        for (status, exceeded, untrusted) in table {
            let r = Ratchets {
                results: vec![result(status)],
                vacuous: Vec::new(),
            };
            assert_eq!(r.exceeded(), exceeded, "{status:?}");
            assert_eq!(r.no_budget(), untrusted, "{status:?}");
            assert_eq!(
                from_summary(Some(&r.results)),
                (exceeded, untrusted),
                "{status:?}"
            );
        }
        assert_eq!(from_summary(None), (0, false));
        let vacuous = Ratchets {
            results: Vec::new(),
            vacuous: vec![VacuousRule::new("v", "from")],
        };
        assert!(
            messages(&Config::default(), &vacuous, true)
                .starts_with("error: ratchet `v` is vacuous")
        );
        assert!(messages(&Config::default(), &vacuous, false).starts_with("warning: ratchet `v`"));
        assert!(
            !vacuous.no_budget(),
            "a vacuous from is not a missing budget"
        );
        let held = Ratchets {
            results: vec![result(RatchetStatus::Held)],
            vacuous: Vec::new(),
        };
        assert_eq!(messages(&Config::default(), &held, true), "");
    }
}
