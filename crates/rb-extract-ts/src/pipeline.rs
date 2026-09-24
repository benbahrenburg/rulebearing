//! From files to modules: which walker a file gets, resolving and filtering what it finds, the
//! initial file list, and following dependencies to the modules they reach.
//!
//! - Plan: [Wave 0, Step 8](../../../docs/plans/pending/0000-wave-0-spike.md#step-8-spike-a-rb-extract-ts-0c)
//!   (`discover.rs`, `parse.rs`, `lib.rs` of the step's table, here as one module)
//! - Source: [design § The five stages](../../../docs/artifacts/design.md#the-five-stages), stages
//!   1 and 2; [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options)
//!   (`doNotFollow`, `exclude`, `includeOnly`, `maxDepth`, `tsPreCompilationDeps`,
//!   `extraExtensionsToScan`, `experimentalStats`)
//! - Plan: [Wave 1, Step 10](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-10-rb-extract-ts-to-100-and-the-option-set-1c)
//!   (`.vue` scripts, `babelConfig` aliases, `line` and `column`, file-level parallelism)
//! - Specification: dependency-cruiser 18.2.0 `src/extract/{extract-dependencies,
//!   gather-initial-sources,index,extract-stats}.mjs`, `src/extract/transpile/vue-template-wrap.cjs`
//!
//! [`extract`] runs in two phases. The first finds every file the run can reach and extracts
//! each one's dependencies in parallel, a breadth-first frontier at a time, with `rayon`. The
//! second replays upstream's depth-first walk over those results, so the module order, the depth
//! `maxDepth` counts and the first error reported are the ones the sequential walk gives. Output
//! is therefore identical however many threads ran.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use oxc_allocator::Allocator;
use oxc_ast::ast::{ImportDeclarationSpecifier, ImportOrExportKind, Statement};
use oxc_ast_visit::Visit;
use oxc_parser::{ParseOptions, Parser as OxcParser};
use oxc_semantic::SemanticBuilder;
use oxc_span::{SourceType, Span};
use rb_model::options::{PathFilter, TsPreCompilationDeps};
use rb_model::{
    DependencyType, ExperimentalStats, ModuleSystem, Parser, Protocol, TypeScriptOptions,
};
use regex::Regex;

use crate::babel::BabelAliases;
use crate::codelayer::{self, FileCode};
use crate::collate;
use crate::md;
use crate::resolve::{self, Context, ResolveConfig, SCANNABLE_EXTENSIONS};
use crate::sfc;
use crate::walk::{self, Flavour, Found, WalkOptions};

/// An option could not be used.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    /// A path pattern is not a valid regular expression.
    #[error("invalid pattern `{pattern}`: {reason}")]
    Pattern {
        /// The pattern.
        pattern: String,
        /// Why.
        reason: String,
    },
    /// Reading a file or a folder failed.
    #[error("{path}: {source}", path = path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The error.
        source: std::io::Error,
    },
    /// A file could not be parsed.
    #[error("{path}: {reason}", path = path.display())]
    Parse {
        /// The file.
        path: PathBuf,
        /// The parser's message.
        reason: String,
    },
}

/// A compiled `doNotFollow`, `exclude` or `includeOnly`.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// The joined path patterns.
    pub path: Option<Regex>,
    /// `doNotFollow.dependencyTypes`.
    pub dependency_types: Vec<DependencyType>,
    /// `exclude.dynamic`.
    pub dynamic: Option<bool>,
}

impl Filter {
    fn compile(filter: Option<&PathFilter>) -> Result<Option<Self>, PipelineError> {
        let Some(filter) = filter else {
            return Ok(None);
        };
        let path = filter
            .path()
            .map(|p| {
                let pattern = p.joined();
                Regex::new(&pattern).map_err(|e| PipelineError::Pattern {
                    pattern,
                    reason: e.to_string(),
                })
            })
            .transpose()?;
        Ok(Some(Self {
            path,
            dependency_types: filter.dependency_types().to_vec(),
            dynamic: filter.dynamic(),
        }))
    }

    fn path_matches(&self, text: &str) -> bool {
        self.path.as_ref().is_some_and(|re| re.is_match(text))
    }
}

/// `tsPreCompilationDeps`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PreCompilation {
    /// Only what survives compilation (the default).
    #[default]
    Off,
    /// Everything in the source.
    On,
    /// Everything, with the pre-compilation-only edges marked.
    Specify,
}

/// dependency-cruiser's cruise options, normalised for extraction.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is one independent option (detectJSDocImports, \
              detectProcessBuiltinModuleCalls, experimentalStats, the code layer, Markdown fences), \
              not a state"
)]
#[derive(Debug, Clone)]
pub struct Settings {
    /// The process working directory; relative paths are taken against it.
    pub cwd: PathBuf,
    /// Output paths are relative to this directory.
    pub base_dir: PathBuf,
    /// The module systems to extract.
    pub module_systems: Vec<ModuleSystem>,
    /// The parser dependency-cruiser would have used.
    pub parser: Option<Parser>,
    /// `tsPreCompilationDeps`.
    pub pre_compilation: PreCompilation,
    /// Exotic require names.
    pub exotic_require_strings: Vec<String>,
    /// JSDoc imports.
    pub detect_jsdoc_imports: bool,
    /// `process.getBuiltinModule`.
    pub detect_process_builtin_module_calls: bool,
    /// Extensions scanned but not parsed.
    pub extra_extensions_to_scan: Vec<String>,
    /// `doNotFollow`.
    pub do_not_follow: Option<Filter>,
    /// `exclude`.
    pub exclude: Option<Filter>,
    /// `includeOnly`.
    pub include_only: Option<Filter>,
    /// `maxDepth`, 0 for unlimited.
    pub max_depth: u32,
    /// `experimentalStats`.
    pub experimental_stats: bool,
    /// `babelConfig`'s module-resolver aliases, applied to what the acorn walker finds.
    pub babel: Option<BabelAliases>,
    /// Whether each extracted file's code layer is read from the same parse
    /// ([`codelayer`]); on by default.
    pub code_layer: bool,
    /// Whether a `.md` file that `extraExtensionsToScan` lists has its JavaScript and TypeScript
    /// fences read ([`md`]). Off by default, which is dependency-cruiser's behaviour (a listed
    /// extension is never read); the command line turns it on for a native configuration
    /// ([ADR-0036](../../../docs/adr/0036-markdown-fences-follow-the-configuration-format.md)).
    pub markdown_fences: bool,
}

impl Settings {
    /// Normalises the options as dependency-cruiser's `normalizeCruiseOptions` does.
    ///
    /// # Errors
    /// When a path pattern is not a valid regular expression.
    pub fn new(options: &TypeScriptOptions, cwd: &Path) -> Result<Self, PipelineError> {
        Ok(Self {
            cwd: cwd.to_path_buf(),
            base_dir: options
                .base_dir
                .as_deref()
                .map_or_else(|| cwd.to_path_buf(), PathBuf::from),
            module_systems: options.module_systems(),
            parser: options.parser,
            pre_compilation: match options.ts_pre_compilation_deps {
                None | Some(TsPreCompilationDeps::Enabled(false)) => PreCompilation::Off,
                Some(TsPreCompilationDeps::Enabled(true)) => PreCompilation::On,
                Some(TsPreCompilationDeps::Specify(_)) => PreCompilation::Specify,
            },
            exotic_require_strings: options.exotic_require_strings().to_vec(),
            detect_jsdoc_imports: options.detect_js_doc_imports.unwrap_or(false),
            detect_process_builtin_module_calls: options
                .detect_process_builtin_module_calls
                .unwrap_or(false),
            extra_extensions_to_scan: options.extra_extensions_to_scan.clone().unwrap_or_default(),
            do_not_follow: Filter::compile(options.do_not_follow.as_ref())?,
            exclude: Filter::compile(options.exclude.as_ref())?,
            include_only: Filter::compile(options.include_only.as_ref())?,
            max_depth: u32::from(options.max_depth()),
            experimental_stats: options.experimental_stats.unwrap_or(false),
            babel: None,
            code_layer: true,
            markdown_fences: false,
        })
    }

