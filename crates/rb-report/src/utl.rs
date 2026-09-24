//! What the graph and HTML reporters share: a module's link and the one-letter dependency type.
//! dependency-cruiser 18.2.0's `src/report/utl/index.mjs`, ported.
//!
//! - Specification: the `dot`, `d2` and `err-html` specs under `test/report`, run by conformance
//!   gate 1 layer 3 ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Coverage: [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   row `prefix`, `suffix`
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01)

use serde_json::Value;

use crate::js;

/// `^[a-z]+://`.
fn has_protocol(prefix: &str) -> bool {
    let letters = prefix.bytes().take_while(u8::is_ascii_lowercase).count();
    letters > 0 && prefix[letters..].starts_with("://")
}

/// `smartURIConcat`: a URI prefix is concatenated, anything else `path.posix.join`ed.
fn smart_uri_concat(prefix: &str, source: &str) -> String {
    if has_protocol(prefix) {
        format!("{prefix}{source}")
    } else {
        js::join(prefix, source)
    }
}

/// `deriveExternalPackageName`: the first `node_modules/<name>` (a name not starting with `@`) or
/// `@scope/name` in the source.
fn external_package_name(source: &str) -> String {
    for (start, _) in source.char_indices() {
        // node_modules\/(?<packageName>[^@][^/]+)
        if let Some(rest) = source[start..].strip_prefix("node_modules/") {
            let mut chars = rest.chars();
            if let Some(first) = chars.next().filter(|c| *c != '@') {
                let tail: String = chars.take_while(|c| *c != '/').collect();
                if !tail.is_empty() {
                    return format!("{first}{tail}");
                }
            }
        }
        // (?<atPackageName>@[^/]+\/[^/]+)
        if let Some(rest) = source[start..].strip_prefix('@') {
            let scope: String = rest.chars().take_while(|c| *c != '/').collect();
            if !scope.is_empty() {
                let after = &rest[scope.len()..];
                if let Some(name_part) = after.strip_prefix('/') {
                    let name: String = name_part.chars().take_while(|c| *c != '/').collect();
                    if !name.is_empty() {
                        return format!("@{scope}/{name}");
                    }
                }
            }
        }
    }
    String::new()
}

/// `getURLForModule(module, prefix, suffix)`.
pub fn url_for_module(module: &Value, prefix: &str, suffix: &str) -> String {
    let source = js::field(module, "source");
    let types = module.get("dependencyTypes");
    if js::includes(types, "core") {
        let package = source.split('/').next().unwrap_or_default();
        if let Some(bun) = package.strip_prefix("bun:") {
            return format!("https://bun.sh/docs/api/{bun}");
        }
        return format!("https://nodejs.org/api/{package}.html");
    }
    if js::some_str(types, |t| t.starts_with("npm")) {
        return format!(
            "https://www.npmjs.com/package/{}",
            external_package_name(&source)
        );
    }
    if !prefix.is_empty() || !suffix.is_empty() {
        let mut url = source;
        if !prefix.is_empty() {
            url = smart_uri_concat(prefix, &url);
        }
        if !suffix.is_empty() {
            url.push_str(suffix);
        }
        return url;
    }
    source
}

/// `getOneLetterDependencyType(dependencyTypes)`.
pub fn one_letter_dependency_type(types: &[&str]) -> &'static str {
    if types.iter().any(|t| t.starts_with("npm")) {
        "n"
    } else if types.contains(&"aliased-subpath-import") {
        "#"
    } else if types.contains(&"aliased") {
        "@"
    } else if types.contains(&"core") {
        "c"
    } else if types.contains(&"export") {
        "x"
    } else if types.contains(&"local") {
        "."
    } else if types.iter().any(|t| t.starts_with("type-")) {
        "T"
    } else {
        ""
    }
}

/// `summary.optionsUsed.<key> ?? ""` as a string.
pub fn option_text(result: &Value, key: &str) -> String {
    match result
        .get("summary")
        .and_then(|s| s.get("optionsUsed"))
        .and_then(|o| o.get(key))
    {
        None | Some(Value::Null) => String::new(),
        Some(other) => js::to_string(Some(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn urls_for_core_npm_and_local_modules() {
        let core = json!({ "source": "fs", "dependencyTypes": ["core"] });
        assert_eq!(
            url_for_module(&core, "", ""),
            "https://nodejs.org/api/fs.html"
        );
        let bun = json!({ "source": "bun:sqlite", "dependencyTypes": ["core"] });
        assert_eq!(
            url_for_module(&bun, "", ""),
            "https://bun.sh/docs/api/sqlite"
        );
        let npm = json!({ "source": "node_modules/lodash/index.js", "dependencyTypes": ["npm"] });
        assert_eq!(
            url_for_module(&npm, "", ""),
            "https://www.npmjs.com/package/lodash"
        );
        let scoped = json!({ "source": "node_modules/@babel/core/lib/index.js", "dependencyTypes": ["npm-dev"] });
        assert_eq!(
            url_for_module(&scoped, "", ""),
            "https://www.npmjs.com/package/@babel/core"
        );
        let nothing = json!({ "source": "x", "dependencyTypes": ["npm"] });
        assert_eq!(
            url_for_module(&nothing, "", ""),
            "https://www.npmjs.com/package/"
        );
        let local = json!({ "source": "src/a.js" });
        assert_eq!(url_for_module(&local, "", ""), "src/a.js");
        assert_eq!(
            url_for_module(&local, "https://x.io/blob/", ""),
            "https://x.io/blob/src/a.js"
        );
        assert_eq!(
            url_for_module(&local, "../prefix/", ".html"),
            "../prefix/src/a.js.html"
        );
        assert_eq!(url_for_module(&local, "", "?x"), "src/a.js?x");
    }

    #[test]
    fn one_letter_types() {
        for (types, letter) in [
            (vec!["npm-dev"], "n"),
            (vec!["aliased-subpath-import", "aliased"], "#"),
            (vec!["aliased"], "@"),
            (vec!["core"], "c"),
            (vec!["export"], "x"),
            (vec!["local"], "."),
            (vec!["type-only"], "T"),
            (vec![], ""),
        ] {
            assert_eq!(one_letter_dependency_type(&types), letter);
        }
    }

    #[test]
    fn options_used_text() {
        let result = json!({ "summary": { "optionsUsed": { "prefix": "p/", "n": null } } });
        assert_eq!(option_text(&result, "prefix"), "p/");
        assert_eq!(option_text(&result, "n"), "");
        assert_eq!(option_text(&json!({}), "prefix"), "");
        assert!(has_protocol("file://x"));
        assert!(!has_protocol("://x"));
        assert!(!has_protocol("HTTP://x"));
    }
}
