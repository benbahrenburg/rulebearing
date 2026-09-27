//! C# source, parsed with `tree-sitter-c-sharp`, reduced to what `import archunit` reads: the
//! using directives, the namespaces, the type declarations with their fields, properties and
//! constructor assignments, and every method's statements as a small expression tree.
//!
//! - Plan: [Wave 2, § 1.7](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#17-decisions-this-wave-must-make)
//!   ("How `import archunit` reads C#": `tree-sitter-c-sharp`, the wave 3 `--mode source` parser),
//!   [Step 11](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f)
//! - Source: [architecture § Technology choices](../../../../../docs/architecture.md#technology-choices)
//! - Requirement: [FR-CLI-04](../../../../../docs/prd.md#fr-cli-04)
//!
//! The tree is parsed, never compiled or run. A construct the importer has no use for becomes
//! [`Expr::Other`] with its source text, so a chain that reaches one is reported with that text
//! rather than guessed at. A file with syntax errors is still read: tree-sitter recovers, and
//! only the declarations and statements it recognised are kept.

use std::fmt::Write as _;

use tree_sitter::{Node, Parser};

use super::ImportError;

/// A type as written: its dotted segments with their generic arity, and whether it was
/// `global::`-qualified.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypeName {
    /// Each segment and its generic arity (`List<>` is `("List", 1)`).
    pub segments: Vec<(String, usize)>,
    /// Written `global::A.B`.
    pub global: bool,
    /// The source text.
    pub text: String,
}

/// One argument: an optional name (`name: value`) and the value.
#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    /// The argument name, for a named argument.
    pub name: Option<String>,
    /// The value.
    pub value: Expr,
}

/// An expression, reduced.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A string literal, decoded.
    Str(String),
    /// `true` or `false`.
    Bool(bool),
    /// A number, as written.
    Num(String),
    /// `null`.
    Null,
    /// An identifier (`this` included).
    Name(String),
    /// `receiver.name`.
    Member(Box<Expr>, String),
    /// `function(arguments)`.
    Call(Box<Expr>, Vec<Arg>),
    /// `typeof(T)`.
    TypeOf(TypeName),
    /// `new T(arguments) { initializer }`, `new T[] { ... }`.
    New(Option<TypeName>, Vec<Arg>, Vec<Expr>),
    /// `new[] { ... }`, `[ ... ]`, `{ ... }`.
    Array(Vec<Expr>),
    /// `(a, b)`.
    Tuple(Vec<Arg>),
    /// `left op right`.
    Binary(Box<Expr>, String, Box<Expr>),
    /// `!operand`.
    Not(Box<Expr>),
    /// `expr is pattern`, the pattern as text.
    Is(Box<Expr>, String),
    /// `$"..."`: literal text and interpolated expressions.
    Interpolated(Vec<Part>),
    /// `parameters => body`, an expression body only; the parameters as written.
    Lambda(String, Box<Expr>),
    /// A generic method's name with its type arguments, as the function of a call
    /// (`namespaceof<PublicClass>()`).
    Generic(String, Vec<TypeName>),
    /// Anything else, as written.
    Other(String),
}

/// A piece of an interpolated string.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    /// Literal text.
    Text(String),
    /// `{expression}`.
    Hole(Expr),
}

/// A statement, reduced; nested blocks are flattened in source order.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `var name = value;` (and `Type name = value;`).
    Local {
        /// The variable.
        name: String,
        /// Its value.
        value: Expr,
        /// The 1-based line.
        line: usize,
    },
    /// `target = value;`.
    Assign {
        /// What is assigned.
        target: Expr,
        /// The value.
        value: Expr,
        /// The 1-based line.
        line: usize,
    },
    /// Any other expression statement, a `return`, or an expression body.
    Expr {
        /// The expression.
        expr: Expr,
        /// The 1-based line.
        line: usize,
    },
}

/// A method (or a constructor, a property getter with a body, a local function).
#[derive(Debug, Clone, PartialEq)]
pub struct Method {
    /// The name.
    pub name: String,
    /// The 1-based line of the declaration.
    pub line: usize,
    /// The statements, in order.
    pub body: Vec<Stmt>,
    /// The parameter names, in order.
    pub parameters: Vec<String>,
    /// Each parameter's type as written, where the parser read one.
    pub parameter_types: Vec<Option<TypeName>>,
    /// Each parameter's default value (`int n = 2`), where it has one.
    pub defaults: Vec<Option<Expr>>,
    /// The last parameter is a `params` array.
    pub params: bool,
    /// The method's type parameters (`M<T, U>`), in order.
    pub type_parameters: Vec<String>,
    /// An extension method: the first parameter is `this`, or the method is declared in a C# 14
    /// `extension(T receiver) { ... }` block, whose receiver is prepended as the first parameter.
    pub extension: bool,
    /// Data rows of a parameterised test (`[InlineData]`, `[TestCase]`, `[DataRow]`): each
    /// attribute as written and its argument expressions.
    pub rows: Vec<(String, Vec<Expr>)>,
    /// The method carries a test attribute (`[Fact]`, `[Theory]`, `[Test]`, `[TestCase]`,
    /// `[TestMethod]`, `[DataTestMethod]` ...).
    pub test: bool,
    /// The declaration as written, for a search of what it calls.
    pub text: String,
}

/// Attributes that make a method a test, by their simple name without `Attribute` (xUnit,
/// `NUnit`, `MSTest`).
const TEST_ATTRIBUTES: &[&str] = &[
    "Fact",
    "Theory",
    "SkippableFact",
    "SkippableTheory",
    "Test",
    "TestCase",
    "TestCaseSource",
    "TestMethod",
    "DataTestMethod",
];

/// One parameter as read.
struct Parameter {
    name: String,
    ty: Option<TypeName>,
    default: Option<Expr>,
}

/// A field or property with a value.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    /// The name.
    pub name: String,
    /// The initializer, or the expression body of a property.
    pub value: Expr,
    /// The 1-based line.
    pub line: usize,
}

