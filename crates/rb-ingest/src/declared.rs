//! Declared edges: `edges.yaml`, and the OpenAPI client and server pairs it names, into
//! dependencies marked `declared: true` and `dependencyKind: declared`.
//!
//! - Architecture: [The graph document](../../../docs/architecture.md#the-graph-document)
//! - Decisions: [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md) (declared
//!   edges are the named wave 4 addition, marked, never inferred),
//!   [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md) (the marks are
//!   additive and `--strict-schema` strips them),
//!   [ADR-0076](../../../docs/adr/0076-fleet-and-declared-edges.md) (where the edges attach, how
//!   an OpenAPI operation names its handler, what a client call is)
//! - Plan: [plan 0004, Step 12](../../../docs/plans/pending/0004-wave-4-reach.md#24-steps-for-sub-wave-4d-fleet-and-declared-edges),
//!   [§ 1.6](../../../docs/plans/pending/0004-wave-4-reach.md#16-interfaces-and-contracts-this-wave-freezes)
//!   (the frozen `edges.yaml`), [§ 1.7](../../../docs/plans/pending/0004-wave-4-reach.md#17-decisions-applied-and-decisions-this-wave-must-make)
//!   (OpenAPI edges read only from a named client directory, by operation id)
//! - Requirement: [FR-REACH-03](../../../docs/prd.md#fr-reach-03)
//!
//! Nothing here reads a file: the caller hands over the text of `edges.yaml`, of each server
//! document and of each client file, so this crate stays on the ingest side of the extractor
//! boundary ([ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md)).
//!
//! | Source | `from` | `to` | `module` | `declaredKind` | `comment` |
//! | --- | --- | --- | --- | --- | --- |
//! | `declared[]` | the entry's `from` | the entry's `to` | the entry's `to` | the entry's `kind` | the entry's `comment` |
//! | `openapi[]` | each client file that names the operation id | the handler the server document records | the operation id | `http` | the operation, its method and path, and the document |
//!
//! An OpenAPI operation declares an edge only when the server document records both its
//! `operationId` and its handler, the file under the server's repository named by the
//! operation's [`HANDLER_KEY`] extension; and only to the client files that name the operation id
//! as a whole identifier, exactly as the document spells it. An operation without an id, or
//! without a handler, or that no client file names, declares nothing, and the
//! [`OpenApiReceipt`] counts it; when a pair declares nothing at all, the receipt says why.
//! A path item given by `$ref` is not followed.

use std::collections::{BTreeMap, BTreeSet};

use rb_model::{
    Dependency, DependencyKind, GraphDocument, Language, ModuleSystem, OpenApiReceipt, SkippedEdge,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The operation extension that names the file handling an operation, relative to the
/// repository the server document is in.
pub const HANDLER_KEY: &str = "x-rulebearing-handler";

/// The `declaredKind` of an edge an OpenAPI document declares.
pub const OPENAPI_KIND: &str = "http";

/// The HTTP methods an OpenAPI path item may hold an operation under, in the specification's
/// order.
pub const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// `edges.yaml`: the edges declared by hand, and the OpenAPI client and server pairs whose
/// operations declare more.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(title = "Rulebearing declared edges")]
pub struct EdgesFile {
    /// The schema this file is written against.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Edges declared one by one.
    #[serde(default)]
    pub declared: Vec<DeclaredEdge>,
    /// OpenAPI client folders and the server documents their operations are matched against.
    #[serde(default)]
    pub openapi: Vec<OpenApiPair>,
}

/// One edge declared by hand.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeclaredEdge {
    /// The module the edge leaves, as the graph names it (in a `fleet`, prefixed with the
    /// repository's name: `web/src/api/client.ts`).
    pub from: String,
    /// The module the edge reaches, as the graph names it.
    pub to: String,
    /// What the edge stands for: `http`, `queue`, or any word the team uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Why the edge exists; a decision token (`adr:0007`) makes it citable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// One OpenAPI client folder and the server document it calls.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenApiPair {
    /// The folder of the generated client, as the graph names it (`web/src/api/generated/`):
    /// only its modules are read for operation ids.
    pub client: String,
    /// The server's OpenAPI document (JSON or YAML), as the graph names its path
    /// (`api/openapi.json`).
    pub server: String,
}