    /// Whether `doNotFollow` matches a resolution, by path or by dependency type.
    fn stops(&self, resolution: &resolve::Resolution) -> bool {
        self.do_not_follow.as_ref().is_some_and(|f| {
            f.path_matches(&resolution.resolved)
                || resolution
                    .dependency_types
                    .iter()
                    .any(|t| f.dependency_types.contains(t))
        })
    }

    fn walk_options(&self) -> WalkOptions {
        WalkOptions {
            module_systems: self.module_systems.clone(),
            exotic_require_strings: self.exotic_require_strings.clone(),
            detect_jsdoc_imports: self.detect_jsdoc_imports,
            detect_process_builtin_module_calls: self.detect_process_builtin_module_calls,
        }
    }

    fn on_disk(&self, file: &str) -> PathBuf {
        let base = if self.base_dir.is_absolute() {
            self.base_dir.clone()
        } else {
            self.cwd.join(&self.base_dir)
        };
        base.join(file)
    }
}

/// One dependency as `extractDependencies` returns it: the form, its attributes and its resolution.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the flags are dependency-cruiser's dependency fields, a public contract (ADR-0004)"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    /// The specifier, protocol stripped.
    pub module: String,
    /// The form's module system.
    pub module_system: ModuleSystem,
    /// `import()`.
    pub dynamic: bool,
    /// Exotically required.
    pub exotically_required: bool,
    /// The exotic name.
    pub exotic_require: Option<String>,
    /// Classification then form types.
    pub dependency_types: Vec<DependencyType>,
    /// `node:`, `data:`, `file:`, `bun:`.
    pub protocol: Option<Protocol>,
    /// The MIME type of a `data:` URL.
    pub mime_type: Option<String>,
    /// Set under `tsPreCompilationDeps: "specify"`.
    pub pre_compilation_only: Option<bool>,
    /// The resolution.
    pub resolved: String,
    /// Built in.
    pub core_module: bool,
    /// Followable, and not stopped by `doNotFollow`.
    pub followable: bool,
    /// Not found.
    pub could_not_resolve: bool,
    /// Matched `doNotFollow`.
    pub matches_do_not_follow: bool,
    /// The npm licence.
    pub license: Option<String>,
    /// Where the form is.
    pub span: Span,
    /// The 1-based line the form starts on.
    pub line: u32,
    /// The 1-based column, in characters, the form starts at.
    pub column: u32,
}

const TS_COMPATIBLE: &[&str] = &[".ts", ".tsx", ".mts", ".cts", ".js", ".mjs", ".cjs", ".vue"];
const PROTOCOL_ONLY: &[&str] = &[
    "node:sea",
    "node:sqlite",
    "node:test",
    "node:test/reporters",
    "bun:ffi",
    "bun:jsc",
    "bun:sqlite",
    "bun:test",
    "bun:wrap",
];

fn node_extname(file: &str) -> &str {
    let name = file.rsplit('/').next().unwrap_or(file);
    match name.rfind('.') {
        Some(0) | None => "",
        Some(at) => &name[at..],
    }
}

/// dependency-cruiser's `extractModuleAttributes`: `node:fs` is module `fs`, protocol `node:`.
pub fn module_attributes(specifier: &str) -> (String, Option<Protocol>, Option<String>) {
    for protocol in [
        Protocol::Node,
        Protocol::File,
        Protocol::Data,
        Protocol::Bun,
    ] {
        let Some(rest) = specifier.strip_prefix(protocol.as_str()) else {
            continue;
        };
        if rest.is_empty() {
            break;
        }
        let (mime, module) = match rest.split_once(',') {
            Some((mime, module)) if !mime.is_empty() && !module.is_empty() => {
                (Some(mime.to_owned()), module)
            }
            _ => (None, rest),
        };
        let js_like = matches!(protocol, Protocol::Node | Protocol::Bun);
        let with_protocol = format!("{}{module}", protocol.as_str());
        let canonical = if !js_like || PROTOCOL_ONLY.contains(&with_protocol.as_str()) {
            with_protocol
        } else {
            module.to_owned()
        };
        return (canonical, Some(protocol), mime);
    }
    (specifier.to_owned(), None, None)
}

fn source_type_for(file: &str) -> SourceType {
    SourceType::from_path(file).unwrap_or_else(|_| SourceType::mjs().with_jsx(true))
}

/// Which walker dependency-cruiser uses for a file.
pub fn flavour_for(settings: &Settings, file: &str) -> Flavour {
    let compatible = TS_COMPATIBLE.contains(&node_extname(file));
    if compatible
        && (settings.pre_compilation != PreCompilation::Off || settings.parser == Some(Parser::Tsc))
    {
        Flavour::Tsc
    } else if compatible && settings.parser == Some(Parser::Swc) {
        Flavour::Swc
    } else {
        Flavour::Acorn
    }
}

/// What TypeScript's `transpileModule` does to a module's declarations before upstream hands
/// the JavaScript to acorn, as spans of the TypeScript source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transpiled {
    /// Declarations the compiled JavaScript no longer has: `import type`, imports whose every
    /// binding is used only as a type or not at all (an empty `{}` included), `export type ...
    /// from`, and `export { ... } from` whose every specifier is type-only (or that has none).
    pub elided: Vec<Span>,
    /// `export * as ns from "x"`, which the ES2015 target lowers to `import * as ns_1 from "x"`
    /// plus a local export, so acorn reads an import.
    pub lowered_to_import: Vec<Span>,
}

/// The spans of a TypeScript source's type-only declarations: `import type`, `export type ...
/// from` and `export type * from`, which stripping the types removes.
fn type_declarations(source: &str, source_type: SourceType) -> Vec<Span> {
    let allocator = Allocator::default();
    let parsed = OxcParser::new(&allocator, source, source_type).parse();
    parsed
        .program
        .body
        .iter()
        .filter_map(|statement| match statement {
            Statement::ImportDeclaration(import)
                if import.import_kind == ImportOrExportKind::Type =>
            {
                Some(import.span)
            }
            Statement::ExportFromDeclaration(export)
                if export.export_kind == ImportOrExportKind::Type =>
            {
                Some(export.span)
            }
            Statement::ExportAllDeclaration(export)
                if export.export_kind == ImportOrExportKind::Type =>
            {
                Some(export.span)
            }
            _ => None,
        })
        .collect()
}

/// [`Transpiled`] for a TypeScript source. `esm` is upstream's ESM flavour (`.mts`, `.d.mts`),
/// which targets ES2022 and keeps `export * as ns` as it is.
pub fn transpiled(source: &str, source_type: SourceType, esm: bool) -> Transpiled {
    let allocator = Allocator::default();
    let parsed = OxcParser::new(&allocator, source, source_type).parse();
    let mut result = Transpiled {
        elided: elided_imports(&parsed.program),
        lowered_to_import: Vec::new(),
    };
    for statement in &parsed.program.body {
        match statement {
            Statement::ExportFromDeclaration(export) => {
                if export.export_kind == ImportOrExportKind::Type
                    || export
                        .specifiers
                        .iter()
                        .all(|s| s.export_kind == ImportOrExportKind::Type)
                {
                    result.elided.push(export.span);
                }
            }
            Statement::ExportAllDeclaration(export) => {
                if export.export_kind == ImportOrExportKind::Type {
                    result.elided.push(export.span);
                } else if export.exported.is_some() && !esm {
                    result.lowered_to_import.push(export.span);
                }
            }
            _ => {}
        }
    }
    result
}

