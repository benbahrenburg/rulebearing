//! One `.cs` file read with `tree-sitter-c-sharp` into what source mode resolves: the types it
//! declares, its `using` directives by the namespace declaration they belong to, and every name
//! it writes in a place a type can stand.
//!
//! - Plan: [Wave 3, Step 14](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)
//!   (`namespace` and file-scoped namespace declarations, `using` directives, and qualified names
//!   in type positions)
//! - Decision: [ADR-0011](../../../../docs/adr/0011-read-dotnet-assemblies-not-source.md)
//!   (source mode is approximate and never the gate)
//! - Architecture: [§ Technology choices](../../../../docs/architecture.md#technology-choices)
//!   (`.NET source mode | tree-sitter-c-sharp`)
//! - Requirement: [FR-EXT-DN-04](../../../../docs/prd.md#fr-ext-dn-04)
//!
//! The tree is parsed, never compiled. A name is recorded with the position it was written in
//! (a base list, an attribute, a field's type, a signature, a `typeof`, a type argument, a body);
//! whether it names a type is decided later against every file's declarations
//! ([`super::namespaces`]). Names a type can never be (a declaration's own name, a member after
//! a dot, a named argument, a method called by its bare name, the operand of `nameof`) are not
//! recorded. The walk keeps its own stack, so a deeply nested expression cannot exhaust the
//! thread's, and a file with syntax errors is still read: tree-sitter recovers, and only what it
//! recognised is kept.

use std::collections::BTreeSet;

use rb_model::DependencyKind;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

/// What a type declaration declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TypeKind {
    /// `class` or `record` (a `record class`).
    Class,
    /// `struct` or `record struct`.
    Struct,
    /// `interface`.
    Interface,
    /// `enum`.
    Enum,
    /// `delegate`.
    Delegate,
}

/// One type declaration, partial or not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Declaration {
    /// The namespace, dotted; empty for the global namespace.
    pub namespace: String,
    /// The enclosing types, outermost first, each with its generic arity.
    pub outer: Vec<(String, u32)>,
    /// The simple name.
    pub name: String,
    /// The generic arity.
    pub arity: u32,
    /// What it declares.
    pub kind: TypeKind,
    /// The 1-based line of the declaration.
    pub line: u32,
    /// The declaration has a constructor of its own (declared, or a primary constructor): the
    /// part a compiled build attributes a partial type to.
    pub constructor: bool,
    /// The declaration has a member with a body: a part of a partial type that does refers to
    /// the type in its code, which a compiled build records as an edge to the file the type
    /// lands in.
    #[serde(default)]
    pub bodies: bool,
    /// Its `const` fields and, for an enum, its members: a compiled build writes their values
    /// where they are used, so `Type.Constant` leaves no reference to the type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constants: Vec<String>,
    /// The extension methods it declares (a first parameter marked `this`), by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<String>,
}

/// A method called on a value (`value.Name(...)`, `value?.Name(...)`): what an extension method
/// call looks like, resolved against the extension methods in scope.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Call {
    /// The method's name.
    pub name: String,
    /// The scope it was written in.
    pub scope: usize,
    /// The innermost type declaration it was written in.
    pub enclosing: Option<usize>,
    /// Written outside any body (an initializer), as [`Reference::member`].
    #[serde(default)]
    pub member: bool,
    /// The 1-based line of the first such call.
    pub line: u32,
    /// The 1-based column.
    pub column: u32,
}

/// One `using` directive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Using {
    /// The namespace or type it names, as dotted segments without generic arguments.
    pub target: Vec<String>,
    /// `using Alias = Target;`.
    pub alias: Option<String>,
    /// `using static Type;`.
    pub is_static: bool,
    /// `global using ...;`: in scope in every file of the project.
    pub global: bool,
    /// The 1-based line.
    pub line: u32,
    /// The 1-based column.
    pub column: u32,
}

/// A namespace declaration, or the compilation unit (scope 0, the global namespace).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    /// The namespace the declaration opens, dotted and complete (`A.B` inside `A` is `A.B`).
    pub namespace: String,
    /// The enclosing declaration's scope; `None` for the compilation unit.
    pub parent: Option<usize>,
    /// The `using` directives written in it.
    pub usings: Vec<Using>,
}

/// A name written where a type can stand.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Reference {
    /// The dotted segments as written, each with the number of type arguments written on it.
    pub segments: Vec<(String, u32)>,
    /// `alias::Name`: the alias (`global` included).
    pub qualifier: Option<String>,
    /// The scope it was written in.
    pub scope: usize,
    /// The innermost type declaration it was written in, an index into
    /// [`FileFacts::declarations`].
    pub enclosing: Option<usize>,
    /// The position: which dependency kind a type found here forms.
    pub kind: Position,
    /// Written outside any body, at member level: compiled into the type, so attributed to the
    /// file the enclosing type lands in rather than to this one.
    #[serde(default)]
    pub member: bool,
    /// The 1-based line of the first time it was written in this position.
    pub line: u32,
    /// The 1-based column.
    pub column: u32,
}

/// Where a name was written, which decides the edge's `dependencyKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Position {
    /// A base list entry: `inherits`, or `implements` for an interface.
    Base,
    /// An attribute's name: `attribute`, also tried with the `Attribute` suffix.
    Attribute,
    /// A field's type: `field`.
    Field,
    /// A parameter, return, property or constraint type: `signature`.
    Signature,
    /// The operand of `typeof`: `typeof`.
    TypeOf,
    /// A type argument: `generic-argument`.
    GenericArgument,
    /// Anywhere in a body or an initializer: `body`.
    Body,
}

