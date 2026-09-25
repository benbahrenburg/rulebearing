//! The parser walk: every import form a Python file can hold, with its position and whether it
//! is type-only or dynamic.
//!
//! - Plan: [Wave 2, Step 4](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#24-step-4-the-python-extractor-2b)
//!   (`parse.rs`)
//! - Decision: [ADR-0013](../../../docs/adr/0013-ruff-parser-for-python.md) (`ruff_python_parser`;
//!   `type-only` for a `TYPE_CHECKING`-guarded import, `dynamic` for a literal
//!   `importlib.import_module` or `__import__`)
//! - Requirement: [FR-EXT-PY-02](../../../docs/prd.md#fr-ext-py-02)
//!
//! | Form | Becomes |
//! | --- | --- |
//! | `import a.b`, `import a.b as c` | one [`ImportSpec`] per name |
//! | `from ..a import b, c` | one [`ImportSpec`] per name, `level` 2, `member` `b` then `c` |
//! | `if TYPE_CHECKING:` (also `typing.TYPE_CHECKING`, an alias such as `t.TYPE_CHECKING`, or `TYPE_CHECKING` imported under another name) | every import in the body `type_only`; `elif` and `else` bodies are run-time code |
//! | `importlib.import_module("x")`, `import_module("x")`, `__import__("x")` with a string literal | `dynamic`; leading dots make it relative |
//! | `__all__ = [...]`, `+=`, annotated, in an `__init__.py` | `from . import name` for each listed name, which the resolver keeps only when a submodule of that name exists |
//!
//! Imports inside functions and classes count, as they do for import-linter. Positions are
//! 1-based lines and 1-based character columns of the imported name.
//!
//! A file whose syntax tree nests deeper than [`MAX_DEPTH`] is not analysed: it is a named
//! warning ([`ParseError::TooDeep`]), and its tree is taken apart with bounded recursion, so no
//! input can overflow a worker's stack and abort the run.

use std::cell::Cell;

use ruff_python_ast::visitor::transformer::{self, Transformer};
use ruff_python_ast::visitor::{
    Visitor, walk_expr, walk_interpolated_string_element, walk_pattern, walk_stmt,
};
use ruff_python_ast::{Expr, ExprCall, InterpolatedStringElement, Mod, ModModule, Pattern, Stmt};
use ruff_text_size::TextRange;

/// Where an [`ImportSpec`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    /// An `import` or `from ... import` statement.
    Statement,
    /// A name listed in an `__init__.py`'s `__all__`.
    All,
    /// A literal `importlib.import_module` or `__import__` call.
    Dynamic,
}

/// One import, before resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSpec {
    /// Leading dots: 0 for an absolute import.
    pub level: u32,
    /// The dotted module after the dots; `None` for `from . import x`.
    pub module: Option<String>,
    /// The name imported from the module (`from a import b` has `b`); `None` for `import a`.
    pub member: Option<String>,
    /// 1-based line.
    pub line: u32,
    /// 1-based column.
    pub column: u32,
    /// Under `if TYPE_CHECKING:`.
    pub type_only: bool,
    /// Where it came from.
    pub origin: Origin,
}

impl ImportSpec {
    /// The module as written, dots included (`..a`, `.`, `os.path`).
    pub fn written(&self) -> String {
        join_written(self.level, self.module.as_deref())
    }

    /// The module with the member appended as written (`..a.b` for `from ..a import b`).
    pub fn written_with_member(&self) -> String {
        match &self.member {
            Some(member) => {
                let base = self.written();
                if base.ends_with('.') {
                    format!("{base}{member}")
                } else {
                    format!("{base}.{member}")
                }
            }
            None => self.written(),
        }
    }
}

/// Dots then the dotted name: `join_written(2, Some("a"))` is `..a`.
pub fn join_written(level: u32, module: Option<&str>) -> String {
    let dots = ".".repeat(usize::try_from(level).unwrap_or(0));
    format!("{dots}{}", module.unwrap_or_default())
}