/// The names read by computed property keys that TypeScript's checker marks as value uses and
/// oxc's semantic does not: keys of type members (interfaces, type literals) and of abstract
/// class members. Keys inside an ambient context (`declare`, `declare module`, `declare
/// global`, a declaration file) are not checked as values, so they are not collected.
#[derive(Default)]
struct ComputedKeyNames {
    ambient: u32,
    names: BTreeSet<String>,
}

impl ComputedKeyNames {
    fn collect(&mut self, computed: bool, key: &oxc_ast::ast::PropertyKey<'_>) {
        struct Names<'n>(&'n mut BTreeSet<String>);
        impl<'a> Visit<'a> for Names<'_> {
            fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
                self.0.insert(it.name.to_string());
            }
        }
        if computed
            && self.ambient == 0
            && let Some(expression) = key.as_expression()
        {
            Names(&mut self.names).visit_expression(expression);
        }
    }

    fn ambient(&mut self, declare: bool, visit: impl FnOnce(&mut Self)) {
        if declare {
            self.ambient += 1;
        }
        visit(self);
        if declare {
            self.ambient -= 1;
        }
    }
}

impl<'a> Visit<'a> for ComputedKeyNames {
    fn visit_ts_property_signature(&mut self, it: &oxc_ast::ast::TSPropertySignature<'a>) {
        self.collect(it.computed, &it.key);
        oxc_ast_visit::walk::walk_ts_property_signature(self, it);
    }

    fn visit_ts_method_signature(&mut self, it: &oxc_ast::ast::TSMethodSignature<'a>) {
        self.collect(it.computed, &it.key);
        oxc_ast_visit::walk::walk_ts_method_signature(self, it);
    }

    fn visit_method_definition(&mut self, it: &oxc_ast::ast::MethodDefinition<'a>) {
        if it.r#type.is_abstract() {
            self.collect(it.computed, &it.key);
        }
        oxc_ast_visit::walk::walk_method_definition(self, it);
    }

    fn visit_property_definition(&mut self, it: &oxc_ast::ast::PropertyDefinition<'a>) {
        self.ambient(it.declare, |this| {
            if it.r#type.is_abstract() {
                this.collect(it.computed, &it.key);
            }
            oxc_ast_visit::walk::walk_property_definition(this, it);
        });
    }

    fn visit_accessor_property(&mut self, it: &oxc_ast::ast::AccessorProperty<'a>) {
        if it.r#type.is_abstract() {
            self.collect(it.computed, &it.key);
        }
        oxc_ast_visit::walk::walk_accessor_property(self, it);
    }

    fn visit_variable_declaration(&mut self, it: &oxc_ast::ast::VariableDeclaration<'a>) {
        self.ambient(it.declare, |this| {
            oxc_ast_visit::walk::walk_variable_declaration(this, it);
        });
    }

    fn visit_class(&mut self, it: &oxc_ast::ast::Class<'a>) {
        self.ambient(it.declare, |this| oxc_ast_visit::walk::walk_class(this, it));
    }

    fn visit_function(&mut self, it: &oxc_ast::ast::Function<'a>, flags: oxc_semantic::ScopeFlags) {
        self.ambient(it.declare, |this| {
            oxc_ast_visit::walk::walk_function(this, it, flags);
        });
    }

    fn visit_ts_namespace_declaration(&mut self, it: &oxc_ast::ast::TSNamespaceDeclaration<'a>) {
        self.ambient(it.declare, |this| {
            oxc_ast_visit::walk::walk_ts_namespace_declaration(this, it);
        });
    }

    fn visit_ts_external_module_declaration(
        &mut self,
        it: &oxc_ast::ast::TSExternalModuleDeclaration<'a>,
    ) {
        self.ambient(true, |this| {
            oxc_ast_visit::walk::walk_ts_external_module_declaration(this, it);
        });
    }

    fn visit_ts_global_declaration(&mut self, it: &oxc_ast::ast::TSGlobalDeclaration<'a>) {
        self.ambient(true, |this| {
            oxc_ast_visit::walk::walk_ts_global_declaration(this, it);
        });
    }
}

/// The import declarations TypeScript's `transpileModule` removes: type-only ones, and ones
/// whose every binding is used only as a type or not at all, `import {} from "x"` among them.
/// Returns their spans.
fn elided_imports(program: &oxc_ast::ast::Program<'_>) -> Vec<Span> {
    let semantic = SemanticBuilder::new().build(program).semantic;
    let scoping = semantic.scoping();
    let mut computed = ComputedKeyNames {
        ambient: u32::from(program.source_type.is_typescript_definition()),
        names: BTreeSet::new(),
    };
    computed.visit_program(program);
    let mut elided = Vec::new();
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else {
            continue;
        };
        if import.import_kind == ImportOrExportKind::Type {
            elided.push(import.span);
            continue;
        }
        let Some(specifiers) = &import.specifiers else {
            continue;
        };
        let used_as_value = specifiers.iter().any(|specifier| {
            let local = match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(s)
                    if s.import_kind == ImportOrExportKind::Type =>
                {
                    return false;
                }
                ImportDeclarationSpecifier::ImportSpecifier(s) => &s.local,
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => &s.local,
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => &s.local,
            };
            computed.names.contains(local.name.as_str())
                || local.symbol_id.get().is_some_and(|symbol| {
                    scoping
                        .get_resolved_references(symbol)
                        .any(oxc_semantic::Reference::is_value)
                })
        });
        if !used_as_value {
            elided.push(import.span);
        }
    }
    elided
}

