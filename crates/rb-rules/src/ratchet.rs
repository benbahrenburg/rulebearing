//! Ratchets: a count of direct edges that may only fall.
//!
//! - Source: [design § The subcommands a guard reaches for](../../../docs/artifacts/design.md#the-subcommands-a-guard-reaches-for)
//!   (`count --from --to [--budget] [--write]`), [design § Rules an agent writes](../../../docs/artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar)
//!   ("Ratchets only fall")
//! - Plan: [Wave 1, Step 7](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-7-liveness-severity-ids-receipts-expires-ratchets-1b)
//!   (`ratchet.rs`)
//! - Requirement: [FR-RULE-06](../../../docs/prd.md#fr-rule-06)
//!
//! Counting is pure; the budget file (`{ "ceiling": n }`) is read and written by the command
//! line. `$1` in `to` takes the capture from `from.path`, as a dependency rule does, so
//! "each app's routes into that same app's server code" is one ratchet.

use rb_config::model::{FromRestriction, ToRestriction};
use rb_config::pattern::replace_group_placeholders;
use rb_model::GraphDocument;
use serde::{Deserialize, Serialize};

use crate::matchers::pattern;
use crate::patterns;

/// One counted edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    /// The module.
    pub from: String,
    /// The dependency's `resolved`.
    pub to: String,
}

/// The direct edges from modules matching `from` to targets matching `to`, in document order.
pub fn edges(document: &GraphDocument, from: &FromRestriction, to: &ToRestriction) -> Vec<Edge> {
    let from_path = pattern(from.path.as_ref());
    let from_not = pattern(from.path_not.as_ref());
    let to_path = pattern(to.path.as_ref());
    let to_not = pattern(to.path_not.as_ref());
    let mut out = Vec::new();
    for module in &document.modules {
        let source = module.source.as_str();
        if from_path
            .as_ref()
            .is_some_and(|p| !patterns::test(p, source))
            || from_not.as_ref().is_some_and(|p| patterns::test(p, source))
        {
            continue;
        }
        let groups = from_path
            .as_ref()
            .map_or_else(Vec::new, |p| patterns::groups(p, source));
        for dependency in &module.dependencies {
            let target = dependency.resolved.as_str();
            let hit = to_path
                .as_ref()
                .is_none_or(|p| patterns::test(&replace_group_placeholders(p, &groups), target))
                && to_not.as_ref().is_none_or(|p| {
                    !patterns::test(&replace_group_placeholders(p, &groups), target)
                });
            if hit {
                out.push(Edge {
                    from: source.to_owned(),
                    to: target.to_owned(),
                });
            }
        }
    }
    out
}

/// A budget file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    /// The most edges allowed.
    pub ceiling: u64,
}

/// What a count means against a budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// No budget: the count is information.
    Unbudgeted,
    /// At or under the ceiling; `headroom` edges below it.
    Within {
        /// Ceiling minus count.
        headroom: u64,
    },
    /// Over the ceiling: a failure.
    Over {
        /// Count minus ceiling.
        excess: u64,
    },
}

/// Compares a count with a budget.
pub fn verdict(count: u64, budget: Option<Budget>) -> Verdict {
    match budget {
        None => Verdict::Unbudgeted,
        Some(b) if count > b.ceiling => Verdict::Over {
            excess: count - b.ceiling,
        },
        Some(b) => Verdict::Within {
            headroom: b.ceiling - count,
        },
    }
}

/// Why `--write` refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "the count {count} is above the ceiling {ceiling}; a ratchet only falls, so --write will not raise it. Remove edges until the count is at or below {ceiling}"
)]
pub struct WouldRaise {
    /// The current count.
    pub count: u64,
    /// The ceiling.
    pub ceiling: u64,
}

/// `--write`: the budget lowered to the count, the same budget when the count equals it, a new
/// budget when there was none; a count above the ceiling is refused.
///
/// # Errors
/// [`WouldRaise`] when the count is above the ceiling.
pub fn lowered(count: u64, budget: Option<Budget>) -> Result<Budget, WouldRaise> {
    match budget {
        Some(b) if count > b.ceiling => Err(WouldRaise {
            count,
            ceiling: b.ceiling,
        }),
        _ => Ok(Budget { ceiling: count }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_model::{Dependency, Module, ModuleSystem};

    fn document() -> GraphDocument {
        let module = |source: &str, deps: &[&str]| Module {
            dependencies: deps
                .iter()
                .map(|d| Dependency::new(*d, *d, ModuleSystem::Es6))
                .collect(),
            ..Module::new(source)
        };
        GraphDocument {
            modules: vec![
                module(
                    "apps/web/src/app/page.tsx",
                    &["apps/web/src/server/db.ts", "apps/api/src/server/x.ts"],
                ),
                module("apps/api/src/app/route.ts", &["apps/api/src/server/x.ts"]),
                module("apps/web/src/lib.ts", &["apps/web/src/server/db.ts"]),
            ],
            ..GraphDocument::default()
        }
    }

    fn restriction(from: &str, to: &str) -> (FromRestriction, ToRestriction) {
        (
            serde_json::from_value(serde_json::json!({ "path": from })).unwrap_or_default(),
            serde_json::from_value(serde_json::json!({ "path": to })).unwrap_or_default(),
        )
    }

    #[test]
    fn the_reference_pipeline_ratchet_counts_same_app_edges() {
        let (from, to) = restriction(
            r"^apps/([^/]+)/src/app/.*/?(page|route)\.tsx?$",
            "^apps/$1/src/(server|domain)/",
        );
        let found = edges(&document(), &from, &to);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].to, "apps/web/src/server/db.ts");
        assert_eq!(found[1].from, "apps/api/src/app/route.ts");
        let (all_from, all_to) = (FromRestriction::default(), ToRestriction::default());
        assert_eq!(edges(&document(), &all_from, &all_to).len(), 4);
        let not: ToRestriction =
            serde_json::from_value(serde_json::json!({ "pathNot": "server" })).unwrap_or_default();
        assert!(edges(&document(), &all_from, &not).is_empty());
        let from_not: FromRestriction =
            serde_json::from_value(serde_json::json!({ "pathNot": "^apps/web" }))
                .unwrap_or_default();
        assert_eq!(edges(&document(), &from_not, &all_to).len(), 1);
    }

    #[test]
    fn budgets_only_fall() {
        assert_eq!(verdict(3, None), Verdict::Unbudgeted);
        assert_eq!(
            verdict(3, Some(Budget { ceiling: 5 })),
            Verdict::Within { headroom: 2 }
        );
        assert_eq!(
            verdict(5, Some(Budget { ceiling: 5 })),
            Verdict::Within { headroom: 0 }
        );
        assert_eq!(
            verdict(6, Some(Budget { ceiling: 5 })),
            Verdict::Over { excess: 1 }
        );
        assert_eq!(
            lowered(3, Some(Budget { ceiling: 5 })),
            Ok(Budget { ceiling: 3 })
        );
        assert_eq!(
            lowered(5, Some(Budget { ceiling: 5 })),
            Ok(Budget { ceiling: 5 })
        );
        assert_eq!(lowered(4, None), Ok(Budget { ceiling: 4 }));
        let refused = lowered(6, Some(Budget { ceiling: 5 }));
        assert_eq!(
            refused,
            Err(WouldRaise {
                count: 6,
                ceiling: 5
            })
        );
        assert!(
            refused
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default()
                .contains("only falls")
        );
    }
}
