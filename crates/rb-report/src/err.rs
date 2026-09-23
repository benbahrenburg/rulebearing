//! `err` and `err-long`: dependency-cruiser 18.2.0's `src/report/error.mjs`, ported, with the
//! rule's `fix` printed under each finding by `err-long`.
//!
//! - Specification: `test/report/error/*.spec.mjs`, run unmodified by conformance gate 1 layer 3
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Source: [design § Reporters](../../../docs/artifacts/design.md#reporters) ("`err-long`
//!   prints the rule comment and now the `fix` under each finding")
//! - Coverage: [coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types),
//!   rows `err`, `err-long`; [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   `reporterOptions.err` (`showAliasedModulesUnresolved`, `showExternalModulesUnresolved`)
//! - Plan: [Wave 1, Step 12](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-12-reporters-1d)
//! - Requirements: [FR-OUT-01](../../../docs/prd.md#fr-out-01), [FR-OUT-03](../../../docs/prd.md#fr-out-03)
//!
//! The `fix` line appears only when a rule has a `fix`, so every dependency-cruiser fixture renders
//! unchanged. It sits under the comment, indented as the comment is, prefixed `fix: `.

use serde_json::Value;

use crate::style::{Style, percentage, styled, wrap_and_indent};
use crate::{Rendered, find_rule, num, severity, text};

const EXTRA_PATH_INDENT: usize = 6;

/// Options `reporterOptions.err` and `err-long` carry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ErrOptions {
    /// `err-long`: print each rule's comment and `fix`.
    pub long: bool,
    /// How unresolved dependencies are printed.
    pub unresolved: Unresolved,
    /// Colour the output.
    pub color: bool,
}

/// `showExternalModulesUnresolved` and `showAliasedModulesUnresolved`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Unresolved {
    /// Print the specifier rather than the resolved path for unresolved npm modules.
    pub external: bool,
    /// Print the specifier rather than the resolved path for unresolved aliases.
    pub aliased: bool,
}

impl ErrOptions {
    /// Reads `reporterOptions.err` (or `err-long`).
    pub fn from_reporter_options(options: Option<&Value>, long: bool, color: bool) -> Self {
        let flag = |k: &str| {
            options
                .and_then(|o| o.get(k))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        };
        Self {
            long,
            unresolved: Unresolved {
                external: flag("showExternalModulesUnresolved"),
                aliased: flag("showAliasedModulesUnresolved"),
            },
            color,
        }
    }
}

fn names(steps: Option<&Value>) -> Vec<String> {
    steps
        .and_then(Value::as_array)
        .map(|s| s.iter().map(|x| text(x, "name")).collect())
        .unwrap_or_default()
}

fn mini(steps: Option<&Value>) -> String {
    format!(
        "\n{}",
        wrap_and_indent(&names(steps).join(" → \n"), EXTRA_PATH_INDENT)
    )
}