/// A source file as dependency-cruiser reads it: `readFileSync(file, "utf8")`, where a byte
/// sequence that is not UTF-8 (a Latin-1 comment, a binary file with a source extension) becomes
/// U+FFFD instead of failing the run. Found by the Python oracle harness on
/// openedx/openedx-platform, whose MPEG transport-stream fixtures are named `.ts`.
fn read(path: &Path) -> Result<String, PipelineError> {
    let bytes = std::fs::read(path).map_err(|source| PipelineError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(String::from_utf8(bytes)
        .unwrap_or_else(|invalid| String::from_utf8_lossy(invalid.as_bytes()).into_owned()))
}

/// What part of a file a text to parse is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Script {
    /// The whole file.
    Whole,
    /// The script blocks of a `.vue` or `.svelte` component, with the first one's `lang`
    /// ([`sfc`]).
    Component(Option<String>),
    /// One code fence of a Markdown file, with its normalised language ([`md`]).
    Fence(&'static str),
}

/// A file's source, and the texts the walker reads from it: a component's scripts, a Markdown
/// file's fences (when `extraExtensionsToScan` lists `.md`), anything else whole. Every text is
/// the file's length, so offsets in it are offsets in the file.
fn source_of(
    settings: &Settings,
    file: &str,
) -> Result<(String, Vec<(String, Script)>), PipelineError> {
    let source = read(&settings.on_disk(file))?;
    let extension = node_extname(file);
    let texts = if sfc::is_component(extension) {
        let scripts = sfc::scripts(&source);
        vec![(scripts.text, Script::Component(scripts.lang))]
    } else if reads_fences(settings, file) {
        md::fences(&source)
            .into_iter()
            .map(|fence| (fence.text, Script::Fence(fence.lang)))
            .collect()
    } else {
        vec![(source.clone(), Script::Whole)]
    };
    Ok((source, texts))
}

/// Whether `file` is Markdown whose fences are read: [`Settings::markdown_fences`] is on,
/// `extraExtensionsToScan` lists `.md`, and the file is not literate CoffeeScript (`.coffee.md`,
/// which is the sidecar's).
fn reads_fences(settings: &Settings, file: &str) -> bool {
    settings.markdown_fences
        && resolve::extension(file) == ".md"
        && settings.extra_extensions_to_scan.iter().any(|e| e == ".md")
}

/// The walker a text of `file` gets: a fence gets the one a file with its language's extension
/// would.
fn flavour_of(settings: &Settings, file: &str, script: &Script) -> Flavour {
    match script {
        Script::Fence(lang) => flavour_for(settings, &format!("fence.{lang}")),
        Script::Whole | Script::Component(_) => flavour_for(settings, file),
    }
}

/// The syntax a text is parsed with. A component's script is TypeScript when it says so, or
/// when tsc reads it (tsc parses an unknown extension as TypeScript).
fn syntax_for(file: &str, script: &Script, flavour: Flavour) -> SourceType {
    match script {
        Script::Whole => source_type_for(file),
        Script::Component(lang) => sfc::syntax(lang.as_deref(), flavour == Flavour::Tsc),
        Script::Fence(lang) => md::syntax(lang),
    }
}

/// The forms in a source, as the chosen walker reports them, before resolution; with `code`
/// (the file's output path), also its code layer, read from the same parse.
fn forms(
    settings: &Settings,
    path: &Path,
    source: &str,
    source_type: SourceType,
    (flavour, component): (Flavour, bool),
    code: Option<&str>,
) -> Result<(Vec<Found>, Option<FileCode>), PipelineError> {
    let parse_error = |e: walk::ParseError| PipelineError::Parse {
        path: path.to_path_buf(),
        reason: e.to_string(),
    };
    let options = settings.walk_options();
    let then = |program: &oxc_ast::ast::Program<'_>| {
        code.map(|file| codelayer::collect(program, source, file))
    };
    match flavour {
        Flavour::Acorn if component && source_type.is_typescript() => {
            // A component's TypeScript is not compiled by `transpileModule`: Vue hands acorn the
            // script as written and Svelte strips types only, keeping every value import (the
            // template may use it). What goes is the type-only declarations.
            let stripped = type_declarations(source, source_type);
            let (mut found, collected) =
                walk::walk_source_then(source, source_type, Flavour::Acorn, &options, then)
                    .map_err(parse_error)?;
            found.retain(|f| !stripped.iter().any(|s| s.contains_inclusive(f.span)));
            Ok((found, collected))
        }
        Flavour::Acorn if source_type.is_typescript() => {
            // acorn reads TypeScript only after compiling it, which drops imports used as types
            // and type-only re-exports, and lowers `export * as ns` for the ES2015 target.
            let esm = path
                .to_str()
                .is_some_and(|p| matches!(resolve::extension(p), ".mts" | ".d.mts"));
            let compiled = transpiled(source, source_type, esm);
            let (mut found, collected) =
                walk::walk_source_then(source, source_type, Flavour::Acorn, &options, then)
                    .map_err(parse_error)?;
            let within =
                |spans: &[Span], f: &Found| spans.iter().any(|s| s.contains_inclusive(f.span));
            found.retain(|f| !within(&compiled.elided, f));
            for f in &mut found {
                if within(&compiled.lowered_to_import, f) {
                    for t in &mut f.dependency_types {
                        if *t == DependencyType::Export {
                            *t = DependencyType::Import;
                        }
                    }
                }
            }
            if esm {
                commonjs_output(&mut found, &options.module_systems);
            }
            Ok((found, collected))
        }
        _ => walk::walk_source_then(source, source_type, flavour, &options, then)
            .map_err(parse_error),
    }
}

/// Line starts of a source, for turning byte offsets into 1-based lines and columns.
#[derive(Debug, Clone)]
pub struct Lines<'s> {
    source: &'s str,
    starts: Vec<usize>,
}

impl<'s> Lines<'s> {
    /// Indexes `source`.
    pub fn new(source: &'s str) -> Self {
        let mut starts = vec![0];
        starts.extend(source.match_indices('\n').map(|(at, _)| at + 1));
        Self { source, starts }
    }

    /// The 1-based line and column (in characters) of byte `offset`, clamped to the source.
    pub fn locate(&self, offset: u32) -> (u32, u32) {
        let mut offset = (offset as usize).min(self.source.len());
        while !self.source.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.starts.partition_point(|start| *start <= offset);
        let start = self
            .starts
            .get(line.saturating_sub(1))
            .copied()
            .unwrap_or(0);
        let column = self.source[start..offset].chars().count() + 1;
        (
            u32::try_from(line).unwrap_or(u32::MAX),
            u32::try_from(column).unwrap_or(u32::MAX),
        )
    }
}

/// Upstream compiles `.mts` and `.d.mts` with `module: "nodenext"` through `transpileModule`,
/// whose file is `module.ts`, so the output is CommonJS: every static import and re-export
/// left after elision is a `require` to acorn. `import()` stays as it is. A form that becomes
/// CommonJS is dropped when `moduleSystems` leaves `cjs` out.
fn commonjs_output(found: &mut Vec<Found>, module_systems: &[ModuleSystem]) {
    let keep = module_systems.is_empty() || module_systems.contains(&ModuleSystem::Cjs);
    found.retain_mut(|f| {
        if f.module_system != ModuleSystem::Es6 || f.dynamic {
            return true;
        }
        f.module_system = ModuleSystem::Cjs;
        f.dependency_types = vec![DependencyType::Require];
        keep
    });
}

fn unique_key(found: &Found) -> String {
    format!(
        "{} {} {}",
        found.module,
        found.module_system,
        found.dependency_types.contains(&DependencyType::TypeOnly)
    )
}

/// Under `tsPreCompilationDeps: "specify"`, which forms the compiled JavaScript no longer has.
fn pre_compilation_only(
    settings: &Settings,
    (file, path): (&str, &Path),
    (source, script): (&str, &Script),
    flavour: Flavour,
    found: &[Found],
) -> Result<Option<Vec<bool>>, PipelineError> {
    if flavour != Flavour::Tsc || settings.pre_compilation != PreCompilation::Specify {
        return Ok(None);
    }
    let (compiled, _) = forms(
        settings,
        path,
        source,
        syntax_for(file, script, Flavour::Acorn),
        (Flavour::Acorn, matches!(script, Script::Component(_))),
        None,
    )?;
    Ok(Some(
        found
            .iter()
            .map(|ts| {
                !compiled.iter().any(|js| {
                    js.module == ts.module
                        && js.dynamic == ts.dynamic
                        && js.exotic_require == ts.exotic_require
                })
            })
            .collect(),
    ))
}

/// Babel runs before acorn reads a file, so the acorn walker sees the specifiers
/// `babel-plugin-module-resolver` rewrote.
fn apply_babel_aliases(settings: &Settings, path: &Path, found: &mut [Found]) {
    let Some(babel) = settings.babel.as_ref().filter(|b| !b.is_empty()) else {
        return;
    };
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        settings.cwd.join(path)
    };
    for form in found {
        if let Some(rewritten) = babel.rewrite(&form.module, &absolute) {
            form.module = rewritten;
        }
    }
}

