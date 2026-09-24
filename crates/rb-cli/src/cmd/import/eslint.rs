//! `rulebearing import eslint`: `import/no-restricted-paths` zones and `eslint-plugin-boundaries`
//! element types as `forbidden` rules.
//!
//! - Source: [design § The developer relations hat](../../../../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo)
//! - Plan: [Wave 2, Step 11](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f)
//! - Decisions: [ADR-0006](../../../../../docs/adr/0006-embedded-quickjs-config-evaluator.md) (the
//!   configuration is evaluated in the QuickJS sandbox, never by Node),
//!   [ADR-0016](../../../../../docs/adr/0016-linear-time-regex-and-strict-compat.md)
//! - Requirement: [FR-CLI-04](../../../../../docs/prd.md#fr-cli-04)
//!
//! A flat configuration (`eslint.config.{js,mjs,cjs}`) and a legacy one (`.eslintrc.*`, or
//! `eslintConfig` in `package.json`) are both read. A JavaScript configuration is evaluated in the
//! same sandbox as a JavaScript `rulebearing` configuration, with one change made to its text
//! first: an import or `require` of a package (a bare specifier that is not a Node built-in) is
//! replaced by an inert stand-in, so no plugin's code runs and a plugin need not be installed.
//! The stand-in answers every property with itself and every call with its arguments flattened,
//! which is what `tseslint.config(...)` and `defineConfig(...)` do with the entries they are
//! given. A Node built-in is left alone and the sandbox refuses it as it always does, so the
//! change adds no reach. Rules that come from a shareable configuration are therefore not read.
//!
//! | `ESLint` | Written as |
//! | --- | --- |
//! | `import/no-restricted-paths` zone `{ target, from, except }` | `from.path` the target, `to.path` the `from` paths, `to.pathNot` the exceptions |
//! | `boundaries/element-types` with `boundaries/elements` | per element type, one rule forbidding the element types it may not import, and one fencing two elements of its own type apart when that is disallowed |
//!
//! A plain path covers the folder and everything under it; a glob is anchored. Each emitted rule
//! is preceded by the `ESLint` setting it came from, as a comment.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use rb_config::js::{self, Kind, Limits};
use rb_config::read::{self, Syntax};
use regex::Regex;
use serde_json::{Map, Value};

use super::ImportError;
use super::pattern;
use super::yaml::{Document, Item, Node};

/// The file names searched for, in order, when `--from` is not given.
pub const DEFAULT_FILES: &[&str] = &[
    "eslint.config.js",
    "eslint.config.mjs",
    "eslint.config.cjs",
    ".eslintrc.js",
    ".eslintrc.cjs",
    ".eslintrc.yaml",
    ".eslintrc.yml",
    ".eslintrc.json",
    ".eslintrc",
    "package.json",
];

/// Node's built-in modules: an `ESLint` configuration importing one is refused by the sandbox,
/// never replaced by the stand-in. `path` and `url` are left out because the sandbox provides
/// pure versions of them ([ADR-0027](../../../../../docs/adr/0027-pure-path-and-url-modules-in-the-config-sandbox.md)).
const NODE_BUILTINS: &[&str] = &[
    "assert",
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
    "domain",
    "events",
    "fs",
    "fs/promises",
    "http",
    "http2",
    "https",
    "inspector",
    "module",
    "net",
    "os",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "repl",
    "stream",
    "string_decoder",
    "sys",
    "timers",
    "tls",
    "trace_events",
    "tty",
    "util",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
    "path",
    "path/posix",
    "url",
];

/// The stand-in a package import becomes.
const STUB: &str = "const __rb_stub = (() => { const target = function () {}; let proxy; const handler = { get: (t, key) => key === Symbol.toPrimitive ? () => \"\" : key === Symbol.iterator ? function* () {} : key === \"then\" || key === \"toJSON\" ? undefined : proxy, apply: (t, self, args) => args.flat(Infinity), construct: () => proxy }; proxy = new Proxy(target, handler); return proxy; })();\n";

