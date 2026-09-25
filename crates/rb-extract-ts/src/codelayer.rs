//! The TypeScript and JavaScript code layer: classes, interfaces, enums, type aliases,
//! functions, their members, decorators and calls, read from the parse the dependency walk
//! already made.
//!
//! - Architecture: [Extractors](../../../docs/architecture.md#extractors);
//!   [The graph document](../../../docs/architecture.md#the-graph-document) (the `code` section)
//! - Decisions: [ADR-0012](../../../docs/adr/0012-oxc-for-typescript.md) (one parser, `oxc`);
//!   [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md) (a name that cannot be
//!   resolved is kept as written, a property the language cannot express is absent)
//! - Plan: [Wave 2C](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#wave-2c-element-slice-and-diagram-rules-the-capability-table-gate-2-to-zero)
//!   (row "TypeScript and Python mappings over fixture packages");
//!   [§ 1.4.3](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#143-element-rule-evaluation)
//!   (`arePublic` reads `export`, `haveAnyAttributes` reads decorators)
//! - Source: [design § One engine, three languages](../../../docs/artifacts/design.md#one-engine-three-languages-one-monorepo)
//!   (row "Code layer")
//! - Requirement: [FR-CORE-01](../../../docs/prd.md#fr-core-01)
//!
//! Reading happens in two steps. [`collect`] runs once per file over the program the dependency
//! walk parsed, and records each element with the names it mentions still unresolved. [`link`]
//! then runs once over every file, and resolves each name through the file's own declarations,
//! its imports and the exporting files' `export` lists.
//!
//! | TypeScript or JavaScript | Element |
//! | --- | --- |
//! | `class C` (`abstract` too) | type `class`; `abstract` (TypeScript) |
//! | `interface I extends J` | type `interface`; `J` in `interfaces` and a dependency `inherits` |
//! | `enum E`, `const enum E` | type `enum` |
//! | `type T = ...` | type `type-alias`; a dependency `signature` on each declared type the right side names |
//! | a top-level `function f`; an exported `const f = () => ...` or `function` expression | type `function` |
//! | `static Inner = class {...}` | type `class`, `nested: true`, `nestedIn` the outer class |
//! | a method, `constructor` | member `method`, `constructor` |
//! | a field, a constructor parameter property | member `field` |
//! | `get x` / `set x`, `accessor x` | member `property` with `getter` and `setter`; `readonly` when there is no setter |
//! | `private`, `protected`, `public`, `#name` | member `visibility`; `#name` is `private`, no modifier is `public` |
//! | `static`, `readonly`, `abstract` | the member's `static`, `readonly` (TypeScript), `abstract` (TypeScript) |
//! | `@decorator(args)` on a class or member | an attribute, and a dependency `attribute` when the decorator resolves |
//! | `extends B`, `implements I` | `baseType`, `interfaces`, dependencies `inherits` and `implements` |
//! | type arguments in `extends` or `implements` | dependencies `generic-argument` |
//! | a member's type annotations | `returnType` and `parameterTypes` as written; a dependency `signature` on each declared type named |
//! | `new X()`, `X.m()`, `x.m()` with `x: X` in a body | a dependency `body` on `X` when `X` is a declared type |
//! | `this.m()`, `this.f.m()` with `f: X`, `x.m()` with `x: X` | a call `Type.m` from the calling member |
//!
//! Full names are `<file>#<qualified name>`, the file the module's `source`: `src/a.ts#Widget`,
//! `src/a.ts#Widget.render` for a member, `src/a.ts#Shapes.Circle` for a class in `namespace
//! Shapes`. `namespace` is the file, the TypeScript module identity. A class in a TypeScript
//! `namespace` is not nested: a namespace is not a type (as a C# namespace is not), so its
//! declarations carry the namespace in their full name and `nested: false`. An anonymous
//! `export default class` or `function` is named `default`.
//!
//! Visibility of a type: `public` when the module exports it (`export`, `export default`,
//! `export { X }` or `export { Y as X }` in the same file, `export =`, or CommonJS
//! `module.exports = X`, `module.exports = { X }` and `exports.Y = X`); a declaration in a
//! `namespace` is `public` when it is exported from its namespace and every enclosing namespace
//! is public; anything else is `internal`, visible only inside its module.
//!
//! Absent, because the language has no such property: `sealed`, `record`, `valueType`,
//! `assemblyQualifiedName`, `attribution`, `static` on a type, `virtual` on a member; and in
//! JavaScript also `abstract`, `generic`, `immutable` and a field's `readonly`.
//!
//! A name resolves when it is declared in the file, or imported from a module the extractor
//! resolved to an extracted file that declares or re-exports it (through `export { X } from`,
//! `export *` and `export * as ns`), and names a type of the layer. Anything else keeps the text
//! as written in `baseType`, `interfaces` and `attributeType`, and forms no dependency and no
//! call.

use std::collections::{BTreeMap, BTreeSet};

use oxc_ast::ast::{
    Argument, AssignmentOperator, AssignmentTarget, BindingPattern, CatchClause, Class,
    ClassElement, Declaration, Decorator, Expression, ForInStatement, ForOfStatement, ForStatement,
    FormalParameters, Function, ImportDeclarationSpecifier, MethodDefinitionKind,
    ObjectPropertyKind, Program, PropertyKey, Statement, TSAccessibility, TSInterfaceDeclaration,
    TSMethodSignatureKind, TSModuleReference, TSNamespaceDeclaration, TSNamespaceDeclarationBody,
    TSSignature, TSType, TSTypeAnnotation, TSTypeName, TSTypeParameterDeclaration,
    TSTypeParameterInstantiation, VariableDeclarationKind,
};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::ScopeFlags;
use oxc_span::{GetSpan, Span};
use rb_model::{
    Accessor, AttributeElement, CallElement, CodeLayer, ElementDependency, Language, Location,
    MemberElement, TypeElement,
};

use crate::pipeline::Lines;

/// What an import binds.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Imported {
    /// One export by name; a default import is the export `default`.
    Named(String),
    /// `import * as ns`: the module's exports as members.
    Namespace,
    /// `require("x")` or `import x = require("x")`: the module object, whose members are its
    /// exports and which is its `module.exports = X`, the export `default`, itself.
    Module,
}

/// Where a name points, as its file sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// Declared in this file: the qualified name, `Outer.Inner` for a namespace member.
    Local(String),
    /// Bound by an import, then any `.member` segments after the binding.
    Imported {
        specifier: String,
        imported: Imported,
        rest: Vec<String>,
    },
    /// Neither: a global, a type parameter, an expression.
    Unbound,
}

/// A name written in the source, before linking.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Reference {
    target: Target,
    written: String,
    line: u32,
}

/// One entry of a module's export list.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Export {
    /// `export class X`, `export { X as Y }`, `export default X`: a name the file declares.
    Local { exported: String, local: String },
    /// `export { X as Y } from "m"`, `export * as ns from "m"`, or an imported binding
    /// exported again.
    From {
        exported: String,
        specifier: String,
        imported: Imported,
    },
    /// `export * from "m"`.
    All { specifier: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingDependency {
    target: Reference,
    kind: &'static str,
    member: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingType {
    element: TypeElement,
    base: Option<Reference>,
    interfaces: Vec<Reference>,
    dependencies: Vec<PendingDependency>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingMember {
    element: MemberElement,
    dependencies: Vec<PendingDependency>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingAttribute {
    element: AttributeElement,
    attribute: Reference,
}

/// The object a method is called on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Receiver {
    /// `this`, inside the type with this full name.
    This(String),
    /// A value whose declared type, or a type itself (a static call), is named.
    Typed(Reference),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingCall {
    from: String,
    receiver: Receiver,
    method: String,
    location: Location,
}

/// One file's code layer, with its names not yet resolved against the other files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileCode {
    file: String,
    types: Vec<PendingType>,
    members: Vec<PendingMember>,
    attributes: Vec<PendingAttribute>,
    calls: Vec<PendingCall>,
    exports: Vec<Export>,
    /// The specifiers a name or a re-export comes through.
    specifiers: BTreeSet<String>,
    /// Specifier to the extracted file it resolves to.
    resolved: BTreeMap<String, String>,
}

impl FileCode {
    /// The file, as the module's `source`.
    pub fn file(&self) -> &str {
        &self.file
    }

    /// The module specifiers this file's names are imported or re-exported through, each once.
    pub fn specifiers(&self) -> Vec<String> {
        self.specifiers.iter().cloned().collect()
    }

    /// Records that `specifier` resolves to the extracted file `resolved`.
    pub fn set_resolved(&mut self, specifier: String, resolved: String) {
        self.resolved.insert(specifier, resolved);
    }

    /// How many types, members, attributes and calls the file declares.
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        (
            self.types.len(),
            self.members.len(),
            self.attributes.len(),
            self.calls.len(),
        )
    }
}

/// `public` for a type its module exports, `internal` for one only its module sees.
pub fn type_visibility(exported: bool) -> &'static str {
    if exported { "public" } else { "internal" }
}

/// A member's visibility: its modifier, `private` for a `#name`, else `public`.
pub fn member_visibility(
    accessibility: Option<TSAccessibility>,
    hash_private: bool,
) -> &'static str {
    match accessibility {
        _ if hash_private => "private",
        Some(TSAccessibility::Private) => "private",
        Some(TSAccessibility::Protected) => "protected",
        Some(TSAccessibility::Public) | None => "public",
    }
}

/// A module's `language`, by extension. The code layer's elements take theirs from the parse
/// instead ([`collect`]), since a component's script is TypeScript when its `lang` says so.
pub fn language_of(file: &str) -> Language {
    let typescript = std::path::Path::new(file)
        .extension()
        .is_some_and(|e| matches!(e.to_str(), Some("ts" | "tsx" | "mts" | "cts")));
    if typescript {
        Language::Typescript
    } else {
        Language::Javascript
    }
}

/// The code-layer elements `program` declares, with names unresolved; `source` is the text its
/// offsets index and `file` the module's `source`. The elements are TypeScript when the program
/// was parsed as TypeScript: a `.ts` file, and equally a `.vue` or `.svelte` script with
/// `lang="ts"`, whose abstract, generic and readonly facts are read like any other.
pub fn collect(program: &Program<'_>, source: &str, file: &str) -> FileCode {
    let mut index = Index::default();
    index.imports(&program.body);
    index.scan(&program.body, None);
    index.settle();
    let typescript = program.source_type.is_typescript();
    let mut collector = Collector {
        index,
        lines: Lines::new(source),
        source,
        typescript,
        language: if typescript {
            Language::Typescript
        } else {
            Language::Javascript
        },
        out: FileCode {
            file: file.to_owned(),
            ..FileCode::default()
        },
        namespace: Vec::new(),
        type_parameters: Vec::new(),
        types_seen: BTreeMap::new(),
        members_seen: BTreeMap::new(),
    };
    collector.statements(&program.body);
    let Collector { index, mut out, .. } = collector;
    for export in &index.exports {
        match export {
            Export::From { specifier, .. } | Export::All { specifier } => {
                out.specifiers.insert(specifier.clone());
            }
            Export::Local { .. } => {}
        }
    }
    out.exports = index.exports;
    out
}

/// The identifier chain an expression is (`a`, `a.b.c`), if it is one. Walked iteratively, as
/// an untrusted file can nest a chain as deep as it is long.
fn expression_segments(expression: &Expression<'_>) -> Option<Vec<String>> {
    let mut segments = Vec::new();
    let mut current = expression;
    loop {
        match current.get_inner_expression() {
            Expression::Identifier(identifier) => {
                segments.push(identifier.name.to_string());
                break;
            }
            Expression::StaticMemberExpression(member) => {
                segments.push(member.property.name.to_string());
                current = &member.object;
            }
            _ => return None,
        }
    }
    segments.reverse();
    Some(segments)
}

/// The identifier chain a type name is (`A`, `ns.A`), if it is one; iterative, as above.
fn type_name_segments(name: &TSTypeName<'_>) -> Option<Vec<String>> {
    let mut segments = Vec::new();
    let mut current = name;
    loop {
        match current {
            TSTypeName::IdentifierReference(identifier) => {
                segments.push(identifier.name.to_string());
                break;
            }
            TSTypeName::QualifiedName(qualified) => {
                segments.push(qualified.right.name.to_string());
                current = &qualified.left;
            }
            TSTypeName::ThisExpression(_) => return None,
        }
    }
    segments.reverse();
    Some(segments)
}