/// `extractDependencies` for one file: forms found, resolved, filtered and sorted.
///
/// # Errors
/// When the file cannot be read or parsed.
pub fn extract_dependencies(
    file: &str,
    settings: &Settings,
    config: &ResolveConfig,
) -> Result<Vec<Extracted>, PipelineError> {
    resolved_dependencies(file, settings, config, false).map(|(extracted, _, _)| extracted)
}

/// What one file yields: its dependencies and, when [`Settings::code_layer`] is on, its code
/// layer, still to be linked with the other files'.
type FileResult = Result<(Vec<Extracted>, Option<FileCode>), PipelineError>;

fn extract_file(file: &str, settings: &Settings, config: &ResolveConfig) -> FileResult {
    resolved_dependencies(file, settings, config, settings.code_layer)
        .map(|(extracted, _, code)| (extracted, code))
}

/// What [`resolved_dependencies`] returns.
type Resolved = (
    Vec<Extracted>,
    Option<resolve::ExtensionList>,
    Option<FileCode>,
);

/// [`extract_dependencies`], and the extension list of the file's first resolution that found
/// a file, in upstream's resolving order (before filtering), for
/// [`ResolveConfig::settle_followable`]; with `collect`, also the file's code layer.
fn resolved_dependencies(
    file: &str,
    settings: &Settings,
    config: &ResolveConfig,
    collect: bool,
) -> Result<Resolved, PipelineError> {
    if !reads_fences(settings, file)
        && settings
            .extra_extensions_to_scan
            .iter()
            .any(|e| e == node_extname(file))
    {
        return Ok((Vec::new(), None, None));
    }
    let mut first_found = None;
    let path = settings.on_disk(file);
    let (source, texts) = source_of(settings, file)?;
    let mut found = Vec::new();
    let mut pre_compilation: Vec<Option<bool>> = Vec::new();
    let mut code = None;
    for (text, script) in &texts {
        let flavour = flavour_of(settings, file, script);
        let source_type = syntax_for(file, script, flavour);
        // A Markdown fence is an example, not a part of the program: it has no code layer.
        let layer = (collect && !matches!(script, Script::Fence(_))).then_some(file);
        let (mut forms_found, collected) = forms(
            settings,
            &path,
            text,
            source_type,
            (flavour, matches!(script, Script::Component(_))),
            layer,
        )?;
        if flavour == Flavour::Acorn {
            apply_babel_aliases(settings, &path, &mut forms_found);
        }
        let only = pre_compilation_only(
            settings,
            (file, &path),
            (text, script),
            flavour,
            &forms_found,
        )?;
        pre_compilation
            .extend((0..forms_found.len()).map(|i| only.as_ref().and_then(|o| o.get(i).copied())));
        found.append(&mut forms_found);
        code = code.or(collected);
    }
    let lines = Lines::new(&source);
    // Module attributes first, then unique by module, system and type-only-ness.
    let mut seen = BTreeSet::new();
    let mut extracted = Vec::new();
    let file_dir = Path::new(file)
        .parent()
        .map_or_else(|| settings.base_dir.clone(), |d| settings.base_dir.join(d));
    let context = Context {
        cwd: &settings.cwd,
        base_dir: &settings.base_dir,
        file_dir: &file_dir,
    };
    for (index, mut form) in found.drain(..).enumerate() {
        let only = pre_compilation.get(index).copied().flatten();
        if only == Some(true) {
            form.dependency_types
                .push(DependencyType::PreCompilationOnly);
        }
        let (module, protocol, mime_type) = module_attributes(&form.module);
        form.module = module;
        if !seen.insert(unique_key(&form)) {
            continue;
        }
        let resolution = resolve::resolve(
            &form.module,
            form.module_system,
            &form.dependency_types,
            &context,
            config,
        );
        first_found = first_found.or(resolution.asked_with);
        let (line, column) = lines.locate(form.span.start);
        let matches_do_not_follow = settings.stops(&resolution);
        extracted.push(Extracted {
            module: form.module,
            module_system: form.module_system,
            dynamic: form.dynamic,
            exotically_required: form.exotically_required,
            exotic_require: form.exotic_require,
            dependency_types: resolution.dependency_types,
            protocol,
            mime_type,
            pre_compilation_only: only,
            followable: resolution.followable && !matches_do_not_follow,
            resolved: resolution.resolved,
            core_module: resolution.core_module,
            could_not_resolve: resolution.could_not_resolve,
            matches_do_not_follow,
            license: resolution.license,
            span: form.span,
            line,
            column,
        });
    }
    if let Some(code) = &mut code {
        resolve_code_specifiers(code, &extracted, &context, config);
    }
    filter_and_sort(&mut extracted, settings);
    Ok((extracted, first_found, code))
}

/// `exclude` and `includeOnly` over the resolutions, then upstream's order.
fn filter_and_sort(extracted: &mut Vec<Extracted>, settings: &Settings) {
    extracted.retain(|d| {
        !settings
            .exclude
            .as_ref()
            .is_some_and(|f| f.path_matches(&d.resolved))
            && settings
                .include_only
                .as_ref()
                .is_none_or(|f| f.path.is_none() || f.path_matches(&d.resolved))
    });
    extracted.sort_by(|a, b| {
        let key = |d: &Extracted| {
            format!(
                "{} {} {}",
                d.module,
                d.module_system,
                d.dependency_types.contains(&DependencyType::TypeOnly)
            )
        };
        collate::compare(&key(a), &key(b))
    });
}

/// Records where each module a code-layer name is imported from resolves to: the dependency the
/// walk already resolved when there is one, otherwise a resolution of its own (an import used
/// only as a type is elided from the dependencies but still names the type). Built-in and
/// unresolvable modules are left out, so names from them stay as written.
fn resolve_code_specifiers(
    code: &mut FileCode,
    extracted: &[Extracted],
    context: &Context<'_>,
    config: &ResolveConfig,
) {
    for specifier in code.specifiers() {
        let known = extracted
            .iter()
            .find(|d| d.module == specifier)
            .map(|d| (d.resolved.clone(), d.core_module || d.could_not_resolve));
        let (resolved, unusable) = known.unwrap_or_else(|| {
            let resolution = resolve::resolve(
                &specifier,
                ModuleSystem::Es6,
                &[DependencyType::Import],
                context,
                config,
            );
            (
                resolution.resolved,
                resolution.core_module || resolution.could_not_resolve,
            )
        });
        if !unusable {
            code.set_resolved(specifier, resolved);
        }
    }
}

/// Settles which extension list decides `followable` for the run, as upstream's first
/// successful resolution does ([`ResolveConfig::settle_followable`]): the initial sources in
/// order, each file's resolutions in order, until one finds a file. Before that first success
/// nothing is followable, so upstream's depth-first walk reaches no other file first. A file
/// that fails to extract is passed over here; the walk reports it when it gets there.
fn settle_followable(initial: &[String], settings: &Settings, config: &ResolveConfig) {
    if config.bust_the_cache || config.settled_followable().is_some() {
        return;
    }
    for file in initial {
        if let Ok((_, Some(list), _)) = resolved_dependencies(file, settings, config, false) {
            config.settle_followable(list);
            return;
        }
    }
}

fn is_glob(text: &str) -> bool {
    text.contains(['*', '?', '[', '{', '!', '('])
}