impl OpenApiPair {
    /// The client folder with exactly one trailing `/`, so `web/gen` does not take in
    /// `web/generated/`.
    pub fn client_prefix(&self) -> String {
        format!("{}/", self.client.trim_end_matches('/'))
    }
}

/// Why declared edges could not be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DeclaredError {
    /// `edges.yaml` is not the format.
    #[error("edges.yaml: {0}")]
    Edges(String),
    /// A server document is not an OpenAPI document.
    #[error("{server}: {reason}")]
    OpenApi {
        /// The document, as `edges.yaml` names it.
        server: String,
        /// What is wrong with it.
        reason: String,
    },
}

/// Reads `edges.yaml`. Every `from`, `to`, `client` and `server` must be a non-empty relative
/// path.
///
/// # Errors
/// [`DeclaredError::Edges`] naming the entry, when the text is not the format.
pub fn read_edges(text: &str) -> Result<EdgesFile, DeclaredError> {
    let file: EdgesFile =
        serde_yaml::from_str(text).map_err(|e| DeclaredError::Edges(e.to_string()))?;
    let checked = |what: String, path: &str| {
        if path.trim().is_empty() {
            Err(DeclaredError::Edges(format!("{what} is empty")))
        } else if path.starts_with('/') || path.contains('\\') || path.split('/').any(|s| s == "..")
        {
            Err(DeclaredError::Edges(format!(
                "{what} `{path}` is not a path in the graph: write it as the graph names the module, with `/`, relative and without `..`"
            )))
        } else {
            Ok(())
        }
    };
    for (index, edge) in file.declared.iter().enumerate() {
        checked(format!("declared[{index}].from"), &edge.from)?;
        checked(format!("declared[{index}].to"), &edge.to)?;
    }
    for (index, pair) in file.openapi.iter().enumerate() {
        checked(format!("openapi[{index}].client"), &pair.client)?;
        checked(format!("openapi[{index}].server"), &pair.server)?;
    }
    Ok(file)
}

/// One operation of a server document.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Operation {
    /// The path template (`/orders/{id}`).
    pub path: String,
    /// The method, upper case (`POST`).
    pub method: String,
    /// The `operationId`, when the document records one.
    pub operation_id: Option<String>,
    /// The [`HANDLER_KEY`] extension, when the document records one.
    pub handler: Option<String>,
}

/// Reads the operations of an OpenAPI (3.x) or Swagger (2.0) document, JSON or YAML, sorted by
/// path then method.
///
/// # Errors
/// [`DeclaredError::OpenApi`] when the text is neither, or has no `paths` object.
pub fn read_openapi(server: &str, text: &str) -> Result<Vec<Operation>, DeclaredError> {
    let failed = |reason: String| DeclaredError::OpenApi {
        server: server.to_owned(),
        reason,
    };
    let document: Value = if text.trim_start().starts_with('{') {
        serde_json::from_str(text).map_err(|e| failed(format!("not JSON: {e}")))?
    } else {
        serde_yaml::from_str(text).map_err(|e| failed(format!("not YAML: {e}")))?
    };
    if document.get("openapi").is_none() && document.get("swagger").is_none() {
        return Err(failed(
            "not an OpenAPI document: it has neither `openapi` nor `swagger`".into(),
        ));
    }
    let Some(paths) = document.get("paths").and_then(Value::as_object) else {
        return Err(failed("it has no `paths` object".into()));
    };
    let text_of = |value: &Value, key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let mut operations = Vec::new();
    for (path, item) in paths {
        for method in METHODS {
            if let Some(operation) = item.get(method).filter(|o| o.is_object()) {
                operations.push(Operation {
                    path: path.clone(),
                    method: method.to_uppercase(),
                    operation_id: text_of(operation, "operationId"),
                    handler: text_of(operation, HANDLER_KEY),
                });
            }
        }
    }
    operations.sort();
    Ok(operations)
}

/// Whether `source` names `id` as a whole identifier: an occurrence not preceded or followed by
/// a letter, digit, `_` or `$`, spelled exactly as the document spells it.
pub fn names(source: &str, id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    let part = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    source.match_indices(id).any(|(at, _)| {
        let before = source[..at].chars().next_back();
        let after = source[at + id.len()..].chars().next();
        !before.is_some_and(part) && !after.is_some_and(part)
    })
}

