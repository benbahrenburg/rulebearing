//! The Python code layer: classes, module-level functions, methods, properties and decorators.
//!
//! - Plan: [Wave 2, Step 4](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#24-step-4-the-python-extractor-2b)
//!   (`codelayer.rs`)
//! - Decision: [ADR-0013](../../../docs/adr/0013-ruff-parser-for-python.md) (classes, functions,
//!   methods, decorators, bases, `@property`, `@staticmethod`, `@classmethod`, `@dataclass`,
//!   underscore visibility); [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md)
//!   (a name that cannot be resolved is kept as written, never guessed)
//! - Architecture: [The graph document](../../../docs/architecture.md#the-graph-document) (the
//!   `code` section)
//! - Requirement: [FR-EXT-PY-02](../../../docs/prd.md#fr-ext-py-02)
//!
//! | Python | Element |
//! | --- | --- |
//! | `class C(B)` | type `class`, `baseType` the first base, a dependency `inherits` on each base with a dotted full name |
//! | a class inside a class | `nested: true`, `nestedIn` the outer class, full name `mod.Outer.Inner` |
//! | a module-level `def` | type `function` |
//! | a `def` in a class | member `method` (`constructor` for `__init__`); `static` for `@staticmethod` and `@classmethod` |
//! | `@property` | member `property` with a getter, a setter when `@x.setter` exists, `readonly` without one |
//! | `@dataclass(frozen=True)` | `immutable: true`; any other class `immutable: false` |
//! | an `ABC` base, `metaclass=ABCMeta`, or any `@abstractmethod` | `abstract: true` |
//! | a decorator | an attribute on its target, and a dependency `attribute` when its name is dotted |
//! | a leading underscore | visibility `private`; a dunder such as `__init__` is `public` |
//!
//! Full names are the dotted module plus the qualified name. A name is resolved through the
//! module's imports and its own classes and functions: `from pkg.base import Base` makes `Base`
//! `pkg.base.Base`, `import abc` makes `abc.ABC` itself. A name bound nowhere in the module (a
//! builtin such as `Exception`) is kept as written and forms no dependency.

use std::collections::{BTreeMap, BTreeSet};

use rb_model::{
    Accessor, AttributeElement, CodeLayer, ElementDependency, Language, Location, MemberElement,
    NamedArgument, TypeElement,
};
use ruff_python_ast::{Decorator, Expr, ModModule, Stmt, StmtClassDef, StmtFunctionDef};
use ruff_text_size::Ranged;

use crate::parse::Lines;
use crate::resolve::resolve_relative;

/// Where the file sits: what the elements' names and locations are built from.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The dotted module name.
    pub module: &'a str,
    /// The package relative imports resolve against.
    pub package: &'a str,
    /// The repository-relative file.
    pub file: &'a str,
    /// The top-level package, recorded as the element's `assembly`.
    pub project: &'a str,
}

/// The code-layer elements a module declares, unsorted; the caller normalises the merged layer.
pub fn elements(module: &ModModule, lines: &Lines<'_>, context: Context<'_>) -> CodeLayer {
    let mut builder = Builder {
        lines,
        context,
        bindings: BTreeMap::new(),
        layer: CodeLayer::default(),
        seen_types: BTreeSet::new(),
    };
    builder.bind(&module.body);
    builder.statements(&module.body, None);
    builder.layer
}

/// `private` for a leading underscore, `public` for everything else, dunders included.
pub fn visibility(name: &str) -> &'static str {
    let dunder = name.len() > 4 && name.starts_with("__") && name.ends_with("__");
    if name.starts_with('_') && !dunder {
        "private"
    } else {
        "public"
    }
}

struct Builder<'l, 's, 'c> {
    lines: &'l Lines<'s>,
    context: Context<'c>,
    /// Local name to full dotted name.
    bindings: BTreeMap<String, String>,
    layer: CodeLayer,
    seen_types: BTreeSet<String>,
}

/// One thing a decorator says about its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Marker {
    /// `@staticmethod` or `@classmethod`.
    Static,
    /// `@property`.
    Property,
    /// `@abstractmethod`.
    Abstract,
    /// `@dataclass`.
    Dataclass,
    /// `@dataclass(frozen=True)`.
    Frozen,
}