fn scannable(settings: &Settings, file: &str) -> bool {
    let ext = resolve::extension(file);
    SCANNABLE_EXTENSIONS.contains(&ext)
        || settings.extra_extensions_to_scan.iter().any(|e| e == ext)
}

/// `ancestors` are the canonical folders above `directory` on this walk, `directory`'s own
/// included: a symlinked folder is followed as upstream's `readdirSync` walk follows it, unless it
/// leads back to one of them, which would recurse without end.
fn gather_directory(
    directory: &str,
    settings: &Settings,
    ancestors: &mut Vec<PathBuf>,
    out: &mut Vec<String>,
) -> Result<(), PipelineError> {
    let on_disk = settings.on_disk(directory);
    let mut entries: Vec<String> = std::fs::read_dir(&on_disk)
        .map_err(|source| PipelineError::Io {
            path: on_disk.clone(),
            source,
        })?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    for name in entries {
        let path = if directory.is_empty() || directory == "." {
            name
        } else {
            format!("{}/{name}", directory.trim_end_matches('/'))
        };
        let excluded = settings
            .exclude
            .as_ref()
            .is_some_and(|f| f.path_matches(&path))
            || settings
                .do_not_follow
                .as_ref()
                .is_some_and(|f| f.path_matches(&path));
        if excluded {
            continue;
        }
        let on_disk = settings.on_disk(&path);
        let Ok(metadata) = std::fs::metadata(&on_disk) else {
            continue;
        };
        if metadata.is_dir() {
            let Some(canonical) = not_a_cycle(&on_disk, ancestors) else {
                continue;
            };
            ancestors.push(canonical);
            let gathered = gather_directory(&path, settings, ancestors, out);
            ancestors.pop();
            gathered?;
        } else if scannable(settings, &path)
            && settings
                .include_only
                .as_ref()
                .is_none_or(|f| f.path.is_none() || f.path_matches(&path))
        {
            out.push(path);
        }
    }
    Ok(())
}

/// The canonical form of folder `path`, unless it is one of `ancestors` (a symlink cycle).
fn not_a_cycle(path: &Path, ancestors: &[PathBuf]) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(path).ok()?;
    (!ancestors.contains(&canonical)).then_some(canonical)
}

fn normalise(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

fn expand_glob(pattern: &str, settings: &Settings) -> Vec<String> {
    let segments: Vec<&str> = pattern.split('/').collect();
    let base_count = segments.iter().take_while(|s| !is_glob(s)).count();
    let base = segments[..base_count].join("/");
    let glob = segments[base_count..].join("/");
    let Ok(matcher) = globset::GlobBuilder::new(&glob)
        .literal_separator(true)
        .build()
        .map(|g| g.compile_matcher())
    else {
        return Vec::new();
    };
    let mut all = Vec::new();
    let root = settings.on_disk(&base);
    let mut stack: Vec<(PathBuf, Vec<PathBuf>)> =
        vec![(root.clone(), not_a_cycle(&root, &[]).into_iter().collect())];
    while let Some((dir, ancestors)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
                && let Some(canonical) = not_a_cycle(&path, &ancestors)
            {
                let mut below: Vec<PathBuf> = ancestors.clone();
                below.push(canonical);
                stack.push((path.clone(), below));
            }
            let relative = resolve::relative(&root, &path);
            if matcher.is_match(&relative) {
                all.push(if base.is_empty() {
                    relative
                } else {
                    format!("{base}/{relative}")
                });
            }
        }
    }
    all
}

/// `gatherInitialSources`: files and folders expanded to scannable files, sorted.
///
/// # Errors
/// When a named file or folder does not exist.
pub fn gather_initial_sources(
    inputs: &[String],
    settings: &Settings,
) -> Result<Vec<String>, PipelineError> {
    let mut expanded = Vec::new();
    for input in inputs {
        if is_glob(input) {
            expanded.extend(expand_glob(input, settings));
        } else {
            expanded.push(normalise(input));
        }
    }
    let mut files = Vec::new();
    for item in expanded {
        let on_disk = settings.on_disk(&item);
        let metadata = std::fs::metadata(&on_disk).map_err(|source| PipelineError::Io {
            path: on_disk.clone(),
            source,
        })?;
        if metadata.is_dir() {
            let mut ancestors: Vec<PathBuf> = not_a_cycle(&on_disk, &[]).into_iter().collect();
            gather_directory(&item, settings, &mut ancestors, &mut files)?;
        } else {
            files.push(item);
        }
    }
    // JavaScript's default sort: UTF-16 code units, which for these paths is byte order.
    files.sort();
    Ok(files)
}

/// One module as `extract` returns it, before the rule engine sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedModule {
    /// The file, or the resolution of a dependency that is not followed.
    pub source: String,
    /// Its dependencies.
    pub dependencies: Vec<Extracted>,
    /// Statistics, when asked for.
    pub experimental_stats: Option<ExperimentalStats>,
    /// For a module standing for an unfollowed dependency: the dependency's attributes.
    pub as_dependency: Option<Extracted>,
    /// The file's code layer, before linking, when [`Settings::code_layer`] is on.
    pub code: Option<FileCode>,
}

/// Whether a dependency leads to a file the walk extracts in turn.
fn followed(dependency: &Extracted) -> bool {
    dependency.followable && !dependency.matches_do_not_follow
}

/// Phase one of [`extract`]: the dependencies of every file the walk can extract, keyed by file,
/// found in parallel one breadth-first frontier at a time. A file at breadth-first depth `d` is
/// at depth `d` or more in upstream's depth-first walk, so extracting every file with `d` below
/// `maxDepth` covers every file the depth-first walk extracts. A failure is kept, not raised: the
/// replay raises it only if the depth-first walk reaches that file.
fn reachable_dependencies(
    initial: &[String],
    settings: &Settings,
    config: &ResolveConfig,
) -> BTreeMap<String, FileResult> {
    let mut done: BTreeMap<String, FileResult> = BTreeMap::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut frontier: Vec<String> = initial
        .iter()
        .filter(|f| seen.insert(f.as_str()))
        .cloned()
        .collect();
    let mut depth = 0u32;
    while !frontier.is_empty() && (settings.max_depth == 0 || depth < settings.max_depth) {
        let results: Vec<(String, FileResult)> = frontier
            .into_par_iter()
            .map(|file| {
                let result = extract_file(&file, settings, config);
                (file, result)
            })
            .collect();
        let mut next = BTreeSet::new();
        for (_, result) in &results {
            let dependencies = result.iter().flat_map(|(d, _)| d);
            for dependency in dependencies.filter(|d| followed(d)) {
                next.insert(dependency.resolved.clone());
            }
        }
        done.extend(results);
        frontier = next
            .into_iter()
            .filter(|f| !done.contains_key(f) && !seen.contains(f.as_str()))
            .collect();
        depth += 1;
    }
    done
}

