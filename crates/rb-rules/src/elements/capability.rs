//! Validation against the capability table ([`rb_config::capability`]): which language can
//! answer each element predicate, and how.
//!
//! - Plan: [Wave 2, Step 5](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#25-step-5-the-element-rule-engine-and-the-capability-table-2c)
//!   and § 1.4.3 ("cross-language behaviour comes from two data tables")
//! - Decisions: [ADR-0014](../../../../docs/adr/0014-no-invented-cross-language-edges.md) (a key a
//!   language cannot answer is an error, never a silent false),
//!   [ADR-0010](../../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) rule 3 (the
//!   engine has no `match language`: this table is data the engine reads)
//! - Requirement: [FR-RULE-03](../../../../docs/prd.md#fr-rule-03) ("capability table")
//!
//! The table lives beside the vocabulary in `rb-config`, so the configuration schema prints its
//! mappings. A rule that uses an
//! unanswerable key over objects of that language fails validation, naming the key, the language
//! and the rule; `select.language` scopes a rule to the languages that can answer it.

use rb_config::elements::{ElementRule, Expr, Objects, Operand, Selector};
use rb_model::Language;

use super::{Architecture, ElementError};

use Capability::Unanswerable;
pub use rb_config::capability::{Capability, capability};

/// Every test in an expression, nested selectors included.
fn tests<'a>(expr: &'a Expr, out: &mut Vec<(&'a rb_config::elements::Test, Option<&'a Selector>)>) {
    match expr {
        Expr::All(items) | Expr::Any(items) => {
            for item in items {
                tests(item, out);
            }
        }
        Expr::Not(inner) => tests(inner, out),
        Expr::Test(test) => {
            let nested = match &test.operand {
                Operand::Objects(Objects::Selector(s))
                | Operand::Attribute {
                    attribute: Some(Objects::Selector(s)),
                    ..
                } => Some(s.as_ref()),
                _ => None,
            };
            out.push((test, nested));
            if let Some(selector) = nested
                && let Some(where_) = &selector.where_
            {
                tests(where_, out);
            }
        }
    }
}

/// The languages a selector's objects can come from in this run.
fn languages_of(architecture: &Architecture<'_>, selector: &Selector) -> Vec<Language> {
    let mut found: Vec<Language> = architecture
        .of_kind(selector.kind)
        .iter()
        .filter_map(super::Object::language)
        .filter(|l| selector.languages.is_empty() || selector.languages.contains(l))
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Refuses a rule that uses a key some language of its selection cannot answer.
///
/// # Errors
/// [`ElementError::Unanswerable`] naming the rule, the key and the language.
pub fn validate(architecture: &Architecture<'_>, rule: &ElementRule) -> Result<(), ElementError> {
    let languages = languages_of(architecture, &rule.select);
    let mut used = Vec::new();
    if let Some(where_) = &rule.select.where_ {
        tests(where_, &mut used);
    }
    tests(&rule.should, &mut used);
    for (test, _) in used {
        for language in &languages {
            if let Unanswerable(why) = capability(test.concept, *language) {
                return Err(ElementError::Unanswerable {
                    rule: rule.name.clone(),
                    key: test.key.clone(),
                    language: language.as_str().to_owned(),
                    why: why.to_owned(),
                });
            }
        }
    }
    Ok(())
}
