//! `rulebearing test`: each rule's `examples`, proven against a synthetic graph.
//!
//! - Source: [design § Rule metadata that says what to do](../../../../docs/artifacts/design.md#rule-metadata-that-says-what-to-do)
//!   ("`rulebearing test` asserts the allowed ones pass and the forbidden ones fail")
//! - Plan: [Wave 1, Step 14](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-14-rules---json-explain-explain---plain-test-can-import-1e)
//! - Requirement: [FR-CFG-07](../../../../docs/prd.md#fr-cfg-07)
//!
//! Each example is `"from -> to"`. For every rule with examples, a graph holding just those edges
//! (every target a local module) is evaluated against that rule alone: a `forbidden` example must
//! produce a violation of the rule on that edge, an `allowed` one must not. The exit code is the
//! number of examples that did not behave.

use std::fmt::Write as _;

use clap::Args;
use rb_config::model::DependencyRules;
use rb_config::{Config, Family, Rule};
use rb_model::{Dependency, DependencyType, GraphDocument, Module, ModuleSystem};
use rb_rules::{EvalOptions, evaluate};

use crate::cli::ConfigArgs;
use crate::context::Context;
use crate::{Outcome, RunExit, configure};

/// `test`.
#[derive(Debug, Clone, Default, Args)]
pub struct TestArgs {
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
}

/// Splits `"a.ts -> b.ts"`.
pub fn parse_example(example: &str) -> Option<(String, String)> {
    let (from, to) = example.split_once("->")?;
    let (from, to) = (from.trim(), to.trim());
    (!from.is_empty() && !to.is_empty()).then(|| (from.to_owned(), to.to_owned()))
}

/// The graph of the given edges.
pub fn synthetic(edges: &[(String, String)]) -> GraphDocument {
    let mut modules: Vec<Module> = Vec::new();
    let mut add = |source: &str| {
        if !modules.iter().any(|m| m.source == source) {
            modules.push(Module::new(source));
        }
    };
    for (from, to) in edges {
        add(from);
        add(to);
    }
    for (from, to) in edges {
        if let Some(module) = modules.iter_mut().find(|m| m.source == *from) {
            module.dependencies.push(Dependency {
                dependency_types: vec![DependencyType::Local],
                followable: true,
                ..Dependency::new(to.clone(), to.clone(), ModuleSystem::Es6)
            });
        }
    }
    GraphDocument {
        modules,
        ..GraphDocument::default()
    }
}

fn only(rule: &Rule, family: Family, config: &Config) -> Config {
    let mut dependencies = DependencyRules::default();
    match family {
        Family::Forbidden => dependencies.forbidden.push(rule.clone()),
        Family::Allowed => {
            dependencies.allowed.push(rule.clone());
            dependencies.allowed_severity = config.rules.dependencies.allowed_severity;
        }
        Family::Required => dependencies.required.push(rule.clone()),
    }
    let mut single = Config::default();
    single.rules.dependencies = dependencies;
    single
}

/// Whether evaluating `rule` over the one edge flags it.
fn flags(
    rule: &Rule,
    family: Family,
    config: &Config,
    edge: &(String, String),
    today: chrono::NaiveDate,
) -> bool {
    let single = only(rule, family, config);
    let options = EvalOptions {
        liveness: false,
        today,
        ..EvalOptions::default()
    };
    evaluate(synthetic(std::slice::from_ref(edge)), &single, &options).is_ok_and(|e| {
        e.violations().iter().any(|v| {
            (v.from == edge.0 && (v.to == edge.1 || v.to == edge.0))
                && (v.rule.name == rule.name() || family == Family::Allowed)
        })
    })
}

/// Runs `test`.
pub fn run(ctx: &mut Context<'_>, args: &TestArgs) -> Outcome {
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let mut out = String::new();
    let mut failures = 0u64;
    let mut tested = 0usize;
    for (family, rule) in config.rules.all_dependency_rules() {
        let Some(examples) = &rule.meta.examples else {
            continue;
        };
        tested += 1;
        let _ = writeln!(out, "{}", rule.name());
        for (expected_flag, list) in [(true, &examples.forbidden), (false, &examples.allowed)] {
            for example in list {
                let Some(edge) = parse_example(example) else {
                    failures += 1;
                    let _ = writeln!(out, "  FAIL `{example}` is not \"from -> to\"");
                    continue;
                };
                let flagged = flags(rule, family, &config, &edge, ctx.today);
                let ok = flagged == expected_flag;
                if !ok {
                    failures += 1;
                }
                let verdict = if expected_flag { "flags" } else { "allows" };
                let _ = writeln!(
                    out,
                    "  {} {verdict} {} -> {}",
                    if ok { "ok  " } else { "FAIL" },
                    edge.0,
                    edge.1
                );
            }
        }
    }
    let untested = config.rules.all_dependency_rules().count() - tested;
    let _ = writeln!(
        out,
        "\n{tested} rule(s) with examples, {failures} failing; {untested} rule(s) without examples"
    );
    Outcome {
        stdout: out,
        stderr: String::new(),
        code: RunExit::Violations(failures).code(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_parse_and_build_a_graph() {
        assert_eq!(
            parse_example("a.ts -> b.ts"),
            Some(("a.ts".into(), "b.ts".into()))
        );
        assert_eq!(parse_example("a.ts ->"), None);
        assert_eq!(parse_example("nope"), None);
        let graph = synthetic(&[("a".into(), "b".into()), ("a".into(), "c".into())]);
        assert_eq!(graph.modules.len(), 3);
        assert_eq!(graph.modules[0].dependencies.len(), 2);
    }
}