/// Whether `file` holds an `ESLint` configuration.
pub fn holds_config(file: &Path) -> bool {
    if file.file_name().is_some_and(|n| n == "package.json") {
        return std::fs::read_to_string(file)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .is_some_and(|v| v.get("eslintConfig").is_some());
    }
    file.is_file()
}

fn is_package(specifier: &str) -> bool {
    !(specifier.starts_with('.')
        || specifier.starts_with('/')
        || specifier.starts_with("node:")
        || NODE_BUILTINS.contains(&specifier))
}

/// The declarations an import clause becomes when its module is the stand-in.
fn stub_declarations(clause: &str) -> String {
    let clause = clause.trim();
    let mut out = String::new();
    let (default, rest) = match clause.find(['{', '*']) {
        Some(0) => ("", clause),
        Some(at) => (
            clause[..at].trim().trim_end_matches(',').trim(),
            &clause[at..],
        ),
        None => (clause, ""),
    };
    if !default.is_empty() {
        let _ = write!(out, "const {default} = __rb_stub; ");
    }
    let rest = rest.trim();
    if let Some(name) = rest.strip_prefix('*') {
        let name = name.trim().trim_start_matches("as").trim();
        let _ = write!(out, "const {name} = __rb_stub; ");
    } else if let Some(inner) = rest.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
        let names: Vec<String> = inner
            .split(',')
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(|n| match n.split_once(" as ") {
                Some((from, to)) => format!("{}: {}", from.trim(), to.trim()),
                None => n.to_owned(),
            })
            .collect();
        let _ = write!(out, "const {{ {} }} = __rb_stub; ", names.join(", "));
    }
    out
}

/// The configuration's text with package imports replaced by the stand-in and its export wrapped
/// in an object, which is what the sandbox returns.
///
/// # Errors
/// [`ImportError::Invalid`] for an ES module without `export default`.
pub fn prepare(text: &str, kind: Kind, file: &str) -> Result<String, ImportError> {
    let invalid = |e: regex::Error| ImportError::Invalid(e.to_string());
    let import =
        Regex::new(r#"(?m)^[ \t]*import[ \t]+([\w$*{}\s,]+?)\s+from\s*['"]([^'"]+)['"][ \t]*;?"#)
            .map_err(invalid)?;
    let bare = Regex::new(r#"(?m)^[ \t]*import[ \t]*['"]([^'"]+)['"][ \t]*;?"#).map_err(invalid)?;
    let require = Regex::new(r#"require\(\s*['"]([^'"]+)['"]\s*\)"#).map_err(invalid)?;
    let text = import.replace_all(text, |c: &regex::Captures<'_>| {
        if is_package(&c[2]) {
            stub_declarations(&c[1])
        } else {
            c[0].to_owned()
        }
    });
    let text = bare.replace_all(&text, |c: &regex::Captures<'_>| {
        if is_package(&c[1]) {
            String::new()
        } else {
            c[0].to_owned()
        }
    });
    let text = require.replace_all(&text, |c: &regex::Captures<'_>| {
        if is_package(&c[1]) {
            "__rb_stub".to_owned()
        } else {
            c[0].to_owned()
        }
    });
    match kind {
        Kind::Module => {
            let export = Regex::new(r"(?m)^[ \t]*export[ \t]+default[ \t]+").map_err(invalid)?;
            if !export.is_match(&text) {
                return Err(ImportError::Invalid(format!(
                    "{file} has no `export default`"
                )));
            }
            let body = export.replace(&text, "globalThis.__rb_eslint = ");
            Ok(format!(
                "{STUB}{body}\nexport default {{ config: globalThis.__rb_eslint }};\n"
            ))
        }
        Kind::CommonJs | Kind::Json5 => Ok(format!(
            "{STUB}{text}\nmodule.exports = {{ config: module.exports }};\n"
        )),
    }
}