impl Position {
    /// The dependency kind of an edge formed here to a type of kind `target`.
    pub fn dependency_kind(self, target: TypeKind) -> DependencyKind {
        match self {
            Self::Base if target == TypeKind::Interface => DependencyKind::Implements,
            Self::Base => DependencyKind::Inherits,
            Self::Attribute => DependencyKind::Attribute,
            Self::Field => DependencyKind::Field,
            Self::Signature => DependencyKind::Signature,
            Self::TypeOf => DependencyKind::Typeof,
            Self::GenericArgument => DependencyKind::GenericArgument,
            Self::Body => DependencyKind::Body,
        }
    }
}

/// What one file declares and writes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFacts {
    /// The type declarations, in source order.
    pub declarations: Vec<Declaration>,
    /// The compilation unit, then each namespace declaration in source order.
    pub scopes: Vec<Scope>,
    /// The names, each once per (scope, enclosing type, position), sorted.
    pub references: Vec<Reference>,
    /// The methods called on a value, each once per (scope, enclosing type), sorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<Call>,
    /// The file has top-level statements: a compiled build puts them in a `Program` type of
    /// this file.
    pub top_level: bool,
    /// Tree-sitter recovered from a syntax error somewhere in the file.
    pub syntax_errors: bool,
}

/// Why a file could not be parsed at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The grammar and the tree-sitter runtime do not agree on an ABI version.
    #[error("the C# grammar could not be loaded: {0}")]
    Language(String),
    /// The parser gave up (it does only when cancelled or timed out, neither of which is set).
    #[error("tree-sitter returned no tree")]
    NoTree,
}

/// A parser for C#, reusable across files on one thread.
pub struct CSharpParser {
    parser: Parser,
}

impl CSharpParser {
    /// A parser with the C# grammar loaded.
    ///
    /// # Errors
    /// [`ParseError::Language`] when the grammar's ABI is not one the runtime reads.
    pub fn new() -> Result<Self, ParseError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
            .map_err(|e| ParseError::Language(e.to_string()))?;
        Ok(Self { parser })
    }

    /// Reads one file's text.
    ///
    /// # Errors
    /// [`ParseError::NoTree`] when tree-sitter returns nothing.
    pub fn parse(&mut self, text: &str) -> Result<FileFacts, ParseError> {
        let tree = self.parser.parse(text, None).ok_or(ParseError::NoTree)?;
        let root = tree.root_node();
        let mut walk = Walk {
            text: text.as_bytes(),
            facts: FileFacts {
                scopes: vec![Scope {
                    namespace: String::new(),
                    parent: None,
                    usings: Vec::new(),
                }],
                syntax_errors: root.has_error(),
                ..FileFacts::default()
            },
            seen: BTreeSet::new(),
            called: BTreeSet::new(),
        };
        walk.compilation_unit(root);
        let mut facts = walk.facts;
        facts.references.sort();
        facts.calls.sort();
        Ok(facts)
    }
}

/// The context a node is visited in.
#[derive(Debug, Clone, Copy)]
struct Context {
    scope: usize,
    enclosing: Option<usize>,
    position: Position,
    /// Inside a method, accessor or top-level statement body, where a compiled build keeps the
    /// file the code is written in; outside one (a field, a signature, an initializer) it
    /// attributes the reference to the file its type lands in.
    in_body: bool,
}

/// One pending visit: a node, the field it fills in its parent, and the parent's kind.
#[derive(Clone, Copy)]
struct Visit<'t> {
    node: Node<'t>,
    field: Option<&'t str>,
    parent: &'t str,
    context: Context,
}

/// The kinds that declare a type, with what each declares.
fn type_kind(kind: &str) -> Option<TypeKind> {
    Some(match kind {
        "class_declaration" | "record_declaration" => TypeKind::Class,
        "struct_declaration" | "record_struct_declaration" => TypeKind::Struct,
        "interface_declaration" => TypeKind::Interface,
        "enum_declaration" => TypeKind::Enum,
        "delegate_declaration" => TypeKind::Delegate,
        _ => return None,
    })
}

/// Parents whose `name` field is a declaration's own name, never a reference.
const DECLARING: &[&str] = &[
    "class_declaration",
    "record_declaration",
    "struct_declaration",
    "record_struct_declaration",
    "interface_declaration",
    "enum_declaration",
    "delegate_declaration",
    "enum_member_declaration",
    "method_declaration",
    "constructor_declaration",
    "destructor_declaration",
    "local_function_statement",
    "property_declaration",
    "event_declaration",
    "variable_declarator",
    "parameter",
    "type_parameter",
    "catch_declaration",
    "declaration_expression",
    "declaration_pattern",
    "recursive_pattern",
    "tuple_element",
    "foreach_statement",
    "labeled_statement",
    "extern_alias_directive",
    "argument",
    "attribute_argument",
    "anonymous_object_member_declarator",
    "subpattern",
    "from_clause",
    "join_clause",
    "let_clause",
    "query_continuation",
    "join_into_clause",
];