/// Phase two of [`extract`]: upstream's `extractRecursive`, depth first from each initial
/// source, over the dependencies phase one found.
fn replay(
    initial: &[String],
    settings: &Settings,
    config: &ResolveConfig,
    mut found: BTreeMap<String, FileResult>,
) -> Result<Vec<ExtractedModule>, PipelineError> {
    struct Frame {
        follow: Vec<String>,
        next: usize,
        depth: u32,
    }
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();
    let mut visit = |file: &str,
                     depth: u32,
                     visited: &mut BTreeSet<String>,
                     out: &mut Vec<ExtractedModule>|
     -> Result<Frame, PipelineError> {
        visited.insert(file.to_owned());
        let (dependencies, code) = if settings.max_depth == 0 || depth < settings.max_depth {
            match found.remove(file) {
                Some(result) => result?,
                None => extract_file(file, settings, config)?,
            }
        } else {
            (Vec::new(), None)
        };
        let follow = dependencies
            .iter()
            .filter(|d| followed(d))
            .map(|d| d.resolved.clone())
            .collect();
        out.push(ExtractedModule {
            source: file.to_owned(),
            dependencies,
            experimental_stats: None,
            as_dependency: None,
            code,
        });
        Ok(Frame {
            follow,
            next: 0,
            depth,
        })
    };
    for file in initial {
        if visited.contains(file) {
            continue;
        }
        let mut stack = vec![visit(file, 0, &mut visited, &mut out)?];
        while let Some(frame) = stack.last_mut() {
            let Some(next) = frame.follow.get(frame.next).cloned() else {
                stack.pop();
                continue;
            };
            frame.next += 1;
            let depth = frame.depth + 1;
            if !visited.contains(&next) {
                let child = visit(&next, depth, &mut visited, &mut out)?;
                stack.push(child);
            }
        }
    }
    Ok(out)
}

/// `extract`: every module reachable from the inputs, then the unfollowed dependencies as modules.
///
/// # Errors
/// When an input is missing or a file cannot be read or parsed.
pub fn extract(
    inputs: &[String],
    settings: &Settings,
    config: &ResolveConfig,
) -> Result<Vec<ExtractedModule>, PipelineError> {
    let initial = gather_initial_sources(inputs, settings)?;
    settle_followable(&initial, settings, config);
    let found = reachable_dependencies(&initial, settings, config);
    let mut modules = replay(&initial, settings, config, found)?;
    if settings.experimental_stats {
        let all: Vec<Result<ExperimentalStats, PipelineError>> = modules
            .par_iter()
            .map(|m| stats(&m.source, settings))
            .collect();
        for (module, result) in modules.iter_mut().zip(all) {
            module.experimental_stats = Some(result?);
        }
    }
    let mut complete: Vec<ExtractedModule> = Vec::with_capacity(modules.len());
    let mut sources: BTreeSet<String> = BTreeSet::new();
    for module in modules {
        let unfollowed: Vec<ExtractedModule> = module
            .dependencies
            .iter()
            .filter(|d| !d.followable && !sources.contains(&d.resolved))
            .map(|d| ExtractedModule {
                source: d.resolved.clone(),
                dependencies: Vec::new(),
                experimental_stats: None,
                as_dependency: Some(d.clone()),
                code: None,
            })
            .collect();
        // Upstream compares with the modules before this one only, so one module's duplicate
        // unfollowed dependencies each become a module.
        sources.insert(module.source.clone());
        sources.extend(unfollowed.iter().map(|m| m.source.clone()));
        complete.push(module);
        complete.extend(unfollowed);
    }
    if let Some(dynamic) = settings.exclude.as_ref().and_then(|f| f.dynamic) {
        for module in &mut complete {
            module.dependencies.retain(|d| d.dynamic != dynamic);
        }
    }
    Ok(complete)
}

/// `experimentalStats` for one file: top-level statements and size.
///
/// Upstream parses with acorn and falls back to acorn-loose, which never throws, so a file that
/// is not JavaScript (a `.json` added by `extraExtensionsToScan`, say) still gets statistics.
/// Here the recovering parser's result is used the same way: statistics never fail a run.
///
/// # Errors
/// When the file cannot be read.
pub fn stats(file: &str, settings: &Settings) -> Result<ExperimentalStats, PipelineError> {
    let path = settings.on_disk(file);
    let source = read(&path)?;
    let allocator = Allocator::default();
    let parsed = OxcParser::new(&allocator, &source, source_type_for(file))
        .with_options(ParseOptions {
            allow_return_outside_function: true,
            ..ParseOptions::default()
        })
        .parse();
    Ok(ExperimentalStats {
        top_level_statement_count: top_level_statement_count(
            file,
            &source,
            parsed.program.directives.len() + parsed.program.body.len(),
        ),
        size: utf16_length(&source),
    })
}

/// The top-level statement count upstream reports, from the number oxc parsed. A directive
/// prologue (`"use strict";`) is a statement to acorn and tsc alike, so the caller counts it. A
/// JSON document that is not also JavaScript (`{"a": 1}`) stops oxc with nothing parsed, where
/// acorn-loose recovers it as the one block statement it starts.
fn top_level_statement_count(file: &str, source: &str, parsed: usize) -> u64 {
    // Case-sensitive, as upstream's extension lookups are.
    if parsed == 0 && node_extname(file) == ".json" && !source.trim().is_empty() {
        1
    } else {
        parsed as u64
    }
}