/// A type declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    /// The simple name.
    pub name: String,
    /// The generic arity.
    pub arity: usize,
    /// The namespace, dotted.
    pub namespace: String,
    /// Enclosing types, outermost first, each with its arity.
    pub outer: Vec<(String, usize)>,
    /// `class`, `struct`, `interface`, `record`, `enum` or `delegate`.
    pub kind: String,
    /// The base list, as written.
    pub bases: Vec<TypeName>,
    /// The primary constructor's parameters (`class C(Fixture fixture)`), with their types.
    pub primary: Vec<(String, Option<TypeName>)>,
    /// Fields and properties with values; constructor assignments to a simple name are added
    /// after the initializers, so the last value written is found last.
    pub members: Vec<Member>,
    /// Methods, constructors included.
    pub methods: Vec<Method>,
    /// Using directives in force (the file's and those of enclosing namespaces).
    pub usings: Usings,
    /// The 1-based line.
    pub line: usize,
}

impl TypeDecl {
    /// The metadata full name: `Ns.Outer+Inner`, generic arity as `` `N ``.
    pub fn full_name(&self) -> String {
        let mut out = self.namespace.clone();
        for (i, (name, arity)) in self
            .outer
            .iter()
            .chain(std::iter::once(&(self.name.clone(), self.arity)))
            .enumerate()
        {
            if i == 0 {
                if !out.is_empty() {
                    out.push('.');
                }
            } else {
                out.push('+');
            }
            out.push_str(name);
            if *arity > 0 {
                out.push('`');
                out.push_str(&arity.to_string());
            }
        }
        out
    }

    /// The C# path: `Ns.Outer.Inner`, with arity, for lookup of written names.
    pub fn dotted(&self) -> String {
        let mut parts: Vec<String> = if self.namespace.is_empty() {
            Vec::new()
        } else {
            vec![self.namespace.clone()]
        };
        for (name, arity) in self
            .outer
            .iter()
            .chain(std::iter::once(&(self.name.clone(), self.arity)))
        {
            parts.push(arity_name(name, *arity));
        }
        parts.join(".")
    }
}

/// `Name` or ``Name`N``.
pub fn arity_name(name: &str, arity: usize) -> String {
    if arity == 0 {
        name.to_owned()
    } else {
        format!("{name}`{arity}")
    }
}

/// Using directives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usings {
    /// `using A.B;`
    pub namespaces: Vec<String>,
    /// `using static A.B;`
    pub statics: Vec<String>,
    /// `using X = A.B;`
    pub aliases: Vec<(String, String)>,
}

impl Usings {
    fn extend(&mut self, other: &Self) {
        self.namespaces.extend(other.namespaces.iter().cloned());
        self.statics.extend(other.statics.iter().cloned());
        self.aliases.extend(other.aliases.iter().cloned());
    }
}

/// One parsed file.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceFile {
    /// The path as shown in comments (relative, forward slashes).
    pub shown: String,
    /// Every type declared, nested ones included.
    pub types: Vec<TypeDecl>,
    /// `global using` directives, which apply to the whole project.
    pub global_usings: Usings,
}

/// The 1-based line of a node.
fn line(node: Node<'_>) -> usize {
    node.start_position().row + 1
}

fn text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or_default()
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

/// Parses `source` (the file shown as `shown`).
///
/// # Errors
/// [`ImportError::Parse`] when tree-sitter produces no tree at all.
pub fn parse(source: &str, shown: &str) -> Result<SourceFile, ImportError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
        .map_err(|e| ImportError::Parse {
            file: shown.to_owned(),
            reason: e.to_string(),
        })?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| ImportError::Parse {
            file: shown.to_owned(),
            reason: "the parser produced no tree".into(),
        })?;
    let bytes = source.as_bytes();
    let mut file = SourceFile {
        shown: shown.to_owned(),
        types: Vec::new(),
        global_usings: Usings::default(),
    };
    let mut reader = Reader {
        source: bytes,
        file: &mut file,
    };
    reader.scope(tree.root_node(), "", &Usings::default(), &[]);
    Ok(file)
}

struct Reader<'a, 'f> {
    source: &'a [u8],
    file: &'f mut SourceFile,
}

impl Reader<'_, '_> {
    fn text(&self, node: Node<'_>) -> String {
        text(node, self.source).to_owned()
    }

    /// A compilation unit, namespace body or type body.
    fn scope(
        &mut self,
        node: Node<'_>,
        namespace: &str,
        inherited: &Usings,
        outer: &[(String, usize)],
    ) {
        let mut usings = inherited.clone();
        let mut namespace = namespace.to_owned();
        for child in named_children(node) {
            match child.kind() {
                "using_directive" => {
                    let (directive, global) = self.using(child);
                    if global {
                        self.file.global_usings.extend(&directive);
                    } else {
                        usings.extend(&directive);
                    }
                }
                "file_scoped_namespace_declaration" => {
                    if let Some(name) = child.child_by_field_name("name") {
                        namespace = join_ns(&namespace, &self.text(name));
                    }
                    // Its own using directives, when the grammar nests them.
                    self.scope(child, &namespace, &usings, outer);
                }
                "namespace_declaration" => {
                    let name = child
                        .child_by_field_name("name")
                        .map(|n| self.text(n))
                        .unwrap_or_default();
                    if let Some(body) = child.child_by_field_name("body") {
                        self.scope(body, &join_ns(&namespace, &name), &usings, outer);
                    }
                }
                "class_declaration"
                | "struct_declaration"
                | "interface_declaration"
                | "record_declaration"
                | "record_struct_declaration"
                | "enum_declaration"
                | "delegate_declaration" => self.type_decl(child, &namespace, &usings, outer),
                "declaration_list" => self.scope(child, &namespace, &usings, outer),
                _ => {}
            }
        }
    }