/// Nodes whose every identifier is a name that is not a type: argument and member names,
/// parameter lists of lambdas, labels, `goto` targets, preprocessor symbols.
const NOT_TYPES: &[&str] = &[
    "name_colon",
    "name_equals",
    "implicit_parameter",
    "goto_statement",
    "preproc_if",
    "preproc_elif",
    "preproc_define",
    "preproc_undef",
    "preproc_region",
    "preproc_endregion",
    "preproc_pragma",
    "preproc_nullable",
    "preproc_error",
    "preproc_warning",
    "preproc_line",
    "type_parameter_list",
    "type_parameter_constraints_clause_target",
];

/// A reference's identity within a file: (segments, qualifier, scope, enclosing, position,
/// member level).
type Seen = (
    Vec<(String, u32)>,
    Option<String>,
    usize,
    Option<usize>,
    Position,
    bool,
);

struct Walk<'s> {
    text: &'s [u8],
    facts: FileFacts,
    /// (segments, qualifier, scope, enclosing, position) already recorded.
    seen: BTreeSet<Seen>,
    /// (name, scope, enclosing, member level) of the calls already recorded.
    called: BTreeSet<(String, usize, Option<usize>, bool)>,
}

impl<'s> Walk<'s> {
    fn text(&self, node: Node<'_>) -> &'s str {
        node.utf8_text(self.text).unwrap_or("")
    }

    /// The compilation unit's members in order: a file-scoped namespace declaration opens the
    /// scope every member after it belongs to.
    fn compilation_unit(&mut self, root: Node<'_>) {
        let mut scope = 0;
        let mut cursor = root.walk();
        let children: Vec<Node<'_>> = root.named_children(&mut cursor).collect();
        for child in children {
            match child.kind() {
                "file_scoped_namespace_declaration" => {
                    let name = child
                        .child_by_field_name("name")
                        .map(|n| self.dotted(n))
                        .unwrap_or_default();
                    scope = self.open_scope(0, &name);
                }
                "using_directive" => self.using(child, scope),
                "global_statement" => {
                    self.facts.top_level = true;
                    self.run(child, scope, true);
                }
                _ => self.run(child, scope, false),
            }
        }
    }

    fn open_scope(&mut self, parent: usize, name: &str) -> usize {
        let outer = &self.facts.scopes[parent].namespace;
        let namespace = if outer.is_empty() {
            name.to_owned()
        } else if name.is_empty() {
            outer.clone()
        } else {
            format!("{outer}.{name}")
        };
        self.facts.scopes.push(Scope {
            namespace,
            parent: Some(parent),
            usings: Vec::new(),
        });
        self.facts.scopes.len() - 1
    }

    fn using(&mut self, node: Node<'_>, scope: usize) {
        let mut cursor = node.walk();
        let (mut global, mut is_static) = (false, false);
        let mut target = None;
        let alias_node = node.child_by_field_name("name");
        for child in node.children(&mut cursor) {
            match child.kind() {
                "global" => global = true,
                "static" => is_static = true,
                _ if child.is_named() && Some(child) != alias_node => target = Some(child),
                _ => {}
            }
        }
        let Some(target) = target else {
            return;
        };
        let segments = self.segments(target).0;
        let position = node.start_position();
        let alias = alias_node.map(|n| self.text(n).to_owned());
        self.facts.scopes[scope].usings.push(Using {
            target: segments.into_iter().map(|(name, _)| name).collect(),
            alias,
            is_static,
            global,
            line: line(position.row),
            column: line(position.column),
        });
    }

    /// A name's dotted segments with their type-argument counts, the `alias::` qualifier, and
    /// the type-argument lists written on it (visited as generic arguments).
    fn segments<'t>(&self, node: Node<'t>) -> (Vec<(String, u32)>, Option<String>, Vec<Node<'t>>) {
        let mut segments = Vec::new();
        let mut arguments = Vec::new();
        let mut qualifier = None;
        let mut current = Some(node);
        while let Some(n) = current {
            current = None;
            match n.kind() {
                "qualified_name" => {
                    if let Some(name) = n.child_by_field_name("name") {
                        segments.push(self.simple(name, &mut arguments));
                    }
                    current = n.child_by_field_name("qualifier");
                }
                "alias_qualified_name" => {
                    if let Some(name) = n.child_by_field_name("name") {
                        segments.push(self.simple(name, &mut arguments));
                    }
                    qualifier = n
                        .child_by_field_name("alias")
                        .map(|a| self.text(a).to_owned());
                }
                "member_access_expression" => {
                    if let Some(name) = n.child_by_field_name("name") {
                        segments.push(self.simple(name, &mut arguments));
                    }
                    current = n.child_by_field_name("expression");
                }
                "identifier" | "generic_name" => segments.push(self.simple(n, &mut arguments)),
                _ => {}
            }
        }
        segments.reverse();
        (segments, qualifier, arguments)
    }

