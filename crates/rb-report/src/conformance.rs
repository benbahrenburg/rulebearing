//! The reporters' half of the `rulebearing validate` protocol: dependency-cruiser's unit specs for
//! reporter internals (`test/report/dot/theming.spec.mjs`, `module-utl.spec.mjs`,
//! `test/report/error-html/utl.spec.mjs`) call these functions through it, unmodified.
//!
//! - Protocol: `conformance/dependency-cruiser/harness/shim.mjs` (its header is the contract)
//! - Decision: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md)
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Requirement: [NFR-CONF-01](../../../docs/prd.md#nfr-conf-01)
//!
//! A request names a `#report/...` module, an export and one argument list per application of a
//! curried function; [`dispatch`] maps each to the Rust function that ports it. The engine's
//! modules are answered by `rb_rules::conformance`, which this module reuses for the request and
//! error shapes.

use rb_rules::conformance::{ProtocolError, Request};
use serde_json::{Map, Value, json};

use crate::dot::module_utl::{add_url, extract_first_transgression, flat_label, folderify};
use crate::dot::theme::{apply_theme, attributize, normalize_theme, theme_attributes};
use crate::err_html::{
    determine_from_extras, determine_to, format_summary_for_report, formatted_allowed_rule,
    merge_counts_into_rule,
};
use crate::js;

/// The timestamp `formatSummaryForReport` stamps in a conformance reply.
const PROTOCOL_TIMESTAMP: &str = "1970-01-01T00:00:00.000";

fn not_ported(request: &Request) -> ProtocolError {
    ProtocolError::NotPorted {
        module: request.module.clone(),
        export: request.export.clone(),
        path: request
            .path
            .iter()
            .flat_map(|p| [".", p.as_str()])
            .collect(),
    }
}

/// Argument `index` of application `call`; a missing argument is `None` (undefined).
fn arg(request: &Request, call: usize, index: usize) -> Option<&Value> {
    request
        .calls
        .get(call)
        .and_then(|c| c.get(index))
        .filter(|v| !v.is_null())
}

fn object(value: Option<&Value>) -> Map<String, Value> {
    value
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        other => js::to_string(other),
    }
}

/// Answers one request for a reporter module.
///
/// # Errors
/// [`ProtocolError::NotPorted`] for a function this crate does not port.
pub fn dispatch(request: &Request) -> Result<Value, ProtocolError> {
    let empty = Value::Object(Map::new());
    let first = |i| arg(request, 0, i);
    let module = || arg(request, 1, 0).unwrap_or(&empty);
    Ok(match (request.module.as_str(), request.export.as_str()) {
        ("#report/dot/theming.mjs", "normalizeTheme") => normalize_theme(first(0)),
        ("#report/dot/theming.mjs", "getThemeAttributes") => {
            Value::Object(theme_attributes(first(0).unwrap_or(&empty), first(1)))
        }
        ("#report/dot/theming.mjs", "applyTheme") => {
            apply_theme(first(0).unwrap_or(&empty), module())
        }
        ("#report/dot/module-utl.mjs", "attributizeObject") => {
            Value::String(attributize(&object(first(0))))
        }
        ("#report/dot/module-utl.mjs", "extractFirstTransgression") => {
            extract_first_transgression(first(0).unwrap_or(&empty))
        }
        ("#report/dot/module-utl.mjs", "folderify") => folderify(module(), js::truthy(first(0))),
        ("#report/dot/module-utl.mjs", "flatLabel") => flat_label(module(), js::truthy(first(0))),
        ("#report/dot/module-utl.mjs", "addURL") => {
            add_url(module(), &text(first(0)), &text(first(1)))
        }
        ("#report/error-html/utl.mjs", "getFormattedAllowedRule") => {
            formatted_allowed_rule(first(0)).unwrap_or_else(|| json!([]))
        }
        ("#report/error-html/utl.mjs", "mergeCountsIntoRule") => {
            merge_counts_into_rule(first(0).unwrap_or(&empty), &object(first(1)))
        }
        ("#report/error-html/utl.mjs", "formatSummaryForReport") => {
            format_summary_for_report(first(0).unwrap_or(&empty), PROTOCOL_TIMESTAMP)
        }
        ("#report/error-html/utl.mjs", "determineTo") => {
            Value::String(determine_to(first(0).unwrap_or(&empty), &object(first(1))))
        }
        ("#report/error-html/utl.mjs", "determineFromExtras") => {
            Value::String(determine_from_extras(first(0).unwrap_or(&empty)))
        }
        _ => return Err(not_ported(request)),
    })
}

