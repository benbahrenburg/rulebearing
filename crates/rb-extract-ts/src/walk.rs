//! Every dependency form in a JavaScript or TypeScript source, found the way dependency-cruiser
//! finds it.
//!
//! - Plan: [Wave 0, Step 8](../../../docs/plans/pending/0000-wave-0-spike.md#step-8-spike-a-rb-extract-ts-0c)
//!   (`walk.rs`)
//! - Decision: [ADR-0012](../../../docs/adr/0012-oxc-for-typescript.md) (one parser, `oxc_parser`)
//! - Source: [design § The five stages](../../../docs/artifacts/design.md#the-five-stages), stage 2;
//!   [coverage § Dependency types and module systems](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#dependency-types-and-module-systems)
//! - Specification: dependency-cruiser 18.2.0's `src/extract/{tsc,swc,acorn}`, as recorded by
//!   conformance gate 1 layer 1
//!
//! dependency-cruiser has three extractors, one per parser it can use, and they disagree in small,
//! documented ways: the tsc extractor marks `import type` as `type-only` and reads JSDoc and
//! triple-slash directives, the swc extractor does neither, and the acorn extractor runs one pass
//! per module system and reports calls after their arguments. The `parser` option picks which one
//! a user gets, so each is reproduced here as a [`Flavour`] over the same oxc tree rather than
//! approximated by one walker that matches none of them.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, ArrayExpressionElement, BindingPattern, CallExpression, Expression,
    ImportDeclaration, ImportDeclarationSpecifier, ImportOrExportKind, Program, Statement,
    TSModuleReference, TSType,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{GetSpan, SourceType, Span};
use rb_model::{DependencyType, ModuleSystem};

use crate::jsdoc;

/// Which of dependency-cruiser's extractors to reproduce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavour {
    /// The TypeScript compiler's extractor (`parser: tsc`, or `tsPreCompilationDeps`).
    Tsc,
    /// swc's extractor (`parser: swc`).
    Swc,
    /// acorn's extractor, the default for JavaScript.
    Acorn,
}

/// What to look for.
#[derive(Debug, Clone, Default)]
pub struct WalkOptions {
    /// The module systems to extract; the acorn flavour runs one pass per system.
    pub module_systems: Vec<ModuleSystem>,
    /// Names other than `require` that load a module (`need`, `window.require`).
    pub exotic_require_strings: Vec<String>,
    /// Whether JSDoc imports count (tsc only).
    pub detect_jsdoc_imports: bool,
    /// Whether `process.getBuiltinModule` calls count.
    pub detect_process_builtin_module_calls: bool,
}

/// One dependency form found in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The specifier as written.
    pub module: String,
    /// The module system of the form.
    pub module_system: ModuleSystem,
    /// `import()`.
    pub dynamic: bool,
    /// Loaded through an exotic require name.
    pub exotically_required: bool,
    /// The exotic name.
    pub exotic_require: Option<String>,
    /// The form's dependency types.
    pub dependency_types: Vec<DependencyType>,
    /// Where it is, for line and column.
    pub span: Span,
}

impl Found {
    fn new(
        module: &str,
        module_system: ModuleSystem,
        types: &[DependencyType],
        span: Span,
    ) -> Self {
        Self {
            module: module.to_owned(),
            module_system,
            dynamic: false,
            exotically_required: false,
            exotic_require: None,
            dependency_types: types.to_vec(),
            span,
        }
    }

    fn exotic(mut self, name: &str) -> Self {
        self.exotically_required = true;
        self.exotic_require = Some(name.to_owned());
        self
    }

    fn dynamic(mut self) -> Self {
        self.dynamic = true;
        self
    }
}

/// The source could not be parsed at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("could not parse: {0}")]
pub struct ParseError(pub String);

use DependencyType as D;