    /// An identifier or a generic name: its text and its type-argument count.
    fn simple<'t>(&self, node: Node<'t>, arguments: &mut Vec<Node<'t>>) -> (String, u32) {
        if node.kind() != "generic_name" {
            return (self.text(node).to_owned(), 0);
        }
        let mut cursor = node.walk();
        let mut name = String::new();
        let mut arity = 0;
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "identifier" => self.text(child).clone_into(&mut name),
                "type_argument_list" => {
                    // `Foo<,>` writes no type, only commas: the arity is one more than they are.
                    let mut inner = child.walk();
                    let written = child.named_children(&mut inner).count();
                    let commas = self.text(child).matches(',').count();
                    arity = u32::try_from(written.max(commas + 1)).unwrap_or(u32::MAX);
                    arguments.push(child);
                }
                _ => {}
            }
        }
        (name, arity)
    }

    fn dotted(&self, node: Node<'_>) -> String {
        self.segments(node)
            .0
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>()
            .join(".")
    }

    /// Whether a member-access chain is names all the way down (`A.B.C`), so it may name a type
    /// or a namespace, rather than a member of a value (`this.x`, `f().y`).
    fn is_name_chain(node: Node<'_>) -> bool {
        let mut current = node;
        loop {
            match current.kind() {
                "member_access_expression" => {
                    let Some(expression) = current.child_by_field_name("expression") else {
                        return false;
                    };
                    current = expression;
                }
                "identifier" | "generic_name" | "qualified_name" | "alias_qualified_name" => {
                    return true;
                }
                _ => return false,
            }
        }
    }

    fn record<'t>(&mut self, node: Node<'t>, context: Context) -> Vec<Node<'t>> {
        let (segments, qualifier, arguments) = self.segments(node);
        if segments.is_empty() || segments.iter().any(|(name, _)| name.is_empty()) {
            return arguments;
        }
        let key = (
            segments.clone(),
            qualifier.clone(),
            context.scope,
            context.enclosing,
            context.position,
            !context.in_body,
        );
        if self.seen.insert(key) {
            let position = node.start_position();
            self.facts.references.push(Reference {
                segments,
                qualifier,
                scope: context.scope,
                enclosing: context.enclosing,
                kind: context.position,
                member: !context.in_body,
                line: line(position.row),
                column: line(position.column),
            });
        }
        arguments
    }

    /// Whether a field declaration carries the `const` modifier.
    fn is_const(&self, field: Node<'_>) -> bool {
        let mut cursor = field.walk();
        field
            .named_children(&mut cursor)
            .any(|c| c.kind() == "modifier" && self.text(c) == "const")
    }

    /// Whether a constructor declaration in class `class` is a C# 14 extension block: a real
    /// constructor is named after its class.
    fn is_extension_block(&self, node: Node<'_>, class: &str) -> bool {
        node.child_by_field_name("name")
            .is_some_and(|n| self.text(n) == "extension")
            && class != "extension"
    }

    /// The methods an extension block declares.
    fn extension_block_members(&self, node: Node<'_>) -> Vec<String> {
        let Some(body) = node.child_by_field_name("body") else {
            return Vec::new();
        };
        let mut cursor = body.walk();
        body.named_children(&mut cursor)
            .filter(|c| c.kind() == "local_function_statement")
            .filter_map(|f| f.child_by_field_name("name"))
            .map(|n| self.text(n).to_owned())
            .collect()
    }

    /// Whether a method is an extension method: its first parameter carries `this`.
    fn is_extension(&self, method: Node<'_>) -> bool {
        let Some(parameters) = method.child_by_field_name("parameters") else {
            return false;
        };
        let mut cursor = parameters.walk();
        let first = parameters
            .named_children(&mut cursor)
            .find(|p| p.kind() == "parameter");
        first.is_some_and(|parameter| {
            let mut inner = parameter.walk();
            parameter
                .named_children(&mut inner)
                .any(|c| c.kind() == "modifier" && self.text(c) == "this")
        })
    }

    /// Records a method called on a value.
    fn call(&mut self, name: Node<'_>, at: Node<'_>, context: Context) {
        let name = match name.kind() {
            "generic_name" => {
                let mut cursor = name.walk();
                name.named_children(&mut cursor)
                    .find(|c| c.kind() == "identifier")
                    .map(|c| self.text(c).to_owned())
            }
            _ => Some(self.text(name).to_owned()),
        };
        let Some(name) = name.filter(|n| !n.is_empty()) else {
            return;
        };
        let key = (
            name.clone(),
            context.scope,
            context.enclosing,
            !context.in_body,
        );
        if self.called.insert(key) {
            let position = at.start_position();
            self.facts.calls.push(Call {
                name,
                scope: context.scope,
                enclosing: context.enclosing,
                member: !context.in_body,
                line: line(position.row),
                column: line(position.column),
            });
        }
    }

    /// The names a field declaration declares.
    fn declarator_names(&self, field: Node<'_>) -> Vec<String> {
        let mut cursor = field.walk();
        let mut names = Vec::new();
        for declaration in field.named_children(&mut cursor) {
            if declaration.kind() != "variable_declaration" {
                continue;
            }
            let mut inner = declaration.walk();
            names.extend(
                declaration
                    .named_children(&mut inner)
                    .filter(|d| d.kind() == "variable_declarator")
                    .filter_map(|d| d.child_by_field_name("name"))
                    .map(|n| self.text(n).to_owned()),
            );
        }
        names
    }

    fn declaration(&mut self, node: Node<'_>, kind: TypeKind, context: Context) -> usize {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.text(n).to_owned())
            .unwrap_or_default();
        let mut cursor = node.walk();
        let mut arity = 0;
        let mut constructor = false;
        let mut constants = Vec::new();
        let mut extensions = Vec::new();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "type_parameter_list" => {
                    let mut inner = child.walk();
                    let count = child
                        .named_children(&mut inner)
                        .filter(|c| c.kind() == "type_parameter")
                        .count();
                    arity = u32::try_from(count).unwrap_or(u32::MAX);
                }
                // A primary constructor (a delegate's parameters are its signature).
                "parameter_list" => constructor |= kind != TypeKind::Delegate,
                "declaration_list" => {
                    let mut inner = child.walk();
                    for member in child.named_children(&mut inner) {
                        match member.kind() {
                            // A C# 14 `extension(T receiver) { ... }` block, which
                            // tree-sitter-c-sharp 0.23 reads as a constructor named `extension`
                            // holding local functions: each is an extension method.
                            "constructor_declaration" if self.is_extension_block(member, &name) => {
                                extensions.extend(self.extension_block_members(member));
                            }
                            "constructor_declaration" => constructor = true,
                            "field_declaration" if self.is_const(member) => {
                                constants.extend(self.declarator_names(member));
                            }
                            "method_declaration" if self.is_extension(member) => {
                                extensions.extend(
                                    member
                                        .child_by_field_name("name")
                                        .map(|n| self.text(n).to_owned()),
                                );
                            }
                            _ => {}
                        }
                    }
                }
                "enum_member_declaration_list" => {
                    let mut inner = child.walk();
                    constants.extend(
                        child
                            .named_children(&mut inner)
                            .filter(|m| m.kind() == "enum_member_declaration")
                            .filter_map(|m| m.child_by_field_name("name"))
                            .map(|n| self.text(n).to_owned()),
                    );
                }
                _ => {}
            }
        }
        let outer = context
            .enclosing
            .map(|i| {
                let parent = &self.facts.declarations[i];
                let mut outer = parent.outer.clone();
                outer.push((parent.name.clone(), parent.arity));
                outer
            })
            .unwrap_or_default();
        self.facts.declarations.push(Declaration {
            namespace: self.facts.scopes[context.scope].namespace.clone(),
            outer,
            name,
            arity,
            kind,
            line: line(node.start_position().row),
            constructor,
            bodies: false,
            constants,
            extensions,
        });
        self.facts.declarations.len() - 1
    }

    /// Visits `start` and everything below it, keeping its own stack.
    fn run(&mut self, start: Node<'_>, scope: usize, in_body: bool) {
        let mut stack = vec![Visit {
            node: start,
            field: None,
            parent: "compilation_unit",
            context: Context {
                scope,
                enclosing: None,
                position: Position::Body,
                in_body,
            },
        }];
        while let Some(visit) = stack.pop() {
            self.step(visit, &mut stack);
        }
    }

    /// Pushes a node's named children, each in the context `context_of` gives it for its field
    /// and kind, so they are visited in source order.
    fn push_children<'t>(
        node: Node<'t>,
        stack: &mut Vec<Visit<'t>>,
        context_of: impl Fn(Option<&str>, &str) -> Option<Context>,
    ) {
        let mut cursor = node.walk();
        let mut children = Vec::new();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.is_named()
                    && let Some(context) = context_of(cursor.field_name(), child.kind())
                {
                    children.push(Visit {
                        node: child,
                        field: cursor.field_name(),
                        parent: node.kind(),
                        context,
                    });
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        stack.extend(children.into_iter().rev());
    }

    /// The type arguments written on a member's name (`value.Add<T>()`), visited as generic
    /// arguments; the name itself is a member's, never a type.
    fn member_type_arguments<'t>(
        &self,
        node: Node<'t>,
        stack: &mut Vec<Visit<'t>>,
        context: Context,
    ) {
        if let Some(name) = node.child_by_field_name("name")
            && name.kind() == "generic_name"
        {
            let mut arguments = Vec::new();
            self.simple(name, &mut arguments);
            Self::push_arguments(stack, arguments, context);
        }
    }

    fn push_arguments<'t>(stack: &mut Vec<Visit<'t>>, arguments: Vec<Node<'t>>, context: Context) {
        for list in arguments.into_iter().rev() {
            Self::push_children(list, stack, |_, _| {
                Some(Context {
                    position: Position::GenericArgument,
                    ..context
                })
            });
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one match over the grammar's node kinds, so each kind's handling is in one place"
    )]
    fn step<'t>(&mut self, visit: Visit<'t>, stack: &mut Vec<Visit<'t>>) {
        let Visit {
            node,
            field,
            parent,
            context,
        } = visit;
        let kind = node.kind();
        if NOT_TYPES.contains(&kind) || kind == "comment" {
            return;
        }
        if let Some(type_kind) = type_kind(kind) {
            let index = self.declaration(node, type_kind, context);
            let inside = Context {
                enclosing: Some(index),
                in_body: false,
                ..context
            };
            Self::push_children(node, stack, |field, child| match (field, child) {
                (Some("name"), _) => None,
                (_, "base_list") => Some(Context {
                    position: Position::Base,
                    ..inside
                }),
                (_, "attribute_list") => Some(Context {
                    position: Position::Attribute,
                    ..inside
                }),
                (_, "parameter_list" | "type_parameter_constraints_clause")
                | (Some("type" | "parameters"), _) => Some(Context {
                    position: Position::Signature,
                    ..inside
                }),
                _ => Some(Context {
                    position: Position::Body,
                    ..inside
                }),
            });
            return;
        }
        match kind {
            "namespace_declaration" => {
                let name = node
                    .child_by_field_name("name")
                    .map(|n| self.dotted(n))
                    .unwrap_or_default();
                let scope = self.open_scope(context.scope, &name);
                if let Some(body) = node.child_by_field_name("body") {
                    let mut cursor = body.walk();
                    let members: Vec<Node<'t>> = body.named_children(&mut cursor).collect();
                    let mut pending = Vec::new();
                    for member in members {
                        if member.kind() == "using_directive" {
                            self.using(member, scope);
                        } else {
                            pending.push(Visit {
                                node: member,
                                field: None,
                                parent: "declaration_list",
                                context: Context {
                                    scope,
                                    enclosing: None,
                                    position: Position::Body,
                                    in_body: false,
                                },
                            });
                        }
                    }
                    stack.extend(pending.into_iter().rev());
                }
            }
            "using_directive" => self.using(node, context.scope),
            "field_declaration" | "event_field_declaration" => {
                Self::push_children(node, stack, |_, child| match child {
                    "attribute_list" => Some(Context {
                        position: Position::Attribute,
                        ..context
                    }),
                    _ => Some(Context {
                        position: Position::Field,
                        ..context
                    }),
                });
            }
            "variable_declaration" => {
                Self::push_children(node, stack, |field, _| match field {
                    Some("type") => Some(context),
                    _ => Some(Context {
                        position: Position::Body,
                        ..context
                    }),
                });
            }
            "method_declaration"
            | "constructor_declaration"
            | "destructor_declaration"
            | "operator_declaration"
            | "conversion_operator_declaration"
            | "indexer_declaration"
            | "property_declaration"
            | "event_declaration"
            | "local_function_statement" => {
                let has_body = {
                    let mut cursor = node.walk();
                    node.named_children(&mut cursor)
                        .any(|child| match child.kind() {
                            "block" | "arrow_expression_clause" => true,
                            "accessor_list" => {
                                let mut inner = child.walk();
                                child
                                    .named_children(&mut inner)
                                    .any(|a| a.child_by_field_name("body").is_some())
                            }
                            _ => false,
                        })
                };
                if has_body
                    && let Some(declaration) = context
                        .enclosing
                        .and_then(|i| self.facts.declarations.get_mut(i))
                {
                    declaration.bodies = true;
                }
                Self::push_children(node, stack, |field, child| match (field, child) {
                    (Some("name"), _) => None,
                    (_, "attribute_list") => Some(Context {
                        position: Position::Attribute,
                        ..context
                    }),
                    (Some("returns" | "type" | "parameters"), _)
                    | (
                        _,
                        "parameter_list"
                        | "bracketed_parameter_list"
                        | "type_parameter_constraints_clause"
                        | "explicit_interface_specifier",
                    ) => Some(Context {
                        position: Position::Signature,
                        ..context
                    }),
                    // A property's initializer runs in the constructors.
                    (Some("value"), kind) if kind != "arrow_expression_clause" => Some(Context {
                        position: Position::Body,
                        ..context
                    }),
                    _ => Some(Context {
                        position: Position::Body,
                        in_body: true,
                        ..context
                    }),
                });
            }
            "attribute" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let arguments = self.record(
                        name,
                        Context {
                            position: Position::Attribute,
                            ..context
                        },
                    );
                    Self::push_arguments(stack, arguments, context);
                }
                Self::push_children(node, stack, |field, _| match field {
                    Some("name") => None,
                    _ => Some(Context {
                        position: Position::Body,
                        ..context
                    }),
                });
            }
            "typeof_expression" => {
                Self::push_children(node, stack, |_, _| {
                    Some(Context {
                        position: Position::TypeOf,
                        ..context
                    })
                });
            }
            "type_argument_list" => {
                Self::push_children(node, stack, |_, _| {
                    Some(Context {
                        position: Position::GenericArgument,
                        ..context
                    })
                });
            }
            "invocation_expression" => {
                let function = node.child_by_field_name("function");
                let skip_all =
                    function.is_some_and(|f| f.kind() == "identifier" && self.text(f) == "nameof");
                if skip_all {
                    return;
                }
                // `value.Name(...)` and `value?.Name(...)`: perhaps an extension method.
                let called = function.and_then(|f| match f.kind() {
                    "member_access_expression" => f.child_by_field_name("name"),
                    "conditional_access_expression" => {
                        let mut cursor = f.walk();
                        let binding = f
                            .named_children(&mut cursor)
                            .find(|c| c.kind() == "member_binding_expression");
                        binding.and_then(|b| b.child_by_field_name("name"))
                    }
                    _ => None,
                });
                if let Some(name) = called {
                    self.call(name, node, context);
                }
                Self::push_children(node, stack, |field, child| match (field, child) {
                    // A method called by its bare name is a method, never a type.
                    (Some("function"), "identifier" | "generic_name") => None,
                    _ => Some(context),
                });
                if let Some(f) = function
                    && f.kind() == "generic_name"
                {
                    // `Method<T>()`: the name is a method's, the arguments are types.
                    let mut arguments = Vec::new();
                    self.simple(f, &mut arguments);
                    Self::push_arguments(stack, arguments, context);
                }
            }
            "member_access_expression" => {
                if Self::is_name_chain(node) {
                    let arguments = self.record(node, context);
                    Self::push_arguments(stack, arguments, context);
                } else {
                    // `value.Member`: only the value can hold a type name, and the member's
                    // type arguments (`value.Add<T>()`).
                    Self::push_children(node, stack, |field, _| match field {
                        Some("name") => None,
                        _ => Some(context),
                    });
                    self.member_type_arguments(node, stack, context);
                }
            }
            // `?.Member`: only the member's type arguments (`value?.Add<T>()`).
            "member_binding_expression" => self.member_type_arguments(node, stack, context),
            "qualified_name" | "alias_qualified_name" | "generic_name" => {
                let arguments = self.record(node, context);
                Self::push_arguments(stack, arguments, context);
            }
            "identifier" => {
                if field == Some("name") && DECLARING.contains(&parent) {
                    return;
                }
                // `x = ...` and an initializer's `Member = ...`: a variable or a member.
                if field == Some("left") && parent == "assignment_expression" {
                    return;
                }
                if parent == "lambda_expression" || parent == "parenthesized_lambda_expression" {
                    return;
                }
                let text = self.text(node);
                if matches!(text, "var" | "dynamic" | "nint" | "nuint" | "_") {
                    return;
                }
                self.record(node, context);
            }
            "lambda_expression" => {
                Self::push_children(node, stack, |field, child| match (field, child) {
                    (Some("parameters"), "identifier") => None,
                    _ => Some(context),
                });
            }
            _ => Self::push_children(node, stack, |_, _| Some(context)),
        }
    }
}

