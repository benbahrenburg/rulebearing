//! `rulebearing place --imports a,b --imported-by c --language LANG`: where a new module with
//! those edges would be legal.
//!
//! - Source: [design § Questions an agent can ask before it writes the import](../../../../docs/artifacts/design.md#questions-an-agent-can-ask-before-it-writes-the-import)
//!   ("the answer to 'where should this code live' as a query rather than a document")
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//!   (every rule against a synthetic module at each candidate directory; from the cache of Step 13)
//! - Decisions: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md) decision 2,
//!   [ADR-0014](../../../../docs/adr/0014-no-invented-cross-language-edges.md) (candidates come
//!   from the language's own modules)
//! - Requirement: [FR-CLI-02](../../../../docs/prd.md#fr-cli-02)
//!
//! The candidates are every folder holding a module of the language, and every folder above one,
//! in the graph. At each, a module named `--name` (default `new-module` with the language's
//! extension) is added with an edge to every `--imports` target and an edge from every
//! `--imported-by` module. Each edge is a static import in the new module's language (for
//! TypeScript and JavaScript an ES `import`, not dynamic, not type-only) and takes from an edge to
//! the same target already in the graph only what describes the target, as `can-import` does
//! ([`can_import::describes_target`]). The whole rule set is evaluated, so cycles, `required`,
//! reachability, orphan and element rules count as they do in the gate. The graph as it is is
//! evaluated once too, and a folder is legal when the trial adds no error-severity violation, by
//! id ([ADR-0015](../../../../docs/adr/0015-stable-violation-id.md)), to that base run: a
//! violation of an edge between two existing modules that only the new module's edges make
//! reachable rules the folder out as surely as one that names it. Exit 0 with the legal folders,
//! 1 when there is none, 2 when a target is unknown.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use clap::Args;
use rb_model::{
    Dependency, DependencyKind, DependencyType, GraphDocument, Language, Module, ModuleSystem,
    Severity,
};
use rb_rules::{EvalOptions, evaluate};
use serde_json::json;

use crate::cli::ConfigArgs;
use crate::cmd::can_import;
use crate::context::Context;
use crate::{Outcome, RunExit, cache, configure};

/// `place`.
#[derive(Debug, Clone, Default, Args)]
pub struct PlaceArgs {
    /// The modules the new module would import, comma separated
    #[arg(long, value_name = "FILES", value_delimiter = ',')]
    pub imports: Vec<String>,
    /// The modules that would import the new module, comma separated
    #[arg(long, value_name = "FILES", value_delimiter = ',')]
    pub imported_by: Vec<String>,
    /// The new module's language: typescript (ts), javascript (js), dotnet (cs), python (py)
    #[arg(long, value_name = "LANGUAGE")]
    pub language: String,
    /// The new module's file name (default: new-module with the language's extension)
    #[arg(long, value_name = "FILE")]
    pub name: Option<String>,
    /// Print JSON, with the rules that rule out each other folder
    #[arg(long)]
    pub json: bool,
    /// A graph document to answer from instead of the cache
    #[arg(long, value_name = "FILE")]
    pub graph: Option<String>,
    /// Extract afresh, neither reading nor writing the cache
    #[arg(long)]
    pub no_cache: bool,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
}

/// A language as `--language` spells it, with its short forms.
pub fn language(text: &str) -> Option<Language> {
    match text.to_ascii_lowercase().as_str() {
        "ts" | "tsx" => Some(Language::Typescript),
        "js" | "jsx" => Some(Language::Javascript),
        "cs" | "csharp" | "c#" | ".net" | "net" => Some(Language::Dotnet),
        "py" => Some(Language::Python),
        other => other.parse().ok(),
    }
}

/// The default file name of a new module.
pub fn default_name(language: Language) -> &'static str {
    match language {
        Language::Typescript => "new-module.ts",
        Language::Javascript => "new-module.js",
        Language::Dotnet => "NewModule.cs",
        Language::Python => "new_module.py",
    }
}

/// Every folder holding a module of `language`, and every folder above one; `""` is the root.
pub fn candidates(document: &GraphDocument, language: Language) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for module in &document.modules {
        if module.language != Some(language)
            || module.source.contains("node_modules/")
            || module.core_module == Some(true)
        {
            continue;
        }
        let mut folder = module.source.as_str();
        while let Some((parent, _)) = folder.rsplit_once('/') {
            out.insert(parent.to_owned());
            folder = parent;
        }
        out.insert(String::new());
    }
    out
}

fn normalise(path: &str) -> String {
    let mut text = path.trim().replace('\\', "/");
    while let Some(rest) = text.strip_prefix("./") {
        text = rest.to_owned();
    }
    text
}