/// Answers one request as the protocol's JSON reply, `{ "result": ... }`.
///
/// # Errors
/// [`ProtocolError::Argument`] for a request that is not JSON; see [`dispatch`].
pub fn answer(text: &str) -> Result<String, ProtocolError> {
    let request: Request = serde_json::from_str(text).map_err(|e| ProtocolError::Argument {
        index: 0,
        reason: format!("not a request: {e}"),
    })?;
    Ok(json!({ "result": dispatch(&request)? }).to_string())
}

/// Whether a request's module is one of the reporters'.
pub fn handles(module: &str) -> bool {
    module.starts_with("#report/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(module: &str, export: &str, calls: Value) -> Result<Value, ProtocolError> {
        dispatch(&Request {
            module: module.into(),
            export: export.into(),
            path: Vec::new(),
            constructor_args: None,
            calls: serde_json::from_value(calls).unwrap_or_default(),
        })
    }

    #[test]
    fn theming_and_module_utl() -> Result<(), ProtocolError> {
        let attributes = call(
            "#report/dot/theming.mjs",
            "getThemeAttributes",
            json!([[{ "coreModule": true }, normalize_theme(None)["modules"]]]),
        )?;
        assert_eq!(attributes, json!({ "color": "grey", "fontcolor": "grey" }));
        assert_eq!(
            call(
                "#report/dot/theming.mjs",
                "getThemeAttributes",
                json!([[{}, null]])
            )?,
            json!({})
        );
        assert_eq!(
            call("#report/dot/theming.mjs", "normalizeTheme", json!([[]]))?,
            normalize_theme(None)
        );
        let themed = call(
            "#report/dot/theming.mjs",
            "applyTheme",
            json!([[{ "replace": true }], [{ "dependencies": [] }]]),
        )?;
        assert_eq!(themed["themeAttrs"], json!(""));
        let label = call(
            "#report/dot/module-utl.mjs",
            "flatLabel",
            json!([[true], [{ "source": "a/b.js" }]]),
        )?;
        assert_eq!(label["label"], json!("<a/<BR/><B>b.js</B>>"));
        let folder = call(
            "#report/dot/module-utl.mjs",
            "folderify",
            json!([[false], [{ "source": "a/b.js" }]]),
        )?;
        assert_eq!(folder["folder"], json!("a"));
        assert_eq!(
            call(
                "#report/dot/module-utl.mjs",
                "extractFirstTransgression",
                json!([[{ "dependencies": [] }]])
            )?,
            json!({ "dependencies": [] })
        );
        assert_eq!(
            call(
                "#report/dot/module-utl.mjs",
                "attributizeObject",
                json!([[{ "a": 1 }]])
            )?,
            json!("a=\"1\"")
        );
        let url = call(
            "#report/dot/module-utl.mjs",
            "addURL",
            json!([["p/", null], [{ "source": "x" }]]),
        )?;
        assert_eq!(url["URL"], json!("p/x"));
        Ok(())
    }

    #[test]
    fn error_html_utl() -> Result<(), ProtocolError> {
        let m = "#report/error-html/utl.mjs";
        assert_eq!(call(m, "getFormattedAllowedRule", json!([[]]))?, json!([]));
        assert_eq!(
            call(m, "mergeCountsIntoRule", json!([[{ "name": "r" }, {}]]))?,
            json!({ "name": "r", "count": 0, "ignoredCount": 0, "unviolated": true })
        );
        assert_eq!(
            call(
                m,
                "mergeCountsIntoRule",
                json!([[{ "name": "r" }, { "r": { "count": 2, "ignoredCount": 1 } }]])
            )?,
            json!({ "name": "r", "count": 2, "ignoredCount": 1, "unviolated": false })
        );
        let summary = call(m, "formatSummaryForReport", json!([[{}]]))?;
        assert_eq!(summary["violations"], json!([]));
        assert_eq!(summary["runDate"], json!("1970-01-01T00:00:00.000Z"));
        assert_eq!(
            call(m, "determineTo", json!([[{ "type": "module", "to": "a" }]]))?,
            json!("")
        );
        assert_eq!(
            call(
                m,
                "determineFromExtras",
                json!([[{ "type": "dependency" }]])
            )?,
            json!("")
        );
        assert!(call(m, "nope", json!([])).is_err());
        assert!(handles(m) && !handles("#validate/index.mjs"));
        assert_eq!(
            answer(
                r##"{"module":"#report/dot/module-utl.mjs","export":"attributizeObject","calls":[[{"b":"x"}]]}"##
            )?,
            r#"{"result":"b=\"x\""}"#
        );
        assert!(answer("{").is_err());
        Ok(())
    }
}