/// The length of `source` in UTF-16 code units: upstream's `size` is the parsed tree's `end`,
/// an offset into a JavaScript string, so a character outside ASCII counts as one unit (two
/// above U+FFFF), never as its UTF-8 bytes.
pub fn utf16_length(source: &str) -> u64 {
    source.chars().map(|c| c.len_utf16() as u64).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statements_count_directives_and_a_json_document_as_one() {
        assert_eq!(top_level_statement_count("a.json", "{\"a\": 1}", 0), 1);
        assert_eq!(top_level_statement_count("a.json", " \n", 0), 0);
        assert_eq!(top_level_statement_count("a.json", "[1]", 1), 1);
        assert_eq!(top_level_statement_count("a.mjs", "{\"a\": 1}", 0), 0);
        assert_eq!(top_level_statement_count("a.mjs", "x", 3), 3);
        let dir = std::env::temp_dir().join(format!("rb-stats-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("strict.mjs");
        let _ = std::fs::write(&file, "\"use strict\";\nexport const a = 1;\n");
        let counted = Settings::new(&rb_model::TypeScriptOptions::default(), &dir)
            .ok()
            .and_then(|settings| stats("strict.mjs", &settings).ok())
            .map(|s| s.top_level_statement_count);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(counted, Some(2));
    }

    #[test]
    fn a_source_that_is_not_utf8_is_read_as_node_reads_it() {
        let dir = std::env::temp_dir().join(format!("rb-read-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("latin1.js");
        let _ = std::fs::write(&file, b"// caf\xe9\nimport x from './x';\n");
        let text = read(&file).ok();
        let missing = read(&dir.join("missing.js"));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            text.as_deref(),
            Some("// caf\u{fffd}\nimport x from './x';\n")
        );
        assert!(
            matches!(missing, Err(PipelineError::Io { ref path, .. }) if path.ends_with("missing.js"))
        );
    }

    #[test]
    fn size_counts_utf16_code_units_as_javascript_does() {
        assert_eq!(utf16_length(""), 0);
        assert_eq!(utf16_length("abc"), 3);
        // An em dash is three UTF-8 bytes and one UTF-16 unit; an emoji four bytes, two units.
        assert_eq!(utf16_length("a\u{2014}b"), 3);
        assert_eq!(utf16_length("\u{1f600}"), 2);
        assert_eq!(utf16_length("\u{feff}x"), 2);
    }

    #[test]
    fn module_attributes_follow_upstream() {
        assert_eq!(
            module_attributes("protodash"),
            ("protodash".to_owned(), None, None)
        );
        assert_eq!(
            module_attributes("node:fs"),
            ("fs".to_owned(), Some(Protocol::Node), None)
        );
        assert_eq!(
            module_attributes("bun:fs"),
            ("fs".to_owned(), Some(Protocol::Bun), None)
        );
        assert_eq!(
            module_attributes("node:sea"),
            ("node:sea".to_owned(), Some(Protocol::Node), None)
        );
        assert_eq!(
            module_attributes("file:x.js"),
            ("file:x.js".to_owned(), Some(Protocol::File), None)
        );
        assert_eq!(
            module_attributes("data:text/javascript,export default 1"),
            (
                "data:export default 1".to_owned(),
                Some(Protocol::Data),
                Some("text/javascript".to_owned())
            )
        );
        assert_eq!(module_attributes("node:"), ("node:".to_owned(), None, None));
    }

    #[test]
    fn component_and_fence_syntax_and_walkers() -> Result<(), PipelineError> {
        let check = |lang: Option<&str>, flavour: Flavour| {
            syntax_for(
                "a.vue",
                &Script::Component(lang.map(str::to_owned)),
                flavour,
            )
        };
        assert!(check(Some("tsx"), Flavour::Acorn).is_jsx());
        assert!(check(Some("ts"), Flavour::Acorn).is_typescript());
        assert!(check(None, Flavour::Tsc).is_typescript());
        assert!(check(Some("jsx"), Flavour::Acorn).is_jsx());
        assert!(!check(None, Flavour::Acorn).is_typescript());
        assert!(syntax_for("a.ts", &Script::Whole, Flavour::Acorn).is_typescript());
        assert!(syntax_for("a.md", &Script::Fence("ts"), Flavour::Acorn).is_typescript());
        let settings = Settings::new(
            &TypeScriptOptions {
                parser: Some(Parser::Tsc),
                ..TypeScriptOptions::default()
            },
            Path::new("."),
        )?;
        assert_eq!(
            flavour_of(&settings, "a.md", &Script::Fence("ts")),
            Flavour::Tsc
        );
        assert_eq!(
            flavour_of(&settings, "a.md", &Script::Fence("jsx")),
            Flavour::Acorn
        );
        assert_eq!(
            flavour_of(&settings, "a.svelte", &Script::Component(None)),
            Flavour::Acorn
        );
        assert_eq!(
            flavour_of(&settings, "a.vue", &Script::Component(None)),
            Flavour::Tsc
        );
        Ok(())
    }

    #[test]
    fn lines_locate_offsets_in_characters() {
        let lines = Lines::new("ab\ncé\nd");
        assert_eq!(lines.locate(0), (1, 1));
        assert_eq!(lines.locate(3), (2, 1));
        assert_eq!(lines.locate(5), (2, 2));
        assert_eq!(lines.locate(6), (2, 3));
        assert_eq!(lines.locate(7), (3, 1));
        assert_eq!(lines.locate(99), (3, 2));
    }

    #[test]
    fn extname_and_normalise_match_node() {
        assert_eq!(node_extname("a/b.test.js"), ".js");
        assert_eq!(node_extname("a/.hidden"), "");
        assert_eq!(normalise("./a/../b/./c"), "b/c");
        assert_eq!(normalise("../x"), "../x");
        assert_eq!(normalise("."), ".");
        assert!(is_glob("src/**/*.ts") && !is_glob("src/a.ts"));
    }

    #[test]
    fn type_only_and_unused_imports_are_elided_as_compilation_would() {
        let source = "import type A from './a';\nimport { B } from './b';\nimport { C } from './c';\nimport './d';\nimport { E } from './e';\nconst x: B = C;\n";
        let compiled = transpiled(source, SourceType::ts(), false);
        let elided: Vec<&str> = compiled
            .elided
            .iter()
            .map(|s| &source[s.start as usize..s.end as usize])
            .collect();
        assert_eq!(
            elided,
            [
                "import type A from './a';",
                "import { B } from './b';",
                "import { E } from './e';"
            ]
        );
        assert!(compiled.lowered_to_import.is_empty());
    }

    /// A computed property name is an expression wherever it stands, so the import it names
    /// survives compilation, types and abstract members included; not in an ambient context.
    /// Each expectation is `typescript.transpileModule` 6.0.3's (target ES2015).
    #[test]
    fn computed_keys_keep_their_import_outside_ambient_contexts() {
        let cases = [
            (
                "export abstract class C { abstract get [m](): number; }",
                true,
            ),
            ("export abstract class C { abstract [m]: number; }", true),
            ("export abstract class C { abstract [m](): void; }", true),
            ("export interface I { [m]: number }", true),
            ("export interface I { [m](): void }", true),
            ("export type T = { [m]: number };", true),
            ("export function f(a: { [m]: 1 }) {}", true),
            ("export class C { [m]!: number; }", true),
            ("export class C { [m]?(): void; }", true),
            ("declare const x: { [m]: number };", false),
            ("export class C { declare [m]: number; }", false),
            ("export declare class C { [m]: number; }", false),
            ("export declare function f(): { [m]: 1 };", false),
            ("export type T = typeof m;", false),
        ];
        let wrong: Vec<&str> = cases
            .iter()
            .filter(|(body, kept)| {
                let source = format!("import {{ m }} from './a';\n{body}\n");
                transpiled(&source, SourceType::ts(), false)
                    .elided
                    .is_empty()
                    != *kept
            })
            .map(|(body, _)| *body)
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// What `typescript.transpileModule` 6.0.3 does with target ES2015, as upstream calls it.
    #[test]
    fn re_exports_are_elided_and_lowered_as_compilation_would() {
        let source = "export type { A } from './a';\nexport { type B } from './b';\nexport { type C, D } from './c';\nexport {} from './e';\nexport type * from './f';\nexport * from './g';\nexport * as ns from './h';\nexport { I } from './i';\nimport {} from './j';\nimport K, {} from './k';\nimport L, { type M } from './l';\nL();\n";
        let text = |spans: &[Span]| -> Vec<String> {
            spans
                .iter()
                .map(|s| source[s.start as usize..s.end as usize].to_owned())
                .collect()
        };
        let compiled = transpiled(source, SourceType::ts(), false);
        let mut elided = text(&compiled.elided);
        elided.sort();
        assert_eq!(
            elided,
            [
                "export type * from './f';",
                "export type { A } from './a';",
                "export { type B } from './b';",
                "export {} from './e';",
                "import K, {} from './k';",
                "import {} from './j';",
            ]
        );
        assert_eq!(
            text(&compiled.lowered_to_import),
            ["export * as ns from './h';"]
        );
        // The ESM flavour targets ES2022, which has `export * as ns`.
        assert!(
            transpiled(source, SourceType::ts(), true)
                .lowered_to_import
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_cycle_is_walked_once_and_other_symlinked_folders_are_followed() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("rb-gather-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(root.join("src/a"));
        let _ = std::fs::create_dir_all(root.join("elsewhere"));
        let _ = std::fs::write(root.join("src/a/x.js"), "");
        let _ = std::fs::write(root.join("elsewhere/y.js"), "");
        let _ = symlink(root.join("src"), root.join("src/a/loop"));
        let _ = symlink(root.join("elsewhere"), root.join("src/linked"));
        let settings = Settings::new(&TypeScriptOptions::default(), &root);
        let gathered = settings
            .as_ref()
            .map(|s| gather_initial_sources(&["src".to_owned()], s));
        let globbed = settings
            .as_ref()
            .map(|s| gather_initial_sources(&["src/**/*.js".to_owned()], s));
        let _ = std::fs::remove_dir_all(&root);
        let expected = vec!["src/a/x.js".to_owned(), "src/linked/y.js".to_owned()];
        assert_eq!(gathered.ok().and_then(Result::ok), Some(expected.clone()));
        assert_eq!(globbed.ok().and_then(Result::ok), Some(expected));
    }
}