/// Line starts of a source text, for turning byte offsets into 1-based lines and columns.
#[derive(Debug, Clone)]
pub struct Lines<'s> {
    source: &'s str,
    starts: Vec<usize>,
}

impl<'s> Lines<'s> {
    /// Indexes `source`.
    pub fn new(source: &'s str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            source
                .char_indices()
                .filter(|(_, c)| *c == '\n')
                .map(|(i, _)| i + 1),
        );
        Self { source, starts }
    }

    /// The 1-based line and 1-based character column of a byte offset; an offset past the end
    /// is placed at the end.
    pub fn locate(&self, offset: u32) -> (u32, u32) {
        let offset = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(self.source.len());
        let line = self.starts.partition_point(|&s| s <= offset).max(1);
        let start = self.starts.get(line - 1).copied().unwrap_or(0);
        let column = self
            .source
            .get(start..offset)
            .map_or(0, |text| text.chars().count());
        (to_u32(line), to_u32(column + 1))
    }

    /// The source text between two byte offsets; empty when they do not fall on character
    /// boundaries inside the text.
    pub fn text(&self, start: u32, end: u32) -> &'s str {
        let start = usize::try_from(start).unwrap_or(usize::MAX);
        let end = usize::try_from(end).unwrap_or(usize::MAX);
        self.source.get(start..end).unwrap_or_default()
    }
}

fn to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// How deep statements, expressions, patterns and f-string parts may nest before a file is
/// not analysed. The walks over a syntax tree recurse, so an unbounded depth (a generated
/// `1 + 1 + ...` of 40,000 terms) would overflow a thread's stack and abort the process.
/// Real code stays far below it: Python's own tokenizer stops at 200 nested brackets.
pub const MAX_DEPTH: usize = 1_000;

/// How deep the tree a file that nests past [`MAX_DEPTH`] is taken apart at a time.
const DETACH_DEPTH: usize = 64;

/// Why a file could not be analysed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The parser reported a syntax error.
    #[error("syntax error at {line}:{column}: {message}")]
    Syntax {
        /// 1-based line.
        line: u32,
        /// 1-based column.
        column: u32,
        /// The parser's message.
        message: String,
    },
    /// The file nests deeper than [`MAX_DEPTH`].
    #[error("could not be analysed: statements or expressions nest deeper than {MAX_DEPTH} levels")]
    TooDeep,
}

impl ParseError {
    /// What the user can do about it.
    pub fn fix(&self) -> &'static str {
        match self {
            Self::Syntax { .. } => "Fix the syntax, or exclude the file",
            Self::TooDeep => "Split the nested expression, or exclude the file",
        }
    }
}

/// Parses a module.
///
/// # Errors
/// The first syntax error, with its 1-based position, or [`ParseError::TooDeep`] for a tree
/// deeper than [`MAX_DEPTH`]. Either way the tree is taken apart with bounded recursion, so
/// no input overflows the stack while it is dropped.
pub fn parse(source: &str) -> Result<ModModule, ParseError> {
    let parsed = ruff_python_parser::parse_unchecked(
        source,
        ruff_python_parser::ParseOptions::from(ruff_python_parser::Mode::Module),
    );
    let error = parsed.errors().first().map(|error| {
        let (line, column) = Lines::new(source).locate(error.location.start().into());
        ParseError::Syntax {
            line,
            column,
            message: error.error.to_string(),
        }
    });
    let body = match parsed.into_syntax() {
        Mod::Module(module) => module,
        Mod::Expression(expression) => {
            dismantle(Vec::new(), vec![*expression.body]);
            return Err(error.unwrap_or(ParseError::TooDeep));
        }
    };
    let error = error.or_else(|| too_deep(&body).then_some(ParseError::TooDeep));
    match error {
        Some(error) => {
            dismantle(body.body.into_iter().collect(), Vec::new());
            Err(error)
        }
        None => Ok(body),
    }
}