/// Parses `source` and returns its dependency forms in the order `flavour` reports them.
///
/// # Errors
/// Reserved for a source the parser cannot read at all; oxc recovers from syntax errors, so today
/// every source yields a (possibly partial) result.
pub fn walk_source(
    source: &str,
    source_type: SourceType,
    flavour: Flavour,
    options: &WalkOptions,
) -> Result<Vec<Found>, ParseError> {
    let plain = plain_template_imports(source);
    let loose: String;
    let allocator = Allocator::default();
    let mut source = plain.as_str();
    let mut parsed = parse(&allocator, source, source_type);
    // Parsed once; a source whose program oxc could not recover at all is loosened and parsed
    // again.
    if parsed.program.body.is_empty() && parsed.diagnostics.errors().next().is_some() {
        loose = loosen(source, source_type);
        source = loose.as_str();
        parsed = parse(&allocator, source, source_type);
    }
    let errors = |p: &oxc_parser::ParserReturn<'_>| p.diagnostics.errors().count();
    if errors(&parsed) > 0 && source_type.is_module() && !source_type.is_typescript() {
        // acorn retries a module that fails to parse as a script.
        let script = parse(&allocator, source, source_type.with_script(true));
        if errors(&script) < errors(&parsed) {
            parsed = script;
        }
    }
    // Like tsc's parser and acorn's loose fallback, carry on with whatever oxc recovered: a
    // syntax error loses the forms after it, never the whole file.
    let program = &parsed.program;
    Ok(match flavour {
        Flavour::Tsc => tsc(program, source, options),
        Flavour::Swc => swc(program, options),
        Flavour::Acorn => acorn(program, options),
    })
}

/// oxc with the options every flavour shares.
fn parse<'a>(
    allocator: &'a Allocator,
    source: &'a str,
    source_type: SourceType,
) -> oxc_parser::ParserReturn<'a> {
    Parser::new(allocator, source, source_type)
        .with_options(ParseOptions {
            allow_return_outside_function: true,
            ..ParseOptions::default()
        })
        .parse()
}