/// The module system a static import in `language` is written in.
fn module_system(language: Language) -> ModuleSystem {
    match language {
        Language::Typescript | Language::Javascript => ModuleSystem::Es6,
        Language::Dotnet => ModuleSystem::Clr,
        Language::Python => ModuleSystem::Py,
    }
}

/// A new static import from a module in `language` to `target`: `target_types` (what the target
/// is), then `import` for TypeScript and JavaScript; not dynamic, not type-only, kind `import`.
fn new_edge(language: Language, target: &str, target_types: Vec<DependencyType>) -> Dependency {
    let mut dependency_types = if target_types.is_empty() {
        vec![DependencyType::Local]
    } else {
        target_types
    };
    if matches!(language, Language::Typescript | Language::Javascript) {
        dependency_types.push(DependencyType::Import);
    }
    Dependency {
        dependency_types,
        followable: true,
        dependency_kind: Some(DependencyKind::Import),
        type_only: None,
        ..Dependency::new(
            target.to_owned(),
            target.to_owned(),
            module_system(language),
        )
    }
}

/// The edge a new module in `language` would have to `target`: what describes the target copied
/// from an edge to it already in the graph, else a local edge when the target is a module or a
/// file here. How that other edge imports the target (dynamic, type-only, `require`, its kind,
/// line and specifier) is not copied.
fn edge_to(
    ctx: &Context<'_>,
    document: &GraphDocument,
    language: Language,
    target: &str,
) -> Option<Dependency> {
    let known = document
        .modules
        .iter()
        .flat_map(|m| &m.dependencies)
        .find(|d| d.resolved == target);
    if let Some(d) = known {
        let types = d
            .dependency_types
            .iter()
            .copied()
            .filter(|t| can_import::describes_target(*t))
            .collect();
        return Some(Dependency {
            core_module: d.core_module,
            license: d.license.clone(),
            could_not_resolve: d.could_not_resolve,
            followable: d.followable,
            matches_do_not_follow: d.matches_do_not_follow,
            protocol: d.protocol,
            mime_type: d.mime_type.clone(),
            instability: d.instability,
            ..new_edge(language, target, types)
        });
    }
    let local = document.modules.iter().any(|m| m.source == target)
        || (!target.starts_with("node_modules/") && ctx.resolve(target).is_file());
    local.then(|| new_edge(language, target, Vec::new()))
}

/// The ids of the error-severity violations of a run.
fn error_ids(evaluation: &rb_rules::Evaluation) -> BTreeSet<String> {
    evaluation
        .violations()
        .iter()
        .filter(|v| v.rule.severity == Severity::Error)
        .map(violation_key)
        .collect()
}

/// A violation's id, else (a violation without one) its rule, type, ends and path, so two
/// different findings never share a key.
fn violation_key(v: &rb_model::Violation) -> String {
    v.id.clone().unwrap_or_else(|| {
        serde_json::to_string(&json!([
            v.rule.name,
            v.violation_type,
            v.from,
            v.to,
            v.cycle,
            v.via
        ]))
        .unwrap_or_default()
    })
}

/// A folder's verdict: legal, or the rules that rule it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// The folder, `""` for the root.
    pub folder: String,
    /// The error-severity rules the new module would break there; empty when legal.
    pub rules: Vec<String>,
}

fn shown(folder: &str) -> String {
    if folder.is_empty() {
        "./".to_owned()
    } else {
        format!("{folder}/")
    }
}