    fn using(&self, node: Node<'_>) -> (Usings, bool) {
        let mut usings = Usings::default();
        let mut global = false;
        let mut is_static = false;
        for child in children(node) {
            match child.kind() {
                "global" => global = true,
                "static" => is_static = true,
                _ => {}
            }
        }
        let alias = node.child_by_field_name("name").map(|n| self.text(n));
        let target = named_children(node)
            .into_iter()
            .rfind(|c| {
                matches!(
                    c.kind(),
                    "qualified_name" | "identifier" | "generic_name" | "alias_qualified_name"
                ) && Some(c.id()) != node.child_by_field_name("name").map(|n| n.id())
            })
            .map(|n| self.text(n).replace("global::", ""));
        if let Some(target) = target {
            match (alias, is_static) {
                (Some(alias), _) => usings.aliases.push((alias, target)),
                (None, true) => usings.statics.push(target),
                (None, false) => usings.namespaces.push(target),
            }
        }
        (usings, global)
    }

    fn type_decl(
        &mut self,
        node: Node<'_>,
        namespace: &str,
        usings: &Usings,
        outer: &[(String, usize)],
    ) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.text(n))
            .unwrap_or_default();
        let arity = named_children(node)
            .into_iter()
            .find(|c| c.kind() == "type_parameter_list")
            .map_or(0, |list| {
                named_children(list)
                    .iter()
                    .filter(|c| c.kind() == "type_parameter")
                    .count()
            });
        let kind = node
            .kind()
            .trim_end_matches("_declaration")
            .replace("record_struct", "record")
            .clone();
        let bases = named_children(node)
            .into_iter()
            .filter(|c| c.kind() == "base_list")
            .flat_map(named_children)
            .filter_map(|b| self.type_name(b))
            .collect();
        let primary = named_children(node)
            .into_iter()
            .find(|c| c.kind() == "parameter_list")
            .map(|list| {
                self.parameters(list)
                    .0
                    .into_iter()
                    .map(|p| (p.name, p.ty))
                    .collect()
            })
            .unwrap_or_default();
        let mut decl = TypeDecl {
            name: name.clone(),
            arity,
            namespace: namespace.to_owned(),
            outer: outer.to_vec(),
            kind,
            bases,
            primary,
            members: Vec::new(),
            methods: Vec::new(),
            usings: usings.clone(),
            line: line(node),
        };
        let mut nested = outer.to_vec();
        nested.push((name, arity));
        let mut assignments = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            for member in named_children(body) {
                match member.kind() {
                    "field_declaration" | "event_field_declaration" => {
                        for declaration in named_children(member)
                            .into_iter()
                            .filter(|c| c.kind() == "variable_declaration")
                        {
                            for (name, value, at) in self.declarators(declaration) {
                                decl.members.push(Member {
                                    name,
                                    value,
                                    line: at,
                                });
                            }
                        }
                    }
                    "property_declaration" => decl.members.extend(self.property(member)),
                    "constructor_declaration" if self.extension_block(member, &decl.name) => {
                        decl.methods.extend(self.extension_members(member));
                    }
                    "method_declaration"
                    | "constructor_declaration"
                    | "local_function_statement" => {
                        let method = self.method(member);
                        if member.kind() == "constructor_declaration" {
                            assignments.extend(constructor_assignments(&method));
                        }
                        decl.methods.push(method);
                    }
                    "class_declaration"
                    | "struct_declaration"
                    | "interface_declaration"
                    | "record_declaration"
                    | "record_struct_declaration"
                    | "enum_declaration"
                    | "delegate_declaration" => {
                        self.type_decl(member, namespace, usings, &nested);
                    }
                    _ => {}
                }
            }
        }
        decl.members.extend(assignments);
        self.file.types.push(decl);
    }

    /// A property with an initializer or an expression body.
    fn property(&self, member: Node<'_>) -> Option<Member> {
        let name = member
            .child_by_field_name("name")
            .map(|n| self.text(n))
            .unwrap_or_default();
        let value = member
            .child_by_field_name("value")
            .map(|v| {
                if v.kind() == "arrow_expression_clause" {
                    v.named_child(0).unwrap_or(v)
                } else {
                    v
                }
            })
            .or_else(|| {
                named_children(member)
                    .into_iter()
                    .find(|c| c.kind() == "arrow_expression_clause")
                    .and_then(|c| c.named_child(0))
            })
            .map(|v| self.expr(v))?;
        Some(Member {
            name,
            value,
            line: line(member),
        })
    }

    fn declarators(&self, declaration: Node<'_>) -> Vec<(String, Expr, usize)> {
        named_children(declaration)
            .into_iter()
            .filter(|c| c.kind() == "variable_declarator")
            .filter_map(|d| {
                let name = d.child_by_field_name("name").map(|n| self.text(n))?;
                let value = named_children(d)
                    .into_iter()
                    .skip(1)
                    .rfind(|c| c.kind() != "bracketed_argument_list")
                    .map(|v| {
                        if v.kind() == "equals_value_clause" {
                            v.named_child(0)
                                .map_or(Expr::Other(String::new()), |e| self.expr(e))
                        } else {
                            self.expr(v)
                        }
                    })?;
                Some((name, value, line(d)))
            })
            .collect()
    }

    /// Whether a constructor declaration is a C# 14 `extension(T receiver) { ... }` block, which
    /// `tree-sitter-c-sharp` 0.23 reads as a constructor named `extension` holding local
    /// functions. A real constructor is named after its class.
    fn extension_block(&self, node: Node<'_>, class: &str) -> bool {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.text(n))
            .unwrap_or_default();
        name == "extension" && class != "extension"
    }

    /// The members of an extension block, each an extension method whose first parameter is the
    /// block's receiver.
    fn extension_members(&self, node: Node<'_>) -> Vec<Method> {
        let (receiver, _) = node
            .child_by_field_name("parameters")
            .map(|list| self.parameters(list))
            .unwrap_or_default();
        let Some(receiver) = receiver.into_iter().next() else {
            return Vec::new();
        };
        node.child_by_field_name("body")
            .map(named_children)
            .unwrap_or_default()
            .into_iter()
            .filter(|c| c.kind() == "local_function_statement")
            .map(|f| {
                let mut method = self.method(f);
                method.parameters.insert(0, receiver.name.clone());
                method.parameter_types.insert(0, receiver.ty.clone());
                method.defaults.insert(0, None);
                method.extension = true;
                method
            })
            .collect()
    }

    /// A parameter list: each parameter, and whether the last is a `params` array. The grammar
    /// writes a `params T[] name` parameter as `type` and `name` fields of the list itself.
    fn parameters(&self, list: Node<'_>) -> (Vec<Parameter>, bool) {
        let mut out: Vec<Parameter> = named_children(list)
            .into_iter()
            .filter(|p| p.kind() == "parameter")
            .filter_map(|p| {
                let name_node = p.child_by_field_name("name")?;
                let ty = p
                    .child_by_field_name("type")
                    .and_then(|t| self.type_name(t));
                let default = named_children(p)
                    .into_iter()
                    .filter(|c| c.start_byte() > name_node.end_byte())
                    .find(|c| c.kind() != "attribute_list")
                    .map(|c| {
                        if c.kind() == "equals_value_clause" {
                            c.named_child(0)
                                .map_or(Expr::Other(self.text(c)), |e| self.expr(e))
                        } else {
                            self.expr(c)
                        }
                    });
                Some(Parameter {
                    name: self.text(name_node),
                    ty,
                    default,
                })
            })
            .collect();
        let params = match list.child_by_field_name("name") {
            Some(name) => {
                out.push(Parameter {
                    name: self.text(name),
                    ty: list
                        .child_by_field_name("type")
                        .and_then(|t| self.type_name(t)),
                    default: None,
                });
                true
            }
            None => named_children(list)
                .last()
                .is_some_and(|p| children(*p).iter().any(|c| c.kind() == "params")),
        };
        (out, params)
    }

    fn method(&self, node: Node<'_>) -> Method {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.text(n))
            .unwrap_or_default();
        let mut body = Vec::new();
        // An expression body is the `body` field in some grammar versions and a plain child in
        // others; either way it is read once.
        let block = node.child_by_field_name("body").or_else(|| {
            named_children(node)
                .into_iter()
                .find(|c| c.kind() == "arrow_expression_clause")
        });
        match block {
            Some(arrow) if arrow.kind() == "arrow_expression_clause" => {
                if let Some(e) = arrow.named_child(0) {
                    body.push(Stmt::Expr {
                        expr: self.expr(e),
                        line: line(e),
                    });
                }
            }
            Some(block) => self.statements(block, &mut body),
            None => {}
        }
        let list = node.child_by_field_name("parameters");
        let extension = list
            .and_then(|l| {
                named_children(l)
                    .into_iter()
                    .find(|p| p.kind() == "parameter")
            })
            .is_some_and(|p| {
                named_children(p)
                    .iter()
                    .any(|m| m.kind() == "modifier" && text(*m, self.source) == "this")
            });
        let (parameters, params) = list.map(|l| self.parameters(l)).unwrap_or_default();
        let type_parameters = node
            .child_by_field_name("type_parameters")
            .map(|list| {
                named_children(list)
                    .into_iter()
                    .filter_map(|t| t.child_by_field_name("name").map(|n| self.text(n)))
                    .collect()
            })
            .unwrap_or_default();
        let attributes = self.attributes(node);
        Method {
            name,
            line: line(node),
            body,
            parameter_types: parameters.iter().map(|p| p.ty.clone()).collect(),
            defaults: parameters.iter().map(|p| p.default.clone()).collect(),
            parameters: parameters.into_iter().map(|p| p.name).collect(),
            params,
            type_parameters,
            extension,
            rows: self.rows(node),
            test: attributes
                .iter()
                .any(|a| TEST_ATTRIBUTES.contains(&a.as_str())),
            text: self.text(node),
        }
    }

    /// The simple names of a declaration's attributes, without the `Attribute` suffix.
    fn attributes(&self, node: Node<'_>) -> Vec<String> {
        named_children(node)
            .into_iter()
            .filter(|c| c.kind() == "attribute_list")
            .flat_map(named_children)
            .filter(|a| a.kind() == "attribute")
            .filter_map(|a| a.child_by_field_name("name"))
            .map(|n| {
                let name = text(n, self.source);
                let name = name.rsplit('.').next().unwrap_or(name);
                let name = name.split('<').next().unwrap_or(name);
                name.strip_suffix("Attribute").unwrap_or(name).to_owned()
            })
            .collect()
    }

    /// The data rows of a parameterised test method.
    fn rows(&self, method: Node<'_>) -> Vec<(String, Vec<Expr>)> {
        named_children(method)
            .into_iter()
            .filter(|c| c.kind() == "attribute_list")
            .flat_map(named_children)
            .filter(|a| a.kind() == "attribute")
            .filter(|a| {
                a.child_by_field_name("name").is_some_and(|n| {
                    let name = text(n, self.source);
                    let name = name.rsplit('.').next().unwrap_or(name);
                    matches!(
                        name.trim_end_matches("Attribute"),
                        "InlineData" | "TestCase" | "DataRow"
                    )
                })
            })
            .map(|a| {
                let values = named_children(a)
                    .into_iter()
                    .filter(|c| c.kind() == "attribute_argument_list")
                    .flat_map(named_children)
                    .filter(|arg| arg.kind() == "attribute_argument")
                    .filter_map(|arg| named_children(arg).last().map(|e| self.expr(*e)))
                    .collect();
                (format!("[{}]", self.text(a)), values)
            })
            .collect()
    }

    /// Flattens a statement (a block, an `if`, a `using`, a loop, a `try`) into `out`.
    fn statements(&self, node: Node<'_>, out: &mut Vec<Stmt>) {
        match node.kind() {
            "local_declaration_statement" | "using_statement" | "variable_declaration" => {
                for child in named_children(node) {
                    if child.kind() == "variable_declaration" {
                        for (name, value, at) in self.declarators(child) {
                            out.push(Stmt::Local {
                                name,
                                value,
                                line: at,
                            });
                        }
                    } else if node.kind() == "using_statement" {
                        self.statements(child, out);
                    }
                }
                if node.kind() == "variable_declaration" {
                    for (name, value, at) in self.declarators(node) {
                        out.push(Stmt::Local {
                            name,
                            value,
                            line: at,
                        });
                    }
                }
            }
            "expression_statement" => {
                if let Some(e) = node.named_child(0) {
                    if e.kind() == "assignment_expression"
                        && e.child_by_field_name("operator")
                            .is_some_and(|o| text(o, self.source) == "=")
                        && let (Some(left), Some(right)) = (
                            e.child_by_field_name("left"),
                            e.child_by_field_name("right"),
                        )
                    {
                        out.push(Stmt::Assign {
                            target: self.expr(left),
                            value: self.expr(right),
                            line: line(node),
                        });
                    } else {
                        out.push(Stmt::Expr {
                            expr: self.expr(e),
                            line: line(node),
                        });
                    }
                }
            }
            "return_statement" | "throw_statement" | "yield_statement" => {
                if let Some(e) = node.named_child(0) {
                    out.push(Stmt::Expr {
                        expr: self.expr(e),
                        line: line(node),
                    });
                }
            }
            "local_function_statement" => {}
            _ => {
                for child in named_children(node) {
                    let kind = child.kind();
                    if kind == "block"
                        || kind.ends_with("_statement")
                        || kind == "else_clause"
                        || kind == "catch_clause"
                        || kind == "finally_clause"
                    {
                        self.statements(child, out);
                    }
                }
            }
        }
    }

    /// A type as written.
    fn type_name(&self, node: Node<'_>) -> Option<TypeName> {
        let whole = self.text(node);
        let mut segments = Vec::new();
        let mut global = false;
        self.type_segments(node, &mut segments, &mut global)?;
        Some(TypeName {
            segments,
            global,
            text: whole,
        })
    }

    fn type_segments(
        &self,
        node: Node<'_>,
        out: &mut Vec<(String, usize)>,
        global: &mut bool,
    ) -> Option<()> {
        match node.kind() {
            "identifier" | "predefined_type" => out.push((self.text(node), 0)),
            "generic_name" => {
                let name = node.named_child(0).map(|n| self.text(n))?;
                let arity = named_children(node)
                    .into_iter()
                    .find(|c| c.kind() == "type_argument_list")
                    .map_or(0, |list| {
                        let named = named_children(list).len();
                        if named > 0 {
                            named
                        } else {
                            children(list).iter().filter(|c| c.kind() == ",").count() + 1
                        }
                    });
                out.push((name, arity));
            }
            "qualified_name" => {
                for part in named_children(node) {
                    self.type_segments(part, out, global)?;
                }
            }
            "alias_qualified_name" => {
                let parts = named_children(node);
                if parts.first().is_some_and(|p| self.text(*p) == "global") {
                    *global = true;
                    for part in parts.iter().skip(1) {
                        self.type_segments(*part, out, global)?;
                    }
                } else {
                    return None;
                }
            }
            "nullable_type" => {
                let inner = node.named_child(0)?;
                self.type_segments(inner, out, global)?;
            }
            _ => return None,
        }
        Some(())
    }

    fn args(&self, node: Node<'_>) -> Vec<Arg> {
        named_children(node)
            .into_iter()
            .filter(|c| c.kind() == "argument")
            .map(|a| {
                let name = a.child_by_field_name("name").map(|n| self.text(n));
                let value = named_children(a)
                    .into_iter()
                    .rfind(|c| Some(c.id()) != a.child_by_field_name("name").map(|n| n.id()))
                    .map_or(Expr::Other(String::new()), |v| self.expr(v));
                Arg { name, value }
            })
            .collect()
    }

    fn elements(&self, node: Node<'_>) -> Vec<Expr> {
        let mut out = Vec::new();
        for child in named_children(node) {
            match child.kind() {
                "initializer_expression" | "collection_element" | "expression_element" => {
                    out.extend(self.elements(child));
                }
                "spread_element" => out.push(Expr::Other(self.text(child))),
                _ => out.push(self.expr(child)),
            }
        }
        out
    }

    fn string(&self, node: Node<'_>) -> String {
        let mut out = String::new();
        for part in named_children(node) {
            match part.kind() {
                "string_literal_content" => out.push_str(&self.text(part)),
                "escape_sequence" => out.push_str(&unescape(&self.text(part))),
                _ => {}
            }
        }
        out
    }

    /// An expression.
    fn expr(&self, node: Node<'_>) -> Expr {
        if let Some(e) = self.literal(node).or_else(|| self.operator(node)) {
            return e;
        }
        match node.kind() {
            "identifier" | "predefined_type" => Expr::Name(self.text(node)),
            "this_expression" | "this" => Expr::Name("this".into()),
            "generic_name" => node
                .named_child(0)
                .map_or(Expr::Other(self.text(node)), |n| Expr::Name(self.text(n))),
            "qualified_name" => {
                let parts = named_children(node);
                match (parts.first(), parts.last()) {
                    (Some(first), Some(last)) if parts.len() == 2 => {
                        Expr::Member(Box::new(self.expr(*first)), self.text(*last))
                    }
                    _ => Expr::Other(self.text(node)),
                }
            }
            "member_access_expression" => {
                let receiver = node
                    .child_by_field_name("expression")
                    .map_or(Expr::Other(String::new()), |e| self.expr(e));
                let name = node
                    .child_by_field_name("name")
                    .map_or_else(String::new, |n| {
                        if n.kind() == "generic_name" {
                            n.named_child(0).map(|i| self.text(i)).unwrap_or_default()
                        } else {
                            self.text(n)
                        }
                    });
                Expr::Member(Box::new(receiver), name)
            }
            "invocation_expression" => {
                let function =
                    node.child_by_field_name("function")
                        .map_or(Expr::Other(String::new()), |f| {
                            if f.kind() == "generic_name" {
                                self.generic(f)
                            } else {
                                self.expr(f)
                            }
                        });
                let args = node
                    .child_by_field_name("arguments")
                    .map(|a| self.args(a))
                    .unwrap_or_default();
                Expr::Call(Box::new(function), args)
            }
            "typeof_expression" => node
                .child_by_field_name("type")
                .and_then(|t| self.type_name(t))
                .map_or(Expr::Other(self.text(node)), Expr::TypeOf),
            "object_creation_expression" => Expr::New(
                node.child_by_field_name("type")
                    .and_then(|t| self.type_name(t)),
                node.child_by_field_name("arguments")
                    .map(|a| self.args(a))
                    .unwrap_or_default(),
                node.child_by_field_name("initializer")
                    .map(|i| self.elements(i))
                    .unwrap_or_default(),
            ),
            "implicit_object_creation_expression" => Expr::New(
                None,
                named_children(node)
                    .into_iter()
                    .find(|c| c.kind() == "argument_list")
                    .map(|a| self.args(a))
                    .unwrap_or_default(),
                Vec::new(),
            ),
            "array_creation_expression"
            | "implicit_array_creation_expression"
            | "stackalloc_expression"
            | "implicit_stackalloc_expression" => Expr::Array(
                named_children(node)
                    .into_iter()
                    .filter(|c| c.kind() == "initializer_expression")
                    .flat_map(|i| self.elements(i))
                    .collect(),
            ),
            "collection_expression" | "initializer_expression" => Expr::Array(self.elements(node)),
            "tuple_expression" => Expr::Tuple(self.args(node)),
            _ => Expr::Other(self.text(node)),
        }
    }

    /// A generic name in call position, with its type arguments.
    fn generic(&self, node: Node<'_>) -> Expr {
        let Some(name) = node.named_child(0).map(|n| self.text(n)) else {
            return Expr::Other(self.text(node));
        };
        let arguments = named_children(node)
            .into_iter()
            .find(|c| c.kind() == "type_argument_list")
            .map(named_children)
            .unwrap_or_default();
        let types: Option<Vec<TypeName>> = arguments.iter().map(|t| self.type_name(*t)).collect();
        match types {
            Some(types) if !types.is_empty() => Expr::Generic(name, types),
            _ => Expr::Name(name),
        }
    }

    /// A literal, or `None` for another expression.
    fn literal(&self, node: Node<'_>) -> Option<Expr> {
        Some(match node.kind() {
            "string_literal" => Expr::Str(self.string(node)),
            "verbatim_string_literal" => {
                let raw = self.text(node);
                let inner = raw
                    .strip_prefix('@')
                    .and_then(|r| r.strip_prefix('"'))
                    .and_then(|r| r.strip_suffix('"'))
                    .unwrap_or(&raw);
                Expr::Str(inner.replace("\"\"", "\""))
            }
            "raw_string_literal" => Expr::Str(
                named_children(node)
                    .into_iter()
                    .filter(|c| c.kind() == "raw_string_content")
                    .map(|c| self.text(c))
                    .collect(),
            ),
            "boolean_literal" => Expr::Bool(self.text(node) == "true"),
            "integer_literal" | "real_literal" => Expr::Num(self.text(node)),
            "null_literal" => Expr::Null,
            "interpolated_string_expression" => Expr::Interpolated(
                named_children(node)
                    .into_iter()
                    .filter_map(|c| match c.kind() {
                        "string_content" => Some(Part::Text(self.text(c))),
                        "escape_sequence" => Some(Part::Text(unescape(&self.text(c)))),
                        "interpolation" => Some(
                            named_children(c)
                                .into_iter()
                                .find(|e| e.kind() != "interpolation_brace")
                                .map_or(Part::Text(String::new()), |e| Part::Hole(self.expr(e))),
                        ),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => return None,
        })
    }

    /// An operator, a cast, a lambda or a parenthesis, or `None` for another expression.
    fn operator(&self, node: Node<'_>) -> Option<Expr> {
        Some(match node.kind() {
            "parenthesized_expression" => node
                .named_child(0)
                .map_or(Expr::Other(self.text(node)), |e| self.expr(e)),
            "cast_expression" => node
                .child_by_field_name("value")
                .map_or(Expr::Other(self.text(node)), |e| self.expr(e)),
            "binary_expression" => match (
                node.child_by_field_name("left"),
                node.child_by_field_name("operator"),
                node.child_by_field_name("right"),
            ) {
                (Some(l), Some(op), Some(r)) => Expr::Binary(
                    Box::new(self.expr(l)),
                    self.text(op),
                    Box::new(self.expr(r)),
                ),
                _ => Expr::Other(self.text(node)),
            },
            "prefix_unary_expression" => {
                let op = children(node)
                    .first()
                    .map(|o| self.text(*o))
                    .unwrap_or_default();
                match (op.as_str(), node.named_child(0)) {
                    ("!", Some(e)) => Expr::Not(Box::new(self.expr(e))),
                    _ => Expr::Other(self.text(node)),
                }
            }
            "is_pattern_expression" => {
                let parts = named_children(node);
                match (parts.first(), parts.get(1)) {
                    (Some(e), Some(p)) => Expr::Is(Box::new(self.expr(*e)), self.text(*p)),
                    _ => Expr::Other(self.text(node)),
                }
            }
            "lambda_expression" => match node.child_by_field_name("body") {
                Some(body) if body.kind() != "block" => Expr::Lambda(
                    node.child_by_field_name("parameters")
                        .map(|p| self.text(p))
                        .unwrap_or_default(),
                    Box::new(self.expr(body)),
                ),
                _ => Expr::Other(self.text(node)),
            },
            // `x!`, the null-forgiving operator, is `x`.
            "postfix_unary_expression"
                if children(node).last().is_some_and(|o| self.text(*o) == "!") =>
            {
                node.named_child(0)
                    .map_or(Expr::Other(self.text(node)), |e| self.expr(e))
            }
            "await_expression" | "checked_expression" | "ref_expression" => node
                .named_child(0)
                .map_or(Expr::Other(self.text(node)), |e| self.expr(e)),
            _ => return None,
        })
    }
}

/// A constructor's assignments to a simple name (`X = ...`, `this.X = ...`), as member values.
fn constructor_assignments(method: &Method) -> Vec<Member> {
    method
        .body
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Assign {
                target: Expr::Name(name),
                value,
                line,
            } => Some(Member {
                name: name.clone(),
                value: value.clone(),
                line: *line,
            }),
            Stmt::Assign {
                target: Expr::Member(receiver, name),
                value,
                line,
            } if **receiver == Expr::Name("this".into()) => Some(Member {
                name: name.clone(),
                value: value.clone(),
                line: *line,
            }),
            _ => None,
        })
        .collect()
}