/// Rewrites `` import(`x`) `` without placeholders as `import('x')`, byte for byte the same length.
/// TypeScript accepts a template literal in an import type and oxc does not; the specifier is the
/// same either way, and a runtime `import()` is unaffected.
fn plain_template_imports(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(at) = rest.find("import(") {
        let (before, after) = rest.split_at(at + "import(".len());
        out.push_str(before);
        let spaces = after.len() - after.trim_start().len();
        let body = &after[spaces..];
        if let Some(inner) = body.strip_prefix('`')
            && let Some(end) = inner.find('`')
            && !inner[..end].contains(['$', '\'', '\n'])
        {
            out.push_str(&after[..spaces]);
            out.push('\'');
            out.push_str(&inner[..end]);
            out.push('\'');
            rest = &inner[end + 1..];
            continue;
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// When oxc recovers nothing from a source, blanks the offending line, keeping every byte offset,
/// and tries again, until a program comes back or nothing more can be blanked. This stands in for
/// the recovering parsers upstream falls back to (acorn-loose, tsc's own recovery): `export default const x = 1` loses that line, not the
/// imports around it.
fn loosen(source: &str, source_type: SourceType) -> String {
    let mut text = source.to_owned();
    for _ in 0..32 {
        let allocator = Allocator::default();
        let parsed = parse(&allocator, &text, source_type);
        // Only a fatal error loses the program; oxc recovers from the rest by itself, as tsc does.
        if !parsed.program.body.is_empty() {
            break;
        }
        let Some(offset) = parsed
            .diagnostics
            .errors()
            .find_map(|e| e.labels.first().map(oxc_diagnostics::LabeledSpan::offset))
        else {
            break;
        };
        let offset = (offset as usize).min(text.len());
        let start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
        let end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
        if text[start..end].trim().is_empty() {
            break;
        }
        let blank: String = text[start..end]
            .chars()
            .map(|c| {
                if c.is_whitespace() { c } else { ' ' }
                    .to_string()
                    .repeat(c.len_utf8())
            })
            .collect();
        text.replace_range(start..end, &blank);
    }
    text
}

/// The first argument as dependency-cruiser reads it: a string literal, or a template literal
/// without placeholders. `template_ok` is false for the tsc flavour's `import()` in some positions.
fn string_argument<'a>(arguments: &'a [Argument<'a>]) -> Option<&'a str> {
    match arguments.first()? {
        Argument::StringLiteral(s) => Some(s.value.as_str()),
        Argument::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => t.quasis
            [0]
        .value
        .cooked
        .as_ref()
        .map(oxc_ast::ast::Str::as_str),
        _ => None,
    }
}

fn expression_string<'a>(expression: &'a Expression<'a>) -> Option<&'a str> {
    match expression {
        Expression::StringLiteral(s) => Some(s.value.as_str()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => t
            .quasis[0]
            .value
            .cooked
            .as_ref()
            .map(oxc_ast::ast::Str::as_str),
        _ => None,
    }
}

/// The callee as dotted names, when it is `a`, `a.b` or `a.b.c` made of identifiers.
fn callee_path(callee: &Expression<'_>) -> Option<Vec<String>> {
    match callee {
        Expression::Identifier(id) => Some(vec![id.name.to_string()]),
        Expression::StaticMemberExpression(member) => {
            let mut path = callee_path(&member.object)?;
            path.push(member.property.name.to_string());
            Some(path)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// tsc

/// Upstream's `isTypeOnlyImport`: `import type ...`, or a clause whose named bindings
/// (`{ ... }`) are all type-only. The default binding is not looked at, so
/// `import D, { type T } from "x"` counts as type-only, and so does an empty `{}`, because
/// `every` holds on no elements; a namespace binding (`* as ns`) has no elements to test and
/// does not count.
fn is_type_only_import(import: &ImportDeclaration<'_>, source: &str) -> bool {
    if import.import_kind == ImportOrExportKind::Type {
        return true;
    }
    let Some(specifiers) = &import.specifiers else {
        return false;
    };
    let named: Vec<_> = specifiers
        .iter()
        .filter_map(|s| match s {
            ImportDeclarationSpecifier::ImportSpecifier(s) => Some(s),
            _ => None,
        })
        .collect();
    // oxc keeps no node for an empty `{}`, so its braces are looked for in the clause's text.
    let clause = source
        .get(import.span.start as usize..import.source.span.start as usize)
        .unwrap_or_default();
    let has_named_bindings = !named.is_empty() || clause.contains('{');
    has_named_bindings
        && named
            .iter()
            .all(|s| s.import_kind == ImportOrExportKind::Type)
}

fn tsc(program: &Program<'_>, source: &str, options: &WalkOptions) -> Vec<Found> {
    let mut found = Vec::new();
    let mut exports = Vec::new();
    let mut equals = Vec::new();
    for statement in &program.body {
        match statement {
            Statement::ImportDeclaration(import) => {
                let types: &[D] = if is_type_only_import(import, source) {
                    &[D::TypeOnly, D::Import]
                } else {
                    &[D::Import]
                };
                found.push(Found::new(
                    &import.source.value,
                    ModuleSystem::Es6,
                    types,
                    import.span,
                ));
            }
            Statement::ExportAllDeclaration(export) => {
                let types: &[D] = if export.export_kind == ImportOrExportKind::Type {
                    &[D::TypeOnly, D::Export]
                } else {
                    &[D::Export]
                };
                exports.push(Found::new(
                    &export.source.value,
                    ModuleSystem::Es6,
                    types,
                    export.span,
                ));
            }
            Statement::ExportFromDeclaration(export) => {
                let type_only = export.export_kind == ImportOrExportKind::Type
                    || (!export.specifiers.is_empty()
                        && export
                            .specifiers
                            .iter()
                            .all(|s| s.export_kind == ImportOrExportKind::Type));
                let types: &[D] = if type_only {
                    &[D::TypeOnly, D::Export]
                } else {
                    &[D::Export]
                };
                exports.push(Found::new(
                    &export.source.value,
                    ModuleSystem::Es6,
                    types,
                    export.span,
                ));
            }
            Statement::TSImportEqualsDeclaration(equal) => {
                if let TSModuleReference::ExternalModuleReference(reference) =
                    &equal.module_reference
                {
                    equals.push(Found::new(
                        &reference.expression.value,
                        ModuleSystem::Cjs,
                        &[D::ImportEquals],
                        equal.span,
                    ));
                }
            }
            _ => {}
        }
    }
    found.extend(exports);
    found.extend(equals);
    found.extend(triple_slash(program, source));

    let mut nested = TscNested {
        options,
        source,
        found: Vec::new(),
    };
    nested.visit_program(program);
    let mut nested = nested.found;
    if options.detect_jsdoc_imports {
        for comment in program.comments.iter().filter(|c| c.is_jsdoc()) {
            let text = comment.content_span().source_text(source);
            for (module, types) in jsdoc::imports(text) {
                nested.push(Found::new(&module, ModuleSystem::Es6, types, comment.span));
            }
        }
        // A JSDoc comment is visited with the node it documents, which it precedes.
        nested.sort_by_key(|f| f.span.start);
    }
    found.extend(nested);
    found.retain(|f| {
        options.module_systems.is_empty() || options.module_systems.contains(&f.module_system)
    });
    found
}

/// `/// <reference path|types="..." />` and `/// <amd-dependency path="..." />` at the top of the
/// file, in tsc's order: file references, then type references, then AMD dependencies.
fn triple_slash(program: &Program<'_>, source: &str) -> Vec<Found> {
    let first_statement = program.body.first().map_or(u32::MAX, |s| s.span().start);
    let mut paths = Vec::new();
    let mut types = Vec::new();
    let mut amd = Vec::new();
    for comment in program
        .comments
        .iter()
        .filter(|c| c.is_line() && c.span.start < first_statement)
    {
        let text = comment.content_span().source_text(source);
        let Some(directive) = text.strip_prefix('/') else {
            continue;
        };
        let directive = directive.trim();
        let attribute = |name: &str| -> Option<String> {
            let at = directive.find(&format!("{name}="))? + name.len() + 1;
            let quote = directive[at..].chars().next()?;
            let rest = &directive[at + 1..];
            rest.find(quote).map(|end| rest[..end].to_owned())
        };
        if directive.starts_with("<reference") {
            if let Some(path) = attribute("path") {
                paths.push(Found::new(
                    &path,
                    ModuleSystem::Tsd,
                    &[D::TripleSlashDirective, D::TripleSlashFileReference],
                    comment.span,
                ));
            } else if let Some(path) = attribute("types") {
                types.push(Found::new(
                    &path,
                    ModuleSystem::Tsd,
                    &[D::TripleSlashDirective, D::TripleSlashTypeReference],
                    comment.span,
                ));
            }
        } else if directive.starts_with("<amd-dependency")
            && let Some(path) = attribute("path")
        {
            amd.push(Found::new(
                &path,
                ModuleSystem::Tsd,
                &[D::TripleSlashDirective, D::TripleSlashAmdDependency],
                comment.span,
            ));
        }
    }
    paths.extend(types);
    paths.extend(amd);
    paths
}

struct TscNested<'o> {
    options: &'o WalkOptions,
    source: &'o str,
    found: Vec<Found>,
}

/// The module of an import type. A template literal that reached here unrewritten (one with a
/// placeholder) leaves oxc's value empty and names nothing.
fn import_type_module(import: &oxc_ast::ast::TSImportType<'_>, source: &str) -> Option<String> {
    if !import.source.value.is_empty() {
        return Some(import.source.value.to_string());
    }
    let raw = import.source.span.source_text(source);
    let inner = raw.strip_prefix('`')?.strip_suffix('`')?;
    (!inner.is_empty() && !inner.contains("${")).then(|| inner.to_owned())
}

impl<'a> Visit<'a> for TscNested<'_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Some(module) = string_argument(&call.arguments) {
            let path = callee_path(&call.callee);
            let path: Vec<&str> = path.iter().flatten().map(String::as_str).collect();
            if path == ["require"] {
                self.found.push(Found::new(
                    module,
                    ModuleSystem::Cjs,
                    &[D::Require],
                    call.span,
                ));
            } else if let Some(name) = self.options.exotic_require_strings.iter().find(|name| {
                // tsc compares the first two segments of a dotted name only.
                let parts: Vec<&str> = name.split('.').collect();
                match parts.as_slice() {
                    [single] => path == [*single],
                    [object, property, ..] => path == [*object, *property],
                    [] => false,
                }
            }) {
                self.found.push(
                    Found::new(module, ModuleSystem::Cjs, &[D::ExoticRequire], call.span)
                        .exotic(name),
                );
            } else if self.options.detect_process_builtin_module_calls
                && (path == ["process", "getBuiltinModule"]
                    || path == ["globalThis", "process", "getBuiltinModule"])
            {
                self.found.push(Found::new(
                    module,
                    ModuleSystem::Cjs,
                    &[D::ProcessGetBuiltinModule],
                    call.span,
                ));
            }
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_import_expression(&mut self, import: &oxc_ast::ast::ImportExpression<'a>) {
        if let Some(module) = expression_string(&import.source) {
            self.found.push(
                Found::new(module, ModuleSystem::Es6, &[D::DynamicImport], import.span).dynamic(),
            );
        }
        walk::walk_import_expression(self, import);
    }

    fn visit_ts_import_type(&mut self, import: &oxc_ast::ast::TSImportType<'a>) {
        if let Some(module) = import_type_module(import, self.source) {
            self.found.push(Found::new(
                &module,
                ModuleSystem::Es6,
                &[D::TypeImport],
                import.span,
            ));
        }
        walk::walk_ts_import_type(self, import);
    }
}

// ---------------------------------------------------------------------------------------------
// swc

fn swc(program: &Program<'_>, options: &WalkOptions) -> Vec<Found> {
    let mut visitor = SwcVisitor {
        options,
        found: Vec::new(),
    };
    visitor.visit_program(program);
    let mut found = visitor.found;
    found.retain(|f| {
        options.module_systems.is_empty() || options.module_systems.contains(&f.module_system)
    });
    found
}

struct SwcVisitor<'o> {
    options: &'o WalkOptions,
    found: Vec<Found>,
}

impl<'a> Visit<'a> for SwcVisitor<'_> {
    fn visit_import_declaration(&mut self, import: &ImportDeclaration<'a>) {
        self.found.push(Found::new(
            &import.source.value,
            ModuleSystem::Es6,
            &[D::Import],
            import.span,
        ));
        walk::walk_import_declaration(self, import);
    }

    fn visit_export_all_declaration(&mut self, export: &oxc_ast::ast::ExportAllDeclaration<'a>) {
        self.found.push(Found::new(
            &export.source.value,
            ModuleSystem::Es6,
            &[D::Export],
            export.span,
        ));
        walk::walk_export_all_declaration(self, export);
    }

    fn visit_export_from_declaration(&mut self, export: &oxc_ast::ast::ExportFromDeclaration<'a>) {
        self.found.push(Found::new(
            &export.source.value,
            ModuleSystem::Es6,
            &[D::Export],
            export.span,
        ));
        walk::walk_export_from_declaration(self, export);
    }

    fn visit_ts_import_equals_declaration(
        &mut self,
        equal: &oxc_ast::ast::TSImportEqualsDeclaration<'a>,
    ) {
        if let TSModuleReference::ExternalModuleReference(reference) = &equal.module_reference {
            self.found.push(Found::new(
                &reference.expression.value,
                ModuleSystem::Cjs,
                &[D::ImportEquals],
                equal.span,
            ));
        }
        walk::walk_ts_import_equals_declaration(self, equal);
    }

    fn visit_import_expression(&mut self, import: &oxc_ast::ast::ImportExpression<'a>) {
        if let Some(module) = expression_string(&import.source) {
            self.found.push(
                Found::new(module, ModuleSystem::Es6, &[D::DynamicImport], import.span).dynamic(),
            );
        }
        walk::walk_import_expression(self, import);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Some(module) = string_argument(&call.arguments) {
            match &call.callee {
                Expression::Identifier(id) => {
                    let name = id.name.as_str();
                    if name == "require" {
                        self.found.push(Found::new(
                            module,
                            ModuleSystem::Cjs,
                            &[D::Require],
                            call.span,
                        ));
                    } else if self
                        .options
                        .exotic_require_strings
                        .iter()
                        .any(|s| !s.contains('.') && s == name)
                    {
                        self.found.push(
                            Found::new(module, ModuleSystem::Cjs, &[D::ExoticRequire], call.span)
                                .exotic(name),
                        );
                    }
                }
                Expression::StaticMemberExpression(member) => {
                    if let Expression::Identifier(object) = &member.object {
                        for name in self
                            .options
                            .exotic_require_strings
                            .iter()
                            .filter(|s| s.contains('.'))
                        {
                            let mut parts = name.split('.');
                            if parts.next() == Some(object.name.as_str())
                                && parts.next() == Some(member.property.name.as_str())
                            {
                                self.found.push(
                                    Found::new(
                                        module,
                                        ModuleSystem::Cjs,
                                        &[D::ExoticRequire],
                                        call.span,
                                    )
                                    .exotic(name),
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_ts_type_annotation(&mut self, annotation: &oxc_ast::ast::TSTypeAnnotation<'a>) {
        // swc reads an import type only when it is the annotation itself, and walks no deeper
        // into types.
        if let TSType::TSImportType(import) = &annotation.type_annotation {
            self.found.push(Found::new(
                &import.source.value,
                ModuleSystem::Es6,
                &[D::TypeImport],
                import.span,
            ));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// acorn

fn acorn(program: &Program<'_>, options: &WalkOptions) -> Vec<Found> {
    let systems = &options.module_systems;
    let mut found = Vec::new();
    if systems.contains(&ModuleSystem::Cjs) {
        let mut cjs = AcornCjs::new(options, ModuleSystem::Cjs, true);
        cjs.visit_program(program);
        found.extend(cjs.found);
    }
    if systems.contains(&ModuleSystem::Es6) {
        let mut es6 = AcornEs6 {
            loose: program.source_type.is_jsx() && beyond_es2020(program),
            found: Vec::new(),
        };
        es6.visit_program(program);
        found.extend(es6.found);
    }
    if systems.contains(&ModuleSystem::Amd) {
        let mut amd = AcornAmd {
            options,
            found: Vec::new(),
        };
        amd.visit_program(program);
        found.extend(amd.found);
    }
    found
}

/// acorn's CommonJS pass: every call to `require` or an exotic name, reported after its
/// arguments (acorn-walk's `simple` visits children first).
struct AcornCjs<'o> {
    options: &'o WalkOptions,
    module_system: ModuleSystem,
    detect_builtin: bool,
    found: Vec<Found>,
}

impl<'o> AcornCjs<'o> {
    fn new(options: &'o WalkOptions, module_system: ModuleSystem, detect_builtin: bool) -> Self {
        Self {
            options,
            module_system,
            detect_builtin: detect_builtin && options.detect_process_builtin_module_calls,
            found: Vec::new(),
        }
    }
}

impl<'a> Visit<'a> for AcornCjs<'_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        walk::walk_call_expression(self, call);
        let Some(path) = callee_path(&call.callee) else {
            return;
        };
        let path: Vec<&str> = path.iter().map(String::as_str).collect();
        let mut names: Vec<&str> = vec!["require"];
        if self.detect_builtin {
            names.extend([
                "process.getBuiltinModule",
                "globalThis.process.getBuiltinModule",
            ]);
        }
        names.extend(
            self.options
                .exotic_require_strings
                .iter()
                .map(String::as_str),
        );
        for name in names {
            let parts: Vec<&str> = name.split('.').collect();
            if parts.len() > 3 || path != parts {
                continue;
            }
            let Some(module) = string_argument(&call.arguments).filter(|m| !m.is_empty()) else {
                continue;
            };
            let amd = self.module_system == ModuleSystem::Amd;
            let dependency = match name {
                "require" => Found::new(
                    module,
                    self.module_system,
                    &[if amd { D::AmdRequire } else { D::Require }],
                    call.span,
                ),
                "process.getBuiltinModule" | "globalThis.process.getBuiltinModule" => Found::new(
                    module,
                    self.module_system,
                    &[D::ProcessGetBuiltinModule],
                    call.span,
                ),
                exotic => Found::new(
                    module,
                    self.module_system,
                    &[if amd {
                        D::AmdExoticRequire
                    } else {
                        D::ExoticRequire
                    }],
                    call.span,
                )
                .exotic(exotic),
            };
            self.found.push(dependency);
        }
    }
}

/// Whether a program uses syntax newer than ECMAScript 2020, the `ecmaVersion: 11` upstream's
/// acorn parses with. Such a source fails acorn's strict parse and falls back to acorn-loose,
/// which has no JSX support (see `AcornEs6::loose`).
fn beyond_es2020(program: &Program<'_>) -> bool {
    let mut detector = BeyondEs2020(program.hashbang.is_some());
    if !detector.0 {
        detector.visit_program(program);
    }
    detector.0
}

/// Finds class fields, private names, static blocks, accessors, logical assignment, numeric
/// separators and import attributes: the ECMAScript 2021 to 2025 syntax acorn 11 rejects.
struct BeyondEs2020(bool);

impl<'a> Visit<'a> for BeyondEs2020 {
    fn visit_property_definition(&mut self, _: &oxc_ast::ast::PropertyDefinition<'a>) {
        self.0 = true;
    }

    fn visit_private_identifier(&mut self, _: &oxc_ast::ast::PrivateIdentifier<'a>) {
        self.0 = true;
    }

    fn visit_static_block(&mut self, _: &oxc_ast::ast::StaticBlock<'a>) {
        self.0 = true;
    }

    fn visit_accessor_property(&mut self, _: &oxc_ast::ast::AccessorProperty<'a>) {
        self.0 = true;
    }

    fn visit_with_clause(&mut self, _: &oxc_ast::ast::WithClause<'a>) {
        self.0 = true;
    }

    fn visit_assignment_expression(&mut self, it: &oxc_ast::ast::AssignmentExpression<'a>) {
        if it.operator.is_logical() {
            self.0 = true;
        } else if !self.0 {
            walk::walk_assignment_expression(self, it);
        }
    }

    fn visit_numeric_literal(&mut self, it: &oxc_ast::ast::NumericLiteral<'a>) {
        if it.raw.is_some_and(|raw| raw.contains('_')) {
            self.0 = true;
        }
    }
}

/// The offsets of `import` as a whole word in `text`, not followed by `(` or `.`: where
/// acorn-loose, reading JSX text as script, starts an import declaration.
fn loose_import_keywords(text: &str) -> Vec<usize> {
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    text.match_indices("import")
        .filter(|(at, keyword)| {
            let rest = &text[at + keyword.len()..];
            !text[..*at].chars().next_back().is_some_and(word)
                && !rest.chars().next().is_some_and(word)
                && !matches!(rest.trim_start().chars().next(), Some('(' | '.'))
        })
        .map(|(at, _)| at)
        .collect()
}

/// acorn's ES module pass: import and re-export declarations and `import()`.
struct AcornEs6 {
    /// Whether upstream's strict acorn parse would fail, so acorn-loose reads the file: JSX text
    /// is then script, and every `import` word in it starts an import declaration whose source
    /// is acorn-loose's placeholder, `✖`. Upstream's `extract-es6-deps` spec records this as a
    /// known limitation ("does a.t.m. NOT handle certain ways of jsx notation correctly").
    loose: bool,
    found: Vec<Found>,
}

/// acorn-loose's placeholder for a missing string.
const LOOSE_PLACEHOLDER: &str = "\u{2716}";

impl<'a> Visit<'a> for AcornEs6 {
    fn visit_jsx_text(&mut self, text: &oxc_ast::ast::JSXText<'a>) {
        if !self.loose {
            return;
        }
        for at in loose_import_keywords(&text.value) {
            let start = text
                .span
                .start
                .saturating_add(u32::try_from(at).unwrap_or(u32::MAX));
            self.found.push(Found::new(
                LOOSE_PLACEHOLDER,
                ModuleSystem::Es6,
                &[D::Import],
                Span::new(start, start.saturating_add(6)),
            ));
        }
    }

    fn visit_import_declaration(&mut self, import: &ImportDeclaration<'a>) {
        walk::walk_import_declaration(self, import);
        if !import.source.value.is_empty() {
            self.found.push(Found::new(
                &import.source.value,
                ModuleSystem::Es6,
                &[D::Import],
                import.span,
            ));
        }
    }

    fn visit_export_all_declaration(&mut self, export: &oxc_ast::ast::ExportAllDeclaration<'a>) {
        walk::walk_export_all_declaration(self, export);
        if !export.source.value.is_empty() {
            self.found.push(Found::new(
                &export.source.value,
                ModuleSystem::Es6,
                &[D::Export],
                export.span,
            ));
        }
    }

    fn visit_export_from_declaration(&mut self, export: &oxc_ast::ast::ExportFromDeclaration<'a>) {
        walk::walk_export_from_declaration(self, export);
        if !export.source.value.is_empty() {
            self.found.push(Found::new(
                &export.source.value,
                ModuleSystem::Es6,
                &[D::Export],
                export.span,
            ));
        }
    }

    fn visit_import_expression(&mut self, import: &oxc_ast::ast::ImportExpression<'a>) {
        walk::walk_import_expression(self, import);
        if let Some(module) = expression_string(&import.source) {
            self.found.push(
                Found::new(module, ModuleSystem::Es6, &[D::DynamicImport], import.span).dynamic(),
            );
        }
    }
}

/// acorn's AMD pass: `define([...])` and `require([...])` arrays, and the CommonJS wrapper
/// `define(function (require) { ... })`.
struct AcornAmd<'o> {
    options: &'o WalkOptions,
    found: Vec<Found>,
}

impl<'a> Visit<'a> for AcornAmd<'_> {
    fn visit_expression_statement(&mut self, statement: &oxc_ast::ast::ExpressionStatement<'a>) {
        walk::walk_expression_statement(self, statement);
        let Expression::CallExpression(call) = &statement.expression else {
            return;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        let name = callee.name.as_str();
        if name != "define" && name != "require" {
            return;
        }
        for argument in &call.arguments {
            if let Argument::ArrayExpression(array) = argument {
                for element in &array.elements {
                    if let ArrayExpressionElement::StringLiteral(s) = element
                        && !s.value.is_empty()
                    {
                        self.found.push(Found::new(
                            &s.value,
                            ModuleSystem::Amd,
                            &[D::AmdDefine],
                            s.span,
                        ));
                    }
                }
            }
        }
        if name != "define" {
            return;
        }
        for argument in &call.arguments {
            let Argument::FunctionExpression(function) = argument else {
                continue;
            };
            let takes_require = function.params.items.iter().any(|parameter| {
                matches!(&parameter.pattern, BindingPattern::BindingIdentifier(id)
                    if id.name == "require" || self.options.exotic_require_strings.iter().any(|s| s == id.name.as_str()))
            });
            if let (true, Some(body)) = (takes_require, &function.body) {
                let mut cjs = AcornCjs::new(self.options, ModuleSystem::Amd, false);
                cjs.visit_function_body(body);
                self.found.extend(cjs.found);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsc_type_only_imports_look_at_named_bindings_only() {
        let options = WalkOptions::default();
        let type_only = |source: &str| {
            walk_source(source, SourceType::ts(), Flavour::Tsc, &options)
                .ok()
                .and_then(|found| found.into_iter().next())
                .map(|f| f.dependency_types.contains(&D::TypeOnly))
        };
        // As upstream's isTypeOnlyImport: the default binding is not considered.
        assert_eq!(type_only("import D, { type T } from 'x';"), Some(true));
        assert_eq!(type_only("import { type T, type U } from 'x';"), Some(true));
        assert_eq!(type_only("import type D from 'x';"), Some(true));
        assert_eq!(type_only("import {} from 'x';"), Some(true));
        assert_eq!(type_only("import D, {} from 'x';"), Some(true));
        assert_eq!(type_only("import D, { type T, U } from 'x';"), Some(false));
        assert_eq!(type_only("import D from 'x';"), Some(false));
        assert_eq!(type_only("import * as N from 'x';"), Some(false));
        assert_eq!(type_only("import 'x';"), Some(false));
    }

    #[test]
    fn template_imports_become_plain_strings_of_the_same_length() {
        let source = "const t: import(`./types`).T; import(`x/${y}`); import( `z` )";
        let plain = plain_template_imports(source);
        assert_eq!(
            plain,
            "const t: import('./types').T; import(`x/${y}`); import( 'z' )"
        );
        assert_eq!(plain.len(), source.len());
    }

    #[test]
    fn import_words_in_jsx_text_count_only_when_acorn_would_go_loose() {
        let options = WalkOptions {
            module_systems: vec![ModuleSystem::Es6],
            ..WalkOptions::default()
        };
        let modules = |source: &str| {
            walk_source(source, SourceType::jsx(), Flavour::Acorn, &options)
                .map(|found| found.into_iter().map(|f| f.module).collect::<Vec<_>>())
                .ok()
        };
        let fields = "import R from 'r';\nclass C { x = () => <>an import here</>; }";
        assert_eq!(
            modules(fields),
            Some(vec!["r".to_owned(), LOOSE_PLACEHOLDER.to_owned()])
        );
        let es2020 = "import R from 'r';\nconst x = () => <>an import here</>;";
        assert_eq!(modules(es2020), Some(vec!["r".to_owned()]));
        for newer in [
            "#!/usr/bin/env node\nconst x = <>import</>;",
            "a ||= <>import</>;",
            "const n = 1_000; const x = <>import</>;",
            "class A { static { } } const x = <>import</>;",
            "class A { #p() {} } const x = <>import</>;",
            "import j from './j.json' with { type: 'json' }; const x = <>import</>;",
            "class A { accessor a = 1 } const x = <>import</>;",
        ] {
            assert!(
                modules(newer).is_some_and(|m| m.contains(&LOOSE_PLACEHOLDER.to_owned())),
                "{newer}"
            );
        }
        assert_eq!(
            loose_import_keywords("import imports reimport import( import.x"),
            [0]
        );
        assert_eq!(loose_import_keywords("an import"), [3]);
    }

    #[test]
    fn loosening_keeps_the_lines_that_parse() {
        let source = "const a = require('./a');\nexport default const b = 1;\n";
        let loose = loosen(source, SourceType::mjs());
        assert_eq!(loose.len(), source.len());
        assert!(loose.starts_with("const a = require('./a');\n"));
        assert!(!loose.contains("export"));
        let found = walk_source(
            source,
            SourceType::mjs(),
            Flavour::Acorn,
            &WalkOptions {
                module_systems: vec![ModuleSystem::Cjs],
                ..WalkOptions::default()
            },
        );
        assert_eq!(found.map(|f| f.len()).ok(), Some(1));
    }
}