/// Whether a module nests deeper than [`MAX_DEPTH`]; the walk stops descending there.
fn too_deep(module: &ModModule) -> bool {
    let mut depth = Depth::default();
    for stmt in &module.body {
        depth.visit_stmt(stmt);
    }
    depth.exceeded
}

#[derive(Default)]
struct Depth {
    depth: usize,
    exceeded: bool,
}

impl Depth {
    fn enter(&mut self, walk: impl FnOnce(&mut Self)) {
        if self.exceeded {
            return;
        }
        if self.depth >= MAX_DEPTH {
            self.exceeded = true;
            return;
        }
        self.depth += 1;
        walk(self);
        self.depth -= 1;
    }
}

impl<'a> Visitor<'a> for Depth {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        self.enter(|v| walk_stmt(v, stmt));
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        self.enter(|v| walk_expr(v, expr));
    }

    fn visit_pattern(&mut self, pattern: &'a Pattern) {
        self.enter(|v| walk_pattern(v, pattern));
    }

    fn visit_interpolated_string_element(&mut self, element: &'a InterpolatedStringElement) {
        self.enter(|v| walk_interpolated_string_element(v, element));
    }
}

/// Takes the subtrees below [`DETACH_DEPTH`] out of a tree, leaving placeholders, so the rest
/// can be dropped with bounded recursion.
#[derive(Default)]
struct Detach {
    depth: Cell<usize>,
    stmts: Cell<Vec<Stmt>>,
    exprs: Cell<Vec<Expr>>,
    patterns: Cell<Vec<Pattern>>,
    elements: Cell<Vec<InterpolatedStringElement>>,
}

fn push<T>(list: &Cell<Vec<T>>, item: T) {
    let mut items = list.take();
    items.push(item);
    list.set(items);
}

fn pop<T>(list: &Cell<Vec<T>>) -> Option<T> {
    let mut items = list.take();
    let item = items.pop();
    list.set(items);
    item
}

impl Detach {
    /// Walks one level deeper, or returns `false` at [`DETACH_DEPTH`].
    fn deeper(&self, walk: impl FnOnce()) -> bool {
        let depth = self.depth.get();
        if depth >= DETACH_DEPTH {
            return false;
        }
        self.depth.set(depth + 1);
        walk();
        self.depth.set(depth);
        true
    }
}

impl Transformer for Detach {
    fn visit_stmt(&self, stmt: &mut Stmt) {
        if !self.deeper(|| transformer::walk_stmt(self, stmt)) {
            let placeholder = Stmt::Pass(ruff_python_ast::StmtPass {
                node_index: ruff_python_ast::AtomicNodeIndex::default(),
                range: TextRange::default(),
            });
            push(&self.stmts, std::mem::replace(stmt, placeholder));
        }
    }

    fn visit_expr(&self, expr: &mut Expr) {
        if !self.deeper(|| transformer::walk_expr(self, expr)) {
            let placeholder = Expr::NoneLiteral(ruff_python_ast::ExprNoneLiteral::default());
            push(&self.exprs, std::mem::replace(expr, placeholder));
        }
    }

    fn visit_pattern(&self, pattern: &mut Pattern) {
        if !self.deeper(|| transformer::walk_pattern(self, pattern)) {
            let placeholder = Pattern::MatchAs(ruff_python_ast::PatternMatchAs {
                node_index: ruff_python_ast::AtomicNodeIndex::default(),
                range: TextRange::default(),
                pattern: None,
                name: None,
            });
            push(&self.patterns, std::mem::replace(pattern, placeholder));
        }
    }

    fn visit_interpolated_string_element(&self, element: &mut InterpolatedStringElement) {
        if !self.deeper(|| transformer::walk_interpolated_string_element(self, element)) {
            let placeholder = InterpolatedStringElement::Literal(
                ruff_python_ast::InterpolatedStringLiteralElement {
                    range: TextRange::default(),
                    node_index: ruff_python_ast::AtomicNodeIndex::default(),
                    value: Box::default(),
                },
            );
            push(&self.elements, std::mem::replace(element, placeholder));
        }
    }
}