/// Where a declaration came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// `edges.yaml`'s `declared` list.
    EdgesYaml,
    /// An OpenAPI client and server pair.
    OpenApi,
}

/// One edge to declare.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Declaration {
    /// The module the edge leaves.
    pub from: String,
    /// The module the edge reaches.
    pub to: String,
    /// The dependency's `module`: the `to` path, or the operation id.
    pub module: String,
    /// The dependency's `declaredKind`.
    pub kind: Option<String>,
    /// The dependency's `comment`.
    pub comment: Option<String>,
    /// Where it came from.
    pub source: Source,
}

/// The declarations of `edges.yaml`'s `declared` list, in its order.
pub fn hand_declarations(file: &EdgesFile) -> Vec<Declaration> {
    file.declared
        .iter()
        .map(|edge| Declaration {
            from: edge.from.clone(),
            to: edge.to.clone(),
            module: edge.to.clone(),
            kind: edge.kind.clone(),
            comment: edge.comment.clone(),
            source: Source::EdgesYaml,
        })
        .collect()
}

/// The declarations one OpenAPI pair makes, and its receipt. `handler_base` is what a handler
/// path is joined to: the graph's name for the folder of the repository the server document is
/// in (`api/` in a `fleet`). `clients` are the client folder's modules with their text, as
/// `(module, text)`.
pub fn openapi_declarations(
    pair: &OpenApiPair,
    operations: &[Operation],
    handler_base: &str,
    clients: &[(String, String)],
) -> (Vec<Declaration>, OpenApiReceipt) {
    let mut receipt = OpenApiReceipt {
        client: pair.client.clone(),
        server: pair.server.clone(),
        operations: operations.len() as u64,
        ..OpenApiReceipt::default()
    };
    let mut declarations = Vec::new();
    for operation in operations {
        let Some(id) = &operation.operation_id else {
            receipt.without_operation_id += 1;
            continue;
        };
        let Some(handler) = &operation.handler else {
            receipt.without_handler += 1;
            continue;
        };
        let to = join(handler_base, handler);
        let callers: Vec<&String> = clients
            .iter()
            .filter(|(_, text)| names(text, id))
            .map(|(module, _)| module)
            .collect();
        if callers.is_empty() {
            receipt.uncalled += 1;
        }
        for from in callers {
            declarations.push(Declaration {
                from: from.clone(),
                to: to.clone(),
                module: id.clone(),
                kind: Some(OPENAPI_KIND.to_owned()),
                comment: Some(format!(
                    "operation {id} ({} {}) in {}",
                    operation.method, operation.path, pair.server
                )),
                source: Source::OpenApi,
            });
        }
    }
    receipt.edges = declarations.len() as u64;
    receipt.reason = (receipt.edges == 0).then(|| why_none(&receipt, clients.is_empty()));
    (declarations, receipt)
}

/// Why a pair declared no edge, by the first thing that stopped every operation.
fn why_none(receipt: &OpenApiReceipt, no_clients: bool) -> String {
    let with_id = receipt.operations - receipt.without_operation_id;
    if receipt.operations == 0 {
        "the server document lists no operations".into()
    } else if with_id == 0 {
        "the server document records no operationId, so no operation can be matched to a client call".into()
    } else if with_id == receipt.without_handler {
        format!(
            "no operation with an operationId records its handler in `{HANDLER_KEY}`, so no edge has a `to`"
        )
    } else if no_clients {
        format!(
            "the graph has no module under the client folder `{}`",
            receipt.client
        )
    } else {
        format!(
            "no module under the client folder `{}` names an operationId the server document records",
            receipt.client
        )
    }
}