/// Runs `place`.
#[expect(
    clippy::too_many_lines,
    reason = "parse, resolve the edges, evaluate per folder, then render: one query, read top to bottom"
)]
pub fn run(ctx: &mut Context<'_>, args: &PlaceArgs) -> Outcome {
    let Some(lang) = language(&args.language) else {
        return Outcome::failed(
            RunExit::InvalidConfig,
            format!(
                "rulebearing place: `{}` is not a language; use typescript, javascript, dotnet or python\n",
                args.language
            ),
        );
    };
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let document = match cache::document(ctx, &config, args.graph.as_deref(), args.no_cache) {
        Ok(d) => d,
        Err(m) => {
            return Outcome::failed(RunExit::Untrustworthy, format!("rulebearing place: {m}\n"));
        }
    };
    let mut imports = Vec::new();
    for target in args
        .imports
        .iter()
        .map(|t| normalise(t))
        .filter(|t| !t.is_empty())
    {
        let Some(edge) = edge_to(ctx, &document, lang, &target) else {
            return Outcome::failed(
                RunExit::Untrustworthy,
                format!(
                    "rulebearing place: {target} is not in the graph and is not a file here, so its kind of dependency is unknown\n"
                ),
            );
        };
        imports.push(edge);
    }
    let importers: Vec<String> = args
        .imported_by
        .iter()
        .map(|t| normalise(t))
        .filter(|t| !t.is_empty())
        .collect();
    if let Some(missing) = importers
        .iter()
        .find(|i| !document.modules.iter().any(|m| m.source == **i))
    {
        return Outcome::failed(
            RunExit::Untrustworthy,
            format!(
                "rulebearing place: {missing} is not a module of the graph, so it cannot import the new module\n"
            ),
        );
    }
    let file = args
        .name
        .clone()
        .unwrap_or_else(|| default_name(lang).to_owned());
    let options = EvalOptions {
        liveness: false,
        today: ctx.today,
        ..EvalOptions::default()
    };
    let base = match evaluate(document.clone(), &config, &options) {
        Ok(e) => error_ids(&e),
        Err(e) => {
            return Outcome::failed(RunExit::Untrustworthy, format!("rulebearing place: {e}\n"));
        }
    };
    let mut verdicts = Vec::new();
    for folder in candidates(&document, lang) {
        let source = if folder.is_empty() {
            file.clone()
        } else {
            format!("{folder}/{file}")
        };
        if document.modules.iter().any(|m| m.source == source) {
            continue;
        }
        let mut trial = document.clone();
        let mut module = Module::new(source.clone());
        module.language = Some(lang);
        module.dependencies.clone_from(&imports);
        trial.modules.push(module);
        for importer in &importers {
            if let Some(m) = trial.modules.iter_mut().find(|m| m.source == *importer) {
                let language = m.language.unwrap_or(lang);
                m.dependencies.push(new_edge(language, &source, Vec::new()));
            }
        }
        let evaluation = match evaluate(trial, &config, &options) {
            Ok(e) => e,
            Err(e) => {
                return Outcome::failed(
                    RunExit::Untrustworthy,
                    format!("rulebearing place: {e}\n"),
                );
            }
        };
        let mut rules: Vec<String> = evaluation
            .violations()
            .iter()
            .filter(|v| v.rule.severity == Severity::Error && !base.contains(&violation_key(v)))
            .map(|v| v.rule.name.clone())
            .collect();
        rules.sort();
        rules.dedup();
        verdicts.push(Verdict { folder, rules });
    }
    let legal: Vec<&Verdict> = verdicts.iter().filter(|v| v.rules.is_empty()).collect();
    let stdout = if args.json {
        let value = json!({
            "module": file,
            "language": lang.as_str(),
            "legal": legal.iter().map(|v| shown(&v.folder)).collect::<Vec<_>>(),
            "illegal": verdicts.iter().filter(|v| !v.rules.is_empty())
                .map(|v| json!({ "folder": shown(&v.folder), "rules": v.rules }))
                .collect::<Vec<_>>(),
        });
        let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
        text.push('\n');
        text
    } else {
        let mut text = String::new();
        for v in &legal {
            let _ = writeln!(text, "{}", shown(&v.folder));
        }
        text
    };
    if legal.is_empty() {
        return Outcome {
            stdout,
            stderr: format!(
                "rulebearing place: no folder of the {} modules takes a module with these edges; `--json` names the rules that rule each one out\n",
                lang.as_str()
            ),
            code: RunExit::Violations(1).code(),
        };
    }
    Outcome::printed(stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_and_names() {
        for (text, lang) in [
            ("ts", Language::Typescript),
            ("TypeScript", Language::Typescript),
            ("js", Language::Javascript),
            ("csharp", Language::Dotnet),
            ("dotnet", Language::Dotnet),
            ("py", Language::Python),
        ] {
            assert_eq!(language(text), Some(lang), "{text}");
        }
        assert_eq!(language("cobol"), None);
        for lang in Language::ALL {
            assert!(default_name(*lang).contains('.'));
        }
        assert_eq!(shown(""), "./");
        assert_eq!(shown("a/b"), "a/b/");
        assert_eq!(normalise(" ./a\\b "), "a/b");
    }

    #[test]
    fn candidates_are_the_languages_folders_and_their_parents() {
        let mut ts = Module::new("src/a/b/x.ts".to_owned());
        ts.language = Some(Language::Typescript);
        let mut py = Module::new("tools/y.py".to_owned());
        py.language = Some(Language::Python);
        let mut npm = Module::new("node_modules/z/index.js".to_owned());
        npm.language = Some(Language::Typescript);
        let document = GraphDocument {
            modules: vec![ts, py, npm],
            ..GraphDocument::default()
        };
        let found: Vec<String> = candidates(&document, Language::Typescript)
            .into_iter()
            .collect();
        assert_eq!(found, ["", "src", "src/a", "src/a/b"]);
    }
}