fn join_ns(outer: &str, inner: &str) -> String {
    if outer.is_empty() {
        inner.to_owned()
    } else {
        format!("{outer}.{inner}")
    }
}

/// A C# escape sequence's character.
fn unescape(sequence: &str) -> String {
    match sequence {
        "\\n" => "\n".into(),
        "\\t" => "\t".into(),
        "\\r" => "\r".into(),
        "\\0" => "\0".into(),
        "\\\\" => "\\".into(),
        "\\\"" => "\"".into(),
        "\\'" => "'".into(),
        other => other
            .strip_prefix("\\u")
            .or_else(|| other.strip_prefix("\\x"))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .and_then(char::from_u32)
            .map_or_else(|| other.to_owned(), |c| c.to_string()),
    }
}

/// Renders an expression back to C#, whitespace normalised, for the comment above a rule.
pub fn render(expr: &Expr) -> String {
    match expr {
        Expr::Str(s) => serde_json::to_string(s).unwrap_or_default(),
        Expr::Bool(b) => b.to_string(),
        Expr::Num(n) | Expr::Name(n) => n.clone(),
        Expr::Null => "null".into(),
        Expr::Member(recv, name) => format!("{}.{name}", render(recv)),
        Expr::Call(function, args) => format!("{}({})", render(function), render_args(args)),
        Expr::TypeOf(t) => format!("typeof({})", t.text),
        Expr::New(t, args, init) => {
            let mut out = format!(
                "new {}({})",
                t.as_ref().map_or("", |t| t.text.as_str()),
                render_args(args)
            );
            if !init.is_empty() {
                let _ = write!(
                    out,
                    " {{ {} }}",
                    init.iter().map(render).collect::<Vec<_>>().join(", ")
                );
            }
            out
        }
        Expr::Array(items) => format!(
            "[{}]",
            items.iter().map(render).collect::<Vec<_>>().join(", ")
        ),
        Expr::Tuple(args) => format!("({})", render_args(args)),
        Expr::Binary(l, op, r) => format!("{} {op} {}", render(l), render(r)),
        Expr::Not(e) => format!("!{}", render(e)),
        Expr::Is(e, p) => format!("{} is {p}", render(e)),
        Expr::Interpolated(parts) => {
            let mut out = String::from("$\"");
            for part in parts {
                match part {
                    Part::Text(t) => out.push_str(t),
                    Part::Hole(e) => {
                        let _ = write!(out, "{{{}}}", render(e));
                    }
                }
            }
            out.push('"');
            out
        }
        Expr::Lambda(parameters, body) => format!("{parameters} => {}", render(body)),
        Expr::Generic(name, types) => format!(
            "{name}<{}>",
            types
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Other(text) => text.split_whitespace().collect::<Vec<_>>().join(" "),
    }
}

fn render_args(args: &[Arg]) -> String {
    args.iter()
        .map(|a| match &a.name {
            Some(n) => format!("{n}: {}", render(&a.value)),
            None => render(&a.value),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
global using static ArchUnitNET.Fluent.ArchRuleDefinition;
using System;
using Alias = Foo.Bar;
namespace A.B;
public class T<U> : Base, IThing {
    private const string Ns = "A.B.Domain";
    private static readonly IObjectProvider<IType> L = Types().That().ResideInNamespace(Ns, true).As("L");
    public Class R;
    public static string P => @"say ""hi""";
    public T() { R = Architecture.GetClassOfType(typeof(Outer.Inner)); this.Q = $"{Ns}.X"; }
    [Fact]
    public void M() {
        var r = Classes().That().Are(L).Should().HaveName("a\tb").Because("why");
        if (true) { r.Check(Architecture); } else { Assert.False(!r.HasNoViolations(x) is false); }
        x = new[] { "a" }; y = [1, 2]; z = new List<object> { 1 }; w = nameof(Q);
        Assert.Throws<E>(() => r.Check(a));
    }
    public void N() => Types.InAssembly(typeof(global::Z.Y).Assembly).ShouldNot().HaveDependencyOn("a").GetResult();
    class Nested<K, V> { }
}
enum E { A }
"#;

    #[test]
    fn declarations_usings_and_members_are_read() -> Result<(), ImportError> {
        let file = parse(SAMPLE, "T.cs")?;
        assert_eq!(
            file.global_usings.statics,
            ["ArchUnitNET.Fluent.ArchRuleDefinition"]
        );
        let names: Vec<String> = file.types.iter().map(TypeDecl::full_name).collect();
        assert!(names.contains(&"A.B.T`1".to_owned()), "{names:?}");
        assert!(names.contains(&"A.B.T`1+Nested`2".to_owned()), "{names:?}");
        let t = file
            .types
            .iter()
            .find(|t| t.name == "T")
            .ok_or_else(|| ImportError::Invalid("T".into()))?;
        assert_eq!(t.dotted(), "A.B.T`1");
        assert_eq!(t.usings.namespaces, ["System"]);
        assert_eq!(
            t.usings.aliases,
            [("Alias".to_owned(), "Foo.Bar".to_owned())]
        );
        assert_eq!(t.bases.len(), 2);
        let member_names: Vec<&str> = t.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(member_names, ["Ns", "L", "P", "R", "Q"]);
        assert_eq!(t.members[2].value, Expr::Str("say \"hi\"".into()));
        assert!(matches!(&t.members[4].value, Expr::Interpolated(parts) if parts.len() == 2));
        let m = t
            .methods
            .iter()
            .find(|m| m.name == "M")
            .ok_or_else(|| ImportError::Invalid("M".into()))?;
        assert!(matches!(&m.body[0], Stmt::Local { name, .. } if name == "r"));
        assert!(m.body.iter().any(
            |s| matches!(s, Stmt::Expr { expr, .. } if render(expr) == "r.Check(Architecture)")
        ));
        assert!(m.body.iter().any(|s| matches!(s, Stmt::Expr { expr, .. } if render(expr) == "Assert.False(!r.HasNoViolations(x) is false)")));
        assert!(m.body.iter().any(|s| matches!(s, Stmt::Expr { expr, .. } if render(expr) == "Assert.Throws(() => r.Check(a))")));
        assert!(m.body.iter().any(
            |s| matches!(s, Stmt::Assign { value: Expr::Array(items), .. } if items.len() == 2)
        ));
        let n = t
            .methods
            .iter()
            .find(|m| m.name == "N")
            .ok_or_else(|| ImportError::Invalid("N".into()))?;
        let Stmt::Expr { expr, .. } = &n.body[0] else {
            return Err(ImportError::Invalid("N body".into()));
        };
        assert_eq!(
            render(expr),
            "Types.InAssembly(typeof(global::Z.Y).Assembly).ShouldNot().HaveDependencyOn(\"a\").GetResult()"
        );
        let other = file
            .types
            .iter()
            .find(|t| t.name == "E")
            .ok_or_else(|| ImportError::Invalid("E".into()))?;
        assert_eq!(other.full_name(), "A.B.E");
        assert_eq!(other.kind, "enum");
        let blocks = parse(
            "namespace X { namespace Y { class C {} } } namespace Z.W { interface I {} }",
            "b.cs",
        )?;
        let names: Vec<String> = blocks.types.iter().map(TypeDecl::full_name).collect();
        assert_eq!(names, ["X.Y.C", "Z.W.I"]);
        Ok(())
    }

    #[test]
    fn parameters_fixtures_generics_and_extension_blocks_are_read() -> Result<(), ImportError> {
        let file = parse(
            r"public class P(AFixture fixture, int x = 3) : IClassFixture<AFixture>
            {
                public P(BFixture b, params string[] names) { _b = b; }
                static string M<T, U>(T a, int n = 2, params Type[] more) => typeof(T).Namespace!;
                [Fact] void Q() { var r = namespaceof<PublicClass>(); var s = x.Get<int>(); }
                [Xunit.TheoryAttribute] void R(string a, params int[] b) { }
            }
            public static class E
            {
                extension(ClassesShould should)
                {
                    public ClassesShouldConjunction Sealed(string why) => should.BeSealed();
                }
            }",
            "P.cs",
        )?;
        let class = &file.types[0];
        assert_eq!(class.primary.len(), 2);
        assert_eq!(class.primary[0].0, "fixture");
        assert_eq!(
            class.primary[0].1.as_ref().map(|t| t.text.as_str()),
            Some("AFixture")
        );
        assert_eq!(class.bases[0].text, "IClassFixture<AFixture>");
        let method = |name: &str| {
            class
                .methods
                .iter()
                .find(|candidate| candidate.name == name)
                .ok_or_else(|| ImportError::Invalid(name.into()))
        };
        let constructor = method("P")?;
        assert_eq!(constructor.parameters, ["b", "names"]);
        assert!(constructor.params);
        assert!(!constructor.test);
        let generic = method("M")?;
        assert_eq!(generic.type_parameters, ["T", "U"]);
        assert_eq!(generic.parameters, ["a", "n", "more"]);
        assert_eq!(generic.defaults, [None, Some(Expr::Num("2".into())), None]);
        assert!(generic.params);
        // `!` is the expression it forgives.
        assert!(
            matches!(&generic.body[0], Stmt::Expr { expr, .. } if render(expr) == "typeof(T).Namespace")
        );
        let test = method("Q")?;
        assert!(test.test);
        assert!(test.text.starts_with("[Fact] void Q()"));
        let Stmt::Local { value, .. } = &test.body[0] else {
            return Err(ImportError::Invalid("Q body".into()));
        };
        assert!(
            matches!(value, Expr::Call(f, _) if matches!(f.as_ref(), Expr::Generic(n, t) if n == "namespaceof" && t.len() == 1))
        );
        assert_eq!(render(value), "namespaceof<PublicClass>()");
        let theory = method("R")?;
        assert!(theory.test && theory.params);
        let ext = &file.types[1];
        assert_eq!(ext.methods.len(), 1);
        let sealed = &ext.methods[0];
        assert!(sealed.extension);
        assert_eq!(sealed.parameters, ["should", "why"]);
        assert_eq!(
            sealed.parameter_types[0].as_ref().map(|t| t.text.as_str()),
            Some("ClassesShould")
        );
        Ok(())
    }

    #[test]
    fn type_names_keep_arity_and_global() -> Result<(), ImportError> {
        let file = parse(
            "class C { void M() { var a = typeof(List<>); var b = typeof(global::X.Y); var c = typeof(Dictionary<int, string>); var d = typeof(int?); } }",
            "C.cs",
        )?;
        let types: Vec<TypeName> = file.types[0].methods[0]
            .body
            .iter()
            .filter_map(|s| match s {
                Stmt::Local {
                    value: Expr::TypeOf(t),
                    ..
                } => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(types[0].segments, [("List".to_owned(), 1)]);
        assert!(types[1].global);
        assert_eq!(types[1].segments.len(), 2);
        assert_eq!(types[2].segments, [("Dictionary".to_owned(), 2)]);
        assert_eq!(types[3].segments, [("int".to_owned(), 0)]);
        Ok(())
    }

    #[test]
    fn escapes_and_rendering() {
        assert_eq!(unescape("\\n"), "\n");
        assert_eq!(unescape("\\u0041"), "A");
        assert_eq!(unescape("\\q"), "\\q");
        let e = Expr::Call(
            Box::new(Expr::Member(Box::new(Expr::Name("a".into())), "B".into())),
            vec![
                Arg {
                    name: Some("n".into()),
                    value: Expr::Num("1".into()),
                },
                Arg {
                    name: None,
                    value: Expr::Tuple(vec![Arg {
                        name: None,
                        value: Expr::Null,
                    }]),
                },
            ],
        );
        assert_eq!(render(&e), "a.B(n: 1, (null))");
        assert_eq!(render(&Expr::Other("a\n   b".into())), "a b");
        assert_eq!(
            render(&Expr::New(None, Vec::new(), vec![Expr::Bool(true)])),
            "new () { true }"
        );
    }
}