/// `base` and a relative `path` joined, `.` segments dropped and `..` taken off the base.
pub fn join(base: &str, path: &str) -> String {
    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// The module system a declared edge records: the `from` module's language's, since the
/// upstream field is required and has no value for an edge no import formed
/// ([ADR-0076](../../../docs/adr/0076-fleet-and-declared-edges.md)).
pub fn module_system_of(language: Option<Language>) -> ModuleSystem {
    match language {
        Some(Language::Dotnet) => ModuleSystem::Clr,
        Some(Language::Python) => ModuleSystem::Py,
        Some(Language::Typescript | Language::Javascript) | None => ModuleSystem::Es6,
    }
}

/// What [`apply`] added.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// Edges added from `edges.yaml`'s `declared` list.
    pub edges_yaml: u64,
    /// Edges added from OpenAPI pairs.
    pub openapi: u64,
    /// Declarations not added, with the reason.
    pub skipped: Vec<SkippedEdge>,
}

/// Adds each declaration to `document` as a dependency of its `from` module: `declared: true`,
/// `dependencyKind: declared`, no dependency types, not followable. A declaration whose `from`
/// or `to` is not a module of the graph is not added and is listed in [`Applied::skipped`], since
/// an edge must join two modules; one the graph already holds (same `from`, `module` and `to`) is
/// added once. Declarations are added in sorted order, so the result does not depend on the
/// order they were found in.
pub fn apply(document: &mut GraphDocument, declarations: Vec<Declaration>) -> Applied {
    let sources: BTreeMap<String, usize> = document
        .modules
        .iter()
        .enumerate()
        .map(|(index, module)| (module.source.clone(), index))
        .collect();
    let unique: BTreeSet<Declaration> = declarations.into_iter().collect();
    let mut applied = Applied::default();
    let mut seen = BTreeSet::new();
    for declaration in unique {
        let skip = |reason: &str| SkippedEdge {
            from: declaration.from.clone(),
            to: declaration.to.clone(),
            reason: reason.to_owned(),
        };
        let Some(&index) = sources.get(&declaration.from) else {
            applied.skipped.push(skip(
                "`from` is not a module of the graph: the file is gone, excluded, or named differently",
            ));
            continue;
        };
        if !sources.contains_key(&declaration.to) {
            applied.skipped.push(skip(
                "`to` is not a module of the graph: the file is gone, excluded, or named differently",
            ));
            continue;
        }
        let key = (
            declaration.from.clone(),
            declaration.module.clone(),
            declaration.to.clone(),
        );
        if !seen.insert(key) {
            continue;
        }
        let module = &mut document.modules[index];
        let dependency = Dependency {
            declared: Some(true),
            declared_kind: declaration.kind,
            comment: declaration.comment,
            dependency_kind: Some(DependencyKind::Declared),
            ..Dependency::new(
                declaration.module,
                declaration.to,
                module_system_of(module.language),
            )
        };
        module.dependencies.push(dependency);
        match declaration.source {
            Source::EdgesYaml => applied.edges_yaml += 1,
            Source::OpenApi => applied.openapi += 1,
        }
    }
    applied
}

/// The declared edges whose `from` or `to` no longer exists, as `config lint` reports them:
/// `(index, side, path)` for each `declared[index]` side `exists` says is gone, and
/// `(index, "server", path)` for an `openapi[index]` server document that is gone.
pub fn stale(
    file: &EdgesFile,
    exists: &dyn Fn(&str) -> bool,
) -> Vec<(usize, &'static str, String)> {
    let mut out = Vec::new();
    for (index, edge) in file.declared.iter().enumerate() {
        for (side, path) in [("from", &edge.from), ("to", &edge.to)] {
            if !exists(path) {
                out.push((index, side, path.clone()));
            }
        }
    }
    for (index, pair) in file.openapi.iter().enumerate() {
        if !exists(&pair.server) {
            out.push((index, "server", pair.server.clone()));
        }
    }
    out
}

/// The schema `edges.yaml` validates against, published as `schema/edges-v1.json`.
pub mod schema {
    use schemars::generate::SchemaSettings;

    /// The `$id` the schema is published under.
    pub const ID: &str = "https://benbahrenburg.github.io/rulebearing/schema/edges-v1.json";

    /// The committed schema's path, relative to the repository root.
    pub const PATH: &str = "schema/edges-v1.json";

