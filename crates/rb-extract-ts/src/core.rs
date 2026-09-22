//! Runtime built-in modules: `fs`, `node:path`, `bun:ffi`.
//!
//! - Plan: [Wave 0, Step 8](../../../docs/plans/pending/0000-wave-0-spike.md#step-8-spike-a-rb-extract-ts-0c)
//!   (`core.rs`: "bundled per-Node-version core module list, `node:` protocol")
//! - Source: [coverage § Extraction and resolution](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution),
//!   row Core module detection
//! - Specification: dependency-cruiser 18.2.0 `src/extract/resolve/is-built-in.mjs`, which asks
//!   Node's `module.isBuiltin`
//!
//! The list is Node 24's `module.builtinModules`, the runtime the conformance expectations were
//! recorded on. Rulebearing does not run Node, so the list is data; a new Node release that adds
//! a module is a one-line change here, caught by the layer 1 fixtures when upstream adds a case.

use rb_model::options::BuiltInModules;

/// Node 24's `module.builtinModules`.
pub const NODE_BUILTINS: &[&str] = &[
    "_http_agent",
    "_http_client",
    "_http_common",
    "_http_incoming",
    "_http_outgoing",
    "_http_server",
    "_stream_duplex",
    "_stream_passthrough",
    "_stream_readable",
    "_stream_transform",
    "_stream_wrap",
    "_stream_writable",
    "_tls_common",
    "_tls_wrap",
    "assert",
    "assert/strict",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
    "dns",
    "dns/promises",
    "domain",
    "events",
    "fs",
    "fs/promises",
    "http",
    "http2",
    "https",
    "inspector",
    "inspector/promises",
    "module",
    "net",
    "os",
    "path",
    "path/posix",
    "path/win32",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "readline/promises",
    "repl",
    "stream",
    "stream/consumers",
    "stream/promises",
    "stream/web",
    "string_decoder",
    "sys",
    "timers",
    "timers/promises",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "util/types",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
    "node:sea",
    "node:sqlite",
    "node:test",
    "node:test/reporters",
];

/// Node's `module.isBuiltin`: a listed name, or `node:` followed by one.
fn node_is_builtin(name: &str) -> bool {
    NODE_BUILTINS.contains(&name)
        || name
            .strip_prefix("node:")
            .is_some_and(|rest| NODE_BUILTINS.contains(&rest))
}

/// Whether `name` is a runtime built-in, honouring `builtInModules.override` and `.add`.
pub fn is_builtin(name: &str, built_in_modules: Option<&BuiltInModules>) -> bool {
    if let Some(list) = built_in_modules.and_then(|b| b.r#override.as_ref()) {
        return list.iter().any(|m| m == name);
    }
    node_is_builtin(name)
        || name.starts_with("bun:")
        || built_in_modules
            .and_then(|b| b.add.as_ref())
            .is_some_and(|add| add.iter().any(|m| m == name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_node_24s_is_builtin() {
        // The answers `node -e 'require("module").isBuiltin(x)'` gives on Node 24.
        for (name, expected) in [
            ("fs", true),
            ("node:fs", true),
            ("node:test", true),
            ("test", false),
            ("node:sea", true),
            ("fs/promises", true),
            ("node:fs/promises", true),
            ("_http_agent", true),
            ("node:_http_agent", true),
            ("lodash", false),
            ("node:lodash", false),
        ] {
            assert_eq!(is_builtin(name, None), expected, "{name}");
        }
        assert!(is_builtin("bun:ffi", None));
    }

    #[test]
    fn override_replaces_and_add_extends() {
        let override_only = BuiltInModules {
            r#override: Some(vec!["electron".to_owned()]),
            add: None,
        };
        assert!(is_builtin("electron", Some(&override_only)));
        assert!(!is_builtin("fs", Some(&override_only)));
        let add = BuiltInModules {
            r#override: None,
            add: Some(vec!["vscode".to_owned()]),
        };
        assert!(is_builtin("vscode", Some(&add)));
        assert!(is_builtin("fs", Some(&add)));
        assert!(!is_builtin("other", Some(&add)));
    }
}