/// Drops statements and expressions of any depth: each piece is walked at most
/// [`DETACH_DEPTH`] deep, its deeper subtrees moved to a work list, and then dropped.
fn dismantle(stmts: Vec<Stmt>, exprs: Vec<Expr>) {
    let detach = Detach::default();
    detach.stmts.set(stmts);
    detach.exprs.set(exprs);
    loop {
        if let Some(mut stmt) = pop(&detach.stmts) {
            detach.visit_stmt(&mut stmt);
        } else if let Some(mut expr) = pop(&detach.exprs) {
            detach.visit_expr(&mut expr);
        } else if let Some(mut pattern) = pop(&detach.patterns) {
            detach.visit_pattern(&mut pattern);
        } else if let Some(mut element) = pop(&detach.elements) {
            detach.visit_interpolated_string_element(&mut element);
        } else {
            break;
        }
    }
}

/// Every import in `module`, in source order. `package_init` is true for an `__init__.py`,
/// whose `__all__` names become imports.
pub fn imports(module: &ModModule, lines: &Lines<'_>, package_init: bool) -> Vec<ImportSpec> {
    let mut aliases = Aliases::default();
    for stmt in &module.body {
        aliases.visit_stmt(stmt);
    }
    let mut walker = Walker {
        lines,
        aliases: &aliases,
        type_only: 0,
        found: Vec::new(),
    };
    for stmt in &module.body {
        walker.visit_stmt(stmt);
    }
    let mut found = walker.found;
    if package_init {
        for stmt in &module.body {
            found.extend(all_names(stmt, lines));
        }
    }
    found
}

/// The local names bound to `typing`, `TYPE_CHECKING`, `importlib` and `import_module`.
#[derive(Debug, Default)]
struct Aliases {
    typing: Vec<String>,
    type_checking: Vec<String>,
    importlib: Vec<String>,
    import_module: Vec<String>,
}

impl Visitor<'_> for Aliases {
    fn visit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Import(import) => {
                for alias in &import.names {
                    let name = alias.name.as_str();
                    let bound = alias.asname.as_ref().map_or(name, |a| a.as_str());
                    match name {
                        "typing" | "typing_extensions" => self.typing.push(bound.to_owned()),
                        "importlib" => self.importlib.push(bound.to_owned()),
                        _ => {}
                    }
                }
            }
            Stmt::ImportFrom(from) if from.level == 0 => {
                let module = from
                    .module
                    .as_ref()
                    .map(ruff_python_ast::Identifier::as_str);
                for alias in &from.names {
                    let bound = alias
                        .asname
                        .as_ref()
                        .unwrap_or(&alias.name)
                        .as_str()
                        .to_owned();
                    match (module, alias.name.as_str()) {
                        (Some("typing" | "typing_extensions"), "TYPE_CHECKING") => {
                            self.type_checking.push(bound);
                        }
                        (Some("importlib"), "import_module") => self.import_module.push(bound),
                        _ => {}
                    }
                }
            }
            _ => walk_stmt(self, stmt),
        }
    }
}

impl Aliases {
    fn is_type_checking(&self, test: &Expr) -> bool {
        match test {
            Expr::Name(name) => {
                let id = name.id.as_str();
                id == "TYPE_CHECKING" || self.type_checking.iter().any(|a| a == id)
            }
            Expr::Attribute(attribute) => {
                attribute.attr.as_str() == "TYPE_CHECKING"
                    && matches!(&*attribute.value, Expr::Name(base)
                        if matches!(base.id.as_str(), "typing" | "typing_extensions")
                            || self.typing.iter().any(|a| a == base.id.as_str()))
            }
            _ => false,
        }
    }