/// Reads a configuration into JSON: JavaScript in the sandbox, the rest as data.
fn read_config(file: &Path, display: &str) -> Result<Value, ImportError> {
    let text = std::fs::read_to_string(file).map_err(|e| ImportError::Read {
        file: display.to_owned(),
        reason: e.to_string(),
    })?;
    let dir = file.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    let root = rb_config::load::repository_root(&dir);
    let parse = |e: rb_config::ConfigError| ImportError::Parse {
        file: display.to_owned(),
        reason: e.to_string(),
    };
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match file.extension().and_then(|e| e.to_str()) {
        Some("js" | "mjs" | "cjs") => {
            let kind = js::kind_of(file, &text, &root);
            let prepared = prepare(&text, kind, display)?;
            let evaluated = js::evaluate_text(file, &prepared, kind, &root, Limits::default())
                .map_err(|e| ImportError::Parse {
                    file: display.to_owned(),
                    reason: e.to_string(),
                })?;
            Ok(evaluated
                .value
                .get("config")
                .cloned()
                .unwrap_or(Value::Null))
        }
        Some("yaml" | "yml") => {
            read::parse_text(&text, Syntax::Yaml, file, &root, Limits::default()).map_err(parse)
        }
        _ if name == "package.json" => serde_json::from_str::<Value>(&text)
            .map(|v| v.get("eslintConfig").cloned().unwrap_or(Value::Null))
            .map_err(|e| ImportError::Parse {
                file: display.to_owned(),
                reason: e.to_string(),
            }),
        _ => read::parse_text(&text, Syntax::Jsonc, file, &root, Limits::default())
            .or_else(|_| read::parse_text(&text, Syntax::Yaml, file, &root, Limits::default()))
            .map_err(parse),
    }
}

/// One configuration object: where it sits, the files it applies to, its rules and settings.
struct Entry {
    origin: String,
    files: Vec<String>,
    rules: Map<String, Value>,
    settings: Map<String, Value>,
}

fn strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

fn entry(origin: String, object: &Map<String, Value>) -> Entry {
    let object_of = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    };
    Entry {
        origin,
        files: strings(object.get("files")),
        rules: object_of("rules"),
        settings: object_of("settings"),
    }
}