    /// The schema as pretty-printed JSON with a trailing newline, keys sorted.
    pub fn render() -> String {
        let generator = SchemaSettings::draft07().into_generator();
        let mut schema = generator.into_root_schema_for::<super::EdgesFile>();
        schema.insert("$id".to_owned(), serde_json::Value::from(ID));
        let mut value = schema.to_value();
        rb_model::schema::sort_keys(&mut value);
        let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
        text.push('\n');
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_model::Module;

    const EDGES: &str = "declared:
  - from: web/src/api/client.ts
    to: api/src/Web/Endpoints/Orders.cs
    kind: http
    comment: \"OrdersClient calls POST /orders. adr:0007\"
openapi:
  - client: web/src/api/generated/
    server: api/openapi.json
";

    fn graph(sources: &[(&str, Option<Language>)]) -> GraphDocument {
        GraphDocument {
            modules: sources
                .iter()
                .map(|(source, language)| Module {
                    language: *language,
                    ..Module::new(*source)
                })
                .collect(),
            ..GraphDocument::default()
        }
    }

    #[test]
    fn reads_the_frozen_format() {
        let file = read_edges(EDGES).unwrap_or_default();
        assert_eq!(file.declared.len(), 1);
        assert_eq!(file.declared[0].kind.as_deref(), Some("http"));
        assert_eq!(
            file.openapi[0],
            OpenApiPair {
                client: "web/src/api/generated/".into(),
                server: "api/openapi.json".into()
            }
        );
        assert_eq!(file.openapi[0].client_prefix(), "web/src/api/generated/");
        assert_eq!(
            OpenApiPair {
                client: "web/gen".into(),
                server: String::new()
            }
            .client_prefix(),
            "web/gen/"
        );
        assert_eq!(read_edges("").map(|f| f.declared.len()), Ok(0));
    }

    #[test]
    fn refuses_what_is_not_the_format() {
        for (text, says) in [
            ("declared: [{ from: a }]", "missing field `to`"),
            (
                "declared: [{ from: a, to: b, via: c }]",
                "unknown field `via`",
            ),
            ("edges: []", "unknown field `edges`"),
            (
                "declared: [{ from: '', to: b }]",
                "declared[0].from is empty",
            ),
            ("declared: [{ from: a, to: /b }]", "declared[0].to `/b`"),
            (
                "declared: [{ from: ../a, to: b }]",
                "declared[0].from `../a`",
            ),
            ("declared: [{ from: 'a\\b', to: b }]", "declared[0].from"),
            (
                "openapi: [{ client: '', server: s }]",
                "openapi[0].client is empty",
            ),
            (
                "openapi: [{ client: c, server: ' ' }]",
                "openapi[0].server is empty",
            ),
        ] {
            let message = match read_edges(text) {
                Err(DeclaredError::Edges(message)) => message,
                other => format!("not refused: {other:?}"),
            };
            assert!(message.contains(says), "{text}: {message}");
        }
    }

    const SERVER: &str = r#"{
  "openapi": "3.0.3",
  "paths": {
    "/orders": {
      "post": { "operationId": "createOrder", "x-rulebearing-handler": "src/Orders.cs" },
      "get": { "operationId": "listOrders", "x-rulebearing-handler": "./src/Orders.cs" },
      "parameters": []
    },
    "/health": { "get": { "operationId": "health" } },
    "/legacy": { "delete": { "x-rulebearing-handler": "src/Legacy.cs" } },
    "/ref": { "$ref": "other.json#/paths/x" }
  }
}"#;

