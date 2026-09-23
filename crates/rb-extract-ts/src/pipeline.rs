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
use oxc_parser::{ParseOptions, Parser as OxcParser};
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType, Span};
use rb_model::options::{PathFilter, TsPreCompilationDeps};
use rb_model::{
    DependencyType, ExperimentalStats, ModuleSystem, Parser, Protocol, TypeScriptOptions,
};
use regex::Regex;

use crate::babel::BabelAliases;
use crate::collate;
use crate::resolve::{self, Context, ResolveConfig, SCANNABLE_EXTENSIONS};
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

/// The import declarations TypeScript's `transpileModule` removes: type-only ones, and ones
/// whose every binding is used only as a type or not at all. Returns their spans.
pub fn elided_imports(source: &str, source_type: SourceType) -> Vec<Span> {
    let allocator = Allocator::default();
    let parsed = OxcParser::new(&allocator, source, source_type).parse();
    let semantic = SemanticBuilder::new().build(&parsed.program).semantic;
    let scoping = semantic.scoping();
    let mut elided = Vec::new();
    for statement in &parsed.program.body {
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
        if specifiers.is_empty() {
            continue;
        }
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
            local.symbol_id.get().is_some_and(|symbol| {
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

fn read(path: &Path) -> Result<String, PipelineError> {
    std::fs::read_to_string(path).map_err(|source| PipelineError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// The script of a Vue single-file component, as `@vue/compiler-sfc` hands it to upstream's
/// parsers: the `<script>` and `<script setup>` contents. Everything else is blanked to spaces,
/// newlines kept, so every byte offset (and so every line and column) is the one in the file.
/// Also returns the first script's `lang`.
pub fn vue_script(source: &str) -> (String, Option<String>) {
    let mut out: Vec<u8> = source
        .bytes()
        .map(|b| if b == b'\n' || b == b'\r' { b } else { b' ' })
        .collect();
    let mut lang = None;
    let mut from = 0;
    while let Some(at) = source[from..].find("<script").map(|i| i + from) {
        let after_name = at + "<script".len();
        let boundary = source[after_name..].chars().next();
        let Some(open_end) = source[after_name..].find('>').map(|i| i + after_name) else {
            break;
        };
        if !matches!(boundary, Some(c) if c == '>' || c.is_whitespace()) {
            from = after_name;
            continue;
        }
        let attributes = &source[after_name..open_end];
        if lang.is_none() {
            lang = attribute(attributes, "lang");
        }
        let body_start = open_end + 1;
        let body_end = source[body_start..]
            .find("</script>")
            .map_or(source.len(), |i| i + body_start);
        out[body_start..body_end].copy_from_slice(&source.as_bytes()[body_start..body_end]);
        from = body_end;
    }
    // Only ASCII bytes were replaced, and whole script bodies copied back, so this is UTF-8
    // unless a multi-byte character straddled a tag, which the tags' ASCII delimiters rule out.
    (String::from_utf8(out).unwrap_or_default(), lang)
}

/// The value of `name="..."` (or `'...'`, or unquoted) in a tag's attribute text.
fn attribute(attributes: &str, name: &str) -> Option<String> {
    let at = attributes.find(&format!("{name}="))? + name.len() + 1;
    let rest = &attributes[at..];
    let value = match rest.chars().next()? {
        quote @ ('"' | '\'') => rest[1..].split(quote).next()?,
        _ => rest.split(|c: char| c.is_whitespace() || c == '/').next()?,
    };
    Some(value.to_owned())
}

/// A file's source as the walker reads it: a `.vue` file's script, anything else whole. The
/// second value is the Vue script's `lang`.
fn source_of(settings: &Settings, file: &str) -> Result<(String, Option<String>), PipelineError> {
    let source = read(&settings.on_disk(file))?;
    if node_extname(file) == ".vue" {
        Ok(vue_script(&source))
    } else {
        Ok((source, None))
    }
}

/// The syntax a file is parsed with. A Vue script is TypeScript when it says so, or when tsc
/// reads it (tsc parses an unknown extension as TypeScript).
fn syntax_for(file: &str, vue_lang: Option<&str>, flavour: Flavour) -> SourceType {
    if node_extname(file) != ".vue" {
        return source_type_for(file);
    }
    match vue_lang {
        Some("tsx") => SourceType::tsx(),
        Some("ts") => SourceType::ts(),
        Some("jsx") => SourceType::mjs().with_jsx(true),
        _ if flavour == Flavour::Tsc => SourceType::ts(),
        _ => SourceType::mjs(),
    }
}

/// The forms in a source, as the chosen walker reports them, before resolution.
fn forms(
    settings: &Settings,
    path: &Path,
    source: &str,
    source_type: SourceType,
    flavour: Flavour,
) -> Result<Vec<Found>, PipelineError> {
    let parse_error = |e: walk::ParseError| PipelineError::Parse {
        path: path.to_path_buf(),
        reason: e.to_string(),
    };
    let options = settings.walk_options();
    match flavour {
        Flavour::Acorn if source_type.is_typescript() => {
            // acorn reads TypeScript only after compiling it, which drops imports used as types.
            let elided = elided_imports(source, source_type);
            let mut found = walk::walk_source(source, source_type, Flavour::Acorn, &options)
                .map_err(parse_error)?;
            found.retain(|f| !elided.iter().any(|span| span.contains_inclusive(f.span)));
            Ok(found)
        }
        _ => walk::walk_source(source, source_type, flavour, &options).map_err(parse_error),
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
    (source, vue_lang): (&str, Option<&str>),
    flavour: Flavour,
    found: &[Found],
) -> Result<Option<Vec<bool>>, PipelineError> {
    if flavour != Flavour::Tsc || settings.pre_compilation != PreCompilation::Specify {
        return Ok(None);
    }
    let compiled = forms(
        settings,
        path,
        source,
        syntax_for(file, vue_lang, Flavour::Acorn),
        Flavour::Acorn,
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
    if settings
        .extra_extensions_to_scan
        .iter()
        .any(|e| e == node_extname(file))
    {
        return Ok(Vec::new());
    }
    let flavour = flavour_for(settings, file);
    let path = settings.on_disk(file);
    let (source, vue_lang) = source_of(settings, file)?;
    let source_type = syntax_for(file, vue_lang.as_deref(), flavour);
    let mut found = forms(settings, &path, &source, source_type, flavour)?;
    if flavour == Flavour::Acorn {
        apply_babel_aliases(settings, &path, &mut found);
    }
    let pre_compilation = pre_compilation_only(
        settings,
        (file, &path),
        (&source, vue_lang.as_deref()),
        flavour,
        &found,
    )?;
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
        let only = pre_compilation.as_ref().and_then(|p| p.get(index).copied());
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
    Ok(extracted)
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
) -> BTreeMap<String, Result<Vec<Extracted>, PipelineError>> {
    let mut done: BTreeMap<String, Result<Vec<Extracted>, PipelineError>> = BTreeMap::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut frontier: Vec<String> = initial
        .iter()
        .filter(|f| seen.insert(f.as_str()))
        .cloned()
        .collect();
    let mut depth = 0u32;
    while !frontier.is_empty() && (settings.max_depth == 0 || depth < settings.max_depth) {
        let results: Vec<(String, Result<Vec<Extracted>, PipelineError>)> = frontier
            .into_par_iter()
            .map(|file| {
                let result = extract_dependencies(&file, settings, config);
                (file, result)
            })
            .collect();
        let mut next = BTreeSet::new();
        for (_, result) in &results {
            for dependency in result.iter().flatten().filter(|d| followed(d)) {
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
    mut found: BTreeMap<String, Result<Vec<Extracted>, PipelineError>>,
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
        let dependencies = if settings.max_depth == 0 || depth < settings.max_depth {
            match found.remove(file) {
                Some(result) => result?,
                None => extract_dependencies(file, settings, config)?,
            }
        } else {
            Vec::new()
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
/// # Errors
/// When the file cannot be read or parsed.
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
    if parsed.diagnostics.has_errors()
        && parsed.program.body.is_empty()
        && !source.trim().is_empty()
    {
        return Err(PipelineError::Parse {
            path,
            reason: parsed
                .diagnostics
                .errors()
                .next()
                .map_or_else(String::new, ToString::to_string),
        });
    }
    Ok(ExperimentalStats {
        top_level_statement_count: parsed.program.body.len() as u64,
        size: u64::from(parsed.program.span().end),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_vue_file_is_read_as_its_scripts_at_their_own_offsets() {
        let source = "<template>\n  <div>é</div>\n</template>\n<script lang=\"ts\">\nimport a from './a';\n</script>\n<script setup>\nimport b from './b';\n</script>\n<scripts>x</scripts>\n";
        let (script, lang) = vue_script(source);
        assert_eq!(lang.as_deref(), Some("ts"));
        assert_eq!(script.len(), source.len());
        assert_eq!(script.matches('\n').count(), source.matches('\n').count());
        let at = source.find("import a").unwrap_or_default();
        assert_eq!(&script[at..at + 20], "import a from './a';");
        assert!(script.contains("import b from './b';"));
        assert!(!script.contains("template") && !script.contains("<script"));
        assert_eq!(vue_script("<script>x").0, "        x");
        assert_eq!(
            attribute("setup lang='tsx'", "lang").as_deref(),
            Some("tsx")
        );
        assert_eq!(attribute("lang=js setup", "lang").as_deref(), Some("js"));
        assert_eq!(attribute("setup", "lang"), None);
        let check = |lang: Option<&str>, flavour: Flavour| syntax_for("a.vue", lang, flavour);
        assert!(check(Some("tsx"), Flavour::Acorn).is_jsx());
        assert!(check(Some("ts"), Flavour::Acorn).is_typescript());
        assert!(check(None, Flavour::Tsc).is_typescript());
        assert!(check(Some("jsx"), Flavour::Acorn).is_jsx());
        assert!(!check(None, Flavour::Acorn).is_typescript());
        assert!(syntax_for("a.ts", None, Flavour::Acorn).is_typescript());
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
        let spans = elided_imports(source, SourceType::ts());
        let elided: Vec<&str> = spans
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