    fn is_dynamic_callee(&self, func: &Expr) -> bool {
        match func {
            Expr::Name(name) => {
                let id = name.id.as_str();
                id == "__import__" || self.import_module.iter().any(|a| a == id)
            }
            Expr::Attribute(attribute) => {
                attribute.attr.as_str() == "import_module"
                    && matches!(&*attribute.value, Expr::Name(base)
                        if base.id.as_str() == "importlib"
                            || self.importlib.iter().any(|a| a == base.id.as_str()))
            }
            _ => false,
        }
    }
}

struct Walker<'l, 's> {
    lines: &'l Lines<'s>,
    aliases: &'l Aliases,
    type_only: usize,
    found: Vec<ImportSpec>,
}

impl Walker<'_, '_> {
    fn push(
        &mut self,
        level: u32,
        module: Option<String>,
        member: Option<String>,
        at: u32,
        origin: Origin,
    ) {
        let (line, column) = self.lines.locate(at);
        self.found.push(ImportSpec {
            level,
            module,
            member,
            line,
            column,
            type_only: self.type_only > 0,
            origin,
        });
    }

    fn dynamic(&mut self, call: &ExprCall) {
        if !self.aliases.is_dynamic_callee(&call.func) {
            return;
        }
        let Some(Expr::StringLiteral(literal)) = call.arguments.args.first() else {
            return;
        };
        let text = literal.value.to_str();
        let dotted = text.trim_start_matches('.');
        let level = to_u32(text.len() - dotted.len());
        if dotted.is_empty() && level == 0 {
            return;
        }
        let module = (!dotted.is_empty()).then(|| dotted.to_owned());
        self.push(
            level,
            module,
            None,
            literal.range.start().into(),
            Origin::Dynamic,
        );
    }
}

impl<'a> Visitor<'a> for Walker<'_, '_> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::Import(import) => {
                for alias in &import.names {
                    let name = alias.name.as_str().to_owned();
                    self.push(
                        0,
                        Some(name),
                        None,
                        alias.range.start().into(),
                        Origin::Statement,
                    );
                }
            }
            Stmt::ImportFrom(from) => {
                let module = from.module.as_ref().map(|m| m.as_str().to_owned());
                for alias in &from.names {
                    let member = alias.name.as_str();
                    let member = (member != "*").then(|| member.to_owned());
                    self.push(
                        from.level,
                        module.clone(),
                        member,
                        alias.range.start().into(),
                        Origin::Statement,
                    );
                }
            }
            Stmt::If(branch) if self.aliases.is_type_checking(&branch.test) => {
                self.visit_expr(&branch.test);
                self.type_only += 1;
                for inner in &branch.body {
                    self.visit_stmt(inner);
                }
                self.type_only -= 1;
                for clause in &branch.elif_else_clauses {
                    if let Some(test) = &clause.test {
                        self.visit_expr(test);
                    }
                    for inner in &clause.body {
                        self.visit_stmt(inner);
                    }
                }
            }
            _ => walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(call) = expr {
            self.dynamic(call);
        }
        walk_expr(self, expr);
    }
}