    #[test]
    fn reads_every_operation_sorted_with_its_id_and_handler() {
        let operations = read_openapi("api/openapi.json", SERVER).unwrap_or_default();
        let seen: Vec<(&str, &str, Option<&str>, Option<&str>)> = operations
            .iter()
            .map(|o| {
                (
                    o.path.as_str(),
                    o.method.as_str(),
                    o.operation_id.as_deref(),
                    o.handler.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            seen,
            [
                ("/health", "GET", Some("health"), None),
                ("/legacy", "DELETE", None, Some("src/Legacy.cs")),
                (
                    "/orders",
                    "GET",
                    Some("listOrders"),
                    Some("./src/Orders.cs")
                ),
                (
                    "/orders",
                    "POST",
                    Some("createOrder"),
                    Some("src/Orders.cs")
                ),
            ]
        );
        let yaml = "swagger: '2.0'\npaths:\n  /a:\n    put: { operationId: putA }\n";
        let from_yaml = read_openapi("s.yaml", yaml).unwrap_or_default();
        assert_eq!(from_yaml.len(), 1);
        assert_eq!(from_yaml[0].method, "PUT");
    }

    #[test]
    fn refuses_a_document_that_is_not_openapi() {
        for (text, says) in [
            ("{ not json", "not JSON"),
            ("a: [", "not YAML"),
            ("{\"paths\": {}}", "neither `openapi` nor `swagger`"),
            ("openapi: 3.0.0\n", "no `paths` object"),
        ] {
            let reason = match read_openapi("s", text) {
                Err(DeclaredError::OpenApi { server, reason }) if server == "s" => reason,
                other => format!("not refused: {other:?}"),
            };
            assert!(reason.contains(says), "{text}: {reason}");
        }
    }

    #[test]
    fn a_name_is_a_whole_identifier_spelled_exactly() {
        let table = [
            ("client.createOrder(body)", "createOrder", true),
            ("'createOrder'", "createOrder", true),
            ("createOrder", "createOrder", true),
            ("createOrders()", "createOrder", false),
            ("recreateOrder()", "createOrder", false),
            ("_createOrder", "createOrder", false),
            ("$createOrder", "createOrder", false),
            ("createOrder$", "createOrder", false),
            ("CreateOrder()", "createOrder", false),
            ("x createOrder2 createOrder", "createOrder", true),
            ("é createOrder", "createOrder", true),
            ("anything", "", false),
        ];
        for (source, id, expected) in table {
            assert_eq!(names(source, id), expected, "{source} / {id}");
        }
    }

    fn pair() -> OpenApiPair {
        OpenApiPair {
            client: "web/gen/".into(),
            server: "api/openapi.json".into(),
        }
    }

    #[test]
    fn an_operation_declares_an_edge_from_each_client_file_that_names_it() {
        let operations = read_openapi("api/openapi.json", SERVER).unwrap_or_default();
        let clients = vec![
            (
                "web/gen/orders.ts".to_owned(),
                "post('/orders', 'createOrder'); get('listOrders')".to_owned(),
            ),
            ("web/gen/health.ts".to_owned(), "nothing here".to_owned()),
            ("web/gen/more.ts".to_owned(), "createOrder()".to_owned()),
        ];
        let (declarations, receipt) = openapi_declarations(&pair(), &operations, "api/", &clients);
        let edges: Vec<(&str, &str, &str)> = declarations
            .iter()
            .map(|d| (d.from.as_str(), d.module.as_str(), d.to.as_str()))
            .collect();
        assert_eq!(
            edges,
            [
                ("web/gen/orders.ts", "listOrders", "api/src/Orders.cs"),
                ("web/gen/orders.ts", "createOrder", "api/src/Orders.cs"),
                ("web/gen/more.ts", "createOrder", "api/src/Orders.cs"),
            ]
        );
        assert_eq!(
            declarations[1].comment.as_deref(),
            Some("operation createOrder (POST /orders) in api/openapi.json")
        );
        assert!(
            declarations
                .iter()
                .all(|d| d.kind.as_deref() == Some("http") && d.source == Source::OpenApi)
        );
        assert_eq!(
            receipt,
            OpenApiReceipt {
                client: "web/gen/".into(),
                server: "api/openapi.json".into(),
                operations: 4,
                without_operation_id: 1,
                without_handler: 1,
                uncalled: 0,
                edges: 3,
                reason: None,
            }
        );
    }

    /// Operations, client files, and what the reason says.
    type Case<'a> = (Vec<Operation>, &'a [(String, String)], &'a str);

    #[test]
    fn a_pair_that_declares_nothing_says_why() {
        let op = |id: Option<&str>, handler: Option<&str>| Operation {
            path: "/x".into(),
            method: "GET".into(),
            operation_id: id.map(str::to_owned),
            handler: handler.map(str::to_owned),
        };
        let calls = vec![("web/gen/a.ts".to_owned(), "getX()".to_owned())];
        let table: [Case<'_>; 5] = [
            (vec![], &calls, "lists no operations"),
            (
                vec![op(None, Some("h.cs")), op(None, None)],
                &calls,
                "records no operationId",
            ),
            (
                vec![op(Some("getX"), None), op(None, None)],
                &calls,
                "records its handler in `x-rulebearing-handler`",
            ),
            (
                vec![op(Some("getX"), Some("h.cs"))],
                &[],
                "no module under the client folder `web/gen/`",
            ),
            (
                vec![op(Some("getY"), Some("h.cs"))],
                &calls,
                "names an operationId",
            ),
        ];
        for (operations, clients, says) in table {
            let (declarations, receipt) =
                openapi_declarations(&pair(), &operations, "api", clients);
            assert!(declarations.is_empty());
            assert_eq!(receipt.edges, 0);
            let reason = receipt.reason.unwrap_or_default();
            assert!(reason.contains(says), "{says}: {reason}");
        }
        let (_, receipt) =
            openapi_declarations(&pair(), &[op(Some("getY"), Some("h.cs"))], "api", &calls);
        assert_eq!(receipt.uncalled, 1);
    }

    #[test]
    fn joins_a_handler_to_its_repository() {
        for (base, path, expected) in [
            ("api/", "src/Orders.cs", "api/src/Orders.cs"),
            ("api", "./src/Orders.cs", "api/src/Orders.cs"),
            ("api/docs/", "../src/a.cs", "api/src/a.cs"),
            ("", "src//a.cs", "src/a.cs"),
            ("a", "../../b", "b"),
        ] {
            assert_eq!(join(base, path), expected, "{base} + {path}");
        }
    }

    #[test]
    fn the_module_system_is_the_from_modules_languages() {
        assert_eq!(module_system_of(Some(Language::Dotnet)), ModuleSystem::Clr);
        assert_eq!(module_system_of(Some(Language::Python)), ModuleSystem::Py);
        assert_eq!(
            module_system_of(Some(Language::Typescript)),
            ModuleSystem::Es6
        );
        assert_eq!(
            module_system_of(Some(Language::Javascript)),
            ModuleSystem::Es6
        );
        assert_eq!(module_system_of(None), ModuleSystem::Es6);
    }

    #[test]
    fn apply_marks_every_edge_and_skips_what_the_graph_lacks() {
        let mut document = graph(&[
            ("web/a.ts", Some(Language::Typescript)),
            ("api/b.cs", Some(Language::Dotnet)),
            ("pipeline/c.py", Some(Language::Python)),
        ]);
        let declaration = |from: &str, to: &str, source: Source| Declaration {
            from: from.into(),
            to: to.into(),
            module: to.into(),
            kind: Some("queue".into()),
            comment: Some("why".into()),
            source,
        };
        let applied = apply(
            &mut document,
            vec![
                declaration("api/b.cs", "pipeline/c.py", Source::EdgesYaml),
                declaration("web/a.ts", "api/b.cs", Source::OpenApi),
                declaration("web/a.ts", "api/b.cs", Source::OpenApi),
                declaration("web/a.ts", "api/b.cs", Source::EdgesYaml),
                declaration("web/gone.ts", "api/b.cs", Source::EdgesYaml),
                declaration("web/a.ts", "api/gone.cs", Source::EdgesYaml),
            ],
        );
        // The same edge from both sources sorts the `edges.yaml` declaration first and is counted
        // there.
        assert_eq!((applied.edges_yaml, applied.openapi), (2, 0));
        let skipped: Vec<(&str, &str, bool)> = applied
            .skipped
            .iter()
            .map(|s| (s.from.as_str(), s.to.as_str(), s.reason.starts_with("`to`")))
            .collect();
        assert_eq!(
            skipped,
            [
                ("web/a.ts", "api/gone.cs", true),
                ("web/gone.ts", "api/b.cs", false)
            ]
        );
        let web = &document.modules[0].dependencies;
        assert_eq!(web.len(), 1, "the same edge from two sources is added once");
        let edge = &web[0];
        assert_eq!(edge.resolved, "api/b.cs");
        assert_eq!(edge.declared, Some(true));
        assert_eq!(edge.dependency_kind, Some(DependencyKind::Declared));
        assert_eq!(edge.declared_kind.as_deref(), Some("queue"));
        assert_eq!(edge.comment.as_deref(), Some("why"));
        assert_eq!(edge.module_system, ModuleSystem::Es6);
        assert!(edge.dependency_types.is_empty() && !edge.followable && edge.valid);
        assert_eq!(
            document.modules[1].dependencies[0].module_system,
            ModuleSystem::Clr
        );
        assert!(document.modules[2].dependencies.is_empty());
    }

    #[test]
    fn apply_is_independent_of_the_order_declarations_arrive_in() {
        let declaration = |from: &str, to: &str| Declaration {
            from: from.into(),
            to: to.into(),
            module: to.into(),
            kind: None,
            comment: None,
            source: Source::EdgesYaml,
        };
        let forward = vec![declaration("a", "b"), declaration("a", "c")];
        let mut backward = forward.clone();
        backward.reverse();
        let mut first = graph(&[("a", None), ("b", None), ("c", None)]);
        let mut second = first.clone();
        apply(&mut first, forward);
        apply(&mut second, backward);
        assert_eq!(
            serde_json::to_string(&first).ok(),
            serde_json::to_string(&second).ok()
        );
        assert_eq!(first.modules[0].dependencies.len(), 2);
    }

    #[test]
    fn hand_declarations_keep_the_entry() {
        let file = read_edges(EDGES).unwrap_or_default();
        let declarations = hand_declarations(&file);
        assert_eq!(
            declarations,
            [Declaration {
                from: "web/src/api/client.ts".into(),
                to: "api/src/Web/Endpoints/Orders.cs".into(),
                module: "api/src/Web/Endpoints/Orders.cs".into(),
                kind: Some("http".into()),
                comment: Some("OrdersClient calls POST /orders. adr:0007".into()),
                source: Source::EdgesYaml,
            }]
        );
    }

    #[test]
    fn stale_lists_every_side_that_is_gone() {
        let file = read_edges(EDGES).unwrap_or_default();
        let none = stale(&file, &|_| true);
        assert!(none.is_empty());
        let gone = stale(&file, &|path| path.starts_with("web/"));
        assert_eq!(
            gone,
            [
                (0, "to", "api/src/Web/Endpoints/Orders.cs".to_owned()),
                (0, "server", "api/openapi.json".to_owned())
            ]
        );
    }

    #[test]
    fn the_committed_schema_is_current() -> Result<(), Box<dyn std::error::Error>> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(schema::PATH);
        let generated = schema::render();
        if std::env::var_os("RB_UPDATE_SCHEMA").is_some() {
            std::fs::write(&path, &generated)?;
        }
        let committed = std::fs::read_to_string(&path)?;
        assert!(
            committed == generated,
            "{} is stale; run RB_UPDATE_SCHEMA=1 cargo test -p rb-ingest schema",
            schema::PATH
        );
        assert!(generated.contains(schema::ID));
        Ok(())
    }

    proptest::proptest! {
        #[test]
        fn names_never_finds_an_id_inside_a_longer_identifier(
            id in "[a-zA-Z][a-zA-Z0-9]{0,8}",
            glued in "[a-zA-Z0-9_$]{1,3}",
        ) {
            // Glued on either side, the only occurrence is inside a longer identifier.
            let prefixed = [glued.as_str(), id.as_str()].concat();
            let suffixed = [id.as_str(), glued.as_str()].concat();
            let once = |text: &str| text.matches(id.as_str()).count() == 1;
            proptest::prop_assert!(!once(&prefixed) || !names(&prefixed, &id));
            proptest::prop_assert!(!once(&suffixed) || !names(&suffixed, &id));
            let called = ["(", id.as_str(), ")"].concat();
            proptest::prop_assert!(names(&called, &id));
        }

        #[test]
        fn join_never_leaves_a_dot_segment(
            base in "[a-z]{0,3}(/[a-z]{1,3}){0,2}/?",
            path in "(\\.\\.?/|[a-z]{1,3}/){0,4}[a-z]{1,3}",
        ) {
            let joined = join(&base, &path);
            proptest::prop_assert!(!joined.split('/').any(|s| s == "." || s == ".." || s.is_empty()) || joined.is_empty());
        }
    }
}