/// `formatDependencyTo`: the specifier for an unresolved external or aliased module when asked.
pub fn dependency_to(violation: &Value, show_external: bool, show_aliased: bool) -> String {
    if crate::truthy(violation.get("unresolvedTo")) {
        let types: Vec<&str> = violation
            .get("dependencyTypes")
            .and_then(Value::as_array)
            .map(|t| t.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let external = show_external && types.iter().any(|t| t.starts_with("npm"));
        let aliased = show_aliased && types.contains(&"aliased");
        if external || aliased {
            return text(violation, "unresolvedTo");
        }
    }
    text(violation, "to")
}

fn format_violators(v: &Value, o: ErrOptions) -> String {
    let bold = |t: &str| styled(Some(Style::Bold), t, o.color);
    let from = bold(&text(v, "from"));
    let dependency = || {
        format!(
            "{from} → {}",
            bold(&dependency_to(
                v,
                o.unresolved.external,
                o.unresolved.aliased
            ))
        )
    };
    match v.get("type").and_then(Value::as_str) {
        Some("module") => from,
        Some("cycle") => format!("{from} → {}", mini(v.get("cycle"))),
        Some("reachability") => format!("{from} → {}{}", bold(&text(v, "to")), mini(v.get("via"))),
        Some("instability") => {
            let metric = |side: &str| {
                percentage(num(
                    v.get("metrics").and_then(|m| m.get(side)),
                    "instability",
                ))
            };
            format!(
                "{}\n{}",
                dependency(),
                styled(
                    Some(Style::Dim),
                    &wrap_and_indent(
                        &format!("instability: {} → {}", metric("from"), metric("to")),
                        EXTRA_PATH_INDENT
                    ),
                    o.color
                )
            )
        }
        _ => dependency(),
    }
}

fn format_violation(v: &Value, comment: Option<&str>, fix: Option<&str>, o: ErrOptions) -> String {
    let sev = severity(v);
    let mut out = format!(
        "{} {}: {}",
        styled(Style::for_severity(&sev), &sev, o.color),
        v.get("rule").map(|r| text(r, "name")).unwrap_or_default(),
        format_violators(v, o)
    );
    if let Some(comment) = comment.filter(|c| !c.is_empty()) {
        out.push('\n');
        out.push_str(&styled(
            Some(Style::Dim),
            &wrap_and_indent(comment, 4),
            o.color,
        ));
        out.push('\n');
        if let Some(fix) = fix.filter(|f| !f.is_empty()) {
            out.push_str(&styled(
                Some(Style::Dim),
                &wrap_and_indent(&format!("fix: {fix}"), 4),
                o.color,
            ));
            out.push('\n');
        }
    }
    out
}

fn ignore_warning(summary: &Value, color: bool) -> String {
    let ignored = summary.get("ignore").and_then(Value::as_f64).unwrap_or(0.0);
    if ignored > 0.0 {
        styled(
            Some(Style::Yellow),
            &format!(
                "‼ {} known violations ignored. Run with --no-ignore-known to see them.\n",
                crate::js_number(summary.get("ignore"))
            ),
            color,
        )
    } else {
        String::new()
    }
}

fn environment_issues(summary: &Value, color: bool) -> String {
    let issues = summary
        .get("environment")
        .and_then(|e| e.get("issues"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let icon = |s: &str| match s {
        "error" => "x",
        "warn" => "‼",
        "info" => "i",
        "ignore" => "-",
        _ => "undefined",
    };
    let lines: Vec<String> = issues
        .iter()
        .map(|issue| {
            let sev = text(issue, "severity");
            styled(
                Style::for_severity(&sev),
                &format!(
                    "{} {}: {}",
                    icon(&sev),
                    styled(Some(Style::Bold), &text(issue, "name"), color),
                    text(issue, "description")
                ),
                color,
            )
        })
        .collect();
    format!(
        "{}{}",
        if lines.is_empty() { "" } else { "\n" },
        lines.join("\n")
    )
}

/// Renders `err` or `err-long`.
pub fn render(result: &Value, o: ErrOptions) -> Rendered {
    let summary = result.get("summary").cloned().unwrap_or(Value::Null);
    let rule_set = summary.get("ruleSetUsed");
    let violations: Vec<&Value> = summary
        .get("violations")
        .and_then(Value::as_array)
        .map(|v| v.iter().filter(|x| severity(x) != "ignore").collect())
        .unwrap_or_default();
    let modules = crate::js_number(summary.get("totalCruised"));
    let dependencies = crate::js_number(summary.get("totalDependenciesCruised"));
    let output = if violations.is_empty() {
        format!(
            "\n{} no dependency violations found ({modules} modules, {dependencies} dependencies cruised)\n{}{}\n",
            styled(Some(Style::Green), "✔", o.color),
            ignore_warning(&summary, o.color),
            environment_issues(&summary, o.color)
        )
    } else {
        let mut out = String::from("\n");
        for v in violations.iter().rev() {
            let name = v.get("rule").map(|r| text(r, "name")).unwrap_or_default();
            let rule = find_rule(rule_set, &name);
            let comment = if o.long {
                Some(
                    rule.and_then(|r| r.get("comment"))
                        .and_then(Value::as_str)
                        .unwrap_or("-")
                        .to_owned(),
                )
            } else {
                v.get("comment").and_then(Value::as_str).map(str::to_owned)
            };
            let fix = if o.long {
                v.get("fix")
                    .and_then(Value::as_str)
                    .or_else(|| rule.and_then(|r| r.get("fix")).and_then(Value::as_str))
                    .map(str::to_owned)
            } else {
                None
            };
            out.push_str("  ");
            out.push_str(&format_violation(v, comment.as_deref(), fix.as_deref(), o));
            out.push('\n');
        }
        let n = |k: &str| summary.get(k).and_then(Value::as_u64).unwrap_or(0);
        let message = format!(
            "\nx {} dependency violations ({} errors, {} warnings). {modules} modules, {dependencies} dependencies cruised.\n",
            n("error") + n("warn") + n("info"),
            n("error"),
            n("warn")
        );
        out.push_str(&if n("error") > 0 {
            styled(Some(Style::Red), &message, o.color)
        } else {
            message
        });
        out.push_str(&ignore_warning(&summary, o.color));
        out.push_str(&environment_issues(&summary, o.color));
        out.push('\n');
        out
    };
    Rendered {
        output,
        exit_code: summary.get("error").and_then(Value::as_u64).unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn result(violations: &Value, extra: &Value) -> Value {
        let mut summary = json!({ "violations": violations.clone(), "error": 1, "warn": 0, "info": 0, "totalCruised": 3, "totalDependenciesCruised": 4 });
        if let (Value::Object(s), Some(e)) = (&mut summary, extra.as_object()) {
            s.extend(e.clone());
        }
        json!({ "modules": [], "summary": summary })
    }

    fn plain(long: bool) -> ErrOptions {
        ErrOptions {
            long,
            ..ErrOptions::default()
        }
    }

    #[test]
    fn nothing_found() {
        let r = render(&result(&json!([]), &json!({ "error": 0 })), plain(false));
        assert_eq!(
            r.output,
            "\n✔ no dependency violations found (3 modules, 4 dependencies cruised)\n\n"
        );
        assert_eq!(r.exit_code, 0);
        let ignored = render(
            &result(&json!([]), &json!({ "error": 0, "ignore": 2 })),
            plain(false),
        );
        assert!(ignored.output.contains("‼ 2 known violations ignored"));
    }

    #[test]
    fn every_violation_type() {
        let v = json!([
            { "type": "dependency", "from": "a", "to": "b", "rule": { "name": "dep", "severity": "error" } },
            { "type": "module", "from": "m", "to": "m", "rule": { "name": "orphan", "severity": "warn" } },
            { "type": "cycle", "from": "c", "to": "d", "cycle": [{ "name": "d" }, { "name": "c" }], "rule": { "name": "circ", "severity": "error" } },
            { "type": "reachability", "from": "r", "to": "s", "via": [{ "name": "s" }], "rule": { "name": "reach", "severity": "info" } },
            { "type": "instability", "from": "i", "to": "j", "metrics": { "from": { "instability": 0.25 }, "to": { "instability": 0.5 } }, "rule": { "name": "sdp", "severity": "warn" } },
            { "from": "x", "to": "y", "rule": { "name": "untyped", "severity": "error" } },
            { "type": "dependency", "from": "q", "to": "z", "rule": { "name": "gone", "severity": "ignore" } }
        ]);
        let out = render(&result(&v, &json!({})), plain(false)).output;
        assert!(out.contains("error dep: a → b\n"));
        assert!(out.contains("warn orphan: m\n"));
        assert!(
            out.contains("error circ: c → \n      d →\n      c\n"),
            "{out}"
        );
        assert!(out.contains("info reach: r → s\n      s\n"));
        assert!(out.contains("warn sdp: i → j\n      instability: 25% → 50%\n"));
        assert!(out.contains("error untyped: x → y\n"));
        assert!(!out.contains("gone"));
        assert!(out.find("untyped") < out.find("dep:"), "printed in reverse");
        assert!(out.contains(
            "x 1 dependency violations (1 errors, 0 warnings). 3 modules, 4 dependencies cruised.\n"
        ));
    }

    #[test]
    fn long_prints_comment_and_fix() {
        let v = json!([
            { "type": "dependency", "from": "a", "to": "b", "rule": { "name": "dep", "severity": "error" }, "fix": "Move it." },
            { "type": "dependency", "from": "c", "to": "d", "rule": { "name": "bare", "severity": "error" } }
        ]);
        let rules = json!({ "ruleSetUsed": { "forbidden": [{ "name": "dep", "comment": "why adr:0001" }] } });
        let out = render(&result(&v, &rules), plain(true)).output;
        assert!(
            out.contains("error dep: a → b\n    why adr:0001\n    fix: Move it.\n"),
            "{out}"
        );
        assert!(out.contains("error bare: c → d\n    -\n"), "{out}");
        let short = render(&result(&json!([{ "type": "dependency", "from": "a", "to": "b", "rule": { "name": "dep", "severity": "error" }, "fix": "x" }]), &json!({})), plain(false)).output;
        assert!(!short.contains("fix:"));
    }

    #[test]
    fn unresolved_targets_and_options() {
        let v = json!({ "to": "b", "unresolvedTo": "pkg", "dependencyTypes": ["npm-no-pkg"] });
        assert_eq!(dependency_to(&v, false, false), "b");
        assert_eq!(dependency_to(&v, true, false), "pkg");
        let aliased = json!({ "to": "b", "unresolvedTo": "@x", "dependencyTypes": ["aliased"] });
        assert_eq!(dependency_to(&aliased, false, true), "@x");
        assert_eq!(dependency_to(&json!({ "to": "b" }), true, true), "b");
        let o = ErrOptions::from_reporter_options(
            Some(&json!({ "showExternalModulesUnresolved": true })),
            true,
            false,
        );
        assert!(o.unresolved.external && o.long && !o.unresolved.aliased);
    }

    #[test]
    fn colour_and_environment_issues() {
        let issues = json!({ "environment": { "issues": [{ "severity": "warn", "name": "missing-typescript", "description": "d" }] } });
        let colored = render(
            &result(
                &json!([{ "type": "module", "from": "m", "to": "m", "rule": { "name": "r", "severity": "error" } }]),
                &issues,
            ),
            ErrOptions {
                color: true,
                ..ErrOptions::default()
            },
        );
        assert!(colored.output.contains("\u{1b}[31merror\u{1b}[39m"));
        assert!(
            colored
                .output
                .contains("‼ \u{1b}[1mmissing-typescript\u{1b}[22m: d")
        );
        assert_eq!(colored.exit_code, 1);
    }
}