/// What a class's or function's decorators say.
#[derive(Default)]
struct Flags {
    markers: BTreeSet<Marker>,
    setter_of: Option<String>,
}

impl Flags {
    fn has(&self, marker: Marker) -> bool {
        self.markers.contains(&marker)
    }
}

impl Builder<'_, '_, '_> {
    /// Records every name the module binds at its top level (imports, classes, functions),
    /// looking inside `if` and `try` blocks.
    fn bind(&mut self, body: &[Stmt]) {
        for stmt in body {
            match stmt {
                Stmt::Import(import) => {
                    for alias in &import.names {
                        let name = alias.name.as_str();
                        if let Some(asname) = &alias.asname {
                            self.bindings
                                .insert(asname.as_str().to_owned(), name.to_owned());
                        } else {
                            let top = name.split('.').next().unwrap_or(name);
                            self.bindings.insert(top.to_owned(), top.to_owned());
                        }
                    }
                }
                Stmt::ImportFrom(from) => {
                    let module = from.module.as_ref().map_or("", |m| m.as_str());
                    let level = usize::try_from(from.level).unwrap_or(usize::MAX);
                    let Some(base) = resolve_relative(self.context.package, level, module) else {
                        continue;
                    };
                    for alias in &from.names {
                        let name = alias.name.as_str();
                        if name == "*" {
                            continue;
                        }
                        let bound = alias.asname.as_ref().map_or(name, |a| a.as_str());
                        let full = if base.is_empty() {
                            name.to_owned()
                        } else {
                            format!("{base}.{name}")
                        };
                        self.bindings.insert(bound.to_owned(), full);
                    }
                }
                Stmt::ClassDef(class) => {
                    let name = class.name.as_str();
                    self.bindings
                        .insert(name.to_owned(), format!("{}.{name}", self.context.module));
                }
                Stmt::FunctionDef(function) => {
                    let name = function.name.as_str();
                    self.bindings
                        .insert(name.to_owned(), format!("{}.{name}", self.context.module));
                }
                Stmt::If(branch) => {
                    self.bind(&branch.body);
                    for clause in &branch.elif_else_clauses {
                        self.bind(&clause.body);
                    }
                }
                Stmt::Try(block) => {
                    self.bind(&block.body);
                    for handler in &block.handlers {
                        let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                        self.bind(&handler.body);
                    }
                    self.bind(&block.orelse);
                    self.bind(&block.finalbody);
                }
                _ => {}
            }
        }
    }

    /// The full dotted name an expression names, when it names one.
    fn full_name(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Name(name) => {
                let id = name.id.as_str();
                Some(
                    self.bindings
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| id.to_owned()),
                )
            }
            Expr::Attribute(attribute) => self
                .full_name(&attribute.value)
                .map(|base| format!("{base}.{}", attribute.attr.as_str())),
            Expr::Call(call) => self.full_name(&call.func),
            Expr::Subscript(subscript) => self.full_name(&subscript.value),
            _ => None,
        }
    }

    fn location(&self, offset: u32) -> Location {
        let (line, column) = self.lines.locate(offset);
        Location {
            language: Language::Python,
            file: Some(self.context.file.to_owned()),
            line: Some(line),
            column: Some(column),
        }
    }

    /// Literal text for an attribute argument: a string's value, a name's full name, else the
    /// source text.
    fn literal(&self, expr: &Expr) -> String {
        match expr {
            Expr::StringLiteral(literal) => literal.value.to_str().to_owned(),
            Expr::Name(_) | Expr::Attribute(_) => self.full_name(expr).unwrap_or_default(),
            _ => self
                .lines
                .text(expr.range().start().into(), expr.range().end().into())
                .to_owned(),
        }
    }

    fn statements(&mut self, body: &[Stmt], outer: Option<&str>) {
        for stmt in body {
            match stmt {
                Stmt::ClassDef(class) => self.class(class, outer),
                Stmt::FunctionDef(function) if outer.is_none() => self.function(function),
                Stmt::If(branch) if outer.is_none() => {
                    self.statements(&branch.body, None);
                    for clause in &branch.elif_else_clauses {
                        self.statements(&clause.body, None);
                    }
                }
                Stmt::Try(block) if outer.is_none() => {
                    self.statements(&block.body, None);
                    for handler in &block.handlers {
                        let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                        self.statements(&handler.body, None);
                    }
                    self.statements(&block.orelse, None);
                    self.statements(&block.finalbody, None);
                }
                _ => {}
            }
        }
    }

    /// Reads the decorators, records each as an attribute on `target`, and returns what they
    /// say plus the dependencies they form.
    fn decorators(
        &mut self,
        decorators: &[Decorator],
        target: &str,
    ) -> (Flags, Vec<ElementDependency>) {
        let mut flags = Flags::default();
        let mut dependencies = Vec::new();
        for decorator in decorators {
            let expression = &decorator.expression;
            if let Expr::Attribute(attribute) = expression
                && let Expr::Name(owner) = &*attribute.value
                && matches!(attribute.attr.as_str(), "setter" | "deleter")
                && !self.bindings.contains_key(owner.id.as_str())
            {
                if attribute.attr.as_str() == "setter" {
                    flags.setter_of = Some(owner.id.as_str().to_owned());
                }
                continue;
            }
            let Some(name) = self.full_name(expression) else {
                continue;
            };
            match name.as_str() {
                "staticmethod" | "classmethod" => {
                    flags.markers.insert(Marker::Static);
                }
                "property" | "functools.cached_property" => {
                    flags.markers.insert(Marker::Property);
                }
                "abc.abstractmethod" | "abstractmethod" => {
                    flags.markers.insert(Marker::Abstract);
                }
                "dataclasses.dataclass" | "dataclass" => {
                    flags.markers.insert(Marker::Dataclass);
                    if let Expr::Call(call) = expression {
                        let frozen = call.arguments.keywords.iter().any(|k| {
                            k.arg.as_ref().is_some_and(|a| a.as_str() == "frozen")
                                && matches!(k.value, Expr::BooleanLiteral(ref b) if b.value)
                        });
                        if frozen {
                            flags.markers.insert(Marker::Frozen);
                        }
                    }
                }
                _ => {}
            }
            let (arguments, named_arguments) = match expression {
                Expr::Call(call) => (
                    call.arguments
                        .args
                        .iter()
                        .map(|a| self.literal(a))
                        .collect(),
                    call.arguments
                        .keywords
                        .iter()
                        .filter_map(|k| {
                            Some(NamedArgument {
                                name: k.arg.as_ref()?.as_str().to_owned(),
                                value: self.literal(&k.value),
                            })
                        })
                        .collect(),
                ),
                _ => (Vec::new(), Vec::new()),
            };
            let location = self.location(expression.range().start().into());
            if name.contains('.') {
                dependencies.push(ElementDependency {
                    target: name.clone(),
                    kind: "attribute".to_owned(),
                    member: None,
                    line: location.line,
                    form: None,
                });
            }
            self.layer.attributes.push(AttributeElement {
                target: target.to_owned(),
                attribute_type: name,
                arguments,
                named_arguments,
                location,
            });
        }
        (flags, dependencies)
    }

    fn qualified(&self, outer: Option<&str>, name: &str) -> String {
        match outer {
            Some(outer) => format!("{outer}.{name}"),
            None => format!("{}.{name}", self.context.module),
        }
    }

    fn new_type(&self, full_name: &str, name: &str, kind: &str, offset: u32) -> TypeElement {
        let mut element = TypeElement::new(full_name, name, kind, self.location(offset));
        element.namespace = Some(self.context.module.to_owned());
        element.assembly = Some(self.context.project.to_owned());
        element.visibility = Some(visibility(name).to_owned());
        element
    }

    fn function(&mut self, function: &StmtFunctionDef) {
        let name = function.name.as_str();
        let full_name = self.qualified(None, name);
        if !self.seen_types.insert(full_name.clone()) {
            return;
        }
        let (_, dependencies) = self.decorators(&function.decorator_list, &full_name);
        let mut element = self.new_type(
            &full_name,
            name,
            "function",
            function.name.range.start().into(),
        );
        element.dependencies = dependencies;
        self.layer.types.push(element);
    }

    fn class(&mut self, class: &StmtClassDef, outer: Option<&str>) {
        let name = class.name.as_str();
        let full_name = self.qualified(outer, name);
        if !self.seen_types.insert(full_name.clone()) {
            return;
        }
        let (flags, mut dependencies) = self.decorators(&class.decorator_list, &full_name);
        let mut bases = Vec::new();
        let mut abstract_ = false;
        if let Some(arguments) = &class.arguments {
            for base in &arguments.args {
                let Some(base_name) = self.full_name(base) else {
                    continue;
                };
                abstract_ |= matches!(base_name.as_str(), "abc.ABC" | "ABC");
                if base_name.contains('.') {
                    let (line, _) = self.lines.locate(base.range().start().into());
                    dependencies.push(ElementDependency {
                        target: base_name.clone(),
                        kind: "inherits".to_owned(),
                        member: None,
                        line: Some(line),
                        form: None,
                    });
                }
                bases.push(base_name);
            }
            abstract_ |= arguments.keywords.iter().any(|k| {
                k.arg.as_ref().is_some_and(|a| a.as_str() == "metaclass")
                    && self
                        .full_name(&k.value)
                        .is_some_and(|m| matches!(m.as_str(), "abc.ABCMeta" | "ABCMeta"))
            });
        }
        let mut members: Vec<MemberElement> = Vec::new();
        let mut setters: BTreeSet<String> = BTreeSet::new();
        for stmt in &class.body {
            match stmt {
                Stmt::FunctionDef(method) => {
                    if let Some(member) = self.method(method, &full_name, &mut setters) {
                        abstract_ |= member.r#abstract == Some(true);
                        if !members.iter().any(|m| m.name == member.name) {
                            members.push(member);
                        }
                    }
                }
                Stmt::ClassDef(inner) => self.class(inner, Some(&full_name)),
                _ => {}
            }
        }
        for member in &mut members {
            if member.kind == "property" {
                let has_setter = setters.contains(&member.name);
                if has_setter {
                    member.setter = Some(Accessor {
                        visibility: visibility(&member.name).to_owned(),
                    });
                }
                member.readonly = Some(!has_setter);
            }
        }
        self.layer.members.extend(members);
        let mut element = self.new_type(&full_name, name, "class", class.name.range.start().into());
        element.r#abstract = Some(abstract_);
        element.nested = Some(outer.is_some());
        element.nested_in = outer.map(str::to_owned);
        element.immutable = Some(flags.has(Marker::Dataclass) && flags.has(Marker::Frozen));
        element.base_type = bases.first().cloned();
        element.base_types = bases;
        element.dependencies = dependencies;
        self.layer.types.push(element);
    }

    /// A method or property; `None` for a property setter or deleter, whose name goes into
    /// `setters`.
    fn method(
        &mut self,
        method: &StmtFunctionDef,
        declaring: &str,
        setters: &mut BTreeSet<String>,
    ) -> Option<MemberElement> {
        let name = method.name.as_str();
        let full_name = format!("{declaring}.{name}");
        let (flags, dependencies) = self.decorators(&method.decorator_list, &full_name);
        if flags.setter_of.as_deref() == Some(name) {
            setters.insert(name.to_owned());
            return None;
        }
        let is_deleter = method
            .decorator_list
            .iter()
            .any(|d| matches!(&d.expression, Expr::Attribute(a) if a.attr.as_str() == "deleter"));
        if is_deleter {
            return None;
        }
        let kind = if flags.has(Marker::Property) {
            "property"
        } else if name == "__init__" {
            "constructor"
        } else {
            "method"
        };
        let mut member = MemberElement::new(
            declaring,
            name,
            kind,
            self.location(method.name.range.start().into()),
        );
        let shown = visibility(name).to_owned();
        member.visibility = Some(shown.clone());
        member.r#static = Some(flags.has(Marker::Static));
        member.r#abstract = Some(flags.has(Marker::Abstract));
        member.full_name = Some(full_name);
        member.dependencies = dependencies;
        if flags.has(Marker::Property) {
            member.getter = Some(Accessor { visibility: shown });
        }
        Some(member)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(source: &str) -> CodeLayer {
        let parsed = crate::parse::parse(source);
        assert!(parsed.is_ok(), "{parsed:?}");
        let Ok(module) = parsed else {
            return CodeLayer::default();
        };
        let lines = Lines::new(source);
        let mut layer = elements(
            &module,
            &lines,
            Context {
                module: "pkg.shapes",
                package: "pkg",
                file: "src/pkg/shapes.py",
                project: "pkg",
            },
        );
        layer.normalise();
        layer
    }

    fn find<'a>(layer: &'a CodeLayer, full_name: &str) -> Option<&'a TypeElement> {
        layer.types.iter().find(|t| t.full_name == full_name)
    }

    fn member<'a>(layer: &'a CodeLayer, name: &str) -> Option<&'a MemberElement> {
        layer.members.iter().find(|m| m.name == name)
    }

    #[test]
    fn visibility_follows_the_underscore() {
        assert_eq!(visibility("_x"), "private");
        assert_eq!(visibility("__x"), "private");
        assert_eq!(visibility("__init__"), "public");
        assert_eq!(visibility("x"), "public");
        assert_eq!(visibility("____"), "private");
    }

    #[test]
    fn classes_bases_and_abstractness() {
        let layer = layer(
            "\
import abc
from abc import ABC, abstractmethod
from .base import Base as B
from . import models

class Shape(ABC):
    @abstractmethod
    def area(self): ...

class Square(B, models.Mixin, Exception):
    pass

class Meta(metaclass=abc.ABCMeta):
    pass

class Method:
    @abc.abstractmethod
    def run(self): ...

class _Hidden:
    pass
",
        );
        let shape = find(&layer, "pkg.shapes.Shape");
        assert_eq!(shape.and_then(|t| t.r#abstract), Some(true));
        assert_eq!(shape.and_then(|t| t.base_type.as_deref()), Some("abc.ABC"));
        let square = find(&layer, "pkg.shapes.Square");
        assert_eq!(square.and_then(|t| t.r#abstract), Some(false));
        assert_eq!(
            square.map(|t| t.base_types.clone()),
            Some(vec![
                "pkg.base.Base".to_owned(),
                "pkg.models.Mixin".to_owned(),
                "Exception".to_owned()
            ])
        );
        let targets: Vec<(&str, &str)> = square
            .map(|t| {
                t.dependencies
                    .iter()
                    .map(|d| (d.target.as_str(), d.kind.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            targets,
            [
                ("pkg.base.Base", "inherits"),
                ("pkg.models.Mixin", "inherits")
            ]
        );
        assert_eq!(
            find(&layer, "pkg.shapes.Meta").and_then(|t| t.r#abstract),
            Some(true)
        );
        assert_eq!(
            find(&layer, "pkg.shapes.Method").and_then(|t| t.r#abstract),
            Some(true)
        );
        let hidden = find(&layer, "pkg.shapes._Hidden");
        assert_eq!(
            hidden.and_then(|t| t.visibility.as_deref()),
            Some("private")
        );
        assert_eq!(
            hidden.and_then(|t| t.namespace.as_deref()),
            Some("pkg.shapes")
        );
        assert_eq!(hidden.and_then(|t| t.assembly.as_deref()), Some("pkg"));
        assert_eq!(
            hidden.map(|t| (t.location.line, t.location.column)),
            Some((Some(20), Some(7)))
        );
    }

    #[test]
    fn dataclasses_properties_and_members() {
        let layer = layer(
            "\
from dataclasses import dataclass
import dataclasses

@dataclass(frozen=True, order=True)
class Point:
    x: int

@dataclasses.dataclass
class Loose:
    x: int

@dataclass(frozen=False)
class Thawed:
    x: int

class Account:
    def __init__(self): ...

    @property
    def balance(self): ...

    @balance.setter
    def balance(self, value): ...

    @property
    def owner(self): ...

    @owner.deleter
    def owner(self): ...

    @staticmethod
    def make(): ...

    @classmethod
    def load(cls): ...

    def _secret(self): ...

    class Ledger:
        pass
",
        );
        assert_eq!(
            find(&layer, "pkg.shapes.Point").and_then(|t| t.immutable),
            Some(true)
        );
        assert_eq!(
            find(&layer, "pkg.shapes.Loose").and_then(|t| t.immutable),
            Some(false)
        );
        assert_eq!(
            find(&layer, "pkg.shapes.Thawed").and_then(|t| t.immutable),
            Some(false)
        );
        let point_attribute = layer
            .attributes
            .iter()
            .find(|a| a.target == "pkg.shapes.Point");
        assert_eq!(
            point_attribute.map(|a| a.attribute_type.as_str()),
            Some("dataclasses.dataclass")
        );
        assert_eq!(
            point_attribute.map(|a| a
                .named_arguments
                .iter()
                .map(|n| format!("{}={}", n.name, n.value))
                .collect::<Vec<_>>()),
            Some(vec!["frozen=True".to_owned(), "order=True".to_owned()])
        );
        let balance = member(&layer, "balance");
        assert_eq!(balance.map(|m| m.kind.as_str()), Some("property"));
        assert!(balance.is_some_and(|m| m.getter.is_some() && m.setter.is_some()));
        assert_eq!(balance.and_then(|m| m.readonly), Some(false));
        let owner = member(&layer, "owner");
        assert!(owner.is_some_and(|m| m.getter.is_some() && m.setter.is_none()));
        assert_eq!(owner.and_then(|m| m.readonly), Some(true));
        assert_eq!(member(&layer, "make").and_then(|m| m.r#static), Some(true));
        assert_eq!(member(&layer, "load").and_then(|m| m.r#static), Some(true));
        assert_eq!(
            member(&layer, "__init__").map(|m| m.kind.as_str()),
            Some("constructor")
        );
        assert_eq!(
            member(&layer, "_secret").and_then(|m| m.visibility.as_deref()),
            Some("private")
        );
        assert_eq!(
            member(&layer, "_secret").and_then(|m| m.full_name.as_deref()),
            Some("pkg.shapes.Account._secret")
        );
        let ledger = find(&layer, "pkg.shapes.Account.Ledger");
        assert_eq!(ledger.and_then(|t| t.nested), Some(true));
        assert_eq!(
            ledger.and_then(|t| t.nested_in.as_deref()),
            Some("pkg.shapes.Account")
        );
        assert_eq!(
            layer.members.iter().filter(|m| m.name == "balance").count(),
            1
        );
    }

    #[test]
    fn functions_decorators_and_guarded_definitions() {
        let layer = layer(
            "\
import functools
from ..registry import register

@register('name', 3, key=CONST, other=functools.partial)
def handler(): ...

async def _task(): ...

try:
    class Fast: ...
except ImportError:
    class Fast: ...

if True:
    def later(): ...
",
        );
        let handler = find(&layer, "pkg.shapes.handler");
        assert_eq!(handler.map(|t| t.kind.as_str()), Some("function"));
        assert_eq!(
            handler.map(|t| t
                .dependencies
                .iter()
                .map(|d| d.target.clone())
                .collect::<Vec<_>>()),
            Some(vec!["registry.register".to_owned()])
        );
        let attribute = layer
            .attributes
            .iter()
            .find(|a| a.target == "pkg.shapes.handler");
        assert_eq!(
            attribute.map(|a| a.arguments.clone()),
            Some(vec!["name".to_owned(), "3".to_owned()])
        );
        assert_eq!(
            attribute.map(|a| a
                .named_arguments
                .iter()
                .map(|n| n.value.clone())
                .collect::<Vec<_>>()),
            Some(vec!["CONST".to_owned(), "functools.partial".to_owned()])
        );
        assert_eq!(
            find(&layer, "pkg.shapes._task").and_then(|t| t.visibility.as_deref()),
            Some("private")
        );
        assert_eq!(layer.types.iter().filter(|t| t.name == "Fast").count(), 1);
        assert!(find(&layer, "pkg.shapes.later").is_some());
    }
}