/// A class member's name as written: `#name` for a private name; `None` for a computed key.
fn member_name(key: &PropertyKey<'_>, computed: bool) -> Option<String> {
    if let Some(private) = key.private_name() {
        return Some(format!("#{private}"));
    }
    if computed && !matches!(key, PropertyKey::StringLiteral(_)) {
        return None;
    }
    key.static_name().map(std::borrow::Cow::into_owned)
}

/// The first pass: what the file declares, imports and exports.
#[derive(Debug, Default)]
struct Index {
    /// Qualified names of every declaration, namespaces and variables included.
    declared: BTreeSet<String>,
    /// Local binding to specifier and what it binds.
    imports: BTreeMap<String, (String, Imported)>,
    /// Top-level names the module exports.
    file_exported: BTreeSet<String>,
    /// Namespace-scoped qualified names exported from their namespace.
    scope_exported: BTreeSet<String>,
    exports: Vec<Export>,
    /// `export { local as exported }` without a source, settled once every import is known.
    local_specifiers: Vec<(String, String)>,
}

impl Index {
    fn imports(&mut self, body: &[Statement<'_>]) {
        for statement in body {
            match statement {
                Statement::ImportDeclaration(import) => {
                    let specifier = import.source.value.to_string();
                    for binding in import.specifiers.iter().flatten() {
                        let (local, imported) = match binding {
                            ImportDeclarationSpecifier::ImportSpecifier(s) => {
                                (&s.local, Imported::Named(s.imported.name().to_string()))
                            }
                            ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                                (&s.local, Imported::Named("default".to_owned()))
                            }
                            ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                                (&s.local, Imported::Namespace)
                            }
                        };
                        self.imports
                            .insert(local.name.to_string(), (specifier.clone(), imported));
                    }
                }
                Statement::TSImportEqualsDeclaration(import) => {
                    if let TSModuleReference::ExternalModuleReference(reference) =
                        &import.module_reference
                    {
                        self.imports.insert(
                            import.id.name.to_string(),
                            (reference.expression.value.to_string(), Imported::Module),
                        );
                    }
                }
                Statement::VariableDeclaration(declaration) => {
                    for declarator in &declaration.declarations {
                        let Some(Expression::CallExpression(call)) = declarator
                            .init
                            .as_ref()
                            .map(Expression::get_inner_expression)
                        else {
                            continue;
                        };
                        let is_require = matches!(&call.callee, Expression::Identifier(i) if i.name == "require");
                        let Some(Argument::StringLiteral(specifier)) = call.arguments.first()
                        else {
                            continue;
                        };
                        if !is_require {
                            continue;
                        }
                        let specifier = specifier.value.to_string();
                        match &declarator.id {
                            BindingPattern::BindingIdentifier(id) => {
                                self.imports
                                    .insert(id.name.to_string(), (specifier, Imported::Module));
                            }
                            BindingPattern::ObjectPattern(pattern) => {
                                for property in &pattern.properties {
                                    let (Some(key), Some(local)) = (
                                        property.key.static_name(),
                                        property.value.get_identifier_name(),
                                    ) else {
                                        continue;
                                    };
                                    self.imports.insert(
                                        local.to_string(),
                                        (specifier.clone(), Imported::Named(key.into_owned())),
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn qualify(prefix: Option<&str>, name: &str) -> String {
        prefix.map_or_else(|| name.to_owned(), |p| format!("{p}.{name}"))
    }

    /// Declares `name` and, when `exported`, exports it from its scope.
    fn declare(&mut self, prefix: Option<&str>, name: &str, exported: bool) {
        let qualified = Self::qualify(prefix, name);
        if exported {
            if prefix.is_none() {
                self.file_exported.insert(name.to_owned());
                self.exports.push(Export::Local {
                    exported: name.to_owned(),
                    local: name.to_owned(),
                });
            } else {
                self.scope_exported.insert(qualified.clone());
            }
        }
        self.declared.insert(qualified);
    }

    fn declaration(&mut self, declaration: &Declaration<'_>, prefix: Option<&str>, exported: bool) {
        match declaration {
            Declaration::VariableDeclaration(variables) => {
                for declarator in &variables.declarations {
                    for id in declarator.id.get_binding_identifiers() {
                        // A `require` binding is an import, not a declaration.
                        if prefix.is_some() || !self.imports.contains_key(id.name.as_str()) {
                            self.declare(prefix, &id.name, exported);
                        }
                    }
                }
            }
            Declaration::FunctionDeclaration(function) => {
                if let Some(name) = function.name() {
                    self.declare(prefix, &name, exported);
                }
            }
            Declaration::ClassDeclaration(class) => {
                if let Some(id) = &class.id {
                    self.declare(prefix, &id.name, exported);
                }
            }
            Declaration::TSTypeAliasDeclaration(alias) => {
                self.declare(prefix, &alias.id.name, exported);
            }
            Declaration::TSInterfaceDeclaration(interface) => {
                self.declare(prefix, &interface.id.name, exported);
            }
            Declaration::TSEnumDeclaration(declaration) => {
                self.declare(prefix, &declaration.id.name, exported);
            }
            Declaration::TSNamespaceDeclaration(namespace) => {
                self.namespace(namespace, prefix, exported);
            }
            Declaration::TSImportEqualsDeclaration(import) => {
                if prefix.is_some() || !self.imports.contains_key(import.id.name.as_str()) {
                    self.declare(prefix, &import.id.name, exported);
                }
            }
            Declaration::TSExternalModuleDeclaration(_) | Declaration::TSGlobalDeclaration(_) => {}
        }
    }

    fn namespace(
        &mut self,
        namespace: &TSNamespaceDeclaration<'_>,
        prefix: Option<&str>,
        exported: bool,
    ) {
        self.declare(prefix, &namespace.id.name, exported);
        let qualified = Self::qualify(prefix, &namespace.id.name);
        match &namespace.body {
            TSNamespaceDeclarationBody::TSModuleBlock(block) => {
                self.scan(&block.body, Some(&qualified));
            }
            // `namespace A.B {}`: `B` is exported from `A` implicitly.
            TSNamespaceDeclarationBody::TSNamespaceDeclaration(inner) => {
                self.namespace(inner, Some(&qualified), true);
            }
        }
    }

    fn scan(&mut self, body: &[Statement<'_>], prefix: Option<&str>) {
        for statement in body {
            match statement {
                Statement::ExportDeclaration(export) => {
                    self.declaration(&export.declaration, prefix, true);
                }
                Statement::ExportDefaultDeclaration(export) if prefix.is_none() => {
                    use oxc_ast::ast::ExportDefaultDeclarationKind as K;
                    let name = match &export.declaration {
                        K::FunctionDeclaration(f) => Some(
                            f.name()
                                .map_or_else(|| "default".to_owned(), |n| n.to_string()),
                        ),
                        K::ClassDeclaration(c) => Some(
                            c.id.as_ref()
                                .map_or_else(|| "default".to_owned(), |i| i.name.to_string()),
                        ),
                        K::TSInterfaceDeclaration(i) => Some(i.id.name.to_string()),
                        other => {
                            if let Some(expression) = other.as_expression()
                                && let Expression::Identifier(identifier) =
                                    expression.get_inner_expression()
                            {
                                self.local_specifiers
                                    .push((identifier.name.to_string(), "default".to_owned()));
                            }
                            None
                        }
                    };
                    if let Some(name) = name {
                        self.declared.insert(name.clone());
                        self.file_exported.insert(name.clone());
                        self.exports.push(Export::Local {
                            exported: "default".to_owned(),
                            local: name,
                        });
                    }
                }
                Statement::ExportNamedDeclaration(export) if prefix.is_none() => {
                    for specifier in &export.specifiers {
                        self.local_specifiers.push((
                            specifier.local.name().to_string(),
                            specifier.exported.name().to_string(),
                        ));
                    }
                }
                Statement::ExportFromDeclaration(export) if prefix.is_none() => {
                    let from = export.source.value.to_string();
                    for specifier in &export.specifiers {
                        self.exports.push(Export::From {
                            exported: specifier.exported.name().to_string(),
                            specifier: from.clone(),
                            imported: Imported::Named(specifier.local.name().to_string()),
                        });
                    }
                }
                Statement::ExportAllDeclaration(export) if prefix.is_none() => {
                    let specifier = export.source.value.to_string();
                    self.exports.push(match &export.exported {
                        Some(name) => Export::From {
                            exported: name.name().to_string(),
                            specifier,
                            imported: Imported::Namespace,
                        },
                        None => Export::All { specifier },
                    });
                }
                Statement::TSExportAssignment(export) if prefix.is_none() => {
                    if let Expression::Identifier(identifier) =
                        export.expression.get_inner_expression()
                    {
                        self.local_specifiers
                            .push((identifier.name.to_string(), "default".to_owned()));
                    }
                }
                Statement::ExpressionStatement(statement) if prefix.is_none() => {
                    self.commonjs_export(&statement.expression);
                }
                _ => {
                    if let Some(declaration) = statement.as_declaration() {
                        self.declaration(declaration, prefix, false);
                    }
                }
            }
        }
    }

    /// `module.exports = X`, `module.exports = { X, Y: Z }`, `module.exports.Y = X` and
    /// `exports.Y = X`.
    fn commonjs_export(&mut self, expression: &Expression<'_>) {
        let Expression::AssignmentExpression(assignment) = expression.get_inner_expression() else {
            return;
        };
        if assignment.operator != AssignmentOperator::Assign {
            return;
        }
        let AssignmentTarget::StaticMemberExpression(target) = &assignment.left else {
            return;
        };
        let Some(mut path) = expression_segments(&target.object) else {
            return;
        };
        path.push(target.property.name.to_string());
        let path: Vec<&str> = path.iter().map(String::as_str).collect();
        let identifier = |e: &Expression<'_>| match e.get_inner_expression() {
            Expression::Identifier(i) => Some(i.name.to_string()),
            _ => None,
        };
        match path.as_slice() {
            ["module", "exports"] => {
                if let Some(local) = identifier(&assignment.right) {
                    self.local_specifiers.push((local, "default".to_owned()));
                } else if let Expression::ObjectExpression(object) =
                    assignment.right.get_inner_expression()
                {
                    for property in &object.properties {
                        let ObjectPropertyKind::ObjectProperty(property) = property else {
                            continue;
                        };
                        if property.computed {
                            continue;
                        }
                        if let (Some(key), Some(local)) =
                            (property.key.static_name(), identifier(&property.value))
                        {
                            self.local_specifiers.push((local, key.into_owned()));
                        }
                    }
                }
            }
            ["module", "exports", name] | ["exports", name] => {
                if let Some(local) = identifier(&assignment.right) {
                    self.local_specifiers.push((local, (*name).to_owned()));
                }
            }
            _ => {}
        }
    }

    /// Turns each `export { local as exported }` into an export of a declaration or, for an
    /// imported binding, a re-export.
    fn settle(&mut self) {
        for (local, exported) in std::mem::take(&mut self.local_specifiers) {
            if let Some((specifier, imported)) = self.imports.get(&local) {
                self.exports.push(Export::From {
                    exported,
                    specifier: specifier.clone(),
                    imported: imported.clone(),
                });
            } else {
                self.file_exported.insert(local.clone());
                self.exports.push(Export::Local { exported, local });
            }
        }
    }

    /// Whether the qualified name is visible outside the module.
    fn is_public(&self, qualified: &str) -> bool {
        let mut segments = qualified.split('.');
        let Some(first) = segments.next() else {
            return false;
        };
        if !self.file_exported.contains(first) {
            return false;
        }
        let mut prefix = first.to_owned();
        for segment in segments {
            prefix.push('.');
            prefix.push_str(segment);
            if !self.scope_exported.contains(&prefix) {
                return false;
            }
        }
        true
    }
}

/// The second pass: the elements, with their names classified.
struct Collector<'s> {
    index: Index,
    lines: Lines<'s>,
    source: &'s str,
    language: Language,
    typescript: bool,
    out: FileCode,
    /// The enclosing namespaces' names.
    namespace: Vec<String>,
    /// The type parameters in scope, innermost last.
    type_parameters: Vec<String>,
    types_seen: BTreeMap<String, usize>,
    members_seen: BTreeMap<(String, String, bool), usize>,
}

/// What a class member declares about itself, shared by the member kinds.
struct MemberFacts<'m> {
    declaring: &'m str,
    name: &'m str,
    visibility: &'static str,
    is_static: bool,
    is_abstract: bool,
    offset: u32,
}

impl<'s> Collector<'s> {
    fn location(&self, offset: u32) -> Location {
        let (line, column) = self.lines.locate(offset);
        Location {
            language: self.language,
            file: Some(self.out.file.clone()),
            line: Some(line),
            column: Some(column),
        }
    }

    fn line(&self, offset: u32) -> u32 {
        self.lines.locate(offset).0
    }

    fn text(&self, span: Span) -> String {
        self.source
            .get(span.start as usize..span.end as usize)
            .unwrap_or_default()
            .to_owned()
    }

    fn qualified(&self, name: &str) -> String {
        if self.namespace.is_empty() {
            name.to_owned()
        } else {
            format!("{}.{name}", self.namespace.join("."))
        }
    }

    fn full_name(&self, qualified: &str) -> String {
        format!("{}#{qualified}", self.out.file)
    }

    /// Classifies a written name, looking in the enclosing namespaces innermost first, then the
    /// file's top level, then its imports.
    fn reference(&mut self, segments: &[String], offset: u32) -> Reference {
        let written = segments.join(".");
        let line = self.line(offset);
        let Some((first, rest)) = segments.split_first() else {
            return Reference {
                target: Target::Unbound,
                written,
                line,
            };
        };
        if self.type_parameters.iter().any(|p| p == first) {
            return Reference {
                target: Target::Unbound,
                written,
                line,
            };
        }
        for depth in (0..=self.namespace.len()).rev() {
            let mut candidate = self.namespace[..depth].join(".");
            if !candidate.is_empty() {
                candidate.push('.');
            }
            candidate.push_str(first);
            if self.index.declared.contains(&candidate) {
                for segment in rest {
                    candidate.push('.');
                    candidate.push_str(segment);
                }
                return Reference {
                    target: Target::Local(candidate),
                    written,
                    line,
                };
            }
        }
        if let Some((specifier, imported)) = self.index.imports.get(first) {
            self.out.specifiers.insert(specifier.clone());
            return Reference {
                target: Target::Imported {
                    specifier: specifier.clone(),
                    imported: imported.clone(),
                    rest: rest.to_vec(),
                },
                written,
                line,
            };
        }
        Reference {
            target: Target::Unbound,
            written,
            line,
        }
    }

    /// A reference for an expression: an identifier chain classified, anything else unbound
    /// with its source text.
    fn expression_reference(&mut self, expression: &Expression<'_>) -> Reference {
        let start = expression.span().start;
        match expression_segments(expression) {
            Some(segments) => self.reference(&segments, start),
            None => Reference {
                target: Target::Unbound,
                written: self.text(expression.span()),
                line: self.line(start),
            },
        }
    }

    fn type_name_reference(&mut self, name: &TSTypeName<'_>) -> Reference {
        let start = name.span().start;
        match type_name_segments(name) {
            Some(segments) => self.reference(&segments, start),
            None => Reference {
                target: Target::Unbound,
                written: self.text(name.span()),
                line: self.line(start),
            },
        }
    }

    /// A dependency of `kind` on every type a type annotation names.
    fn type_dependencies(
        &mut self,
        ty: &TSType<'_>,
        kind: &'static str,
        into: &mut Vec<PendingDependency>,
    ) {
        let mut names = TypeNames::default();
        names.visit_ts_type(ty);
        for (segments, offset) in names.found {
            let target = self.reference(&segments, offset);
            into.push(PendingDependency {
                target,
                kind,
                member: None,
            });
        }
    }

    fn type_argument_dependencies(
        &mut self,
        arguments: Option<&TSTypeParameterInstantiation<'_>>,
        into: &mut Vec<PendingDependency>,
    ) {
        for ty in arguments.iter().flat_map(|a| a.params.iter()) {
            self.type_dependencies(ty, "generic-argument", into);
        }
    }

    fn annotation_text(&self, annotation: Option<&TSTypeAnnotation<'_>>) -> Option<String> {
        annotation.map(|a| self.text(a.type_annotation.span()))
    }

    fn push_type_parameters(
        &mut self,
        parameters: Option<&TSTypeParameterDeclaration<'_>>,
    ) -> usize {
        let before = self.type_parameters.len();
        for parameter in parameters.iter().flat_map(|p| p.params.iter()) {
            self.type_parameters.push(parameter.name.name.to_string());
        }
        before
    }

    /// The literal text of a decorator argument: a string's value, a number or boolean as
    /// written, anything else its source text.
    fn literal(&self, argument: &Argument<'_>) -> String {
        match argument {
            Argument::StringLiteral(literal) => literal.value.to_string(),
            Argument::BooleanLiteral(literal) => literal.value.to_string(),
            other => self.text(other.span()),
        }
    }

    /// Records each decorator as an attribute on `target` and returns the dependencies they form.
    fn decorators(&mut self, decorators: &[Decorator<'_>], target: &str) -> Vec<PendingDependency> {
        let mut dependencies = Vec::new();
        for decorator in decorators {
            let expression = decorator.expression.get_inner_expression();
            let (callee, arguments) = match expression {
                Expression::CallExpression(call) => (&call.callee, Some(&call.arguments)),
                other => (other, None),
            };
            let attribute = self.expression_reference(callee);
            let arguments = arguments
                .map(|a| a.iter().map(|argument| self.literal(argument)).collect())
                .unwrap_or_default();
            self.out.attributes.push(PendingAttribute {
                element: AttributeElement {
                    target: target.to_owned(),
                    attribute_type: attribute.written.clone(),
                    arguments,
                    named_arguments: Vec::new(),
                    arguments_unknown: false,
                    location: self.location(expression.span().start),
                },
                attribute: attribute.clone(),
            });
            dependencies.push(PendingDependency {
                target: attribute,
                kind: "attribute",
                member: None,
            });
        }
        dependencies
    }

    fn new_type(
        &self,
        qualified: &str,
        name: &str,
        kind: &str,
        offset: u32,
        visibility: &str,
    ) -> TypeElement {
        let mut element =
            TypeElement::new(self.full_name(qualified), name, kind, self.location(offset));
        element.namespace = Some(self.out.file.clone());
        element.visibility = Some(visibility.to_owned());
        element.nested = Some(false);
        element
    }

    /// Adds a type; a second declaration of the same name (declaration merging, overloads)
    /// adds its interfaces and dependencies to the first.
    fn push_type(&mut self, pending: PendingType) {
        if let Some(&at) = self.types_seen.get(&pending.element.full_name)
            && let Some(first) = self.out.types.get_mut(at)
        {
            first.interfaces.extend(pending.interfaces);
            first.dependencies.extend(pending.dependencies);
            if first.base.is_none() {
                first.base = pending.base;
            }
            return;
        }
        self.types_seen
            .insert(pending.element.full_name.clone(), self.out.types.len());
        self.out.types.push(pending);
    }

    /// The index of the member, created by `make` the first time it is seen.
    fn member_slot(
        &mut self,
        facts: &MemberFacts<'_>,
        kind: &str,
        make: impl FnOnce(&mut MemberElement),
    ) -> usize {
        let key = (
            facts.declaring.to_owned(),
            facts.name.to_owned(),
            facts.is_static,
        );
        if let Some(&at) = self.members_seen.get(&key) {
            return at;
        }
        let mut element = MemberElement::new(
            facts.declaring,
            facts.name,
            kind,
            self.location(facts.offset),
        );
        element.visibility = Some(facts.visibility.to_owned());
        element.r#static = Some(facts.is_static);
        if self.typescript {
            element.r#abstract = Some(facts.is_abstract);
        }
        element.full_name = Some(format!("{}.{}", facts.declaring, facts.name));
        make(&mut element);
        let at = self.out.members.len();
        self.out.members.push(PendingMember {
            element,
            dependencies: Vec::new(),
        });
        self.members_seen.insert(key, at);
        at
    }

    fn member_dependencies(&mut self, at: usize, dependencies: Vec<PendingDependency>) {
        if let Some(member) = self.out.members.get_mut(at) {
            member.dependencies.extend(dependencies);
        }
    }

    fn statements(&mut self, body: &[Statement<'_>]) {
        for statement in body {
            match statement {
                Statement::ExportDeclaration(export) => self.declaration(&export.declaration),
                Statement::ExportDefaultDeclaration(export) if self.namespace.is_empty() => {
                    use oxc_ast::ast::ExportDefaultDeclarationKind as K;
                    match &export.declaration {
                        K::FunctionDeclaration(function) => {
                            let name = function
                                .name()
                                .map_or_else(|| "default".to_owned(), |n| n.to_string());
                            let offset = function
                                .id
                                .as_ref()
                                .map_or(function.span.start, |i| i.span.start);
                            self.function(function, &name, offset);
                        }
                        K::ClassDeclaration(class) => {
                            let name = class
                                .id
                                .as_ref()
                                .map_or_else(|| "default".to_owned(), |i| i.name.to_string());
                            let offset =
                                class.id.as_ref().map_or(class.span.start, |i| i.span.start);
                            let visibility = type_visibility(self.index.is_public(&name));
                            self.class(class, &name, &name, offset, visibility, None);
                        }
                        K::TSInterfaceDeclaration(interface) => self.interface(interface),
                        _ => {}
                    }
                }
                _ => {
                    if let Some(declaration) = statement.as_declaration() {
                        self.declaration(declaration);
                    }
                }
            }
        }
    }

    fn declaration(&mut self, declaration: &Declaration<'_>) {
        match declaration {
            Declaration::ClassDeclaration(class) => {
                if let Some(id) = &class.id {
                    let qualified = self.qualified(&id.name);
                    let visibility = type_visibility(self.index.is_public(&qualified));
                    self.class(class, &id.name, &qualified, id.span.start, visibility, None);
                }
            }
            Declaration::FunctionDeclaration(function) => {
                if let Some(id) = &function.id {
                    self.function(function, &id.name, id.span.start);
                }
            }
            Declaration::TSInterfaceDeclaration(interface) => self.interface(interface),
            Declaration::TSEnumDeclaration(declaration) => {
                let qualified = self.qualified(&declaration.id.name);
                let visibility = type_visibility(self.index.is_public(&qualified));
                let element = self.new_type(
                    &qualified,
                    &declaration.id.name,
                    "enum",
                    declaration.id.span.start,
                    visibility,
                );
                self.push_type(PendingType {
                    element,
                    base: None,
                    interfaces: Vec::new(),
                    dependencies: Vec::new(),
                });
            }
            Declaration::TSTypeAliasDeclaration(alias) => {
                let qualified = self.qualified(&alias.id.name);
                let visibility = type_visibility(self.index.is_public(&qualified));
                let before = self.push_type_parameters(alias.type_parameters.as_deref());
                let mut element = self.new_type(
                    &qualified,
                    &alias.id.name,
                    "type-alias",
                    alias.id.span.start,
                    visibility,
                );
                element.generic = Some(alias.type_parameters.is_some());
                let mut dependencies = Vec::new();
                self.type_dependencies(&alias.type_annotation, "signature", &mut dependencies);
                self.type_parameters.truncate(before);
                self.push_type(PendingType {
                    element,
                    base: None,
                    interfaces: Vec::new(),
                    dependencies,
                });
            }
            Declaration::VariableDeclaration(variables) => self.constants(variables),
            Declaration::TSNamespaceDeclaration(namespace) => self.namespace(namespace),
            Declaration::TSImportEqualsDeclaration(_)
            | Declaration::TSExternalModuleDeclaration(_)
            | Declaration::TSGlobalDeclaration(_) => {}
        }
    }

    /// An exported `const` bound to an arrow function, a function expression or a class
    /// expression is a type of that kind; any other variable is not a type.
    fn constants(&mut self, variables: &oxc_ast::ast::VariableDeclaration<'_>) {
        if variables.kind != VariableDeclarationKind::Const {
            return;
        }
        for declarator in &variables.declarations {
            let (BindingPattern::BindingIdentifier(id), Some(init)) =
                (&declarator.id, declarator.init.as_ref())
            else {
                continue;
            };
            let qualified = self.qualified(&id.name);
            if !self.index.is_public(&qualified) {
                continue;
            }
            match init.get_inner_expression() {
                Expression::ArrowFunctionExpression(arrow) => {
                    self.arrow(arrow, &id.name, &qualified, id.span.start);
                }
                Expression::FunctionExpression(function) => {
                    self.function_named(function, &id.name, &qualified, id.span.start);
                }
                Expression::ClassExpression(class) => {
                    self.class(class, &id.name, &qualified, id.span.start, "public", None);
                }
                _ => {}
            }
        }
    }

    fn arrow(
        &mut self,
        arrow: &oxc_ast::ast::ArrowFunctionExpression<'_>,
        name: &str,
        qualified: &str,
        offset: u32,
    ) {
        let mut element = self.new_type(qualified, name, "function", offset, "public");
        let before = self.push_type_parameters(arrow.type_parameters.as_deref());
        if self.typescript {
            element.generic = Some(arrow.type_parameters.is_some());
        }
        let mut dependencies = self.signature(&arrow.params, arrow.return_type.as_deref());
        let (body, _) = self.walk_body(None, None, &BTreeMap::new(), |walker| {
            walker.visit_arrow_function_expression(arrow);
        });
        dependencies.extend(body);
        self.type_parameters.truncate(before);
        self.push_type(PendingType {
            element,
            base: None,
            interfaces: Vec::new(),
            dependencies,
        });
    }

    fn namespace(&mut self, namespace: &TSNamespaceDeclaration<'_>) {
        self.namespace.push(namespace.id.name.to_string());
        match &namespace.body {
            TSNamespaceDeclarationBody::TSModuleBlock(block) => self.statements(&block.body),
            TSNamespaceDeclarationBody::TSNamespaceDeclaration(inner) => self.namespace(inner),
        }
        self.namespace.pop();
    }

    /// Dependencies `signature` on the types a parameter list and return type name.
    fn signature(
        &mut self,
        parameters: &FormalParameters<'_>,
        return_type: Option<&TSTypeAnnotation<'_>>,
    ) -> Vec<PendingDependency> {
        let mut dependencies = Vec::new();
        let annotations = parameters
            .items
            .iter()
            .map(|p| p.type_annotation.as_deref())
            .chain(std::iter::once(
                parameters
                    .rest
                    .as_ref()
                    .and_then(|r| r.type_annotation.as_deref()),
            ))
            .chain(std::iter::once(return_type));
        for annotation in annotations.flatten() {
            self.type_dependencies(&annotation.type_annotation, "signature", &mut dependencies);
        }
        dependencies
    }

    /// Each parameter's type annotation as written, `""` for one without; empty when none has one.
    fn parameter_types(&self, parameters: &FormalParameters<'_>) -> Vec<String> {
        let annotations: Vec<Option<&TSTypeAnnotation<'_>>> = parameters
            .items
            .iter()
            .map(|p| p.type_annotation.as_deref())
            .chain(
                parameters
                    .rest
                    .as_ref()
                    .map(|r| r.type_annotation.as_deref()),
            )
            .collect();
        if annotations.iter().all(Option::is_none) {
            return Vec::new();
        }
        annotations
            .into_iter()
            .map(|a| self.annotation_text(a).unwrap_or_default())
            .collect()
    }

    fn function(&mut self, function: &Function<'_>, name: &str, offset: u32) {
        let qualified = self.qualified(name);
        self.function_named(function, name, &qualified, offset);
    }

    fn function_named(
        &mut self,
        function: &Function<'_>,
        name: &str,
        qualified: &str,
        offset: u32,
    ) {
        let visibility = type_visibility(self.index.is_public(qualified));
        let mut element = self.new_type(qualified, name, "function", offset, visibility);
        let before = self.push_type_parameters(function.type_parameters.as_deref());
        if self.typescript {
            element.generic = Some(function.type_parameters.is_some());
        }
        let mut dependencies = self.signature(&function.params, function.return_type.as_deref());
        let (body, _) = self.walk_body(None, None, &BTreeMap::new(), |walker| {
            walker.visit_function(function, ScopeFlags::Function);
        });
        dependencies.extend(body);
        self.type_parameters.truncate(before);
        self.push_type(PendingType {
            element,
            base: None,
            interfaces: Vec::new(),
            dependencies,
        });
    }

    /// Walks code with `this` bound to `this_type`; returns the dependencies it forms and, when
    /// `from` names the calling member, records its calls.
    fn walk_body(
        &mut self,
        from: Option<&str>,
        this_type: Option<&str>,
        fields: &BTreeMap<String, Option<Reference>>,
        visit: impl FnOnce(&mut BodyWalker<'_, 's>),
    ) -> (Vec<PendingDependency>, usize) {
        let mut walker = BodyWalker {
            collector: self,
            this_type: this_type.map(str::to_owned),
            fields,
            scopes: vec![Vec::new()],
            function_depth: 0,
            dependencies: Vec::new(),
            calls: Vec::new(),
        };
        visit(&mut walker);
        let BodyWalker {
            dependencies,
            calls,
            ..
        } = walker;
        let count = calls.len();
        if let Some(from) = from {
            for (receiver, method, offset) in calls {
                let location = self.location(offset);
                self.out.calls.push(PendingCall {
                    from: from.to_owned(),
                    receiver,
                    method,
                    location,
                });
            }
        }
        (dependencies, count)
    }

    /// The type a variable or field annotation names, for calls on it: only a plain type
    /// reference (`x: Repo`, `x: Repo<T>`); a union or an array names no single type.
    fn annotation_reference(
        &mut self,
        annotation: Option<&TSTypeAnnotation<'_>>,
    ) -> Option<Reference> {
        match &annotation?.type_annotation {
            TSType::TSTypeReference(reference) => {
                Some(self.type_name_reference(&reference.type_name))
            }
            _ => None,
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one pass over a class body, each member kind a short arm"
    )]
    fn class(
        &mut self,
        class: &Class<'_>,
        name: &str,
        qualified: &str,
        offset: u32,
        visibility: &str,
        nested_in: Option<&str>,
    ) {
        let full = self.full_name(qualified);
        let before = self.push_type_parameters(class.type_parameters.as_deref());
        let mut element = self.new_type(qualified, name, "class", offset, visibility);
        element.nested = Some(nested_in.is_some());
        element.nested_in = nested_in.map(str::to_owned);
        if self.typescript {
            element.r#abstract = Some(class.r#abstract);
            element.generic = Some(class.type_parameters.is_some());
        }
        let mut dependencies = self.decorators(&class.decorators, &full);
        let base = class.heritage.as_ref().map(|heritage| {
            let base = self.expression_reference(&heritage.expression);
            self.type_argument_dependencies(heritage.type_arguments.as_deref(), &mut dependencies);
            base
        });
        if let Some(base) = &base {
            dependencies.push(PendingDependency {
                target: base.clone(),
                kind: "inherits",
                member: None,
            });
        }
        let mut interfaces = Vec::new();
        for implemented in &class.implements {
            let interface = self.type_name_reference(&implemented.expression);
            self.type_argument_dependencies(
                implemented.type_arguments.as_deref(),
                &mut dependencies,
            );
            dependencies.push(PendingDependency {
                target: interface.clone(),
                kind: "implements",
                member: None,
            });
            interfaces.push(interface);
        }

        // The declared type of every field and parameter property, for `this.field.m()`.
        let mut fields: BTreeMap<String, Option<Reference>> = BTreeMap::new();
        for member in &class.body.body {
            match member {
                ClassElement::PropertyDefinition(property) => {
                    if let Some(field) = member_name(&property.key, property.computed) {
                        let declared =
                            self.annotation_reference(property.type_annotation.as_deref());
                        fields.insert(field, declared);
                    }
                }
                ClassElement::MethodDefinition(method)
                    if method.kind == MethodDefinitionKind::Constructor =>
                {
                    for parameter in &method.value.params.items {
                        if let Some(field) = parameter.pattern.get_identifier_name()
                            && (parameter.accessibility.is_some() || parameter.readonly)
                        {
                            let declared =
                                self.annotation_reference(parameter.type_annotation.as_deref());
                            fields.insert(field.to_string(), declared);
                        }
                    }
                }
                _ => {}
            }
        }

        let mut mutable = false;
        for member in &class.body.body {
            match member {
                ClassElement::MethodDefinition(method) => {
                    let Some(member_name) = member_name(&method.key, method.computed) else {
                        continue;
                    };
                    let facts = MemberFacts {
                        declaring: &full,
                        name: &member_name,
                        visibility: member_visibility(
                            method.accessibility,
                            method.key.is_private_identifier(),
                        ),
                        is_static: method.r#static,
                        is_abstract: method.r#type.is_abstract(),
                        offset: method.key.span().start,
                    };
                    let function = &method.value;
                    let at = match method.kind {
                        MethodDefinitionKind::Get | MethodDefinitionKind::Set => {
                            let getter = method.kind == MethodDefinitionKind::Get;
                            let written = if getter {
                                self.annotation_text(function.return_type.as_deref())
                            } else {
                                function.params.items.first().and_then(|p| {
                                    self.annotation_text(p.type_annotation.as_deref())
                                })
                            };
                            let at = self.member_slot(&facts, "property", |_| {});
                            self.accessor(at, getter, facts.visibility, written);
                            mutable |= !getter;
                            if facts.is_abstract
                                && let Some(slot) = self.out.members.get_mut(at)
                            {
                                slot.element.r#abstract = slot.element.r#abstract.map(|_| true);
                            }
                            at
                        }
                        MethodDefinitionKind::Constructor | MethodDefinitionKind::Method => {
                            let kind = if method.kind == MethodDefinitionKind::Constructor {
                                "constructor"
                            } else {
                                "method"
                            };
                            let return_type = self.annotation_text(function.return_type.as_deref());
                            let parameter_types = self.parameter_types(&function.params);
                            self.member_slot(&facts, kind, |element| {
                                element.return_type = return_type;
                                element.parameter_types = parameter_types;
                            })
                        }
                    };
                    let before_method =
                        self.push_type_parameters(function.type_parameters.as_deref());
                    let mut member_dependencies =
                        self.decorators(&method.decorators, &format!("{full}.{member_name}"));
                    member_dependencies
                        .extend(self.signature(&function.params, function.return_type.as_deref()));
                    if let Some(body) = &function.body {
                        let from = format!("{full}.{member_name}");
                        let (body_dependencies, _) =
                            self.walk_body(Some(&from), Some(&full), &fields, |walker| {
                                walker.enter_parameters(&function.params);
                                walker.parameter_defaults(&function.params);
                                walker.visit_function_body(body);
                            });
                        member_dependencies.extend(body_dependencies);
                    }
                    self.type_parameters.truncate(before_method);
                    self.member_dependencies(at, member_dependencies);
                    if method.kind == MethodDefinitionKind::Constructor {
                        mutable |= self.parameter_properties(&full, &function.params);
                    }
                }
                ClassElement::PropertyDefinition(property) => {
                    let Some(member_name) = member_name(&property.key, property.computed) else {
                        continue;
                    };
                    let facts = MemberFacts {
                        declaring: &full,
                        name: &member_name,
                        visibility: member_visibility(
                            property.accessibility,
                            property.key.is_private_identifier(),
                        ),
                        is_static: property.r#static,
                        is_abstract: property.r#type.is_abstract(),
                        offset: property.key.span().start,
                    };
                    let return_type = self.annotation_text(property.type_annotation.as_deref());
                    let typescript = self.typescript;
                    let readonly = property.readonly;
                    mutable |= !readonly;
                    let at = self.member_slot(&facts, "field", |element| {
                        element.return_type = return_type;
                        if typescript {
                            element.readonly = Some(readonly);
                        }
                    });
                    let member_full = format!("{full}.{member_name}");
                    let mut member_dependencies =
                        self.decorators(&property.decorators, &member_full);
                    if let Some(annotation) = property.type_annotation.as_deref() {
                        self.type_dependencies(
                            &annotation.type_annotation,
                            "signature",
                            &mut member_dependencies,
                        );
                    }
                    match property
                        .value
                        .as_ref()
                        .map(Expression::get_inner_expression)
                    {
                        Some(Expression::ClassExpression(inner)) if property.r#static => {
                            let inner_qualified = format!("{qualified}.{member_name}");
                            self.class(
                                inner,
                                &member_name,
                                &inner_qualified,
                                facts.offset,
                                facts.visibility,
                                Some(&full),
                            );
                        }
                        Some(value) => {
                            let (body_dependencies, _) = self.walk_body(
                                Some(&member_full),
                                Some(&full),
                                &fields,
                                |walker| {
                                    walker.visit_expression(value);
                                },
                            );
                            member_dependencies.extend(body_dependencies);
                        }
                        None => {}
                    }
                    self.member_dependencies(at, member_dependencies);
                }
                ClassElement::AccessorProperty(property) => {
                    let Some(member_name) = member_name(&property.key, property.computed) else {
                        continue;
                    };
                    let facts = MemberFacts {
                        declaring: &full,
                        name: &member_name,
                        visibility: member_visibility(
                            property.accessibility,
                            property.key.is_private_identifier(),
                        ),
                        is_static: property.r#static,
                        is_abstract: property.r#type.is_abstract(),
                        offset: property.key.span().start,
                    };
                    mutable = true;
                    let return_type = self.annotation_text(property.type_annotation.as_deref());
                    let at = self.member_slot(&facts, "property", |element| {
                        let accessor = Accessor {
                            visibility: facts.visibility.to_owned(),
                        };
                        element.getter = Some(accessor.clone());
                        element.setter = Some(accessor);
                        element.readonly = Some(false);
                        element.return_type = return_type;
                    });
                    let member_full = format!("{full}.{member_name}");
                    let mut member_dependencies =
                        self.decorators(&property.decorators, &member_full);
                    if let Some(annotation) = property.type_annotation.as_deref() {
                        self.type_dependencies(
                            &annotation.type_annotation,
                            "signature",
                            &mut member_dependencies,
                        );
                    }
                    self.member_dependencies(at, member_dependencies);
                }
                ClassElement::StaticBlock(_) | ClassElement::TSIndexSignature(_) => {}
            }
        }
        if self.typescript {
            element.immutable = Some(!mutable);
        }
        self.type_parameters.truncate(before);
        self.push_type(PendingType {
            element,
            base,
            interfaces,
            dependencies,
        });
    }

    /// Constructor parameter properties (`constructor(private readonly repo: Repo)`) as fields;
    /// returns whether one is mutable.
    fn parameter_properties(&mut self, full: &str, parameters: &FormalParameters<'_>) -> bool {
        let mut mutable = false;
        for parameter in &parameters.items {
            if parameter.accessibility.is_none() && !parameter.readonly {
                continue;
            }
            let Some(binding) = parameter.pattern.get_binding_identifier() else {
                continue;
            };
            let name = binding.name.to_string();
            mutable |= !parameter.readonly;
            let facts = MemberFacts {
                declaring: full,
                name: &name,
                visibility: member_visibility(parameter.accessibility, false),
                is_static: false,
                is_abstract: false,
                offset: binding.span.start,
            };
            let return_type = self.annotation_text(parameter.type_annotation.as_deref());
            let readonly = parameter.readonly;
            let at = self.member_slot(&facts, "field", |element| {
                element.return_type = return_type;
                element.readonly = Some(readonly);
            });
            let mut dependencies = Vec::new();
            if let Some(annotation) = parameter.type_annotation.as_deref() {
                self.type_dependencies(&annotation.type_annotation, "signature", &mut dependencies);
            }
            self.member_dependencies(at, dependencies);
        }
        mutable
    }

    fn interface(&mut self, interface: &TSInterfaceDeclaration<'_>) {
        let name = interface.id.name.as_str();
        let qualified = self.qualified(name);
        let full = self.full_name(&qualified);
        let visibility = type_visibility(self.index.is_public(&qualified));
        let before = self.push_type_parameters(interface.type_parameters.as_deref());
        let mut element = self.new_type(
            &qualified,
            name,
            "interface",
            interface.id.span.start,
            visibility,
        );
        element.generic = Some(interface.type_parameters.is_some());
        let mut dependencies = Vec::new();
        let mut interfaces = Vec::new();
        for heritage in &interface.extends {
            let extended = self.type_name_reference(&heritage.type_name);
            self.type_argument_dependencies(heritage.type_arguments.as_deref(), &mut dependencies);
            dependencies.push(PendingDependency {
                target: extended.clone(),
                kind: "inherits",
                member: None,
            });
            interfaces.push(extended);
        }
        let mut mutable = false;
        for signature in &interface.body.body {
            match signature {
                TSSignature::TSPropertySignature(property) => {
                    let Some(member_name) = member_name(&property.key, property.computed) else {
                        continue;
                    };
                    let facts = MemberFacts {
                        declaring: &full,
                        name: &member_name,
                        visibility: "public",
                        is_static: false,
                        is_abstract: false,
                        offset: property.key.span().start,
                    };
                    mutable |= !property.readonly;
                    let return_type = self.annotation_text(property.type_annotation.as_deref());
                    let readonly = property.readonly;
                    let at = self.member_slot(&facts, "field", |element| {
                        element.r#abstract = None;
                        element.readonly = Some(readonly);
                        element.return_type = return_type;
                    });
                    let mut member_dependencies = Vec::new();
                    if let Some(annotation) = property.type_annotation.as_deref() {
                        self.type_dependencies(
                            &annotation.type_annotation,
                            "signature",
                            &mut member_dependencies,
                        );
                    }
                    self.member_dependencies(at, member_dependencies);
                }
                TSSignature::TSMethodSignature(method) => {
                    mutable |= self.interface_method(method, &full);
                }
                TSSignature::TSIndexSignature(_)
                | TSSignature::TSCallSignatureDeclaration(_)
                | TSSignature::TSConstructSignatureDeclaration(_) => {}
            }
        }
        element.immutable = Some(!mutable);
        self.type_parameters.truncate(before);
        self.push_type(PendingType {
            element,
            base: None,
            interfaces,
            dependencies,
        });
    }
    /// An interface's method or accessor signature; returns whether it is a setter.
    fn interface_method(
        &mut self,
        method: &oxc_ast::ast::TSMethodSignature<'_>,
        full: &str,
    ) -> bool {
        let Some(member_name) = member_name(&method.key, method.computed) else {
            return false;
        };
        let facts = MemberFacts {
            declaring: full,
            name: &member_name,
            visibility: "public",
            is_static: false,
            is_abstract: false,
            offset: method.key.span().start,
        };
        let before = self.push_type_parameters(method.type_parameters.as_deref());
        let mut setter = false;
        let at = if method.kind == TSMethodSignatureKind::Method {
            let return_type = self.annotation_text(method.return_type.as_deref());
            let parameter_types = self.parameter_types(&method.params);
            self.member_slot(&facts, "method", |element| {
                element.r#abstract = None;
                element.return_type = return_type;
                element.parameter_types = parameter_types;
            })
        } else {
            let getter = method.kind == TSMethodSignatureKind::Get;
            setter = !getter;
            let written = if getter {
                self.annotation_text(method.return_type.as_deref())
            } else {
                method
                    .params
                    .items
                    .first()
                    .and_then(|p| self.annotation_text(p.type_annotation.as_deref()))
            };
            let at = self.member_slot(&facts, "property", |element| {
                element.r#abstract = None;
            });
            self.accessor(at, getter, "public", written);
            at
        };
        let dependencies = self.signature(&method.params, method.return_type.as_deref());
        self.type_parameters.truncate(before);
        self.member_dependencies(at, dependencies);
        setter
    }

    /// Adds a getter or setter to the property at `at`; `written` is its type as written.
    fn accessor(&mut self, at: usize, getter: bool, visibility: &str, written: Option<String>) {
        let Some(slot) = self.out.members.get_mut(at) else {
            return;
        };
        let element = &mut slot.element;
        let accessor = Some(Accessor {
            visibility: visibility.to_owned(),
        });
        if getter {
            element.getter = accessor;
        } else {
            element.setter = accessor;
        }
        element.readonly = Some(element.setter.is_none());
        if element.return_type.is_none() {
            element.return_type = written;
        }
    }
}

/// Every type reference in a type annotation, with its offset; `typeof x` and `import("m")`
/// types are values and modules, not type names, and are passed over.
#[derive(Default)]
struct TypeNames {
    found: Vec<(Vec<String>, u32)>,
}

impl<'a> Visit<'a> for TypeNames {
    crate::walk::iterative_chains!();

    fn visit_ts_type_reference(&mut self, it: &oxc_ast::ast::TSTypeReference<'a>) {
        if let Some(segments) = type_name_segments(&it.type_name) {
            self.found.push((segments, it.span.start));
        }
        if let Some(arguments) = &it.type_arguments {
            self.visit_ts_type_parameter_instantiation(arguments);
        }
    }

    fn visit_ts_type_query(&mut self, _it: &oxc_ast::ast::TSTypeQuery<'a>) {}

    fn visit_ts_import_type(&mut self, _it: &oxc_ast::ast::TSImportType<'a>) {}
}

/// A walk over a body: the types it constructs and calls into, with a scope of local bindings
/// so a local that shadows a type is not taken for it.
struct BodyWalker<'c, 's> {
    collector: &'c mut Collector<'s>,
    this_type: Option<String>,
    fields: &'c BTreeMap<String, Option<Reference>>,
    /// Local bindings, innermost scope last, each with its declared type when it has a plain one.
    scopes: Vec<Vec<(String, Option<Reference>)>>,
    /// How many non-arrow functions deep the walk is: `this` is the class's only at 0.
    function_depth: u32,
    dependencies: Vec<PendingDependency>,
    calls: Vec<(Receiver, String, u32)>,
}

impl BodyWalker<'_, '_> {
    fn lookup(&self, name: &str) -> Option<&Option<Reference>> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.iter().rev().find(|(n, _)| n == name).map(|(_, r)| r))
    }

    fn declare(&mut self, pattern: &BindingPattern<'_>, annotation: Option<&TSTypeAnnotation<'_>>) {
        let declared = match pattern {
            BindingPattern::BindingIdentifier(_) => self.collector.annotation_reference(annotation),
            _ => None,
        };
        let names: Vec<String> = pattern
            .get_binding_identifiers()
            .iter()
            .map(|id| id.name.to_string())
            .collect();
        if let Some(scope) = self.scopes.last_mut() {
            for name in names {
                scope.push((name, declared.clone()));
            }
        }
    }

    fn enter_parameters(&mut self, parameters: &FormalParameters<'_>) {
        for parameter in &parameters.items {
            self.declare(&parameter.pattern, parameter.type_annotation.as_deref());
        }
        if let Some(rest) = &parameters.rest {
            self.declare(&rest.rest.argument, None);
        }
    }

    /// The default values in a parameter list (`r = new Repo()`, `{ r = new Repo() } = {}`),
    /// walked as a function's own parameters are by [`walk::walk_function`]; decorators and
    /// type annotations are the signature's, not the body's.
    fn parameter_defaults(&mut self, parameters: &FormalParameters<'_>) {
        for parameter in &parameters.items {
            self.visit_binding_pattern(&parameter.pattern);
            if let Some(initializer) = &parameter.initializer {
                self.visit_expression(initializer);
            }
        }
        if let Some(rest) = &parameters.rest {
            self.visit_binding_pattern(&rest.rest.argument);
        }
    }

    fn scoped(&mut self, visit: impl FnOnce(&mut Self)) {
        self.scopes.push(Vec::new());
        visit(self);
        self.scopes.pop();
    }

    /// A type named by an identifier chain whose root is not a local binding.
    fn named_type(&mut self, expression: &Expression<'_>) -> Option<Reference> {
        let segments = expression_segments(expression)?;
        if segments
            .first()
            .is_some_and(|root| self.lookup(root).is_some())
        {
            return None;
        }
        Some(self.collector.reference(&segments, expression.span().start))
    }

    fn call(&mut self, receiver: Receiver, method: &str, offset: u32) {
        if let Receiver::Typed(reference) = &receiver {
            self.dependencies.push(PendingDependency {
                target: reference.clone(),
                kind: "body",
                member: Some(method.to_owned()),
            });
        }
        self.calls.push((receiver, method.to_owned(), offset));
    }
}

impl<'a> Visit<'a> for BodyWalker<'_, '_> {
    crate::walk::iterative_chains!();

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        self.function_depth += 1;
        self.scoped(|this| {
            this.enter_parameters(&it.params);
            walk::walk_function(this, it, flags);
        });
        self.function_depth -= 1;
    }

    fn visit_arrow_function_expression(&mut self, it: &oxc_ast::ast::ArrowFunctionExpression<'a>) {
        self.scoped(|this| {
            this.enter_parameters(&it.params);
            walk::walk_arrow_function_expression(this, it);
        });
    }

    fn visit_class(&mut self, _it: &Class<'a>) {}

    fn visit_block_statement(&mut self, it: &oxc_ast::ast::BlockStatement<'a>) {
        self.scoped(|this| walk::walk_block_statement(this, it));
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        self.scoped(|this| walk::walk_for_statement(this, it));
    }

    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        self.scoped(|this| walk::walk_for_in_statement(this, it));
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        self.scoped(|this| walk::walk_for_of_statement(this, it));
    }

    fn visit_catch_clause(&mut self, it: &CatchClause<'a>) {
        self.scoped(|this| {
            if let Some(parameter) = &it.param {
                this.declare(&parameter.pattern, None);
            }
            walk::walk_catch_clause(this, it);
        });
    }

    fn visit_variable_declarator(&mut self, it: &oxc_ast::ast::VariableDeclarator<'a>) {
        self.declare(&it.id, it.type_annotation.as_deref());
        walk::walk_variable_declarator(self, it);
    }

    fn visit_new_expression(&mut self, it: &oxc_ast::ast::NewExpression<'a>) {
        if let Some(target) = self.named_type(&it.callee) {
            self.dependencies.push(PendingDependency {
                target,
                kind: "body",
                member: Some("constructor".to_owned()),
            });
        }
        walk::walk_new_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &oxc_ast::ast::CallExpression<'a>) {
        let offset = it.span.start;
        let this_is_class = self.function_depth == 0;
        match it.callee.get_inner_expression() {
            Expression::StaticMemberExpression(member) => {
                let method = member.property.name.as_str();
                match member.object.get_inner_expression() {
                    Expression::ThisExpression(_) if this_is_class => {
                        if let Some(this_type) = self.this_type.clone() {
                            self.call(Receiver::This(this_type), method, offset);
                        }
                    }
                    Expression::StaticMemberExpression(field)
                        if this_is_class
                            && matches!(
                                field.object.get_inner_expression(),
                                Expression::ThisExpression(_)
                            ) =>
                    {
                        if let Some(Some(declared)) = self.fields.get(field.property.name.as_str())
                        {
                            self.call(Receiver::Typed(declared.clone()), method, offset);
                        }
                    }
                    Expression::PrivateFieldExpression(field)
                        if this_is_class
                            && matches!(
                                field.object.get_inner_expression(),
                                Expression::ThisExpression(_)
                            ) =>
                    {
                        let name = format!("#{}", field.field.name);
                        if let Some(Some(declared)) = self.fields.get(&name) {
                            self.call(Receiver::Typed(declared.clone()), method, offset);
                        }
                    }
                    Expression::Identifier(identifier)
                        if self.lookup(identifier.name.as_str()).is_some() =>
                    {
                        if let Some(Some(declared)) = self.lookup(identifier.name.as_str()).cloned()
                        {
                            self.call(Receiver::Typed(declared), method, offset);
                        }
                    }
                    object => {
                        if let Some(target) = self.named_type(object) {
                            self.call(Receiver::Typed(target), method, offset);
                        }
                    }
                }
            }
            Expression::PrivateFieldExpression(member)
                if this_is_class
                    && matches!(
                        member.object.get_inner_expression(),
                        Expression::ThisExpression(_)
                    ) =>
            {
                if let Some(this_type) = self.this_type.clone() {
                    let method = format!("#{}", member.field.name);
                    self.call(Receiver::This(this_type), &method, offset);
                }
            }
            _ => {}
        }
        walk::walk_call_expression(self, it);
    }
}

/// Resolves every file's names against the others' and returns the layer, normalised.
pub fn link(files: Vec<FileCode>) -> CodeLayer {
    let modules: BTreeMap<String, (Vec<Export>, BTreeMap<String, String>)> = files
        .iter()
        .map(|f| (f.file.clone(), (f.exports.clone(), f.resolved.clone())))
        .collect();
    let known: BTreeSet<String> = files
        .iter()
        .flat_map(|f| f.types.iter().map(|t| t.element.full_name.clone()))
        .collect();
    let linker = Linker {
        modules: &modules,
        known: &known,
    };
    let mut layer = CodeLayer::default();
    let mut pending_calls: Vec<(String, PendingCall)> = Vec::new();
    for file in files {
        let resolve = |reference: &Reference| linker.resolve(&file.file, reference);
        let dependency = |pending: &PendingDependency| {
            resolve(&pending.target).map(|target| ElementDependency {
                target,
                kind: pending.kind.to_owned(),
                member: pending.member.clone(),
                line: Some(pending.target.line),
                // A body dependency is `new X()` or a call on X: both are calls.
                form: (pending.kind == "body").then(|| "call".to_owned()),
            })
        };
        for pending in &file.types {
            let mut element = pending.element.clone();
            element.base_type = pending
                .base
                .as_ref()
                .map(|b| resolve(b).unwrap_or_else(|| b.written.clone()));
            element.interfaces = pending
                .interfaces
                .iter()
                .map(|i| resolve(i).unwrap_or_else(|| i.written.clone()))
                .collect();
            element.dependencies = pending.dependencies.iter().filter_map(dependency).collect();
            layer.types.push(element);
        }
        for pending in &file.members {
            let mut element = pending.element.clone();
            element.dependencies = pending.dependencies.iter().filter_map(dependency).collect();
            layer.members.push(element);
        }
        for pending in &file.attributes {
            let mut element = pending.element.clone();
            if let Some(resolved) = resolve(&pending.attribute) {
                element.attribute_type = resolved;
            }
            layer.attributes.push(element);
        }
        for call in file.calls {
            pending_calls.push((file.file.clone(), call));
        }
    }
    close_chains(&mut layer);
    let base_of: BTreeMap<String, String> = layer
        .types
        .iter()
        .filter_map(|t| Some((t.full_name.clone(), t.base_type.clone()?)))
        .collect();
    let declared_members: BTreeSet<(String, String)> = layer
        .members
        .iter()
        .map(|m| (m.declaring_type.clone(), m.name.clone()))
        .collect();
    for (file, call) in pending_calls {
        let receiver = match &call.receiver {
            Receiver::This(this_type) => Some(this_type.clone()),
            Receiver::Typed(reference) => linker.resolve(&file, reference),
        };
        let Some(receiver) = receiver else {
            continue;
        };
        let declaring = declaring_type(&receiver, &call.method, &base_of, &declared_members);
        layer.calls.push(CallElement {
            from: call.from,
            to: format!("{declaring}.{}", call.method),
            location: call.location,
        });
    }
    // A type depends on whatever its members depend on.
    let positions: BTreeMap<String, usize> = layer
        .types
        .iter()
        .enumerate()
        .map(|(at, t)| (t.full_name.clone(), at))
        .collect();
    for member in &layer.members {
        if let Some(ty) = positions
            .get(&member.declaring_type)
            .and_then(|&at| layer.types.get_mut(at))
        {
            ty.dependencies.extend(member.dependencies.iter().cloned());
        }
    }
    layer.normalise();
    layer
}

/// The type along `receiver`'s base chain that declares `method`, else `receiver` itself.
fn declaring_type(
    receiver: &str,
    method: &str,
    base_of: &BTreeMap<String, String>,
    members: &BTreeSet<(String, String)>,
) -> String {
    let mut current = receiver.to_owned();
    let mut seen = BTreeSet::new();
    loop {
        if members.contains(&(current.clone(), method.to_owned())) {
            return current;
        }
        if !seen.insert(current.clone()) {
            return receiver.to_owned();
        }
        match base_of.get(&current) {
            Some(next) => current.clone_from(next),
            None => return receiver.to_owned(),
        }
    }
}

/// `baseTypes`: the base type, then its base, as far as the extracted code declares them.
fn close_chains(layer: &mut CodeLayer) {
    let base_of: BTreeMap<String, String> = layer
        .types
        .iter()
        .filter_map(|t| Some((t.full_name.clone(), t.base_type.clone()?)))
        .collect();
    for ty in &mut layer.types {
        let mut chain: Vec<String> = Vec::new();
        let mut next = ty.base_type.clone();
        while let Some(base) = next {
            if base == ty.full_name || chain.contains(&base) {
                break;
            }
            next = base_of.get(&base).cloned();
            chain.push(base);
        }
        ty.base_types = chain;
    }
}

/// Resolves names across files.
struct Linker<'l> {
    modules: &'l BTreeMap<String, (Vec<Export>, BTreeMap<String, String>)>,
    known: &'l BTreeSet<String>,
}

impl Linker<'_> {
    /// The full name of the type `reference` names from `file`, when it names one.
    fn resolve(&self, file: &str, reference: &Reference) -> Option<String> {
        match &reference.target {
            Target::Local(qualified) => self.known_type(file, qualified, &[]),
            Target::Imported {
                specifier,
                imported,
                rest,
            } => {
                let target = self.modules.get(file)?.1.get(specifier)?;
                self.export(target, imported, rest, &mut BTreeSet::new())
            }
            Target::Unbound => None,
        }
    }

    fn known_type(&self, file: &str, local: &str, rest: &[String]) -> Option<String> {
        let mut full = format!("{file}#{local}");
        for segment in rest {
            full.push('.');
            full.push_str(segment);
        }
        self.known.contains(&full).then_some(full)
    }

    /// `seen` holds the (file, export) pairs already asked, so a re-export cycle ends and a
    /// diamond of `export *` is searched once.
    fn export(
        &self,
        file: &str,
        imported: &Imported,
        rest: &[String],
        seen: &mut BTreeSet<(String, String)>,
    ) -> Option<String> {
        match imported {
            Imported::Named(name) => self.named(file, name, rest, seen),
            Imported::Namespace => {
                let (first, more) = rest.split_first()?;
                self.named(file, first, more, seen)
            }
            Imported::Module => rest
                .split_first()
                .and_then(|(first, more)| self.named(file, first, more, seen))
                .or_else(|| self.named(file, "default", rest, seen)),
        }
    }

    fn named(
        &self,
        file: &str,
        name: &str,
        rest: &[String],
        seen: &mut BTreeSet<(String, String)>,
    ) -> Option<String> {
        if !seen.insert((file.to_owned(), name.to_owned())) {
            return None;
        }
        let (exports, resolved) = self.modules.get(file)?;
        for export in exports {
            match export {
                Export::Local { exported, local } if exported == name => {
                    return self.known_type(file, local, rest);
                }
                Export::From {
                    exported,
                    specifier,
                    imported,
                } if exported == name => {
                    let target = resolved.get(specifier)?;
                    return self.export(target, imported, rest, seen);
                }
                _ => {}
            }
        }
        // `export *` passes on every name but `default`.
        if name == "default" {
            return None;
        }
        exports.iter().find_map(|export| match export {
            Export::All { specifier } => {
                let target = resolved.get(specifier)?;
                self.named(target, name, rest, seen)
            }
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    fn collected(file: &str, source: &str) -> FileCode {
        let allocator = Allocator::default();
        let source_type = SourceType::from_path(file).unwrap_or_default();
        let parsed = Parser::new(&allocator, source, source_type).parse();
        collect(&parsed.program, source, file)
    }

    fn layer(files: &[(&str, &str)], resolved: &[(&str, &str, &str)]) -> CodeLayer {
        let mut codes: Vec<FileCode> = files.iter().map(|(f, s)| collected(f, s)).collect();
        for (file, specifier, target) in resolved {
            if let Some(code) = codes.iter_mut().find(|c| c.file() == *file) {
                code.set_resolved((*specifier).to_owned(), (*target).to_owned());
            }
        }
        link(codes)
    }

    fn ty<'a>(layer: &'a CodeLayer, full_name: &str) -> Option<&'a TypeElement> {
        layer.types.iter().find(|t| t.full_name == full_name)
    }

    fn visibility(layer: &CodeLayer, full_name: &str) -> Option<String> {
        ty(layer, full_name).and_then(|t| t.visibility.clone())
    }

    #[test]
    fn visibility_vocabulary() {
        assert_eq!(type_visibility(true), "public");
        assert_eq!(type_visibility(false), "internal");
        assert_eq!(member_visibility(None, false), "public");
        assert_eq!(
            member_visibility(Some(TSAccessibility::Public), false),
            "public"
        );
        assert_eq!(
            member_visibility(Some(TSAccessibility::Private), false),
            "private"
        );
        assert_eq!(
            member_visibility(Some(TSAccessibility::Protected), false),
            "protected"
        );
        assert_eq!(member_visibility(None, true), "private");
        assert_eq!(language_of("a.ts"), Language::Typescript);
        assert_eq!(language_of("a.d.mts"), Language::Typescript);
        assert_eq!(language_of("a.jsx"), Language::Javascript);
        assert_eq!(language_of("a.vue"), Language::Javascript);
    }

    /// A component's `lang="ts"` script is TypeScript to the code layer whatever the file's
    /// extension: abstract, generic and readonly are read and the location says typescript.
    #[test]
    fn a_component_typescript_script_is_typescript() {
        let parsed = |script: &str, source_type: SourceType| {
            let allocator = Allocator::default();
            let program = Parser::new(&allocator, script, source_type).parse().program;
            link(vec![collect(&program, script, "C.vue")])
        };
        let ts = parsed(
            "export abstract class Base<T> { readonly id = 1; abstract make(): T; }\n",
            SourceType::ts(),
        );
        let base = ty(&ts, "C.vue#Base");
        assert_eq!(
            base.map(|t| t.location.language),
            Some(Language::Typescript)
        );
        assert_eq!(base.and_then(|t| t.r#abstract), Some(true));
        assert_eq!(base.and_then(|t| t.generic), Some(true));
        let js = parsed("export class Base {}\n", SourceType::mjs());
        let base = ty(&js, "C.vue#Base");
        assert_eq!(
            base.map(|t| t.location.language),
            Some(Language::Javascript)
        );
        assert_eq!(base.and_then(|t| t.r#abstract), None);
    }

    /// A method's parameter defaults are part of its body, as a function's are: `new Repo()` in
    /// `m(r = new Repo())`, in a destructured default and in a rest pattern's default forms a body
    /// dependency either way.
    #[test]
    fn method_parameter_defaults_are_walked_like_a_function_s() {
        let layer = layer(
            &[(
                "p.ts",
                "export class Repo { static make() { return new Repo(); } static spare() { return new Repo(); } }\n\
                 export function f(r = new Repo()) {}\n\
                 export class Service {\n\
                 \x20 m(r = new Repo(), { q = Repo.make() }: { q?: Repo } = {}, ...[z = Repo.spare()]: Repo[]) {}\n\
                 }\n",
            )],
            &[],
        );
        let body = |full_name: &str| {
            ty(&layer, full_name).map(|t| {
                t.dependencies
                    .iter()
                    .filter(|d| d.kind == "body")
                    .map(|d| format!("{} {}", d.target, d.member.clone().unwrap_or_default()))
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(
            body("p.ts#f"),
            Some(vec!["p.ts#Repo constructor".to_owned()])
        );
        assert_eq!(
            body("p.ts#Service"),
            Some(vec![
                "p.ts#Repo constructor".to_owned(),
                "p.ts#Repo make".to_owned(),
                "p.ts#Repo spare".to_owned(),
            ])
        );
    }

    #[test]
    fn export_forms_decide_visibility() {
        let layer = layer(
            &[(
                "a.ts",
                "export class A {}\nclass B {}\nclass C {}\nexport { C as Renamed };\n\
                 export default class D {}\ninterface E {}\nexport type { E };\n\
                 namespace N { export class Inner {} class Hidden {} }\n\
                 export namespace P { export class Out {} namespace Q { export class Deep {} } }\n\
                 export namespace R.S { export class T {} }\n\
                 export const f = () => 1;\nconst g = () => 2;\nfunction h() {}\n",
            )],
            &[],
        );
        let cases = [
            ("a.ts#A", Some("public")),
            ("a.ts#B", Some("internal")),
            ("a.ts#C", Some("public")),
            ("a.ts#D", Some("public")),
            ("a.ts#E", Some("public")),
            ("a.ts#N.Inner", Some("internal")),
            ("a.ts#N.Hidden", Some("internal")),
            ("a.ts#P.Out", Some("public")),
            ("a.ts#P.Q.Deep", Some("internal")),
            ("a.ts#R.S.T", Some("public")),
            ("a.ts#f", Some("public")),
            ("a.ts#g", None),
            ("a.ts#h", Some("internal")),
        ];
        for (name, expected) in cases {
            assert_eq!(visibility(&layer, name).as_deref(), expected, "{name}");
        }
        assert_eq!(
            ty(&layer, "a.ts#N.Inner").and_then(|t| t.nested),
            Some(false)
        );
    }

    #[test]
    fn commonjs_exports_are_exports() {
        let layer = layer(
            &[
                (
                    "a.js",
                    "class A {}\nclass B {}\nclass C {}\nclass D {}\nfunction e() {}\n\
                     module.exports = { A, Bee: B };\nexports.C = C;\nmodule.exports.e = e;\n",
                ),
                ("b.js", "class Main {}\nmodule.exports = Main;\n"),
                (
                    "c.js",
                    "const { Bee } = require('./a');\nconst Main = require('./b');\n\
                     const a = require('./a');\n\
                     class X extends Bee {}\nclass Y extends Main {}\nclass Z extends a.C {}\n\
                     class W extends a.D {}\n",
                ),
            ],
            &[("c.js", "./a", "a.js"), ("c.js", "./b", "b.js")],
        );
        assert_eq!(visibility(&layer, "a.js#A").as_deref(), Some("public"));
        assert_eq!(visibility(&layer, "a.js#D").as_deref(), Some("internal"));
        assert_eq!(visibility(&layer, "a.js#e").as_deref(), Some("public"));
        assert_eq!(visibility(&layer, "b.js#Main").as_deref(), Some("public"));
        let base = |name: &str| ty(&layer, name).and_then(|t| t.base_type.clone());
        assert_eq!(base("c.js#X").as_deref(), Some("a.js#B"));
        assert_eq!(base("c.js#Y").as_deref(), Some("b.js#Main"));
        assert_eq!(base("c.js#Z").as_deref(), Some("a.js#C"));
        // Declared in `a.js` but not exported from it: kept as written.
        assert_eq!(base("c.js#W").as_deref(), Some("a.D"));
        // JavaScript cannot say abstract, generic or immutable.
        let x = ty(&layer, "c.js#X");
        assert_eq!(x.and_then(|t| t.r#abstract), None);
        assert_eq!(x.and_then(|t| t.generic), None);
        assert_eq!(x.and_then(|t| t.immutable), None);
        assert_eq!(x.map(|t| t.location.language), Some(Language::Javascript));
    }

    #[test]
    fn names_resolve_through_imports_and_re_exports() {
        let layer = layer(
            &[
                (
                    "base.ts",
                    "export class Base {}\nexport default class Root {}\nexport interface Shape {}\n",
                ),
                (
                    "barrel.ts",
                    "export * from './base';\nexport { default as Root2 } from './base';\nexport * as ns from './base';\n",
                ),
                (
                    "use.ts",
                    "import { Base as B, Root2, ns } from './barrel';\nimport Root from './base';\n\
                     import * as all from './base';\nimport type { Missing } from './base';\n\
                     import { External } from 'pkg';\n\
                     class One extends B {}\nclass Two extends Root2 implements ns.Shape {}\n\
                     class Three extends Root implements all.Shape, Missing, External {}\n\
                     class Four<Base> { f: Base; }\n",
                ),
            ],
            &[
                ("barrel.ts", "./base", "base.ts"),
                ("use.ts", "./barrel", "barrel.ts"),
                ("use.ts", "./base", "base.ts"),
            ],
        );
        let base = |name: &str| ty(&layer, name).and_then(|t| t.base_type.clone());
        let interfaces = |name: &str| {
            ty(&layer, name)
                .map(|t| t.interfaces.clone())
                .unwrap_or_default()
        };
        assert_eq!(base("use.ts#One").as_deref(), Some("base.ts#Base"));
        assert_eq!(base("use.ts#Two").as_deref(), Some("base.ts#Root"));
        assert_eq!(interfaces("use.ts#Two"), ["base.ts#Shape"]);
        assert_eq!(base("use.ts#Three").as_deref(), Some("base.ts#Root"));
        assert_eq!(
            interfaces("use.ts#Three"),
            ["base.ts#Shape", "Missing", "External"]
        );
        assert_eq!(
            ty(&layer, "use.ts#One").map(|t| t.base_types.clone()),
            Some(vec!["base.ts#Base".to_owned()])
        );
        // A type parameter shadows the class of the same name.
        let four = ty(&layer, "use.ts#Four");
        assert!(four.is_some_and(|t| t.dependencies.is_empty()), "{four:?}");
        let three = ty(&layer, "use.ts#Three").map(|t| {
            t.dependencies
                .iter()
                .map(|d| format!("{} {}", d.kind, d.target))
                .collect::<Vec<_>>()
        });
        assert_eq!(
            three,
            Some(vec![
                "inherits base.ts#Root".to_owned(),
                "implements base.ts#Shape".to_owned()
            ])
        );
    }

    #[test]
    fn a_re_export_cycle_ends() {
        let layer = layer(
            &[
                ("a.ts", "export * from './b';\n"),
                ("b.ts", "export * from './a';\n"),
                ("c.ts", "import { X } from './a';\nclass C extends X {}\n"),
            ],
            &[
                ("a.ts", "./b", "b.ts"),
                ("b.ts", "./a", "a.ts"),
                ("c.ts", "./a", "a.ts"),
            ],
        );
        assert_eq!(
            ty(&layer, "c.ts#C")
                .and_then(|t| t.base_type.clone())
                .as_deref(),
            Some("X")
        );
    }

    #[test]
    fn calls_follow_this_fields_parameters_and_statics() {
        let layer = layer(
            &[(
                "s.ts",
                "export class Repo { save(): void {} static make(): Repo { return new Repo(); } }\n\
                 export class Special extends Repo {}\n\
                 export class Service {\n  private repo: Repo;\n  constructor(private readonly other: Special) {}\n\
                   run(r: Repo, n: number): void {\n    this.helper();\n    this.repo.save();\n    this.other.save();\n\
                     r.save();\n    Repo.make();\n    const local: Repo = r;\n    local.save();\n\
                     { const Repo = 1; Repo.toString(); }\n    function inner(this: unknown) { this.helper(); }\n\
                     const arrow = () => this.helper();\n    unknown.call();\n  }\n  helper(): void {}\n}\n",
            )],
            &[],
        );
        let calls: Vec<String> = layer
            .calls
            .iter()
            .map(|c| format!("{} -> {}", c.from, c.to))
            .collect();
        assert_eq!(
            calls,
            [
                "s.ts#Service.run -> s.ts#Repo.make",
                "s.ts#Service.run -> s.ts#Repo.save",
                "s.ts#Service.run -> s.ts#Repo.save",
                "s.ts#Service.run -> s.ts#Repo.save",
                "s.ts#Service.run -> s.ts#Repo.save",
                "s.ts#Service.run -> s.ts#Service.helper",
                "s.ts#Service.run -> s.ts#Service.helper",
            ],
            "{calls:#?}"
        );
        let service = ty(&layer, "s.ts#Service");
        let body: Vec<String> = service
            .map(|t| {
                t.dependencies
                    .iter()
                    .filter(|d| d.kind == "body")
                    .map(|d| format!("{}.{}", d.target, d.member.clone().unwrap_or_default()))
                    .collect()
            })
            .unwrap_or_default();
        // One per line of reference: `this.repo`, `r` and `local` each call `save`; `this.other`
        // is a `Special`, which inherits `save` but is the type the body names.
        assert_eq!(
            body,
            [
                "s.ts#Repo.make",
                "s.ts#Repo.save",
                "s.ts#Repo.save",
                "s.ts#Repo.save",
                "s.ts#Special.save"
            ]
        );
        let make = ty(&layer, "s.ts#Repo").map(|t| {
            t.dependencies
                .iter()
                .map(|d| format!("{} {}", d.kind, d.target))
                .collect::<Vec<_>>()
        });
        assert_eq!(
            make,
            Some(vec![
                "body s.ts#Repo".to_owned(),
                "signature s.ts#Repo".to_owned()
            ])
        );
    }

    #[test]
    fn members_accessors_and_parameter_properties() {
        let layer = layer(
            &[(
                "m.ts",
                "export abstract class M<T> {\n  #secret = 1;\n  protected static count: number;\n\
                   readonly id: string;\n  get name(): string { return ''; }\n  set name(v: string) {}\n\
                   get only(): number { return 1; }\n  accessor auto: boolean = false;\n\
                   abstract run(a: T, b): void;\n  constructor(public x: number, private readonly y: string) {}\n\
                   m(): void;\n  m(a?: number): void {}\n  [computed](): void {}\n}\n\
                 export interface I { readonly a: string; b(): void; get c(): number; set c(v: number); }\n",
            )],
            &[],
        );
        let member = |name: &str| layer.members.iter().find(|m| m.name == name);
        let secret = member("#secret");
        assert_eq!(
            secret.and_then(|m| m.visibility.as_deref()),
            Some("private")
        );
        assert_eq!(
            secret.and_then(|m| m.full_name.as_deref()),
            Some("m.ts#M.#secret")
        );
        let count = member("count");
        assert_eq!(
            count.and_then(|m| m.visibility.as_deref()),
            Some("protected")
        );
        assert_eq!(count.and_then(|m| m.r#static), Some(true));
        assert_eq!(member("id").and_then(|m| m.readonly), Some(true));
        let name = member("name");
        assert_eq!(name.map(|m| m.kind.as_str()), Some("property"));
        assert!(name.is_some_and(|m| m.getter.is_some() && m.setter.is_some()));
        assert_eq!(name.and_then(|m| m.readonly), Some(false));
        assert_eq!(name.and_then(|m| m.return_type.as_deref()), Some("string"));
        assert_eq!(member("only").and_then(|m| m.readonly), Some(true));
        assert!(member("auto").is_some_and(|m| m.setter.is_some()));
        let run = member("run");
        assert_eq!(run.and_then(|m| m.r#abstract), Some(true));
        assert_eq!(
            run.map(|m| m.parameter_types.clone()),
            Some(vec!["T".to_owned(), String::new()])
        );
        assert_eq!(
            member("constructor").map(|m| m.kind.as_str()),
            Some("constructor")
        );
        let y = member("y");
        assert_eq!(y.map(|m| m.kind.as_str()), Some("field"));
        assert_eq!(y.and_then(|m| m.visibility.as_deref()), Some("private"));
        assert_eq!(y.and_then(|m| m.readonly), Some(true));
        assert_eq!(member("x").and_then(|m| m.readonly), Some(false));
        assert_eq!(layer.members.iter().filter(|m| m.name == "m").count(), 1);
        assert!(member("computed").is_none());
        let class = ty(&layer, "m.ts#M");
        assert_eq!(class.and_then(|t| t.r#abstract), Some(true));
        assert_eq!(class.and_then(|t| t.generic), Some(true));
        assert_eq!(class.and_then(|t| t.immutable), Some(false));
        let interface = ty(&layer, "m.ts#I");
        assert_eq!(interface.and_then(|t| t.immutable), Some(false));
        let c = layer
            .members
            .iter()
            .find(|m| m.full_name.as_deref() == Some("m.ts#I.c"));
        assert!(c.is_some_and(|m| m.getter.is_some() && m.setter.is_some()));
        assert_eq!(c.and_then(|m| m.return_type.as_deref()), Some("number"));
    }

    #[test]
    fn decorators_become_attributes_with_literal_arguments() {
        let layer = layer(
            &[
                (
                    "d.ts",
                    "export function Component(o: object) { return (t: unknown) => t; }\n",
                ),
                (
                    "e.ts",
                    "import { Component } from './d';\nimport { Injectable } from '@angular/core';\n\
                     @Component({ selector: 'x' })\n@Injectable('root', 3, true, KEY)\n\
                     export class E { @Input() value = 1; }\n",
                ),
            ],
            &[("e.ts", "./d", "d.ts")],
        );
        let on_e: Vec<(String, Vec<String>)> = layer
            .attributes
            .iter()
            .filter(|a| a.target == "e.ts#E")
            .map(|a| (a.attribute_type.clone(), a.arguments.clone()))
            .collect();
        assert_eq!(
            on_e,
            [
                (
                    "Injectable".to_owned(),
                    vec![
                        "root".to_owned(),
                        "3".to_owned(),
                        "true".to_owned(),
                        "KEY".to_owned()
                    ]
                ),
                (
                    "d.ts#Component".to_owned(),
                    vec!["{ selector: 'x' }".to_owned()]
                ),
            ]
        );
        assert!(
            layer
                .attributes
                .iter()
                .any(|a| a.target == "e.ts#E.value" && a.attribute_type == "Input")
        );
        let e = ty(&layer, "e.ts#E").map(|t| {
            t.dependencies
                .iter()
                .map(|d| format!("{} {}", d.kind, d.target))
                .collect::<Vec<_>>()
        });
        assert_eq!(e, Some(vec!["attribute d.ts#Component".to_owned()]));
    }

    #[test]
    fn nested_class_expressions_and_merged_declarations() {
        let layer = layer(
            &[(
                "n.ts",
                "export class Outer { static Inner = class {}; private static Hidden = class {}; }\n\
                 interface Merged { a: string }\ninterface Merged extends Base { b: string }\n\
                 class Base {}\nexport const Klass = class extends Base {};\n\
                 export const fn = function named() {};\ndeclare module 'x' { class Ambient {} }\n\
                 declare global { class Global {} }\nlet notConst = () => 1;\nexport { notConst };\n",
            )],
            &[],
        );
        let inner = ty(&layer, "n.ts#Outer.Inner");
        assert_eq!(inner.and_then(|t| t.nested), Some(true));
        assert_eq!(
            inner.and_then(|t| t.nested_in.as_deref()),
            Some("n.ts#Outer")
        );
        assert_eq!(inner.and_then(|t| t.visibility.as_deref()), Some("public"));
        assert_eq!(
            visibility(&layer, "n.ts#Outer.Hidden").as_deref(),
            Some("private")
        );
        let merged: Vec<&TypeElement> = layer.types.iter().filter(|t| t.name == "Merged").collect();
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged.first().map(|t| t.interfaces.clone()),
            Some(vec!["n.ts#Base".to_owned()])
        );
        assert_eq!(
            layer
                .members
                .iter()
                .filter(|m| m.declaring_type == "n.ts#Merged")
                .count(),
            2
        );
        assert_eq!(
            ty(&layer, "n.ts#Klass")
                .and_then(|t| t.base_type.clone())
                .as_deref(),
            Some("n.ts#Base")
        );
        assert_eq!(
            ty(&layer, "n.ts#fn").map(|t| t.kind.as_str()),
            Some("function")
        );
        assert!(ty(&layer, "n.ts#Ambient").is_none() && ty(&layer, "n.ts#Global").is_none());
        assert!(ty(&layer, "n.ts#notConst").is_none());
    }

    const FRAGMENTS: &[&str] = &[
        "export class A extends B implements I { m(x: A): B { return new B(); } }\n",
        "class B { static s() {} #p = 1; get g() { return 1; } }\n",
        "export interface I extends J { a: A; b(): void }\n",
        "interface J {}\n",
        "export enum E { X }\n",
        "export type T<U> = U | A;\n",
        "export function f(a: A) { B.s(); }\n",
        "export const g = (b: B) => b.s();\n",
        "import { Z } from './z';\n",
        "export * from './y';\n",
        "namespace N { export class C extends A {} }\n",
        "@d() export class D { @e x = 1; }\n",
        "export default class {}\n",
        "module.exports = { A };\n",
        "{ ( [ <<>> class\n",
    ];

    proptest::proptest! {
        /// Over any mix of declarations, broken ones included: every element belongs to the
        /// file, every member to a type of the layer, the output is sorted, and linking is a
        /// function of its input.
        #[test]
        fn the_layer_is_well_formed_and_deterministic(
            picks in proptest::collection::vec(0..FRAGMENTS.len(), 0..12)
        ) {
            let source: String = picks.iter().filter_map(|&i| FRAGMENTS.get(i).copied()).collect();
            let first = layer(&[("p.ts", &source)], &[]);
            let second = layer(&[("p.ts", &source)], &[]);
            proptest::prop_assert_eq!(&first, &second);
            let names: Vec<&str> = first.types.iter().map(|t| t.full_name.as_str()).collect();
            let mut sorted = names.clone();
            sorted.sort_unstable();
            proptest::prop_assert_eq!(&names, &sorted);
            proptest::prop_assert!(names.iter().all(|n| n.starts_with("p.ts#")));
            proptest::prop_assert!(first.members.iter().all(|m| names.contains(&m.declaring_type.as_str())));
            proptest::prop_assert!(first.calls.iter().all(|c| c.from.starts_with("p.ts#")));
        }

        /// Any text at all is read without a panic.
        #[test]
        fn any_text_is_read(source in "\\PC{0,200}") {
            let code = collected("q.ts", &source);
            let _ = link(vec![code]);
        }
    }

    #[test]
    fn locations_are_one_based_and_output_is_sorted() {
        let layer = layer(
            &[("z.ts", "\n  export class Z {}\nexport enum A { X }\n")],
            &[],
        );
        let z = ty(&layer, "z.ts#Z");
        assert_eq!(
            z.map(|t| (t.location.line, t.location.column)),
            Some((Some(2), Some(16)))
        );
        assert_eq!(z.and_then(|t| t.namespace.as_deref()), Some("z.ts"));
        let names: Vec<&str> = layer.types.iter().map(|t| t.full_name.as_str()).collect();
        assert_eq!(names, ["z.ts#A", "z.ts#Z"]);
        let counts = collected("z.ts", "export class Z { m() { this.m(); } }\n").counts();
        assert_eq!(counts, (1, 1, 0, 1));
    }

    #[test]
    fn a_dotted_chain_as_long_as_the_file_does_not_overflow_a_worker_stack() {
        use crate::walk::{Flavour, WalkOptions, walk_source_then};
        // 100,000 segments each in an `extends`, type references, a `new` and calls, analysed by
        // every walker and the code layer on a thread with a rayon worker's 2 MiB stack.
        let chain = format!("a{}", ".b".repeat(100_000));
        let source = format!(
            "export class X extends {chain} {{\n  m(p: {chain}) {{ new {chain}(); return this.n({chain}()); }}\n  n(q: unknown) {{}}\n}}\nlet v: {chain};\n{chain}();\n"
        );
        let analysed = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let options = WalkOptions::default();
                [Flavour::Acorn, Flavour::Swc, Flavour::Tsc]
                    .into_iter()
                    .map(|flavour| {
                        walk_source_then(&source, SourceType::ts(), flavour, &options, |program| {
                            collect(program, &source, "x.ts").counts()
                        })
                        .map(|(_, counts)| counts)
                        .ok()
                    })
                    .collect::<Vec<_>>()
            })
            .map(std::thread::JoinHandle::join);
        assert!(
            matches!(&analysed, Ok(Ok(counts)) if *counts == vec![Some((1, 2, 0, 2)); 3]),
            "{analysed:?}"
        );
    }
}