/// The configuration objects of a flat array or a legacy object with `overrides`.
fn entries(config: &Value) -> Vec<Entry> {
    match config {
        Value::Array(items) => {
            let mut out = Vec::new();
            let mut stack: Vec<(String, &Value)> = items
                .iter()
                .enumerate()
                .rev()
                .map(|(i, v)| (format!("config[{i}]"), v))
                .collect();
            while let Some((origin, value)) = stack.pop() {
                match value {
                    Value::Object(object) => out.push(entry(origin, object)),
                    Value::Array(inner) => stack.extend(
                        inner
                            .iter()
                            .enumerate()
                            .rev()
                            .map(|(i, v)| (format!("{origin}[{i}]"), v)),
                    ),
                    _ => {}
                }
            }
            out
        }
        Value::Object(object) => {
            let mut out = vec![entry("config".into(), object)];
            for (i, value) in object
                .get("overrides")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                if let Value::Object(o) = value {
                    out.push(entry(format!("overrides[{i}]"), o));
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

/// A rule's severity and options, or `None` when it is off.
fn severity_and_options(value: &Value) -> Option<(&'static str, Vec<Value>)> {
    let (level, options) = match value {
        Value::Array(items) => (items.first()?, items[1..].to_vec()),
        other => (other, Vec::new()),
    };
    let severity = match level {
        Value::String(s) if s == "error" => "error",
        Value::String(s) if s == "warn" => "warn",
        Value::Number(n) if n.as_u64() == Some(2) => "error",
        Value::Number(n) if n.as_u64() == Some(1) => "warn",
        _ => return None,
    };
    Some((severity, options))
}

/// Folds an absolute path from the configuration (`path.resolve(__dirname, "src")`) back to one
/// relative to the configuration's folder.
fn under(dir: &str, path: &str) -> String {
    let path = path.replace('\\', "/");
    let dir = dir.trim_end_matches('/');
    path.strip_prefix(dir)
        .map_or(path.clone(), |rest| rest.trim_start_matches('/').to_owned())
}

fn path_node(patterns: Vec<String>) -> Node {
    if patterns.len() == 1 {
        Node::str(patterns.into_iter().next().unwrap_or_default())
    } else {
        Node::strs(&patterns)
    }
}

fn restricted_paths(
    entry: &Entry,
    rule: &str,
    severity: &str,
    options: &[Value],
    context: &Context<'_>,
    items: &mut Vec<Item>,
) {
    let settings = options.first().and_then(Value::as_object);
    let base = settings
        .and_then(|s| s.get("basePath"))
        .and_then(Value::as_str)
        .map_or_else(String::new, |b| under(context.dir, b));
    let zones = settings
        .and_then(|s| s.get("zones"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (index, zone) in zones.iter().enumerate() {
        let relative = |p: &str| pattern::join(&base, &under(context.dir, p));
        let targets: Vec<String> = strings(zone.get("target"))
            .iter()
            .map(|t| pattern::path_or_glob(&relative(t)))
            .collect();
        let froms = strings(zone.get("from"));
        let excepts = strings(zone.get("except"));
        let to: Vec<String> = froms
            .iter()
            .map(|f| pattern::path_or_glob(&relative(f)))
            .collect();
        let mut not = Vec::new();
        for from in &froms {
            for except in &excepts {
                if pattern::is_glob(from) {
                    not.push(pattern::path_or_glob(&relative(except)));
                } else {
                    not.push(pattern::path_or_glob(&pattern::join(
                        &relative(from),
                        except,
                    )));
                }
            }
        }
        let mut comments = vec![format!(
            "{rule} ({}), zones[{index}]: {}",
            entry.origin,
            serde_json::to_string(zone).unwrap_or_default()
        )];
        if !entry.files.is_empty() {
            comments.push(format!(
                "ESLint applied this rule to files matching {}; the target above is what the rule checks",
                entry.files.join(", ")
            ));
        }
        let mut comment = format!("imported from {}: {rule} zones[{index}]", context.display);
        if let Some(message) = zone.get("message").and_then(Value::as_str) {
            let _ = write!(comment, " ({message})");
        }
        let mut to_node = vec![("path", path_node(to.clone()))];
        if !not.is_empty() {
            to_node.push(("pathNot", path_node(not)));
        }
        let disabled = targets.is_empty() || to.is_empty();
        if disabled {
            comments.push("the zone has no target or no from: nothing to forbid".into());
        }
        items.push(Item {
            comments,
            node: Node::map(vec![
                (
                    "name",
                    Node::str(format!("no-restricted-paths:{}", context.next_zone())),
                ),
                ("comment", Node::str(comment)),
                ("severity", Node::str(severity)),
                ("from", Node::map(vec![("path", path_node(targets))])),
                ("to", Node::map(to_node)),
            ]),
            disabled,
        });
    }
}

/// One `boundaries/elements` entry: its type, and the path pattern of its elements with the
/// element itself as the first capturing group.
struct Element {
    kind: String,
    pattern: Result<String, String>,
    source: Value,
}

fn glob_body(glob: &str) -> String {
    let anchored = pattern::glob(glob);
    anchored
        .strip_prefix('^')
        .and_then(|b| b.strip_suffix('$'))
        .unwrap_or(&anchored)
        .to_owned()
}

fn element(value: &Value) -> Option<Element> {
    let object = value.as_object()?;
    let kind = object.get("type")?.as_str()?.to_owned();
    let patterns = strings(object.get("pattern"));
    let mode = object
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("folder");
    let pattern = if object.contains_key("basePattern") {
        Err("`basePattern` narrows where the pattern applies, which the importer does not translate".to_owned())
    } else if patterns.is_empty() {
        Err("the element has no pattern".to_owned())
    } else {
        let bodies = patterns
            .iter()
            .map(|p| glob_body(&pattern::relative(p)))
            .collect::<Vec<_>>()
            .join("|");
        match mode {
            "folder" => Ok(format!("(?:^|/)((?:{bodies}))/")),
            "file" => Ok(format!("(?:^|/)((?:{bodies}))$")),
            "full" => Ok(format!("^((?:{bodies}))$")),
            other => Err(format!("mode `{other}` is not folder, file or full")),
        }
    };
    Some(Element {
        kind,
        pattern,
        source: value.clone(),
    })
}

/// A selector of element types: a type name or a glob over names; a captured-value selector is
/// refused.
fn selected(selector: &Value, elements: &[Element]) -> Result<Vec<String>, String> {
    let names: Vec<String> = match selector {
        Value::String(s) => vec![s.clone()],
        Value::Array(items) if items.len() == 2 && items[0].is_string() && items[1].is_object() => {
            return Err(format!(
                "`{}` selects by captured values, which the importer does not translate",
                serde_json::to_string(selector).unwrap_or_default()
            ));
        }
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                out.extend(selected(item, elements)?);
            }
            return Ok(out);
        }
        other => {
            return Err(format!(
                "`{}` is not an element type",
                serde_json::to_string(other).unwrap_or_default()
            ));
        }
    };
    let mut out = Vec::new();
    for name in names {
        if name.contains("${") {
            return Err(format!(
                "`{name}` is a template over captured values, which the importer does not translate"
            ));
        }
        let matcher = Regex::new(&pattern::glob(&name)).map_err(|e| e.to_string())?;
        out.extend(
            elements
                .iter()
                .filter(|e| matcher.is_match(&e.kind))
                .map(|e| e.kind.clone()),
        );
    }
    Ok(out)
}

/// The element types each type may not import: the default, then each rule in order, a later
/// rule overriding an earlier one.
fn disallowed(
    options: &Map<String, Value>,
    elements: &[Element],
) -> Result<Vec<(String, Vec<String>)>, String> {
    let kinds: Vec<String> = elements.iter().fold(Vec::new(), |mut k, e| {
        if !k.contains(&e.kind) {
            k.push(e.kind.clone());
        }
        k
    });
    let default_allow = options.get("default").and_then(Value::as_str) != Some("disallow");
    let rules = options
        .get("rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for kind in &kinds {
        let mut allowed: std::collections::BTreeSet<String> = if default_allow {
            kinds.iter().cloned().collect()
        } else {
            std::collections::BTreeSet::new()
        };
        for rule in &rules {
            let from = selected(rule.get("from").unwrap_or(&Value::Null), elements)?;
            if !from.contains(kind) {
                continue;
            }
            if let Some(allow) = rule.get("allow") {
                allowed.extend(selected(allow, elements)?);
            }
            if let Some(disallow) = rule.get("disallow") {
                for name in selected(disallow, elements)? {
                    allowed.remove(&name);
                }
            }
        }
        let denied: Vec<String> = kinds
            .iter()
            .filter(|k| !allowed.contains(*k))
            .cloned()
            .collect();
        out.push((kind.clone(), denied));
    }
    Ok(out)
}

fn boundaries(
    entry: &Entry,
    all: &[Entry],
    severity: &str,
    options: &[Value],
    context: &Context<'_>,
    items: &mut Vec<Item>,
) {
    let elements_value = entry
        .settings
        .get("boundaries/elements")
        .or_else(|| {
            all.iter()
                .rev()
                .find_map(|e| e.settings.get("boundaries/elements"))
        })
        .cloned()
        .unwrap_or(Value::Array(Vec::new()));
    let elements: Vec<Element> = elements_value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(element)
        .collect();
    let rule_options = options
        .first()
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let original = format!(
        "boundaries/element-types ({}): {}",
        entry.origin,
        serde_json::to_string(&rule_options).unwrap_or_default()
    );
    let mut first = true;
    let mut push = |mut comments: Vec<String>,
                    node: Node,
                    disabled: bool,
                    items: &mut Vec<Item>| {
        if first {
            comments.insert(0, original.clone());
            comments.insert(
                1,
                "eslint-plugin-boundaries gives a file to the first element whose pattern matches; the patterns below do not encode that order".into(),
            );
            first = false;
        }
        items.push(Item {
            comments,
            node,
            disabled,
        });
    };
    let table = match disallowed(&rule_options, &elements) {
        Ok(table) => table,
        Err(reason) => {
            push(
                vec![format!("not imported: {reason}")],
                Node::map(vec![("name", Node::str("boundaries"))]),
                true,
                items,
            );
            return;
        }
    };
    let comment = format!(
        "imported from {}: boundaries/element-types",
        context.display
    );
    for (kind, denied) in table {
        for (comments, node, disabled) in kind_rules(&kind, &denied, &elements, severity, &comment)
        {
            push(comments, node, disabled, items);
        }
    }
}

/// The rules for one element type: the types it may not import, and the fence between two
/// elements of its own type when that is disallowed. Each is `(comments, rule, disabled)`.
fn kind_rules(
    kind: &str,
    denied: &[String],
    elements: &[Element],
    severity: &str,
    comment: &str,
) -> Vec<(Vec<String>, Node, bool)> {
    let pattern_of = |kind: &str| -> Result<Vec<String>, String> {
        elements
            .iter()
            .filter(|e| e.kind == kind)
            .map(|e| e.pattern.clone())
            .collect()
    };
    let comment = format!("{comment}, element type `{kind}`");
    let mut out = Vec::new();
    let sources: Vec<String> = elements
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| {
            format!(
                "boundaries/elements: {}",
                serde_json::to_string(&e.source).unwrap_or_default()
            )
        })
        .collect();
    let from = pattern_of(kind);
    let others: Result<Vec<String>, String> = denied
        .iter()
        .filter(|d| *d != kind)
        .map(|d| pattern_of(d))
        .collect::<Result<Vec<_>, _>>()
        .map(|v| v.into_iter().flatten().collect());
    match (&from, &others) {
        (Ok(from), Ok(to)) if !to.is_empty() => out.push((
            sources,
            Node::map(vec![
                ("name", Node::str(format!("boundaries:{kind}"))),
                ("comment", Node::str(comment.clone())),
                ("severity", Node::str(severity)),
                ("from", Node::map(vec![("path", path_node(from.clone()))])),
                ("to", Node::map(vec![("path", path_node(to.clone()))])),
            ]),
            false,
        )),
        (Err(reason), _) | (_, Err(reason)) => out.push((
            vec![format!("not imported: {reason}")],
            Node::map(vec![("name", Node::str(format!("boundaries:{kind}")))]),
            true,
        )),
        _ => {}
    }
    let name = Node::str(format!("boundaries:{kind}-to-{kind}"));
    match &from {
        Ok(from) if denied.iter().any(|d| d == kind) && from.len() > 1 => out.push((
            vec![format!(
                "two `{kind}` elements may not import each other, but the type has several patterns, so the elements cannot be told apart by one capture"
            )],
            Node::map(vec![("name", name)]),
            true,
        )),
        Ok(from) if denied.iter().any(|d| d == kind) => {
            // Two elements of one type: the file's own element, captured, is left out.
            let fences: Vec<String> = from
                .iter()
                .map(|p| {
                    let start = p.find("((?:").unwrap_or(0);
                    let end = p.rfind("))").map_or(p.len(), |e| e + 2);
                    format!("{}$1{}", &p[..start], &p[end..])
                })
                .collect();
            out.push((
                vec![format!(
                    "two `{kind}` elements may not import each other; one importing itself is not checked, as in eslint-plugin-boundaries"
                )],
                Node::map(vec![
                    ("name", name),
                    ("comment", Node::str(comment)),
                    ("severity", Node::str(severity)),
                    ("from", Node::map(vec![("path", path_node(from.clone()))])),
                    (
                        "to",
                        Node::map(vec![
                            ("path", path_node(from.clone())),
                            ("pathNot", path_node(fences)),
                        ]),
                    ),
                ]),
                false,
            ));
        }
        _ => {}
    }
    out
}

/// What a rule needs to know about the file it came from.
struct Context<'a> {
    display: &'a str,
    dir: &'a str,
    zones: std::cell::Cell<usize>,
}

impl Context<'_> {
    fn next_zone(&self) -> usize {
        let n = self.zones.get() + 1;
        self.zones.set(n);
        n
    }
}

/// `ESLint` rule names the importer reads.
const RESTRICTED: &[&str] = &["import/no-restricted-paths", "import-x/no-restricted-paths"];

/// Imports the `ESLint` configuration in `file`, displayed as `display`.
///
/// # Errors
/// [`ImportError`] when the file cannot be read or evaluated.
pub fn import(file: &Path, display: &str) -> Result<Document, ImportError> {
    let config = read_config(file, display)?;
    if config.is_null() {
        return Err(ImportError::Invalid(format!(
            "{display} holds no ESLint configuration"
        )));
    }
    let dir = file
        .parent()
        .and_then(|d| d.canonicalize().ok())
        .map(|d| d.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let context = Context {
        display,
        dir: &dir,
        zones: std::cell::Cell::new(0),
    };
    let all = entries(&config);
    let mut items = Vec::new();
    let mut skipped = Vec::new();
    for entry in &all {
        for (name, value) in &entry.rules {
            let Some((severity, options)) = severity_and_options(value) else {
                continue;
            };
            if RESTRICTED.contains(&name.as_str()) {
                restricted_paths(entry, name, severity, &options, &context, &mut items);
            } else if name == "boundaries/element-types" {
                boundaries(entry, &all, severity, &options, &context, &mut items);
            } else if name.starts_with("boundaries/") {
                skipped.push(format!("{name} ({})", entry.origin));
            }
        }
    }
    let mut header = vec![
        format!("Imported from {display} by `rulebearing import eslint`."),
        "Each rule is preceded by the ESLint setting it came from. Package imports in the configuration were not run, so rules from shareable configurations are not here.".into(),
    ];
    if !skipped.is_empty() {
        header.push(format!(
            "Not imported (no forbidden-rule equivalent in this importer): {}",
            skipped.join(", ")
        ));
    }
    if items.is_empty() {
        header.push(
            "No import/no-restricted-paths zone or boundaries/element-types rule was found.".into(),
        );
    }
    let mut body = vec![("$schema".to_owned(), Node::str(super::SCHEMA))];
    let rules = if items.is_empty() {
        Node::Map(Vec::new())
    } else {
        Node::map(vec![(
            "dependencies",
            Node::map(vec![("forbidden", Node::List(items))]),
        )])
    };
    body.push(("rules".to_owned(), rules));
    Ok(Document {
        header,
        body,
        key_comments: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_clauses_become_stub_declarations() {
        assert_eq!(stub_declarations("x"), "const x = __rb_stub; ");
        assert_eq!(stub_declarations("* as x"), "const x = __rb_stub; ");
        assert_eq!(
            stub_declarations("{ a, b as c }"),
            "const { a, b: c } = __rb_stub; "
        );
        assert_eq!(
            stub_declarations("d, { a }"),
            "const d = __rb_stub; const { a } = __rb_stub; "
        );
    }

    #[test]
    fn package_imports_are_replaced_and_built_ins_kept() -> Result<(), ImportError> {
        let text = "import a from \"eslint-plugin-import\";\nimport fs from \"fs\";\nimport p from \"node:path\";\nimport \"side-effect\";\nimport local from \"./local.js\";\nconst b = require('pkg');\nconst c = require('child_process');\nexport default [a];\n";
        let prepared = prepare(text, Kind::Module, "x")?;
        assert!(prepared.contains("const a = __rb_stub;"));
        assert!(prepared.contains("import fs from \"fs\";"));
        assert!(prepared.contains("import p from \"node:path\";"));
        assert!(!prepared.contains("side-effect"));
        assert!(prepared.contains("import local from \"./local.js\";"));
        assert!(prepared.contains("const b = __rb_stub;"));
        assert!(prepared.contains("require('child_process')"));
        assert!(prepared.contains("globalThis.__rb_eslint = [a];"));
        assert!(prepare("const x = 1;", Kind::Module, "x").is_err());
        let cjs = prepare("module.exports = {};", Kind::CommonJs, "x")?;
        assert!(cjs.ends_with("module.exports = { config: module.exports };\n"));
        Ok(())
    }

    #[test]
    fn severities_and_options() {
        assert_eq!(severity_and_options(&Value::from("off")), None);
        assert_eq!(severity_and_options(&Value::from(0)), None);
        assert_eq!(
            severity_and_options(&Value::from(1)).map(|s| s.0),
            Some("warn")
        );
        let with = serde_json::json!(["error", {"zones": []}]);
        let (severity, options) = severity_and_options(&with).unwrap_or(("", Vec::new()));
        assert_eq!(severity, "error");
        assert_eq!(options.len(), 1);
        assert_eq!(severity_and_options(&serde_json::json!([])), None);
    }

    #[test]
    fn absolute_paths_fold_under_the_configuration() {
        assert_eq!(under("/repo", "/repo/src/a"), "src/a");
        assert_eq!(under("/repo/", "./src"), "./src");
    }

    #[test]
    fn elements_by_mode() {
        let folder = element(&serde_json::json!({"type": "c", "pattern": "components/*"}));
        let pattern = folder.and_then(|e| e.pattern.ok()).unwrap_or_default();
        assert_eq!(pattern, "(?:^|/)((?:components/[^/]*))/");
        let re = Regex::new(&pattern).ok();
        assert!(
            re.as_ref()
                .is_some_and(|r| r.is_match("src/components/a/b.ts"))
        );
        let file =
            element(&serde_json::json!({"type": "h", "pattern": "helpers/*.js", "mode": "file"}));
        assert!(file.is_some_and(|e| e.pattern.is_ok_and(|p| p.ends_with("))$"))));
        let base = element(&serde_json::json!({"type": "h", "pattern": "x", "basePattern": "y"}));
        assert!(base.is_some_and(|e| e.pattern.is_err()));
        let odd = element(&serde_json::json!({"type": "h", "pattern": "x", "mode": "odd"}));
        assert!(odd.is_some_and(|e| e.pattern.is_err()));
        assert!(element(&serde_json::json!({"pattern": "x"})).is_none());
    }

    #[test]
    fn element_type_rules_override_in_order() {
        let elements: Vec<Element> = ["a", "b", "c"]
            .iter()
            .filter_map(|k| element(&serde_json::json!({"type": k, "pattern": format!("{k}/*")})))
            .collect();
        let options = serde_json::json!({
            "default": "disallow",
            "rules": [
                {"from": "a", "allow": ["b", "c"]},
                {"from": ["a"], "disallow": "c"},
                {"from": "*", "allow": "a"}
            ]
        });
        let table =
            disallowed(options.as_object().unwrap_or(&Map::new()), &elements).unwrap_or_default();
        assert_eq!(
            table,
            [
                ("a".to_owned(), vec!["c".to_owned()]),
                ("b".to_owned(), vec!["b".to_owned(), "c".to_owned()]),
                ("c".to_owned(), vec!["b".to_owned(), "c".to_owned()]),
            ]
        );
        let captured = serde_json::json!({"rules": [{"from": ["a", {"x": "y"}], "allow": "b"}]});
        assert!(disallowed(captured.as_object().unwrap_or(&Map::new()), &elements).is_err());
        let template = serde_json::json!({"rules": [{"from": "a", "allow": "${from.x}"}]});
        assert!(disallowed(template.as_object().unwrap_or(&Map::new()), &elements).is_err());
    }
}