/// The names a top-level `__all__` statement lists, as `from . import name` imports.
fn all_names(stmt: &Stmt, lines: &Lines<'_>) -> Vec<ImportSpec> {
    let (target, value) = match stmt {
        Stmt::Assign(assign) => match assign.targets.as_slice() {
            [target] => (target, Some(&*assign.value)),
            _ => return Vec::new(),
        },
        Stmt::AnnAssign(assign) => (&*assign.target, assign.value.as_deref()),
        Stmt::AugAssign(assign) => (&*assign.target, Some(&*assign.value)),
        _ => return Vec::new(),
    };
    let is_all = matches!(target, Expr::Name(name) if name.id.as_str() == "__all__");
    let elements = match value {
        Some(Expr::List(list)) if is_all => &list.elts,
        Some(Expr::Tuple(tuple)) if is_all => &tuple.elts,
        _ => return Vec::new(),
    };
    elements
        .iter()
        .filter_map(|element| {
            let Expr::StringLiteral(literal) = element else {
                return None;
            };
            let name = literal.value.to_str();
            if !crate::discover::is_identifier(name) {
                return None;
            }
            let (line, column) = lines.locate(literal.range.start().into());
            Some(ImportSpec {
                level: 1,
                module: None,
                member: Some(name.to_owned()),
                line,
                column,
                type_only: false,
                origin: Origin::All,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specs(source: &str, init: bool) -> Vec<(String, bool, Origin, u32, u32)> {
        let parsed = parse(source);
        assert!(parsed.is_ok(), "{parsed:?}");
        let Ok(module) = parsed else {
            return Vec::new();
        };
        let lines = Lines::new(source);
        imports(&module, &lines, init)
            .into_iter()
            .map(|s| {
                (
                    s.written_with_member(),
                    s.type_only,
                    s.origin,
                    s.line,
                    s.column,
                )
            })
            .collect()
    }

    fn written(source: &str) -> Vec<String> {
        specs(source, false).into_iter().map(|s| s.0).collect()
    }

    #[test]
    fn statements_become_one_spec_per_name() {
        let source =
            "import os, a.b as c\nfrom ..x import y, z\nfrom . import w\nfrom .q import *\n";
        assert_eq!(written(source), ["os", "a.b", "..x.y", "..x.z", ".w", ".q"]);
        let found = specs(source, false);
        assert_eq!((found[1].3, found[1].4), (1, 12));
        assert_eq!((found[3].3, found[3].4), (2, 20));
    }

    #[test]
    fn imports_inside_functions_count() {
        assert_eq!(
            written("def f():\n    import json\n    class C:\n        from x import y\n"),
            ["json", "x.y"]
        );
    }

    #[test]
    fn type_checking_forms_mark_type_only() {
        let source = "\
import typing
import typing as t
from typing import TYPE_CHECKING as TC
if TYPE_CHECKING:
    import a
if typing.TYPE_CHECKING:
    import b
elif x:
    import c
else:
    import d
if t.TYPE_CHECKING:
    import e
if TC:
    import f
if other.TYPE_CHECKING:
    import g
if typing_extensions.TYPE_CHECKING:
    import h
";
        let marked: Vec<(String, bool)> = specs(source, false)
            .into_iter()
            .map(|s| (s.0, s.1))
            .filter(|(w, _)| w.len() == 1)
            .collect();
        let expected: Vec<(String, bool)> = [
            ("a", true),
            ("b", true),
            ("c", false),
            ("d", false),
            ("e", true),
            ("f", true),
            ("g", false),
            ("h", true),
        ]
        .iter()
        .map(|(w, t)| ((*w).to_owned(), *t))
        .collect();
        assert_eq!(marked, expected);
    }

    #[test]
    fn literal_dynamic_imports() {
        let source = "\
import importlib
import importlib as il
from importlib import import_module as im
importlib.import_module('a.b')
il.import_module(\"c\")
im('.d', package=__package__)
__import__('e')
importlib.import_module(name)
other.import_module('f')
__import__('')
";
        let found = specs(source, false);
        let dynamic: Vec<&str> = found
            .iter()
            .filter(|s| s.2 == Origin::Dynamic)
            .map(|s| s.0.as_str())
            .collect();
        assert_eq!(dynamic, ["a.b", "c", ".d", "e"]);
        assert_eq!(
            found.iter().find(|s| s.0 == "a.b").map(|s| (s.3, s.4)),
            Some((4, 25))
        );
    }

    #[test]
    fn all_in_a_package_init_lists_submodules() {
        let source = "\
__all__ = ['a', \"b\"]
__all__ += ('c',)
__all__: list[str] = ['d', 'not-a-name', 1]
other = ['x']
x = y = ['z']
__all__.extend(['w'])
";
        let found: Vec<String> = specs(source, true)
            .into_iter()
            .filter(|s| s.2 == Origin::All)
            .map(|s| s.0)
            .collect();
        assert_eq!(found, [".a", ".b", ".c", ".d"]);
        assert!(specs(source, false).iter().all(|s| s.2 != Origin::All));
    }

    #[test]
    fn written_forms() {
        assert_eq!(join_written(0, Some("a.b")), "a.b");
        assert_eq!(join_written(2, None), "..");
        let spec = ImportSpec {
            level: 1,
            module: None,
            member: Some("x".into()),
            line: 1,
            column: 1,
            type_only: false,
            origin: Origin::Statement,
        };
        assert_eq!(spec.written(), ".");
        assert_eq!(spec.written_with_member(), ".x");
    }

    #[test]
    fn syntax_errors_are_messages_with_a_position() {
        let error = parse("def f(:\n  pass\n")
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(error.starts_with("syntax error at 1:"), "{error}");
        assert!(parse("x = 1\n").is_ok());
    }

    /// Runs `f` on a thread with rayon's default 2 MiB stack, as the extractor's workers are.
    fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(f)
            .ok()?
            .join()
            .ok()
    }

    fn analyse(source: &str) -> Result<usize, ParseError> {
        let module = parse(source)?;
        let lines = Lines::new(source);
        let specs = imports(&module, &lines, true);
        let code = crate::codelayer::elements(
            &module,
            &lines,
            crate::codelayer::Context {
                module: "m",
                package: "",
                file: "m.py",
                project: "m",
            },
        );
        Ok(specs.len() + code.types.len())
    }

    #[test]
    fn a_deeply_nested_file_is_a_named_error_not_a_stack_overflow() {
        let cases: Vec<(&str, String)> = vec![
            (
                "operator chain",
                format!("x = 1{}\n", " + 1".repeat(200_000)),
            ),
            (
                "attribute chain",
                format!("@a{}\ndef f(): ...\n", ".b".repeat(200_000)),
            ),
            (
                "nested lists",
                format!("x = {}{}\n", "[".repeat(50_000), "]".repeat(50_000)),
            ),
            (
                "nested patterns",
                format!(
                    "match x:\n    case {}1{}:\n        pass\n",
                    "[".repeat(20_000),
                    "]".repeat(20_000)
                ),
            ),
            (
                "nested statements",
                (0..1_500)
                    .map(|depth| format!("{}if x:\n", " ".repeat(depth)))
                    .chain(std::iter::once(format!("{}pass\n", " ".repeat(1_500))))
                    .collect(),
            ),
        ];
        for (name, source) in cases {
            let outcome = on_small_stack(move || analyse(&source).err());
            assert_eq!(outcome, Some(Some(ParseError::TooDeep)), "{name}");
        }
        let broken = format!("x = 1{} +\n", " + 1".repeat(200_000));
        let outcome = on_small_stack(move || analyse(&broken).err());
        assert!(
            matches!(outcome, Some(Some(ParseError::Syntax { .. }))),
            "{outcome:?}"
        );
        assert!(
            ParseError::TooDeep
                .to_string()
                .contains("could not be analysed")
        );
        assert!(ParseError::TooDeep.fix().contains("exclude the file"));
    }

    #[test]
    fn a_file_just_inside_the_limit_is_analysed_on_a_small_stack() {
        let depth = MAX_DEPTH - 10;
        let source = format!(
            "import os\n@a{}\nclass C(b{}):\n    x = 1{}\n",
            ".b".repeat(depth),
            ".c".repeat(depth),
            " + 1".repeat(depth)
        );
        let outcome = on_small_stack(move || analyse(&source).ok());
        assert_eq!(outcome, Some(Some(2)));
    }

    #[test]
    fn lines_are_one_based_in_characters() {
        let lines = Lines::new("a\n\u{e9}\u{e9}x\n");
        assert_eq!(lines.locate(0), (1, 1));
        assert_eq!(lines.locate(2), (2, 1));
        assert_eq!(lines.locate(6), (2, 3));
        assert_eq!(lines.locate(999), (3, 1));
        assert_eq!(lines.text(2, 4), "\u{e9}");
        assert_eq!(lines.text(3, 4), "");
        assert_eq!(lines.text(5, 99), "");
    }
}