/// A 0-based tree-sitter row or column as a 1-based line or column.
fn line(zero_based: usize) -> u32 {
    u32::try_from(zero_based.saturating_add(1)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A declaration as the first test compares it.
    type Row = (String, Vec<(String, u32)>, String, u32, TypeKind, bool);

    fn parse(text: &str) -> Result<FileFacts, ParseError> {
        CSharpParser::new().and_then(|mut p| p.parse(text))
    }

    fn names(facts: &FileFacts) -> Vec<String> {
        facts
            .references
            .iter()
            .map(|r| {
                r.segments
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(".")
            })
            .collect()
    }

    #[test]
    fn declarations_carry_namespace_nesting_arity_and_constructors() -> Result<(), ParseError> {
        let facts = parse(
            "namespace A.B { namespace C { public partial class Outer<T> { public Outer() {} class Inner {} } } }\nnamespace D; record R(int X); interface I {} enum E { X } delegate void Fn(int a);",
        )?;
        let found: Vec<Row> = facts
            .declarations
            .iter()
            .map(|d| {
                (
                    d.namespace.clone(),
                    d.outer.clone(),
                    d.name.clone(),
                    d.arity,
                    d.kind,
                    d.constructor,
                )
            })
            .collect();
        assert_eq!(
            found,
            vec![
                (
                    "A.B.C".into(),
                    vec![],
                    "Outer".into(),
                    1,
                    TypeKind::Class,
                    true
                ),
                (
                    "A.B.C".into(),
                    vec![("Outer".into(), 1)],
                    "Inner".into(),
                    0,
                    TypeKind::Class,
                    false
                ),
                ("D".into(), vec![], "R".into(), 0, TypeKind::Class, true),
                (
                    "D".into(),
                    vec![],
                    "I".into(),
                    0,
                    TypeKind::Interface,
                    false
                ),
                ("D".into(), vec![], "E".into(), 0, TypeKind::Enum, false),
                (
                    "D".into(),
                    vec![],
                    "Fn".into(),
                    0,
                    TypeKind::Delegate,
                    false
                ),
            ]
        );
        Ok(())
    }

    #[test]
    fn extension_methods_and_calls_on_values_are_recorded() -> Result<(), ParseError> {
        let facts = parse(
            "public static class E { public static int Twice(this int x) => x; public static void Other(int y) {} }\nclass U { void M(C c) { c.Twice(); c?.Twice(); c.Chain().Again<int>(); Local(); } }",
        )?;
        assert_eq!(facts.declarations[0].extensions, vec!["Twice"]);
        let block = parse(
            "public static class B { extension(int value) { public int Thrice() => value; } }",
        )?;
        assert_eq!(block.declarations[0].extensions, vec!["Thrice"]);
        assert!(
            !block.declarations[0].constructor,
            "an extension block is no constructor"
        );
        let called: Vec<&str> = facts.calls.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(called, ["Again", "Chain", "Twice"]);
        Ok(())
    }

    #[test]
    fn constants_and_enum_members_are_recorded() -> Result<(), ParseError> {
        let facts = parse(
            "class C { public const string A = \"x\"; static readonly int B = 1; const int D = 2, E = 3; } enum Tier { Basic, Gold = 2 }",
        )?;
        assert_eq!(facts.declarations[0].constants, vec!["A", "D", "E"]);
        assert_eq!(facts.declarations[1].constants, vec!["Basic", "Gold"]);
        Ok(())
    }

    #[test]
    fn usings_belong_to_the_declaration_they_are_written_in() -> Result<(), ParseError> {
        let facts = parse(
            "global using G.H;\nusing static S.T;\nusing Alias = X.Y<int>;\nusing Top;\nnamespace N { using Inner; class C {} }",
        )?;
        let top: Vec<(String, Option<String>, bool, bool)> = facts.scopes[0]
            .usings
            .iter()
            .map(|u| (u.target.join("."), u.alias.clone(), u.is_static, u.global))
            .collect();
        assert_eq!(
            top,
            vec![
                ("G.H".into(), None, false, true),
                ("S.T".into(), None, true, false),
                ("X.Y".into(), Some("Alias".into()), false, false),
                ("Top".into(), None, false, false),
            ]
        );
        assert_eq!(facts.scopes[1].namespace, "N");
        assert_eq!(facts.scopes[1].parent, Some(0));
        assert_eq!(facts.scopes[1].usings[0].target, vec!["Inner".to_owned()]);
        Ok(())
    }

    #[test]
    fn file_scoped_namespaces_take_the_usings_after_them() -> Result<(), ParseError> {
        let facts = parse("using Before;\nnamespace F.G;\nusing After;\nclass C {}")?;
        assert_eq!(facts.scopes[0].usings[0].target, vec!["Before".to_owned()]);
        assert_eq!(facts.scopes[1].namespace, "F.G");
        assert_eq!(facts.scopes[1].usings[0].target, vec!["After".to_owned()]);
        assert_eq!(facts.declarations[0].namespace, "F.G");
        Ok(())
    }

    #[test]
    fn names_are_recorded_by_position_and_declarations_are_not() -> Result<(), ParseError> {
        let facts = parse(
            "namespace N;\n[Audit(\"x\", Level = 2)]\nclass C : Base, IThing {\n  private Field f;\n  public Ret M(Param p) { var x = new Made(); Helper.Run(typeof(Seen)); return nameof(Hidden); }\n  public List<Arg> P { get; }\n}",
        )?;
        let by_kind: BTreeSet<(String, Position)> = facts
            .references
            .iter()
            .map(|r| {
                (
                    r.segments
                        .iter()
                        .map(|(n, _)| n.as_str())
                        .collect::<Vec<_>>()
                        .join("."),
                    r.kind,
                )
            })
            .collect();
        for expected in [
            ("Audit", Position::Attribute),
            ("Base", Position::Base),
            ("IThing", Position::Base),
            ("Field", Position::Field),
            ("Ret", Position::Signature),
            ("Param", Position::Signature),
            ("Made", Position::Body),
            ("Helper.Run", Position::Body),
            ("Seen", Position::TypeOf),
            ("List", Position::Signature),
            ("Arg", Position::GenericArgument),
        ] {
            assert!(
                by_kind.contains(&(expected.0.to_owned(), expected.1)),
                "{expected:?} missing from {by_kind:?}"
            );
        }
        let all = names(&facts);
        for absent in ["C", "M", "P", "f", "p", "x", "Level", "Hidden", "nameof"] {
            assert!(
                !all.iter().any(|n| n == absent),
                "{absent} recorded: {all:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn generic_arity_and_alias_qualifiers_are_kept() -> Result<(), ParseError> {
        let facts = parse("class C { global::A.B<int, string> x; object y = typeof(D<,>); }")?;
        let ab = facts
            .references
            .iter()
            .find(|r| r.segments.first().is_some_and(|(n, _)| n == "A"));
        assert_eq!(
            ab.map(|r| (r.segments.clone(), r.qualifier.clone())),
            Some((
                vec![("A".into(), 0), ("B".into(), 2)],
                Some("global".into())
            ))
        );
        assert!(
            facts
                .references
                .iter()
                .any(|r| r.segments == vec![("D".to_owned(), 2)])
        );
        Ok(())
    }

    #[test]
    fn a_generic_members_type_arguments_are_names() -> Result<(), ParseError> {
        let facts = parse(
            "class C { void M(S s) { s.Services().AddScoped<IUser, User>(); s?.Get<Thing>(); } }",
        )?;
        let all = names(&facts);
        for expected in ["IUser", "User", "Thing"] {
            assert!(
                all.iter().any(|n| n == expected),
                "{expected} missing from {all:?}"
            );
        }
        assert!(!all.iter().any(|n| n.contains("AddScoped") || n == "Get"));
        Ok(())
    }

    #[test]
    fn a_member_of_a_value_is_not_a_name() -> Result<(), ParseError> {
        let facts = parse("class C { void M() { this.Thing.Other(); Make().Value.Go(); } }")?;
        let all = names(&facts);
        assert!(
            !all.iter()
                .any(|n| n.contains("Thing") || n.contains("Value"))
        );
        Ok(())
    }

    #[test]
    fn top_level_statements_and_syntax_errors_are_seen() -> Result<(), ParseError> {
        let facts =
            parse("using System;\nConsole.WriteLine(Helper.Name);\nclass Broken { void M( }")?;
        assert!(facts.top_level);
        assert!(facts.syntax_errors);
        assert!(names(&facts).iter().any(|n| n == "Helper.Name"));
        Ok(())
    }

    #[test]
    fn deep_nesting_does_not_exhaust_the_stack() -> Result<(), ParseError> {
        let mut body = String::from("class C { string s = \"a\"");
        for _ in 0..20_000 {
            body.push_str(" + \"a\"");
        }
        body.push_str("; }");
        let facts = parse(&body)?;
        assert_eq!(facts.declarations.len(), 1);
        Ok(())
    }

    #[test]
    fn a_reference_is_recorded_once_per_position() -> Result<(), ParseError> {
        let facts = parse("class C { void M() { new A(); new A(); } A a; }")?;
        let count = facts
            .references
            .iter()
            .filter(|r| r.segments == vec![("A".to_owned(), 0)])
            .count();
        assert_eq!(count, 2, "{:?}", facts.references);
        Ok(())
    }
}
