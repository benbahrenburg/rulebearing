//! `rulebearing import archunit`: ArchUnitNET and NetArchTest test projects as element and slice
//! rules, each preceded by the C# it came from.
//!
//! - Source: [design § The developer relations hat](../../../../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo),
//!   the [ArchUnitNET 0.13.4 coverage tab](../../../../../docs/artifacts/archunitnet-0.13.4-coverage.md)
//!   (each fluent method's key), [conformance/netarchtest/README.md](../../../../../conformance/netarchtest/README.md#the-mapping)
//! - Plan: [Wave 2, Step 11](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f),
//!   and "How `import archunit` reads C#" in [§ 1.7](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#17-decisions-this-wave-must-make)
//! - Decisions: [ADR-0005](../../../../../docs/adr/0005-native-config-superset-and-compat.md) (keys
//!   are ArchUnitNET's method names in camelCase), [ADR-0007](../../../../../docs/adr/0007-vacuous-rules-fail-by-default.md)
//! - Requirement: [FR-CLI-04](../../../../../docs/prd.md#fr-cli-04)
//!
//! Every `.cs` file under the directory is parsed ([`super::csharp`]); names resolve against the
//! declarations of the solution around it ([`super::types`]). In each method the statements are
//! read in order, locals bound as they are declared, and every call that runs a rule (`Check`,
//! `AssertNoViolations`, `HasNoViolations`, NetArchTest's `GetResult`) is one candidate: its
//! receiver is evaluated to a fluent chain, following locals, fields, properties, constructor
//! assignments and base classes, and the chain is mapped call by call.
//!
//! | C# | Written as |
//! | --- | --- |
//! | `Types()` ... `PropertyMembers()`, `Types(true)` | `select.kind`, `includeReferenced`, and `select.language: dotnet` |
//! | a predicate or condition | its key, the method name in camelCase, checked against `rb-config`'s vocabulary |
//! | `And()` / `Or()`, `AndShould()` / `OrShould()` (ArchUnitNET) | folded left to right into `all` / `any` |
//! | `And()` / `Or()` (NetArchTest, both sides of `Should()`) | each `Or()` starts a group: `any` of the `all` of each `And()` run |
//! | `...TypesThat()` and what follows | a nested selector |
//! | `typeof(X)`, `GetClassOfType(typeof(X))` | the full name |
//! | a provider (`Types().That()...`, held in a field) | a nested selector |
//! | `Because(...)`, `WithoutRequiringPositiveResults()` | `because`, `allowEmpty: true` |
//! | any NetArchTest chain | `allowEmpty: true`: NetArchTest's `GetResult()` passes a rule whose selection is empty, so the imported rule keeps that verdict; delete the key to have Rulebearing report the selection as vacuous ([ADR-0007](../../../../../docs/adr/0007-vacuous-rules-fail-by-default.md)), as gate 2 records it |
//! | `Slices().Matching(p)` ... | a slice rule |
//! | `Types.InAssembly(typeof(X).Assembly)` (NetArchTest) | `resideInAssembly` on X's assembly, then the README table; the assembly also joins `languages.dotnet.assemblies` |
//! | `new ArchLoader().LoadAssemblies(...)` | `languages.dotnet` |
//! | a helper the sources declare whose body is one returned expression (`GetTypesThat()`, `namespaceof<T>()`, an extension method over its `this` or C# 14 `extension(...)` receiver) | its expression, evaluated with the arguments, defaults, `params` array and type arguments bound |
//! | a constructor parameter of a class that implements `IClassFixture<T>` | an instance of `T`, whose members are read |
//! | `selection.GetTypes().Select(t => t.FullName)` (NetArchTest), given to a dependency search | the selection's types and the types nested in them, a nested selector of .NET types |
//!
//! A chain is written commented out, with the reason, when it holds a custom predicate (`stays in
//! ArchUnitNET: custom predicate`), when a method has no key, when a value cannot be read from
//! source (a helper with more than one statement or an overload chosen by type, a member
//! reference, a type whose namespace is not in the sources read), when the tests use
//! NetArchTest.eNhancedEdition, which is not NetArchTest 1.3.2, or when the test expects the rule
//! to fail (`Assert.False`, `Assert.Throws`, `AssertOnlyViolations`). A test in a file that uses
//! either library and runs no rule at all (it queries the architecture and asserts in C#) is
//! written as a commented-out entry that stays with the incumbent, as a custom predicate does.
//! The importer never guesses.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rb_config::elements::{Side, VOCABULARY, ValueKind, split_key};
use serde_json::Value;

use super::ImportError;
use super::csharp::{self, Arg, Expr, Part, SourceFile, Stmt, TypeDecl};
use super::netarchtest;
use super::types::{self, Index, TypeInfo};
use super::yaml::{Document, Item, Node};

/// What to import.
#[derive(Debug, Clone)]
pub struct Request {
    /// The directory of C# tests.
    pub dir: PathBuf,
    /// The directory as shown in comments.
    pub shown: String,
    /// More directories whose declarations resolve names.
    pub sources: Vec<PathBuf>,
    /// A graph document whose code layer names more types.
    pub graph: Option<PathBuf>,
    /// The working directory, which loader paths are relative to.
    pub cwd: PathBuf,
}

/// A value read from source.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    /// A string.
    Str(String),
    /// A boolean.
    Bool(bool),
    /// A number, as written.
    Num(String),
    /// `null`.
    Null,
    /// A type.
    Type(TypeInfo),
    /// An assembly (or its `AssemblyName`).
    Assembly {
        /// The simple name.
        name: String,
        /// The folder of the project that builds it, when known.
        project: Option<String>,
    },
    /// A list.
    List(Vec<Val>),
    /// A tuple.
    Tuple(Vec<Val>),
    /// A fluent chain: a rule, a provider, or a loader.
    Chain(Chain),
    /// An instance of a class the sources declare (`new Helper()`).
    Instance(String),
    /// An enum member, as written (`StringComparison.Ordinal`).
    Enum(String),
    /// The full names of the types a NetArchTest selection returns
    /// (`GetTypes().Select(t => t.FullName)`), the selection kept as its chain.
    Names(Chain),
    /// The same names, the selection mapped to its nested selector.
    Selected(Node),
}

impl Val {
    /// The value in words, for a reason.
    pub fn describe(&self) -> String {
        match self {
            Self::Str(s) => format!("the string {s:?}"),
            Self::Bool(b) => format!("`{b}`"),
            Self::Num(n) => format!("the number {n}"),
            Self::Null => "`null`".into(),
            Self::Type(t) => format!("the type `{}`", t.full),
            Self::Assembly { name, .. } => format!("the assembly `{name}`"),
            Self::List(_) => "a list".into(),
            Self::Tuple(_) => "a tuple".into(),
            Self::Chain(c) => format!("the chain `{}`", c.text),
            Self::Instance(c) => format!("an instance of `{c}`"),
            Self::Enum(e) => format!("`{e}`"),
            Self::Names(c) => format!("the names of the types `{}` selects", c.text),
            Self::Selected(_) => "the names of the types of a selection".into(),
        }
    }
}

/// Where a chain starts.
#[derive(Debug, Clone, PartialEq)]
pub enum Root {
    /// `Types()`, `Classes()` ... with the `select.kind` and `Types(true)`.
    Elements {
        /// The kind.
        kind: &'static str,
        /// `Types(true)`.
        referenced: bool,
    },
    /// `SliceRuleDefinition.Slices()`.
    Slices,
    /// NetArchTest's `Types.InAssembly(...)` and the other `Types.In...` roots.
    NetArchTest {
        /// The method.
        method: String,
        /// Its arguments.
        args: Vec<Result<Val, String>>,
    },
    /// `new ArchLoader()`.
    Loader,
}

/// One call in a chain.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    /// The method.
    pub name: String,
    /// Its arguments, each read or the reason it could not be.
    pub args: Vec<Result<Val, String>>,
}

/// A fluent chain.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    /// Where it starts.
    pub root: Root,
    /// The calls after the root.
    pub calls: Vec<Call>,
    /// The C#, whitespace normalised.
    pub text: String,
}

/// `ArchRuleDefinition`'s roots and their `select.kind`.
const ROOTS: &[(&str, &str)] = &[
    ("Types", "type"),
    ("Classes", "class"),
    ("Interfaces", "interface"),
    ("Attributes", "attribute"),
    ("Members", "member"),
    ("FieldMembers", "field"),
    ("MethodMembers", "method"),
    ("PropertyMembers", "property"),
];

/// NetArchTest's `Types.` roots.
const NETARCHTEST_ROOTS: &[&str] = &[
    "InAssembly",
    "InAssemblies",
    "InNamespace",
    "InCurrentDomain",
    "FromFile",
    "FromPath",
];

/// Calls that stay in ArchUnitNET or NetArchTest.
const CUSTOM: &[&str] = &[
    "FollowCustomPredicate",
    "FollowCustomCondition",
    "MeetCustomRule",
];

/// `Architecture.Get...OfType(typeof(X))`.
const OF_TYPE: &[&str] = &[
    "GetClassOfType",
    "GetInterfaceOfType",
    "GetAttributeOfType",
    "GetStructOfType",
    "GetEnumOfType",
    "GetITypeOfType",
    "GetTypeOfType",
    "GetIType",
];

/// What a test expects of the rule it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    /// The rule holds.
    Passes,
    /// The rule is broken.
    Fails,
}

impl Expect {
    const fn flip(self) -> Self {
        match self {
            Self::Passes => Self::Fails,
            Self::Fails => Self::Passes,
        }
    }
}

/// Calls that run a rule.
const SINKS: &[&str] = &[
    "Check",
    "AssertNoViolations",
    "AssertOnlyViolations",
    "AssertAnyViolations",
    "HasNoViolations",
    "GetResult",
];

/// The parsed program: every file read and the index over their declarations.
pub struct Program {
    /// Every parsed file with its path.
    pub files: Vec<(PathBuf, SourceFile)>,
    /// The type index.
    pub index: Index,
    /// Which files are the tests to import (the others only declare types).
    tests: usize,
    /// The test project references NetArchTest.eNhancedEdition, not NetArchTest.
    pub netarchtest_enhanced: bool,
}

impl Program {
    /// Parses `tests` (the files to import) and `declarations` (files read only for their types).
    ///
    /// # Errors
    /// [`ImportError`] when a file cannot be read.
    pub fn read(
        tests: &[(PathBuf, String)],
        declarations: &[(PathBuf, String)],
        stop: &Path,
        cwd: &Path,
    ) -> Result<Self, ImportError> {
        let mut files = Vec::new();
        for (path, shown) in tests.iter().chain(declarations) {
            let text = std::fs::read_to_string(path).map_err(|e| ImportError::Read {
                file: shown.clone(),
                reason: e.to_string(),
            })?;
            files.push((path.clone(), csharp::parse(&text, shown)?));
        }
        let mut index = Index::default();
        types::index_files(&mut index, &files, stop, cwd);
        Ok(Self {
            files,
            index,
            tests: tests.len(),
            netarchtest_enhanced: false,
        })
    }

    /// A program over sources already parsed, with an index the caller has filled (from a
    /// graph document, say); the files' own declarations are added to it, with no assembly.
    pub fn from_parts(files: Vec<(PathBuf, SourceFile)>, mut index: Index) -> Self {
        for (_, file) in &files {
            index.add_source(file, None);
        }
        let tests = files.len();
        Self {
            files,
            index,
            tests,
            netarchtest_enhanced: false,
        }
    }

    /// Whether the sources declare an extension method of this name that does not itself run
    /// the rule (a project's own helper over a fluent chain).
    fn declares_extension(&self, name: &str) -> bool {
        self.files.iter().any(|(_, file)| {
            file.types
                .iter()
                .flat_map(|t| t.methods.iter())
                .any(|m| m.extension && m.name == name)
        })
    }

    /// Whether an expression is a call of a method the sources declare, whose result a rule-running
    /// call then runs (`Built().GetResult()`): a rule the importer reports even when it cannot
    /// read it.
    fn calls_a_helper(&self, expr: &Expr) -> bool {
        let Expr::Call(function, _) = expr else {
            return false;
        };
        let (Expr::Name(name) | Expr::Generic(name, _) | Expr::Member(_, name)) = function.as_ref()
        else {
            return false;
        };
        self.files.iter().any(|(_, file)| {
            file.types
                .iter()
                .flat_map(|t| t.methods.iter())
                .any(|m| &m.name == name)
        })
    }

    fn decl(&self, full: &str) -> Option<(&PathBuf, &TypeDecl)> {
        self.files.iter().find_map(|(path, file)| {
            file.types
                .iter()
                .find(|t| t.full_name() == full)
                .map(|t| (path, t))
        })
    }
}

/// Where an expression is evaluated.
#[derive(Clone)]
struct Scope<'p> {
    path: &'p Path,
    decl: &'p TypeDecl,
    locals: Vec<(String, Result<Val, String>, Expr)>,
    /// A generic helper's type parameters, bound to the types of the call.
    types: Vec<(String, TypeInfo)>,
}

impl<'p> Scope<'p> {
    /// A scope in a type declaration, with no locals.
    const fn new(path: &'p Path, decl: &'p TypeDecl) -> Self {
        Self {
            path,
            decl,
            locals: Vec::new(),
            types: Vec::new(),
        }
    }

    fn enclosing(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut prefix = self.decl.namespace.clone();
        for (name, arity) in self
            .decl
            .outer
            .iter()
            .chain(std::iter::once(&(self.decl.name.clone(), self.decl.arity)))
        {
            prefix = if prefix.is_empty() {
                csharp::arity_name(name, *arity)
            } else {
                format!("{prefix}.{}", csharp::arity_name(name, *arity))
            };
            out.push(prefix.clone());
        }
        out
    }

    fn local(&self, name: &str) -> Option<&(String, Result<Val, String>, Expr)> {
        self.locals.iter().rev().find(|(n, _, _)| n == name)
    }
}

const DEPTH: usize = 24;

impl Program {
    fn resolve_type(&self, scope: &Scope<'_>, name: &csharp::TypeName) -> Result<TypeInfo, String> {
        if let [(single, 0)] = name.segments.as_slice()
            && !name.global
            && let Some((_, bound)) = scope.types.iter().rev().find(|(t, _)| t == single)
        {
            return Ok(bound.clone());
        }
        self.index.resolve(
            name,
            &scope.decl.namespace,
            &scope.enclosing(),
            &scope.decl.usings,
        )
    }

    /// A class member's value, looked up in the class and its bases.
    fn member(&self, full: &str, name: &str, depth: usize) -> Option<Result<Val, String>> {
        let (path, decl) = self.decl(full)?;
        if let Some(member) = decl.members.iter().rev().find(|m| m.name == name) {
            let scope = Scope::new(path, decl);
            return Some(self.eval(&scope, &member.value, depth + 1));
        }
        let base_scope = Scope::new(path, decl);
        for base in &decl.bases {
            if let Ok(info) = self.resolve_type(&base_scope, base)
                && let Some(found) = self.member(&info.full, name, depth + 1)
            {
                return Some(found);
            }
        }
        None
    }

    fn has_member(&self, full: &str, name: &str, depth: usize) -> bool {
        if depth > DEPTH {
            return false;
        }
        let Some((path, decl)) = self.decl(full) else {
            return false;
        };
        if decl.members.iter().any(|m| m.name == name) {
            return true;
        }
        let scope = Scope::new(path, decl);
        decl.bases.iter().any(|b| {
            self.resolve_type(&scope, b)
                .is_ok_and(|info| self.has_member(&info.full, name, depth + 1))
        })
    }

    fn bound(&self, scope: &Scope<'_>, name: &str) -> bool {
        scope.local(name).is_some() || self.has_member(&scope.decl.full_name(), name, 0)
    }

    fn eval_args(&self, scope: &Scope<'_>, args: &[Arg], depth: usize) -> Vec<Result<Val, String>> {
        args.iter()
            .map(|a| self.eval(scope, &a.value, depth + 1))
            .collect()
    }

    /// Evaluates an expression, as far as source allows.
    fn eval(&self, scope: &Scope<'_>, expr: &Expr, depth: usize) -> Result<Val, String> {
        if depth > DEPTH {
            return Err(format!("`{}` refers to itself", csharp::render(expr)));
        }
        let cannot = || {
            Err(format!(
                "`{}` cannot be read from source",
                csharp::render(expr)
            ))
        };
        match expr {
            Expr::Str(s) => Ok(Val::Str(s.clone())),
            Expr::Bool(b) => Ok(Val::Bool(*b)),
            Expr::Num(n) => Ok(Val::Num(n.clone())),
            Expr::Null => Ok(Val::Null),
            Expr::Interpolated(parts) => {
                let mut out = String::new();
                for part in parts {
                    match part {
                        Part::Text(t) => out.push_str(t),
                        Part::Hole(e) => match self.eval(scope, e, depth + 1)? {
                            Val::Str(s) | Val::Num(s) => out.push_str(&s),
                            other => {
                                return Err(format!(
                                    "`{}` interpolates {}",
                                    csharp::render(expr),
                                    other.describe()
                                ));
                            }
                        },
                    }
                }
                Ok(Val::Str(out))
            }
            Expr::Binary(l, op, r) if op == "+" => {
                match (
                    self.eval(scope, l, depth + 1)?,
                    self.eval(scope, r, depth + 1)?,
                ) {
                    (Val::Str(a), Val::Str(b)) => Ok(Val::Str(a + &b)),
                    _ => cannot(),
                }
            }
            Expr::Name(name) => {
                if let Some((_, value, _)) = scope.local(name) {
                    return value.clone();
                }
                if let Some(found) = self.member(&scope.decl.full_name(), name, depth) {
                    return found;
                }
                if let Some(found) = self.fixture(scope, name) {
                    return found;
                }
                Err(format!(
                    "`{name}` is not a local, field or property the importer can read"
                ))
            }
            Expr::TypeOf(t) => self.resolve_type(scope, t).map(Val::Type),
            Expr::Array(items) => items
                .iter()
                .map(|e| self.eval(scope, e, depth + 1))
                .collect::<Result<Vec<_>, _>>()
                .map(Val::List),
            Expr::Tuple(args) => args
                .iter()
                .map(|a| self.eval(scope, &a.value, depth + 1))
                .collect::<Result<Vec<_>, _>>()
                .map(Val::Tuple),
            Expr::New(Some(t), args, init) => {
                let last = t.segments.last().map_or("", |(n, _)| n.as_str());
                if last == "ArchLoader" {
                    return Ok(Val::Chain(Chain {
                        root: Root::Loader,
                        calls: Vec::new(),
                        text: csharp::render(expr),
                    }));
                }
                if !init.is_empty() || matches!(last, "List" | "HashSet" | "Collection") {
                    return init
                        .iter()
                        .map(|e| self.eval(scope, e, depth + 1))
                        .collect::<Result<Vec<_>, _>>()
                        .map(Val::List);
                }
                match self.resolve_type(scope, t) {
                    Ok(info) if args.is_empty() && self.decl(&info.full).is_some() => {
                        Ok(Val::Instance(info.full))
                    }
                    _ => cannot(),
                }
            }
            Expr::Member(receiver, name) => self.eval_member(scope, expr, receiver, name, depth),
            Expr::Call(function, args) => self.eval_call(scope, expr, function, args, depth),
            _ => cannot(),
        }
    }

    fn eval_member(
        &self,
        scope: &Scope<'_>,
        expr: &Expr,
        receiver: &Expr,
        name: &str,
        depth: usize,
    ) -> Result<Val, String> {
        if let Expr::Name(n) = receiver {
            if n == "this" {
                return self
                    .member(&scope.decl.full_name(), name, depth)
                    .unwrap_or_else(|| Err(format!("`this.{name}` is not a field or property")));
            }
            if !self.bound(scope, n) {
                // A type name: a static member of a declared class, or an enum member.
                let written = csharp::TypeName {
                    segments: vec![(n.clone(), 0)],
                    global: false,
                    text: n.clone(),
                };
                if let Ok(info) = self.resolve_type(scope, &written)
                    && let Some(found) = self.member(&info.full, name, depth)
                {
                    return found;
                }
                if n.chars().next().is_some_and(char::is_uppercase) {
                    return Ok(Val::Enum(format!("{n}.{name}")));
                }
            }
        }
        // ArchUnitNET's `IType.Namespace.FullName`.
        if name == "FullName"
            && let Expr::Member(inner, namespace) = receiver
            && namespace == "Namespace"
            && let Ok(Val::Type(t)) = self.eval(scope, inner, depth + 1)
        {
            return Ok(Val::Str(t.namespace));
        }
        match (self.eval(scope, receiver, depth + 1)?, name) {
            (Val::Type(t), "FullName") => Ok(Val::Str(t.full)),
            (Val::Type(t), "Name") => Ok(Val::Str(t.name)),
            (Val::Type(t), "Namespace") => Ok(Val::Str(t.namespace)),
            (Val::Type(t), "Assembly") => match t.assembly {
                Some(assembly) => Ok(Val::Assembly {
                    project: self.index.project_of(&assembly).map(str::to_owned),
                    name: assembly,
                }),
                None => Err(format!(
                    "the assembly of `{}` is not known from the sources read",
                    t.full
                )),
            },
            (Val::Assembly { name, .. }, "Name") => Ok(Val::Str(name)),
            (Val::Instance(class), member) => {
                self.member(&class, member, depth).unwrap_or_else(|| {
                    Err(format!(
                        "`{class}` has no field or property `{member}` the importer can read"
                    ))
                })
            }
            _ => Err(format!(
                "`{}` cannot be read from source",
                csharp::render(expr)
            )),
        }
    }

    fn eval_call(
        &self,
        scope: &Scope<'_>,
        expr: &Expr,
        function: &Expr,
        args: &[Arg],
        depth: usize,
    ) -> Result<Val, String> {
        let text = csharp::render(expr);
        match function {
            Expr::Name(name) if !self.bound(scope, name) => {
                self.name_call(scope, text, name, &[], args, depth)
            }
            Expr::Generic(name, types) => self.name_call(scope, text, name, types, args, depth),
            Expr::Member(receiver, method) => {
                // `Assembly.Load`, or `System.Reflection.Assembly.Load` written out.
                let owner = match receiver.as_ref() {
                    // C#'s "Color Color" rule: a property named `Types` of type `Types` does not
                    // hide NetArchTest's static `Types.InAssembly(...)`.
                    Expr::Name(owner)
                        if owner == "Types" && NETARCHTEST_ROOTS.contains(&method.as_str()) =>
                    {
                        Some(owner.clone())
                    }
                    Expr::Name(owner) if !self.bound(scope, owner) => Some(owner.clone()),
                    qualified @ Expr::Member(..) => {
                        let written = csharp::render(qualified);
                        written
                            .strip_prefix("System.Reflection.")
                            .filter(|rest| !rest.contains('.'))
                            .map(str::to_owned)
                    }
                    _ => None,
                };
                if let Some(owner) = owner
                    && let Some(found) = self
                        .static_call(scope, &text, &owner, method, args, depth)
                        .or_else(|| self.static_helper(scope, &text, &owner, method, args, depth))
                {
                    return found;
                }
                if OF_TYPE.contains(&method.as_str()) {
                    return match self.eval_args(scope, args, depth).into_iter().next() {
                        Some(Ok(Val::Type(t))) => Ok(Val::Type(t)),
                        Some(Ok(other)) => Err(format!("{} is not a type", other.describe())),
                        Some(Err(e)) => Err(e),
                        None => Err(format!("`{text}` names no type")),
                    };
                }
                match self.eval(scope, receiver, depth + 1)? {
                    Val::Chain(chain)
                        if !library_member(method) && self.declares_extension(method) =>
                    {
                        self.extension_call(scope, &text, Val::Chain(chain), method, args, depth)
                    }
                    Val::Chain(chain) if method == "Select" => names_of(chain, args, &text),
                    names @ Val::Names(_)
                        if args.is_empty()
                            && matches!(method.as_str(), "Distinct" | "ToArray" | "ToList") =>
                    {
                        Ok(names)
                    }
                    Val::Chain(mut chain) => {
                        chain.calls.push(Call {
                            name: method.clone(),
                            args: self.eval_args(scope, args, depth),
                        });
                        chain.text = format!(
                            "{}.{method}({})",
                            chain.text,
                            args.iter()
                                .map(|a| csharp::render(&a.value))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                        Ok(Val::Chain(chain))
                    }
                    Val::Type(t) if method == "GetTypeInfo" => Ok(Val::Type(t)),
                    assembly @ Val::Assembly { .. } if method == "GetName" => Ok(assembly),
                    other => Err(format!(
                        "`{text}` calls `{method}` on {}, which the importer does not evaluate",
                        other.describe()
                    )),
                }
            }
            _ => Err(format!("`{text}` cannot be read from source")),
        }
    }

    fn elements_root(
        &self,
        scope: &Scope<'_>,
        kind: &'static str,
        args: &[Arg],
        depth: usize,
        text: String,
    ) -> Val {
        Val::Chain(Chain {
            root: Root::Elements {
                kind,
                referenced: matches!(
                    self.eval_args(scope, args, depth).first(),
                    Some(Ok(Val::Bool(true)))
                ),
            },
            calls: Vec::new(),
            text,
        })
    }

    /// A call of a bare name: an `ArchRuleDefinition` root through `using static`, `Slices()`,
    /// `nameof(...)`, or a helper method the sources declare.
    fn name_call(
        &self,
        scope: &Scope<'_>,
        text: String,
        name: &str,
        types: &[csharp::TypeName],
        args: &[Arg],
        depth: usize,
    ) -> Result<Val, String> {
        if !types.is_empty() {
            return self.helper_call(scope, &text, name, types, args, depth);
        }
        if let Some((_, kind)) = ROOTS.iter().find(|(r, _)| *r == name) {
            return Ok(self.elements_root(scope, kind, args, depth, text));
        }
        if name == "Slices" {
            return Ok(Val::Chain(Chain {
                root: Root::Slices,
                calls: Vec::new(),
                text,
            }));
        }
        if name == "nameof" {
            return args
                .first()
                .map(|a| csharp::render(&a.value))
                .and_then(|t| t.rsplit('.').next().map(str::to_owned))
                .map(Val::Str)
                .ok_or_else(|| "`nameof()` names nothing".to_owned());
        }
        self.helper_call(scope, &text, name, types, args, depth)
    }

    /// A call of a static method on a type name, or `None` when it is not one the importer
    /// knows.
    fn static_call(
        &self,
        scope: &Scope<'_>,
        text: &str,
        owner: &str,
        method: &str,
        args: &[Arg],
        depth: usize,
    ) -> Option<Result<Val, String>> {
        let first = || self.eval_args(scope, args, depth).into_iter().next();
        let assembly = |name: String| Val::Assembly {
            project: self.index.project_of(&name).map(str::to_owned),
            name,
        };
        Some(match (owner, method) {
            ("ArchRuleDefinition", root) => {
                let (_, kind) = ROOTS.iter().find(|(r, _)| *r == root)?;
                Ok(self.elements_root(scope, kind, args, depth, text.to_owned()))
            }
            ("SliceRuleDefinition", "Slices") => Ok(Val::Chain(Chain {
                root: Root::Slices,
                calls: Vec::new(),
                text: text.to_owned(),
            })),
            ("Types", m) if NETARCHTEST_ROOTS.contains(&m) => Ok(Val::Chain(Chain {
                root: Root::NetArchTest {
                    method: m.to_owned(),
                    args: self.eval_args(scope, args, depth),
                },
                calls: Vec::new(),
                text: text.to_owned(),
            })),
            ("Assembly", "GetAssembly") => match first() {
                Some(Ok(Val::Type(t))) => assembly_of(&self.index, &t),
                Some(Ok(other)) => Err(format!("{} is not a type", other.describe())),
                Some(Err(e)) => Err(e),
                None => Err(format!("`{text}` names no type")),
            },
            ("Assembly", "Load" | "LoadFrom" | "LoadWithPartialName") => match first() {
                Some(Ok(Val::Str(name))) => Ok(assembly(
                    name.split(',')
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .trim_end_matches(".dll")
                        .to_owned(),
                )),
                _ => Err(format!("`{text}` does not name an assembly literally")),
            },
            ("Assembly", "GetExecutingAssembly" | "GetCallingAssembly") => {
                match self.index.assembly_of_file(scope.path) {
                    Some(name) => Ok(assembly(name.to_owned())),
                    None => Err(format!(
                        "`{text}`: no project file was found above {}",
                        scope.path.display()
                    )),
                }
            }
            _ => return None,
        })
    }
}

/// The helpers: methods the sources declare whose body is one returned expression, evaluated with
/// their arguments bound to their parameters.
impl Program {
    /// The methods a bare call of `name` can reach from `scope`: the class and its bases, the
    /// enclosing classes, and the classes a `using static` names.
    fn reachable(&self, scope: &Scope<'_>, name: &str) -> Vec<(&Path, &TypeDecl, &csharp::Method)> {
        let mut classes: Vec<String> = Vec::new();
        let mut pending = vec![scope.decl.full_name()];
        pending.extend(scope.enclosing());
        while let Some(full) = pending.pop() {
            if classes.contains(&full) || classes.len() > DEPTH {
                continue;
            }
            if let Some((path, decl)) = self.decl(&full) {
                let base_scope = Scope::new(path, decl);
                pending.extend(
                    decl.bases
                        .iter()
                        .filter_map(|b| self.resolve_type(&base_scope, b).ok())
                        .map(|info| info.full),
                );
            }
            classes.push(full);
        }
        let globals = self
            .files
            .iter()
            .flat_map(|(_, f)| f.global_usings.statics.iter());
        classes.extend(scope.decl.usings.statics.iter().chain(globals).cloned());
        let mut out = Vec::new();
        for full in &classes {
            if let Some((path, decl)) = self.decl(full) {
                for method in decl
                    .methods
                    .iter()
                    .filter(|m| m.name == name && !m.extension)
                {
                    if !out
                        .iter()
                        .any(|(_, _, m): &(&Path, &TypeDecl, &csharp::Method)| {
                            std::ptr::eq(*m, method)
                        })
                    {
                        out.push((path.as_path(), decl, method));
                    }
                }
            }
        }
        out
    }

    /// A call of a helper method the sources declare, by its bare name.
    fn helper_call(
        &self,
        scope: &Scope<'_>,
        text: &str,
        name: &str,
        types: &[csharp::TypeName],
        args: &[Arg],
        depth: usize,
    ) -> Result<Val, String> {
        let found = self.reachable(scope, name);
        if found.is_empty() {
            return Err(format!("`{text}` cannot be read from source"));
        }
        let (path, decl, method) = one_overload(&found, text, name, args.len())?;
        let site = Site {
            text,
            receiver: None,
            types,
            args,
        };
        self.invoke(scope, site, (path, decl, method), depth)
    }

    /// `Helper.Method(...)` on a class the sources declare, or `None` when the owner is not one.
    fn static_helper(
        &self,
        scope: &Scope<'_>,
        text: &str,
        owner: &str,
        method: &str,
        args: &[Arg],
        depth: usize,
    ) -> Option<Result<Val, String>> {
        let written = csharp::TypeName {
            segments: vec![(owner.to_owned(), 0)],
            global: false,
            text: owner.to_owned(),
        };
        let info = self.resolve_type(scope, &written).ok()?;
        let (path, decl) = self.decl(&info.full)?;
        let found: Vec<(&Path, &TypeDecl, &csharp::Method)> = decl
            .methods
            .iter()
            .filter(|m| m.name == method && !m.extension)
            .map(|m| (path.as_path(), decl, m))
            .collect();
        if found.is_empty() {
            return None;
        }
        Some(
            one_overload(&found, text, method, args.len()).and_then(|target| {
                let site = Site {
                    text,
                    receiver: None,
                    types: &[],
                    args,
                };
                self.invoke(scope, site, target, depth)
            }),
        )
    }

    /// `receiver.Method(...)` where the sources declare an extension method `Method`.
    fn extension_call(
        &self,
        scope: &Scope<'_>,
        text: &str,
        receiver: Val,
        method: &str,
        args: &[Arg],
        depth: usize,
    ) -> Result<Val, String> {
        let mut found: Vec<(&Path, &TypeDecl, &csharp::Method)> = Vec::new();
        for (path, file) in &self.files {
            for decl in &file.types {
                for m in decl
                    .methods
                    .iter()
                    .filter(|m| m.extension && m.name == method)
                {
                    found.push((path.as_path(), decl, m));
                }
            }
        }
        let target = one_overload(&found, text, method, args.len() + 1)?;
        let site = Site {
            text,
            receiver: Some(receiver),
            types: &[],
            args,
        };
        self.invoke(scope, site, target, depth)
    }

    /// Evaluates a helper's returned expression with its parameters bound: the receiver of an
    /// extension method, the arguments by position or name, a `params` array, the defaults, and
    /// the type arguments of a generic helper.
    fn invoke(
        &self,
        caller: &Scope<'_>,
        site: Site<'_>,
        (path, decl, method): (&Path, &TypeDecl, &csharp::Method),
        depth: usize,
    ) -> Result<Val, String> {
        let Site {
            text,
            receiver,
            types,
            args,
        } = site;
        let name = &method.name;
        let [Stmt::Expr { expr: body, .. }] = method.body.as_slice() else {
            return Err(format!(
                "`{text}` calls `{name}`, whose body is not one returned expression, which the importer does not evaluate"
            ));
        };
        if types.len() != method.type_parameters.len() {
            return Err(format!(
                "`{text}` calls the generic `{name}` without its type arguments, which the importer does not infer"
            ));
        }
        let mut inner = Scope::new(path, decl);
        for (parameter, written) in method.type_parameters.iter().zip(types) {
            inner
                .types
                .push((parameter.clone(), self.resolve_type(caller, written)?));
        }
        let mut values: Vec<Option<Result<Val, String>>> = vec![None; method.parameters.len()];
        let mut next = 0;
        if let Some(receiver) = receiver {
            if let Some(slot) = values.first_mut() {
                *slot = Some(Ok(receiver));
            }
            next = 1;
        }
        let last = method.parameters.len().checked_sub(1);
        let mut rest = Vec::new();
        for arg in args {
            let value = self.eval(caller, &arg.value, depth + 1);
            if let Some(named) = &arg.name {
                match method.parameters.iter().position(|p| p == named) {
                    Some(at) => values[at] = Some(value),
                    None => {
                        return Err(format!(
                            "`{text}` names `{named}`, which `{name}` does not take"
                        ));
                    }
                }
            } else if method.params && Some(next) == last {
                rest.push(value);
            } else if next < values.len() {
                values[next] = Some(value);
                next += 1;
            } else {
                return Err(format!(
                    "`{text}` passes more arguments than `{name}` takes"
                ));
            }
        }
        if method.params
            && let Some(at) = last
            && values[at].is_none()
        {
            values[at] = Some(match rest.as_slice() {
                [Ok(Val::List(items))] => Ok(Val::List(items.clone())),
                _ => rest
                    .into_iter()
                    .collect::<Result<Vec<_>, _>>()
                    .map(Val::List),
            });
        }
        for (at, parameter) in method.parameters.iter().enumerate() {
            let value = match values[at].take() {
                Some(value) => value,
                None => match method.defaults.get(at).and_then(Option::as_ref) {
                    Some(default) => self.eval(&inner, default, depth + 1),
                    None => Err(format!("`{text}` does not give `{name}` its `{parameter}`")),
                },
            };
            inner
                .locals
                .push((parameter.clone(), value, Expr::Name(parameter.clone())));
        }
        self.eval(&inner, body, depth + 1)
            .map_err(|reason| format!("{reason} (in `{name}`, called by `{text}`)"))
    }

    /// A parameter the test framework fills with a class fixture: `C(Fixture fixture)`, primary
    /// or not, in a class that implements `IClassFixture<Fixture>`. xUnit builds the fixture with
    /// its parameterless constructor, so it is an instance of a class the sources declare.
    fn fixture(&self, scope: &Scope<'_>, name: &str) -> Option<Result<Val, String>> {
        let decl = scope.decl;
        let constructors = decl.methods.iter().filter(|m| m.name == decl.name);
        let written = decl
            .primary
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, t)| t.clone())
            .or_else(|| {
                constructors
                    .flat_map(|m| m.parameters.iter().zip(&m.parameter_types))
                    .find(|(n, _)| *n == name)
                    .map(|(_, t)| t.clone())
            })?;
        let Some(written) = written else {
            return Some(Err(format!(
                "`{name}` is a constructor parameter whose type the importer cannot read"
            )));
        };
        let fixture = format!("IClassFixture<{}>", written.text.replace(' ', ""));
        let class_fixture = decl.bases.iter().any(|b| {
            let base = b.text.replace(' ', "");
            base == fixture || base.ends_with(&format!(".{fixture}"))
        });
        if !class_fixture {
            return Some(Err(format!(
                "`{name}` is a constructor parameter the test framework fills, which the importer does not evaluate"
            )));
        }
        Some(match self.resolve_type(scope, &written) {
            Ok(info) if self.decl(&info.full).is_some() => Ok(Val::Instance(info.full)),
            Ok(info) => Err(format!(
                "`{name}` is the class fixture `{}`, which the sources read do not declare",
                info.full
            )),
            Err(reason) => Err(reason),
        })
    }
}

/// A call of a helper: its C#, the receiver of an extension method, and the type arguments and
/// arguments written.
struct Site<'a> {
    text: &'a str,
    receiver: Option<Val>,
    types: &'a [csharp::TypeName],
    args: &'a [Arg],
}

/// The one method of `found` a call with `given` arguments can reach; an overload the importer
/// cannot choose between by the number of arguments is refused.
fn one_overload<'m>(
    found: &[(&'m Path, &'m TypeDecl, &'m csharp::Method)],
    text: &str,
    name: &str,
    given: usize,
) -> Result<(&'m Path, &'m TypeDecl, &'m csharp::Method), String> {
    let fits: Vec<_> = found
        .iter()
        .filter(|(_, _, m)| {
            let required = m
                .defaults
                .iter()
                .enumerate()
                .filter(|(at, d)| d.is_none() && !(m.params && at + 1 == m.parameters.len()))
                .count();
            given >= required && (m.params || given <= m.parameters.len())
        })
        .copied()
        .collect();
    match fits.as_slice() {
        [one] => Ok(*one),
        [] => Err(format!(
            "`{text}` calls `{name}` with {given} arguments, which no declaration of it takes"
        )),
        _ => Err(format!(
            "`{text}` calls `{name}`, overloaded in the sources, which the importer does not resolve"
        )),
    }
}

/// Whether ArchUnitNET or NetArchTest has a member of this name on a fluent chain, which C# calls
/// in preference to an extension method of the same name.
fn library_member(name: &str) -> bool {
    const STRUCTURE: &[&str] = &[
        "That",
        "As",
        "And",
        "Or",
        "Should",
        "ShouldNot",
        "AndShould",
        "OrShould",
        "Because",
        "WithoutRequiringPositiveResults",
        "GetTypes",
        "Select",
    ];
    if STRUCTURE.contains(&name) || SINKS.contains(&name) || CUSTOM.contains(&name) {
        return true;
    }
    let key = lower_first(name);
    [Side::Where, Side::Should].into_iter().any(|side| {
        let (base, _, _) = split_key(&key, side);
        VOCABULARY.iter().any(|(n, _, _, _)| *n == base)
    })
}

/// `selection.GetTypes().Select(t => t.FullName)` on a NetArchTest chain: the full names of the
/// types the selection returns.
fn names_of(mut chain: Chain, args: &[Arg], text: &str) -> Result<Val, String> {
    let full_name = matches!(
        args,
        [Arg { value: Expr::Lambda(parameter, body), .. }]
            if matches!(body.as_ref(), Expr::Member(of, n) if n == "FullName" && **of == Expr::Name(parameter.trim().to_owned()))
    );
    let from_types = chain
        .calls
        .last()
        .is_some_and(|c| c.name == "GetTypes" && c.args.is_empty());
    if !(full_name && from_types && matches!(chain.root, Root::NetArchTest { .. })) {
        return Err(format!("`{text}` cannot be read from source"));
    }
    chain.calls.pop();
    if let Some(at) = chain.text.rfind(".GetTypes()") {
        chain.text.truncate(at);
    }
    Ok(Val::Names(chain))
}

fn assembly_of(index: &Index, t: &TypeInfo) -> Result<Val, String> {
    match &t.assembly {
        Some(name) => Ok(Val::Assembly {
            project: index.project_of(name).map(str::to_owned),
            name: name.clone(),
        }),
        None => Err(format!(
            "the assembly of `{}` is not known from the sources read",
            t.full
        )),
    }
}

/// How a chain maps.
#[derive(Debug, Clone, PartialEq)]
pub enum Mapped {
    /// An element rule's body: `because`, `allowEmpty`, `select`, `should`.
    Element(Vec<(String, Node)>),
    /// A slice rule's body.
    Slice(Vec<(String, Node)>),
    /// It holds a custom predicate or condition.
    Custom,
    /// It cannot be mapped, and why.
    Unmapped(String),
}

/// Folding of `And` / `Or` into `all` / `any`.
///
/// ArchUnitNET folds left to right: `A.And().B().Or().C()` is `(A and B) or C`. NetArchTest
/// (1.3.2, `PredicateList` and `ConditionList`) groups instead: every `Or()` starts a new group of
/// `And()`-joined terms and the groups are or-ed, so `A.And().B().Or().C().And().D()` is
/// `(A and B) or (C and D)` (upstream's `Or_MultipleInstances_TreatedAsSeparateGroups`).
#[derive(Debug, Clone, Default)]
struct Fold {
    expr: Option<Node>,
    /// The combinator of `expr` when the fold built it, so a run of one connective stays flat.
    op: Option<&'static str>,
    pending: Option<&'static str>,
    /// `Some` in NetArchTest's grouped mode: the `And()` runs so far, each a group.
    groups: Option<Vec<Vec<Node>>>,
}

impl Fold {
    /// A fold with NetArchTest's grouping of `And()` runs.
    fn grouped() -> Self {
        Self {
            groups: Some(Vec::new()),
            ..Self::default()
        }
    }

    fn connective(&mut self, op: &'static str) {
        self.pending = Some(op);
    }

    fn push(&mut self, term: Node) {
        let op = self.pending.take().unwrap_or("all");
        if let Some(groups) = &mut self.groups {
            match groups.last_mut() {
                Some(group) if op == "all" => group.push(term),
                _ => groups.push(vec![term]),
            }
            return;
        }
        match self.expr.take() {
            None => self.expr = Some(term),
            Some(Node::Map(mut pairs)) if self.op == Some(op) && pairs.len() == 1 => {
                if let Some((_, Node::List(items))) = pairs.first_mut() {
                    items.push(Item::plain(term));
                }
                self.expr = Some(Node::Map(pairs));
            }
            Some(previous) => {
                self.expr = Some(Node::map(vec![(op, Node::list(vec![previous, term]))]));
                self.op = Some(op);
            }
        }
    }

    /// The folded expression, if any term was pushed.
    fn finish(self) -> Option<Node> {
        let Some(groups) = self.groups else {
            return self.expr;
        };
        let mut groups: Vec<Node> = groups
            .into_iter()
            .map(|mut group| {
                if group.len() == 1 {
                    group.remove(0)
                } else {
                    netarchtest::all(group)
                }
            })
            .collect();
        match groups.len() {
            0 => None,
            1 => Some(groups.remove(0)),
            _ => Some(netarchtest::any(groups)),
        }
    }
}

fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |c| {
        c.to_ascii_lowercase().to_string() + chars.as_str()
    })
}

/// A scalar argument as the node an attribute argument list holds.
fn literal(val: &Val) -> Result<Node, String> {
    match val {
        Val::Str(s) => Ok(Node::str(s.clone())),
        Val::Bool(b) => Ok(Node::Bool(*b)),
        Val::Num(n) => Ok(n
            .parse::<i64>()
            .map_or_else(|_| Node::str(n.clone()), Node::Int)),
        Val::Type(t) => Ok(Node::str(t.full.clone())),
        Val::Enum(e) => Ok(Node::str(e.clone())),
        Val::Null => Ok(Node::str("null")),
        other => Err(format!(
            "{} is not an attribute argument value",
            other.describe()
        )),
    }
}

fn flatten(args: &[Result<Val, String>]) -> Result<Vec<Val>, String> {
    let mut out = Vec::new();
    for arg in args {
        match arg {
            Ok(Val::List(items)) => out.extend(items.iter().cloned()),
            Ok(other) => out.push(other.clone()),
            Err(reason) => return Err(reason.clone()),
        }
    }
    Ok(out)
}

/// What a nested selector (`...TypesThat()`) builds while its predicates follow.
struct Nested {
    key: String,
    kind: &'static str,
    fold: Fold,
}

fn nested_kind(key: &str) -> &'static str {
    if key.ends_with("MethodMembersThat") {
        "method"
    } else if key.ends_with("AttributesThat") {
        "attribute"
    } else if key.ends_with("MembersThat") {
        "member"
    } else {
        "type"
    }
}

/// The state of mapping one element chain, call by call.
struct Builder {
    netarchtest: bool,
    side: Side,
    where_: Fold,
    should: Fold,
    nested: Option<Nested>,
    because: Option<String>,
    allow_empty: bool,
    should_not: bool,
    combined: Vec<(&'static str, Chain)>,
}

impl Builder {
    fn new(netarchtest: bool) -> Self {
        Self {
            netarchtest,
            side: Side::Where,
            where_: if netarchtest {
                Fold::grouped()
            } else {
                Fold::default()
            },
            should: if netarchtest {
                Fold::grouped()
            } else {
                Fold::default()
            },
            nested: None,
            because: None,
            allow_empty: false,
            should_not: false,
            combined: Vec::new(),
        }
    }

    /// The fold a term or connective goes to now.
    fn fold(&mut self) -> &mut Fold {
        match (&mut self.nested, self.side) {
            (Some(n), _) => &mut n.fold,
            (None, Side::Where) => &mut self.where_,
            (None, Side::Should) => &mut self.should,
        }
    }

    /// Ends a nested selector, adding it as a term on its side.
    fn close(&mut self) {
        if let Some(n) = self.nested.take() {
            let mut selector = vec![("kind", Node::str(n.kind))];
            if let Some(w) = n.fold.finish() {
                selector.push(("where", w));
            }
            let term = Node::Map(vec![(n.key, Node::map(selector))]);
            self.fold().push(term);
        }
    }

    fn call(&mut self, program: &Program, call: &Call) -> Result<(), Mapped> {
        let name = call.name.as_str();
        match name {
            "That" | "As" => {}
            "And" | "Or" if call.args.is_empty() => {
                self.fold()
                    .connective(if name == "And" { "all" } else { "any" });
            }
            "And" | "Or" => match call.args.first() {
                Some(Ok(Val::Chain(other))) if self.side == Side::Should => {
                    self.combined
                        .push((if name == "And" { "all" } else { "any" }, other.clone()));
                }
                Some(Err(reason)) => return Err(Mapped::Unmapped(reason.clone())),
                _ => {
                    return Err(Mapped::Unmapped(format!(
                        "`{name}(...)` combines something the importer does not read"
                    )));
                }
            },
            "Should" => {
                self.close();
                self.side = Side::Should;
            }
            "ShouldNot" if self.netarchtest => {
                self.close();
                self.side = Side::Should;
                self.should_not = true;
            }
            "AndShould" | "OrShould" => {
                self.close();
                self.should
                    .connective(if name == "AndShould" { "all" } else { "any" });
            }
            "Because" => {
                self.close();
                match call.args.first() {
                    Some(Ok(Val::Str(s))) => self.because = Some(s.clone()),
                    Some(Err(reason)) => return Err(Mapped::Unmapped(reason.clone())),
                    _ => {
                        return Err(Mapped::Unmapped("`Because` has no literal reason".into()));
                    }
                }
            }
            "WithoutRequiringPositiveResults" => self.allow_empty = true,
            _ => {
                let side = if self.nested.is_some() {
                    Side::Where
                } else {
                    self.side
                };
                let term = if self.netarchtest {
                    program
                        .selections(&call.args)
                        .and_then(|args| netarchtest::term(name, &args, side))
                        .map(|n| (n, None))
                } else {
                    program.term(name, &call.args, side)
                };
                match term.map_err(Mapped::Unmapped)? {
                    (node, None) => self.fold().push(node),
                    (_, Some(key)) => {
                        self.close();
                        self.nested = Some(Nested {
                            kind: nested_kind(&key),
                            key,
                            fold: Fold::default(),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

impl Program {
    /// Maps a chain to a rule body.
    pub fn map(&self, chain: &Chain) -> Mapped {
        if contains_custom(chain) {
            return Mapped::Custom;
        }
        if self.netarchtest_enhanced && matches!(chain.root, Root::NetArchTest { .. }) {
            return Mapped::Unmapped(format!(
                "the tests use {}, which is not backwards compatible with NetArchTest 1.3.2, the version the mapping is proven against (conformance/netarchtest/README.md)",
                types::NETARCHTEST_ENHANCED
            ));
        }
        match &chain.root {
            Root::Slices => map_slices(chain),
            Root::Elements { .. } | Root::NetArchTest { .. } => self.map_elements(chain),
            Root::Loader => Mapped::Unmapped("a loader is not a rule".into()),
        }
    }

    fn map_elements(&self, chain: &Chain) -> Mapped {
        let netarchtest = matches!(chain.root, Root::NetArchTest { .. });
        let (kind, referenced, root_term) = match &chain.root {
            Root::Elements { kind, referenced } => (*kind, *referenced, None),
            Root::NetArchTest { method, args } => match netarchtest::root(method, args) {
                Ok(term) => ("type", false, term),
                Err(reason) => return Mapped::Unmapped(reason),
            },
            _ => return Mapped::Unmapped("not an element chain".into()),
        };
        let mut builder = Builder::new(netarchtest);
        for call in &chain.calls {
            if let Err(mapped) = builder.call(self, call) {
                return mapped;
            }
        }
        builder.close();
        let Some(mut should_expr) = std::mem::take(&mut builder.should).finish() else {
            return Mapped::Unmapped("the chain has no condition (`Should()...`)".into());
        };
        let select = dotnet_scoped(Self::select_node(
            kind,
            referenced,
            root_term,
            std::mem::take(&mut builder.where_).finish(),
        ));
        for (op, other) in std::mem::take(&mut builder.combined) {
            match self.combine(&select, &other) {
                Ok(theirs) => {
                    should_expr = Node::map(vec![(op, Node::list(vec![should_expr, theirs]))]);
                }
                Err(mapped) => return mapped,
            }
        }
        if builder.should_not {
            should_expr = netarchtest::not(should_expr);
        }
        let mut body = Vec::new();
        if let Some(b) = builder.because {
            body.push(("because".to_owned(), Node::str(b)));
        }
        // NetArchTest passes an empty selection; the import keeps its verdict.
        if builder.allow_empty || builder.netarchtest {
            body.push(("allowEmpty".to_owned(), Node::Bool(true)));
        }
        body.push(("select".to_owned(), select));
        body.push(("should".to_owned(), should_expr));
        let mut probe = vec![("name".to_owned(), Node::str("probe"))];
        probe.extend(body.iter().cloned());
        if let Err(e) =
            rb_config::elements::parse_elements(&Value::Array(vec![Node::Map(probe).to_json()]))
        {
            return Mapped::Unmapped(e.to_string());
        }
        Mapped::Element(body)
    }

    /// The condition of a rule combined with `rule.And(other)` / `rule.Or(other)`, when both
    /// select the same objects.
    fn combine(&self, select: &Node, other: &Chain) -> Result<Node, Mapped> {
        match self.map(other) {
            Mapped::Element(body) => {
                let theirs = body
                    .iter()
                    .find(|(k, _)| k == "select")
                    .map(|(_, v)| v.to_json());
                if theirs != Some(select.to_json()) {
                    return Err(Mapped::Unmapped(
                        "it combines rules that select different objects".into(),
                    ));
                }
                body.into_iter()
                    .find(|(k, _)| k == "should")
                    .map(|(_, v)| v)
                    .ok_or_else(|| Mapped::Unmapped("the combined rule has no condition".into()))
            }
            Mapped::Custom => Err(Mapped::Custom),
            Mapped::Slice(_) => Err(Mapped::Unmapped(
                "it combines an element rule with a slice rule".into(),
            )),
            Mapped::Unmapped(reason) => Err(Mapped::Unmapped(reason)),
        }
    }

    fn select_node(kind: &str, referenced: bool, root: Option<Node>, where_: Option<Node>) -> Node {
        let mut select = vec![("kind", Node::str(kind))];
        if referenced {
            select.push(("includeReferenced", Node::Bool(true)));
        }
        let where_ = match (root, where_) {
            (Some(root), None) => Some(root),
            (Some(root), Some(Node::Map(pairs))) if pairs.len() == 1 && pairs[0].0 == "all" => {
                let mut items = vec![Item::plain(root)];
                if let Some((_, Node::List(rest))) = pairs.into_iter().next() {
                    items.extend(rest);
                }
                Some(Node::map(vec![("all", Node::List(items))]))
            }
            (Some(root), Some(other)) => Some(netarchtest::all(vec![root, other])),
            (None, w) => w,
        };
        if let Some(w) = where_ {
            select.push(("where", w));
        }
        Node::map(select)
    }

    /// The arguments of a NetArchTest call with each `Val::Names` mapped to its selection.
    fn selections(&self, args: &[Result<Val, String>]) -> Result<Vec<Result<Val, String>>, String> {
        args.iter()
            .map(|arg| match arg {
                Ok(Val::Names(chain)) => self
                    .netarchtest_selection(chain)
                    .map(|n| Ok(Val::Selected(n))),
                other => Ok(other.clone()),
            })
            .collect()
    }

    /// A NetArchTest selection (`Types.InAssembly(...).That()...`, no condition) as a nested
    /// selector of .NET types: the types its `GetTypes()` returns.
    fn netarchtest_selection(&self, chain: &Chain) -> Result<Node, String> {
        let Root::NetArchTest { method, args } = &chain.root else {
            return Err(format!("`{}` is not a NetArchTest selection", chain.text));
        };
        let root = netarchtest::root(method, args)?;
        let mut builder = Builder::new(true);
        for call in &chain.calls {
            if matches!(call.name.as_str(), "Should" | "ShouldNot") {
                return Err(format!(
                    "`{}` has a condition, so it is not a selection",
                    chain.text
                ));
            }
            builder.call(self, call).map_err(|mapped| match mapped {
                Mapped::Unmapped(reason) => reason,
                _ => "stays in NetArchTest: custom predicate (MeetCustomRule)".to_owned(),
            })?;
        }
        builder.close();
        // NetArchTest's types are .NET types, and a key such as `resideInAssembly` has no
        // answer for another language, so the selection is scoped as a rule's own `select` is.
        Ok(dotnet_scoped(Self::select_node(
            "type",
            false,
            root,
            std::mem::take(&mut builder.where_).finish(),
        )))
    }

    /// A provider chain (`Types().That()...`) as a nested selector.
    fn selector(&self, chain: &Chain) -> Result<Node, String> {
        let Root::Elements { kind, referenced } = &chain.root else {
            return Err(format!("`{}` is not an ArchUnitNET selection", chain.text));
        };
        let mut fold = Fold::default();
        for call in &chain.calls {
            match call.name.as_str() {
                "That" | "As" => {}
                "And" if call.args.is_empty() => fold.connective("all"),
                "Or" if call.args.is_empty() => fold.connective("any"),
                name if CUSTOM.contains(&name) => {
                    return Err("stays in ArchUnitNET: custom predicate".into());
                }
                "Should" => {
                    return Err(format!("`{}` is a rule, not a selection", chain.text));
                }
                name => match self.term(name, &call.args, Side::Where)? {
                    (node, None) => fold.push(node),
                    (_, Some(key)) => {
                        return Err(format!(
                            "`{key}` inside a selection passed as an argument is not read"
                        ));
                    }
                },
            }
        }
        Ok(Self::select_node(kind, *referenced, None, fold.finish()))
    }

    /// One `ArchUnitNET` predicate or condition: the term, or the key of a nested selector whose
    /// predicates follow.
    fn term(
        &self,
        method: &str,
        args: &[Result<Val, String>],
        side: Side,
    ) -> Result<(Node, Option<String>), String> {
        let mut key = lower_first(method);
        let mut args = args.to_vec();
        // ArchUnitNET's older `(pattern, useRegularExpressions)` overloads.
        if args.len() == 2
            && let Some(Ok(Val::Bool(regex))) = args.get(1)
        {
            let regex = *regex;
            args.truncate(1);
            if regex {
                key.push_str("Matching");
            }
        }
        let (base, _, selector) = split_key(&key, side);
        let Some((_, _, kind, _)) = VOCABULARY.iter().find(|(n, _, _, _)| *n == base) else {
            return Err(format!(
                "`{method}` has no element key (docs/artifacts/archunitnet-0.13.4-coverage.md)"
            ));
        };
        if selector {
            if !args.is_empty() {
                return Err(format!(
                    "`{method}` takes its selection from the calls after it"
                ));
            }
            return Ok((Node::Map(Vec::new()), Some(key)));
        }
        let value = match kind {
            ValueKind::Flag => {
                if !args.is_empty() {
                    return Err(format!(
                        "`{method}` takes arguments the importer does not read"
                    ));
                }
                Node::Bool(true)
            }
            ValueKind::Names => {
                let mut names = Vec::new();
                for val in flatten(&args)? {
                    match val {
                        Val::Str(s) | Val::Num(s) => names.push(s),
                        Val::Assembly { name, .. } => names.push(name),
                        other => return Err(format!("{} is not a name", other.describe())),
                    }
                }
                match names.len() {
                    0 => return Err(format!("`{method}` names nothing")),
                    1 => Node::str(names.remove(0)),
                    _ => Node::strs(&names),
                }
            }
            ValueKind::Pattern => match flatten(&args)?.as_slice() {
                [Val::Str(s)] => Node::str(s.clone()),
                _ => return Err(format!("`{method}` takes one literal pattern")),
            },
            ValueKind::Diagram => match flatten(&args)?.as_slice() {
                [Val::Str(s)] => Node::str(s.clone()),
                _ => return Err(format!("`{method}` takes the diagram's path as a literal")),
            },
            ValueKind::Objects => self.objects(method, &args)?,
            ValueKind::AttributeArguments | ValueKind::AttributeNamedArguments => {
                let Some(first) = args.first() else {
                    return Err(format!("`{method}` names no attribute"));
                };
                let attribute = self.objects(method, std::slice::from_ref(first))?;
                let attribute = match attribute {
                    Node::List(mut items) if items.len() == 1 => items.remove(0).node,
                    other => other,
                };
                let rest = flatten(&args[1..])?;
                let arguments = if *kind == ValueKind::AttributeArguments {
                    Node::list(rest.iter().map(literal).collect::<Result<Vec<_>, _>>()?)
                } else {
                    named(&rest)?
                };
                Node::map(vec![("attribute", attribute), ("arguments", arguments)])
            }
            ValueKind::ArgumentValues => Node::list(
                flatten(&args)?
                    .iter()
                    .map(literal)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            ValueKind::NamedArgumentValues => named(&flatten(&args)?)?,
        };
        Ok((Node::Map(vec![(key, value)]), None))
    }

    fn objects(&self, method: &str, args: &[Result<Val, String>]) -> Result<Node, String> {
        let values = flatten(args)?;
        let mut names = Vec::new();
        let mut selectors = Vec::new();
        for val in &values {
            match val {
                Val::Type(t) => names.push(t.full.clone()),
                Val::Chain(chain) => selectors.push(self.selector(chain)?),
                other => {
                    return Err(format!(
                        "`{method}` is given {}, which is not a type or a selection",
                        other.describe()
                    ));
                }
            }
        }
        match (names.is_empty(), selectors.len()) {
            (_, 0) => Ok(Node::strs(&names)),
            (true, 1) => Ok(selectors.remove(0)),
            _ => Err(format!(
                "`{method}` mixes selections, or named types with a selection, which one key cannot hold"
            )),
        }
    }
}

fn named(values: &[Val]) -> Result<Node, String> {
    let mut pairs = Vec::new();
    for value in values {
        match value {
            Val::Tuple(items) if items.len() == 2 => match &items[0] {
                Val::Str(name) => pairs.push((name.clone(), literal(&items[1])?)),
                other => return Err(format!("{} is not an argument name", other.describe())),
            },
            other => {
                return Err(format!("{} is not a (name, value) pair", other.describe()));
            }
        }
    }
    Ok(Node::Map(pairs))
}

fn contains_custom(chain: &Chain) -> bool {
    chain.calls.iter().any(|c| {
        CUSTOM.contains(&c.name.as_str())
            || c.args.iter().any(|a| match a {
                Ok(Val::Chain(inner)) => contains_custom(inner),
                Ok(Val::List(items)) => items
                    .iter()
                    .any(|i| matches!(i, Val::Chain(inner) if contains_custom(inner))),
                _ => false,
            })
    })
}

fn map_slices(chain: &Chain) -> Mapped {
    let mut matching = None;
    let mut should = Vec::new();
    let mut after_should = false;
    for call in &chain.calls {
        match call.name.as_str() {
            "Matching" | "MatchingWithPackages" => match call.args.first() {
                Some(Ok(Val::Str(s))) => matching = Some(s.clone()),
                Some(Err(reason)) => return Mapped::Unmapped(reason.clone()),
                _ => return Mapped::Unmapped("`Matching` has no literal pattern".into()),
            },
            "Should" => after_should = true,
            "AndShould" => {}
            "NotDependOnEachOther" | "BeFreeOfCycles" if after_should => {
                should.push(lower_first(&call.name));
            }
            other => {
                return Mapped::Unmapped(format!("`{other}` has no slice-rule key"));
            }
        }
    }
    let Some(matching) = matching else {
        return Mapped::Unmapped("the slices are not `Matching` a pattern".into());
    };
    if should.is_empty() {
        return Mapped::Unmapped("the slice chain has no condition".into());
    }
    let should_node = if should.len() == 1 {
        Node::str(should.remove(0))
    } else {
        Node::strs(&should)
    };
    let body = vec![
        ("matching".to_owned(), Node::str(matching)),
        ("should".to_owned(), should_node),
    ];
    let mut probe = vec![("name".to_owned(), Node::str("probe"))];
    probe.extend(body.iter().cloned());
    if let Err(e) =
        rb_config::elements::parse_slices(&Value::Array(vec![Node::Map(probe).to_json()]))
    {
        return Mapped::Unmapped(e.to_string());
    }
    Mapped::Slice(body)
}

/// One rule-running call found in a test.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// The file, as shown.
    pub file: String,
    /// The 1-based line.
    pub line: usize,
    /// The test method.
    pub method: String,
    /// The C# of the chain and the call that ran it.
    pub text: String,
    /// The sink call, rendered (`.Check(Architecture)`).
    pub sink: String,
    /// The data row of a parameterised test, as written (`[InlineData("a")]`).
    pub row: Option<String>,
    /// What the test expects.
    pub expect: Expect,
    /// The chain, or why it could not be read.
    pub chain: Result<Chain, String>,
}

impl Program {
    /// Extension methods the sources declare that run the rule they extend (a project's own
    /// `ShouldSucceed()` over NetArchTest's `GetResult()`), with what they expect of it.
    fn extension_sinks(&self) -> BTreeMap<String, Expect> {
        let mut out = BTreeMap::new();
        for (_, file) in &self.files {
            for method in file.types.iter().flat_map(|t| t.methods.iter()) {
                let Some(subject) = method.parameters.first().filter(|_| method.extension) else {
                    continue;
                };
                let subject = Expr::Name(subject.clone());
                let mut expect = None;
                for (index, stmt) in method.body.iter().enumerate() {
                    let (expr, bind) = match stmt {
                        Stmt::Expr { expr, .. } | Stmt::Assign { value: expr, .. } => (expr, None),
                        Stmt::Local { name, value, .. } => (value, Some(name)),
                    };
                    let mut found = Vec::new();
                    sinks(expr, Expect::Passes, &BTreeMap::new(), &mut found);
                    for (receiver, sink, _, found_expect) in found {
                        if *receiver != subject || expect.is_some() {
                            continue;
                        }
                        // `var result = list.GetResult(); Assert.True(result.IsSuccessful);`
                        expect = Some(match (sink.as_str(), bind) {
                            ("GetResult", Some(name)) => {
                                result_expectation(name, &method.body[index + 1..])
                            }
                            _ => found_expect,
                        });
                    }
                }
                if let Some(expect) = expect {
                    out.insert(method.name.clone(), expect);
                }
            }
        }
        out
    }

    /// Every rule-running call in the test files, in file and line order; a parameterised test
    /// is read once per data row, its parameters bound to the row's values.
    pub fn candidates(&self) -> Vec<Candidate> {
        let extensions = self.extension_sinks();
        let mut out = Vec::new();
        for (path, file) in self.files.iter().take(self.tests) {
            for decl in &file.types {
                for method in decl.methods.iter().filter(|m| !m.extension) {
                    let rows: Vec<Option<&(String, Vec<Expr>)>> =
                        if method.rows.is_empty() || method.parameters.is_empty() {
                            vec![None]
                        } else {
                            method.rows.iter().map(Some).collect()
                        };
                    for row in rows {
                        let mut scope = Scope::new(path, decl);
                        if let Some((_, values)) = row {
                            for (name, value) in method.parameters.iter().zip(values) {
                                let read = self.eval(&scope, value, 0);
                                scope.locals.push((name.clone(), read, value.clone()));
                            }
                        } else {
                            for name in &method.parameters {
                                scope.locals.push((
                                    name.clone(),
                                    Err(format!(
                                        "`{name}` is a parameter of the test whose values do not come from literal rows ([MemberData], [TestCaseSource] or computed), which the importer does not evaluate"
                                    )),
                                    Expr::Name(name.clone()),
                                ));
                            }
                        }
                        out.extend(self.method_candidates(
                            &mut scope,
                            file,
                            method,
                            row.map(|(text, _)| text.as_str()),
                            &extensions,
                        ));
                    }
                }
            }
        }
        out
    }

    fn method_candidates(
        &self,
        scope: &mut Scope<'_>,
        file: &SourceFile,
        method: &csharp::Method,
        row: Option<&str>,
        extensions: &BTreeMap<String, Expect>,
    ) -> Vec<Candidate> {
        let mut out = Vec::new();
        for (index, stmt) in method.body.iter().enumerate() {
            let (expr, line, bind) = match stmt {
                Stmt::Local { name, value, line } => (value, *line, Some(name.clone())),
                Stmt::Assign {
                    target,
                    value,
                    line,
                } => (
                    value,
                    *line,
                    match target {
                        Expr::Name(n) => Some(n.clone()),
                        _ => None,
                    },
                ),
                Stmt::Expr { expr, line } => (expr, *line, None),
            };
            let mut found = Vec::new();
            sinks(expr, Expect::Passes, extensions, &mut found);
            for (receiver, sink, sink_args, mut expect) in found {
                if sink == "GetResult"
                    && let Some(var) = &bind
                {
                    expect = result_expectation(var, &method.body[index + 1..]);
                }
                let chain = match self.eval(scope, receiver, 0) {
                    Ok(Val::Chain(chain)) if chain.root != Root::Loader => Ok(chain),
                    Ok(other)
                        if looks_like_rule(receiver, scope) || self.calls_a_helper(receiver) =>
                    {
                        Err(format!("the rule is {}", other.describe()))
                    }
                    Err(reason)
                        if looks_like_rule(receiver, scope) || self.calls_a_helper(receiver) =>
                    {
                        Err(reason)
                    }
                    _ => continue,
                };
                let text = match &chain {
                    Ok(c) => c.text.clone(),
                    Err(_) => csharp::render(receiver),
                };
                out.push(Candidate {
                    file: file.shown.clone(),
                    line,
                    method: method.name.clone(),
                    text,
                    sink: format!(
                        ".{sink}({})",
                        sink_args
                            .iter()
                            .map(|a| csharp::render(&a.value))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    row: row.map(str::to_owned),
                    expect,
                    chain,
                });
            }
            if let Some(name) = bind {
                let value = self.eval(scope, expr, 0);
                scope.locals.push((name, value, expr.clone()));
            }
        }
        out
    }

    /// Every `new ArchLoader()...` chain in any file, with where it was written.
    fn loaders(&self) -> Vec<(String, usize, Result<Chain, String>)> {
        let mut out = Vec::new();
        for (path, file) in &self.files {
            for decl in &file.types {
                let scope = Scope::new(path, decl);
                let mut exprs: Vec<(&Expr, usize)> =
                    decl.members.iter().map(|m| (&m.value, m.line)).collect();
                for method in &decl.methods {
                    for stmt in &method.body {
                        match stmt {
                            Stmt::Local { value, line, .. } | Stmt::Assign { value, line, .. } => {
                                exprs.push((value, *line));
                            }
                            Stmt::Expr { expr, line } => exprs.push((expr, *line)),
                        }
                    }
                }
                for (expr, line) in exprs {
                    let mut roots = Vec::new();
                    loader_roots(expr, false, &mut roots);
                    for root in roots {
                        let chain = match self.eval(&scope, root, 0) {
                            Ok(Val::Chain(chain)) => Ok(chain),
                            Ok(other) => Err(format!("the loader is {}", other.describe())),
                            Err(e) => Err(e),
                        };
                        out.push((file.shown.clone(), line, chain));
                    }
                }
            }
        }
        out.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
        out.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
        out
    }
}

/// Whether an expression the importer could not read is still recognisably a rule, so it is
/// reported rather than skipped.
fn looks_like_rule(receiver: &Expr, scope: &Scope<'_>) -> bool {
    let text = match receiver {
        Expr::Name(n) => scope
            .local(n)
            .map_or_else(|| csharp::render(receiver), |(_, _, e)| csharp::render(e)),
        other => csharp::render(other),
    };
    text.contains(".Should()")
        || text.contains(".ShouldNot()")
        || text.contains("Slices()")
        || ROOTS
            .iter()
            .any(|(r, _)| text.starts_with(&format!("{r}(")))
        || text.starts_with("Types.In")
}

/// Finds rule-running calls in `expr`, with what the surrounding assertion expects.
fn sinks<'e>(
    expr: &'e Expr,
    expect: Expect,
    extra: &BTreeMap<String, Expect>,
    out: &mut Vec<(&'e Expr, String, &'e [Arg], Expect)>,
) {
    match expr {
        Expr::Call(function, args) => {
            if let Expr::Member(receiver, method) = function.as_ref() {
                if let Some(own) = extra.get(method.as_str()) {
                    let expect = if *own == Expect::Fails {
                        expect.flip()
                    } else {
                        expect
                    };
                    out.push((receiver, method.clone(), args, expect));
                    return;
                }
                if SINKS.contains(&method.as_str()) {
                    let expect = match method.as_str() {
                        "AssertOnlyViolations" | "AssertAnyViolations" => Expect::Fails,
                        _ => expect,
                    };
                    out.push((receiver, method.clone(), args, expect));
                    return;
                }
                if let Expr::Name(owner) = receiver.as_ref() {
                    match (owner.as_str(), method.as_str()) {
                        ("Assert", "False" | "IsFalse") => {
                            for a in args.iter().take(1) {
                                sinks(&a.value, expect.flip(), extra, out);
                            }
                            return;
                        }
                        ("Assert", "True" | "IsTrue" | "That") => {
                            for a in args.iter().take(1) {
                                sinks(&a.value, expect, extra, out);
                            }
                            return;
                        }
                        ("Assert", m) if m.starts_with("Throws") => {
                            for a in args {
                                sinks(&a.value, Expect::Fails, extra, out);
                            }
                            return;
                        }
                        ("ArchRuleAssert", "CheckRule" | "FulfilsRule") => {
                            if let Some(rule) = args.get(1) {
                                out.push((&rule.value, "Check".into(), &args[..1], expect));
                            }
                            return;
                        }
                        _ => {}
                    }
                }
                match method.as_str() {
                    "ShouldBeFalse" | "BeFalse" => {
                        sinks(strip_should(receiver), expect.flip(), extra, out);
                        return;
                    }
                    "ShouldBeTrue" | "BeTrue" => {
                        sinks(strip_should(receiver), expect, extra, out);
                        return;
                    }
                    _ => {}
                }
                sinks(receiver, expect, extra, out);
            } else {
                sinks(function, expect, extra, out);
            }
            for a in args {
                sinks(&a.value, expect, extra, out);
            }
        }
        Expr::Member(receiver, _) => sinks(receiver, expect, extra, out),
        Expr::Not(inner) => sinks(inner, expect.flip(), extra, out),
        Expr::Is(inner, pattern) => {
            let expect = if pattern.trim() == "false" {
                expect.flip()
            } else {
                expect
            };
            sinks(inner, expect, extra, out);
        }
        Expr::Binary(l, op, r) => {
            let flips = (op == "==" && matches!(r.as_ref(), Expr::Bool(false)))
                || (op == "!=" && matches!(r.as_ref(), Expr::Bool(true)));
            sinks(l, if flips { expect.flip() } else { expect }, extra, out);
            sinks(r, expect, extra, out);
        }
        Expr::Lambda(_, body) => sinks(body, expect, extra, out),
        Expr::Array(items) => {
            for i in items {
                sinks(i, expect, extra, out);
            }
        }
        Expr::Tuple(args) | Expr::New(_, args, _) => {
            for a in args {
                sinks(&a.value, expect, extra, out);
            }
        }
        _ => {}
    }
}

/// `x.Should()` in `FluentAssertions`' `x.Should().BeTrue()`: the `x`.
fn strip_should(expr: &Expr) -> &Expr {
    match expr {
        Expr::Call(function, args) if args.is_empty() => match function.as_ref() {
            Expr::Member(receiver, m) if m == "Should" => receiver,
            _ => expr,
        },
        _ => expr,
    }
}

/// What the statements after `var result = ...GetResult();` assert of `result.IsSuccessful`.
fn result_expectation(var: &str, rest: &[Stmt]) -> Expect {
    let successful = Expr::Member(Box::new(Expr::Name(var.to_owned())), "IsSuccessful".into());
    for stmt in rest {
        let expr = match stmt {
            Stmt::Expr { expr, .. }
            | Stmt::Local { value: expr, .. }
            | Stmt::Assign { value: expr, .. } => expr,
        };
        if let Some(expect) = asserts(expr, &successful, Expect::Passes) {
            return expect;
        }
    }
    Expect::Passes
}

fn asserts(expr: &Expr, subject: &Expr, expect: Expect) -> Option<Expect> {
    if expr == subject {
        return Some(expect);
    }
    match expr {
        Expr::Call(function, args) => {
            if let Expr::Member(receiver, method) = function.as_ref() {
                if let Expr::Name(owner) = receiver.as_ref()
                    && owner == "Assert"
                {
                    let flip = matches!(method.as_str(), "False" | "IsFalse");
                    let e = if flip { expect.flip() } else { expect };
                    return args.iter().find_map(|a| asserts(&a.value, subject, e));
                }
                match method.as_str() {
                    "BeFalse" | "ShouldBeFalse" => {
                        return asserts(strip_should(receiver), subject, expect.flip());
                    }
                    "BeTrue" | "ShouldBeTrue" => {
                        return asserts(strip_should(receiver), subject, expect);
                    }
                    _ => {}
                }
            }
            None
        }
        Expr::Not(inner) => asserts(inner, subject, expect.flip()),
        Expr::Is(inner, pattern) => asserts(
            inner,
            subject,
            if pattern.trim() == "false" {
                expect.flip()
            } else {
                expect
            },
        ),
        _ => None,
    }
}

/// The outermost call chains rooted at `new ArchLoader()`.
fn loader_roots<'e>(expr: &'e Expr, as_receiver: bool, out: &mut Vec<&'e Expr>) {
    let rooted = |e: &Expr| {
        let mut current = e;
        loop {
            match current {
                Expr::Call(f, _) => current = f,
                Expr::Member(r, _) => current = r,
                Expr::New(Some(t), _, _) => {
                    return t.segments.last().is_some_and(|(n, _)| n == "ArchLoader");
                }
                _ => return false,
            }
        }
    };
    match expr {
        Expr::Call(function, args) => {
            if !as_receiver && rooted(expr) {
                out.push(expr);
                return;
            }
            if let Expr::Member(receiver, _) = function.as_ref() {
                loader_roots(receiver, true, out);
            }
            for a in args {
                loader_roots(&a.value, false, out);
            }
        }
        Expr::Member(receiver, _) => loader_roots(receiver, true, out),
        Expr::Lambda(_, body) => loader_roots(body, false, out),
        Expr::New(_, args, _) => {
            for a in args {
                loader_roots(&a.value, false, out);
            }
        }
        _ => {}
    }
}

/// A rule's top-level `select` with `language: dotnet` after its `kind`. An ArchUnitNET or
/// NetArchTest test checks the assemblies it loads and nothing else, so the imported rule selects
/// .NET objects only; unscoped, a key only .NET can answer (`resideInAssembly`) makes the whole
/// run exit 3 in a repository that also holds TypeScript or Python
/// ([ADR-0014](../../../../../docs/adr/0014-no-invented-cross-language-edges.md)). Nested selectors
/// stay unscoped: they name what a .NET object may depend on, be or be assignable to.
fn dotnet_scoped(select: Node) -> Node {
    match select {
        Node::Map(mut pairs) => {
            let at = pairs
                .iter()
                .position(|(k, _)| k == "kind")
                .map_or(0, |i| i + 1);
            pairs.insert(at, ("language".to_owned(), Node::str("dotnet")));
            Node::Map(pairs)
        }
        other => other,
    }
}

/// A test method name in kebab case: `DomainShouldNotDependOnInfrastructure` is
/// `domain-should-not-depend-on-infrastructure`.
pub fn kebab(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' || c == '-' || c == ' ' {
            if !out.ends_with('-') && !out.is_empty() {
                out.push('-');
            }
            continue;
        }
        if c.is_uppercase() && i > 0 {
            let previous = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if (previous.is_lowercase()
                || previous.is_ascii_digit()
                || (previous.is_uppercase() && next_lower))
                && !out.ends_with('-')
            {
                out.push('-');
            }
        }
        out.extend(c.to_lowercase());
    }
    out.trim_matches('-').to_owned()
}

/// The chain as comment lines: the selection, then the condition from `Should()`, then the call
/// that ran it with the file and line.
fn chain_comment(candidate: &Candidate) -> Vec<String> {
    let text = &candidate.text;
    let at = text.find(".Should()").or_else(|| text.find(".ShouldNot()"));
    let mut lines = match at {
        Some(at) => vec![text[..at].to_owned(), format!("  {}", &text[at..])],
        None => vec![text.clone()],
    };
    let file = candidate.file.rsplit('/').next().unwrap_or(&candidate.file);
    lines.push(format!(
        "  {}   [{file}:{}]",
        candidate.sink, candidate.line
    ));
    if let Some(row) = &candidate.row {
        lines.push(format!("  with {row}"));
    }
    lines
}

/// What the loaders name, gathered.
#[derive(Default)]
struct Loaded {
    assemblies: Vec<String>,
    directories: Vec<(String, Option<String>)>,
    namespaces: Vec<String>,
    dependencies: bool,
}

/// An assembly as the loader glob that finds its build output.
fn assembly_glob(val: &Val) -> Result<String, String> {
    match val {
        Val::Assembly {
            name,
            project: Some(dir),
        } => Ok(if dir.is_empty() {
            format!("bin/**/{name}.dll")
        } else {
            format!("{dir}/bin/**/{name}.dll")
        }),
        Val::Assembly {
            name,
            project: None,
        } => Err(format!(
            "the project that builds `{name}` was not found, so its build output cannot be named"
        )),
        other => Err(format!("{} is not an assembly", other.describe())),
    }
}

impl Loaded {
    /// One `ArchLoader` call.
    fn call(&mut self, call: &Call) -> Result<(), String> {
        let name = call.name.as_str();
        match name {
            "LoadAssembly"
            | "LoadAssemblies"
            | "LoadAssemblyIncludingDependencies"
            | "LoadAssembliesIncludingDependencies"
            | "LoadAssembliesRecursively" => {
                for val in flatten(&call.args)? {
                    self.assemblies.push(assembly_glob(&val)?);
                }
                if name.ends_with("IncludingDependencies") || name.ends_with("Recursively") {
                    self.dependencies = true;
                }
            }
            "LoadFilteredDirectory" | "LoadFilteredDirectoryIncludingDependencies" => {
                match flatten(&call.args)?.as_slice() {
                    [Val::Str(dir), Val::Str(filter), ..] => {
                        self.directories.push((dir.clone(), Some(filter.clone())));
                    }
                    [Val::Str(dir)] => self.directories.push((dir.clone(), None)),
                    _ => return Err(format!("`{name}` has no literal folder")),
                }
                if name.ends_with("IncludingDependencies") {
                    self.dependencies = true;
                }
            }
            "LoadNamespacesWithinAssembly" => {
                let values = flatten(&call.args)?;
                let Some((first, rest)) = values.split_first() else {
                    return Err(format!("`{name}` names no assembly"));
                };
                self.assemblies.push(assembly_glob(first)?);
                for ns in rest {
                    match ns {
                        Val::Str(s) => self.namespaces.push(s.clone()),
                        other => return Err(format!("{} is not a namespace", other.describe())),
                    }
                }
            }
            "Build" => {}
            other => return Err(format!("`{other}` has no languages.dotnet key")),
        }
        Ok(())
    }

    /// The `languages` block, or `None` when nothing was loaded.
    fn node(mut self) -> Option<Node> {
        self.assemblies.sort();
        self.assemblies.dedup();
        self.directories.sort();
        self.directories.dedup();
        self.namespaces.sort();
        self.namespaces.dedup();
        if self.assemblies.is_empty() && self.directories.is_empty() {
            return None;
        }
        let mut pairs = Vec::new();
        if !self.assemblies.is_empty() {
            pairs.push(("assemblies", Node::strs(&self.assemblies)));
        }
        if self.dependencies {
            pairs.push(("includeDependencies", Node::Bool(true)));
        }
        if !self.directories.is_empty() {
            let directories = self
                .directories
                .iter()
                .map(|(dir, filter)| {
                    let mut p = vec![("dir", Node::str(dir.clone()))];
                    if let Some(f) = filter {
                        p.push(("filter", Node::str(f.clone())));
                    }
                    Node::map(p)
                })
                .collect();
            pairs.push(("directories", Node::list(directories)));
        }
        if !self.namespaces.is_empty() {
            pairs.push(("namespaces", Node::strs(&self.namespaces)));
        }
        Some(Node::map(vec![("dotnet", Node::map(pairs))]))
    }
}

/// The `languages.dotnet` block from the loaders, with the comments above it.
fn dotnet_block(program: &Program) -> (Option<Node>, Vec<String>) {
    let loaders = program.loaders();
    let mut comments = Vec::new();
    let mut loaded = Loaded::default();
    for (file, line, chain) in &loaders {
        let name = file.rsplit('/').next().unwrap_or(file);
        match chain {
            Ok(chain) => {
                comments.push(format!("{}   [{name}:{line}]", chain.text));
                for call in &chain.calls {
                    if let Err(reason) = loaded.call(call) {
                        comments.push(format!("  not imported: {reason}"));
                    }
                }
            }
            Err(reason) => {
                comments.push(format!("[{name}:{line}] a loader not imported: {reason}"));
            }
        }
    }
    if loaders.len() > 1 {
        comments.push(format!(
            "{} loaders were found; ArchUnitNET loads each architecture on its own, a configuration has one languages.dotnet block, so they are merged",
            loaders.len()
        ));
    }
    netarchtest_assemblies(program, &mut loaded, &mut comments);
    (loaded.node(), comments)
}

/// NetArchTest has no loader: `Types.InAssembly(typeof(X).Assembly)` loads X's assembly for the
/// rule that follows, so each assembly a NetArchTest chain names is loaded too. Without it an
/// imported NetArchTest rule selects from nothing and is vacuous (found by the .NET oracle
/// harness on phongnguyend/Practical.CleanArchitecture).
fn netarchtest_assemblies(program: &Program, loaded: &mut Loaded, comments: &mut Vec<String>) {
    let mut named = Vec::new();
    let mut refused = Vec::new();
    for candidate in program.candidates() {
        let Ok(Chain {
            root: Root::NetArchTest { method, args },
            ..
        }) = &candidate.chain
        else {
            continue;
        };
        if !matches!(method.as_str(), "InAssembly" | "InAssemblies") {
            continue;
        }
        match flatten(args).and_then(|vals| {
            vals.iter()
                .map(assembly_glob)
                .collect::<Result<Vec<_>, _>>()
        }) {
            Ok(globs) => named.extend(globs),
            Err(reason) => refused.push(format!("  not loaded: {reason}")),
        }
    }
    if named.is_empty() && refused.is_empty() {
        return;
    }
    comments
        .push("NetArchTest's Types.InAssembly(...) loads the assemblies its tests name".to_owned());
    refused.sort();
    refused.dedup();
    comments.extend(refused);
    loaded.assemblies.extend(named);
}

/// The imported rules, their names made unique.
pub struct Imported {
    /// `rules.elements` items.
    pub elements: Vec<Item>,
    /// `rules.slices` items.
    pub slices: Vec<Item>,
    /// How many chains were read and how many were enabled.
    pub read: usize,
    /// How many were written as rules rather than commented out.
    pub enabled: usize,
    /// How many tests run no rule and check the architecture in C#.
    pub queried: usize,
}

/// A test that runs no rule: it queries the architecture and asserts in C# (`GetTypes()` or
/// `Architecture.Types` and LINQ, or a condition's `GetReflectionTypes()`), so it stays with the
/// incumbent as a custom predicate does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// The file, as shown.
    pub file: String,
    /// The 1-based line of the test method.
    pub line: usize,
    /// The test method.
    pub method: String,
    /// The library the file uses: `ArchUnitNET`, `NetArchTest`, or both.
    pub tool: &'static str,
}

/// Calls that run a rule, beside [`SINKS`]: ArchUnitNET's `IArchRule.Evaluate`, NetArchTest's
/// `Policy.Evaluate`, and `ArchRuleAssert`'s two assertions.
const RUNS: &[&str] = &["Evaluate", "CheckRule", "FulfilsRule"];

/// The names a piece of C# calls, over-approximated: every identifier followed by `(` or `<`.
fn called_names(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if matches!(bytes.get(j), Some(b'(' | b'<')) {
                out.push(&text[start..i]);
            }
        } else {
            i += 1;
        }
    }
    out
}

impl Program {
    /// The library a test file uses, from its using directives and the test files' global ones;
    /// `None` when it uses neither (a unit test beside the architecture tests).
    fn tool_of(&self, decl: &TypeDecl) -> Option<&'static str> {
        let globals = self.files.iter().take(self.tests).flat_map(|(_, f)| {
            f.global_usings
                .namespaces
                .iter()
                .chain(&f.global_usings.statics)
        });
        let (mut archunit, mut netarchtest) = (false, false);
        for used in decl
            .usings
            .namespaces
            .iter()
            .chain(&decl.usings.statics)
            .chain(globals)
        {
            archunit |= used.starts_with("ArchUnitNET");
            netarchtest |= used.starts_with("NetArchTest");
        }
        match (archunit, netarchtest) {
            (true, true) => Some("ArchUnitNET and NetArchTest"),
            (true, false) => Some("ArchUnitNET"),
            (false, true) => Some("NetArchTest"),
            (false, false) => None,
        }
    }

    /// Whether a method, or any method of that name the sources declare that it calls, calls
    /// something that runs a rule. Names are matched as written, so a call the text only
    /// resembles counts too: a test is said to run no rule only when nothing it reaches could.
    /// Another test is not followed (no test calls a test; a repository names its tests after
    /// the API they test, `HaveNameStartingWith`).
    fn runs_a_rule(&self, method: &csharp::Method, extensions: &BTreeMap<String, Expect>) -> bool {
        let mut seen: Vec<&str> = vec![method.name.as_str()];
        let mut pending: Vec<&str> = vec![method.text.as_str()];
        while let Some(text) = pending.pop() {
            for name in called_names(text) {
                if SINKS.contains(&name) || RUNS.contains(&name) || extensions.contains_key(name) {
                    return true;
                }
                if seen.contains(&name) {
                    continue;
                }
                seen.push(name);
                for (index, (_, file)) in self.files.iter().enumerate() {
                    // Outside the tests, the incumbent's own sources (a repository that is the
                    // library) implement its API; a call of `That()` is not a helper.
                    let library = |decl: &&TypeDecl| {
                        index >= self.tests
                            && (decl.namespace.starts_with("ArchUnitNET")
                                || decl.namespace.starts_with("NetArchTest"))
                    };
                    for decl in file.types.iter().filter(|d| !library(d)) {
                        pending.extend(
                            decl.methods
                                .iter()
                                .filter(|m| m.name == name && !m.test)
                                .map(|m| m.text.as_str()),
                        );
                    }
                }
            }
        }
        false
    }

    /// Every test method in a file that uses ArchUnitNET or NetArchTest that has no rule chain
    /// and runs no rule, in file and line order.
    pub fn queries(&self) -> Vec<Query> {
        let extensions = self.extension_sinks();
        let with_rules: std::collections::BTreeSet<(String, String)> = self
            .candidates()
            .into_iter()
            .map(|c| (c.file, c.method))
            .collect();
        let mut out = Vec::new();
        for (_, file) in self.files.iter().take(self.tests) {
            for decl in &file.types {
                let Some(tool) = self.tool_of(decl) else {
                    continue;
                };
                for method in decl.methods.iter().filter(|m| m.test) {
                    if with_rules.contains(&(file.shown.clone(), method.name.clone()))
                        || self.runs_a_rule(method, &extensions)
                    {
                        continue;
                    }
                    out.push(Query {
                        file: file.shown.clone(),
                        line: method.line,
                        method: method.name.clone(),
                        tool,
                    });
                }
            }
        }
        out.sort_by(|a, b| {
            let at = |f: &str| self.files.iter().position(|(_, s)| s.shown == f);
            (at(&a.file), a.line).cmp(&(at(&b.file), b.line))
        });
        out
    }
}

/// `base`, or `base-2`, `base-3` ... the first not in `used`, which it joins. Two methods whose
/// names kebab alike (`Foo` twice and `Foo_2`) still get two names.
fn unique_name(base: &str, used: &mut std::collections::BTreeSet<String>) -> String {
    let mut name = base.to_owned();
    let mut n = 1usize;
    while used.contains(&name) {
        n += 1;
        name = format!("{base}-{n}");
    }
    used.insert(name.clone());
    name
}

/// A candidate's rule body, whether it is a slice rule, and why it is commented out.
type Outcome = (Option<Vec<(String, Node)>>, bool, Option<String>);

/// What a candidate becomes: see [`Outcome`].
fn mapped(program: &Program, candidate: &Candidate) -> Outcome {
    let mapped = match &candidate.chain {
        Ok(chain) => program.map(chain),
        Err(reason) => Mapped::Unmapped(reason.clone()),
    };
    let (body, slice, reason) = match mapped {
        Mapped::Element(body) => (Some(body), false, None),
        Mapped::Slice(body) => (Some(body), true, None),
        Mapped::Custom => (
            None,
            false,
            Some(
                if matches!(
                    &candidate.chain,
                    Ok(Chain {
                        root: Root::NetArchTest { .. },
                        ..
                    })
                ) {
                    "stays in NetArchTest: custom predicate (MeetCustomRule)"
                } else {
                    "stays in ArchUnitNET: custom predicate"
                }
                .to_owned(),
            ),
        ),
        Mapped::Unmapped(reason) => (None, false, Some(format!("not imported: {reason}"))),
    };
    let reason = reason.or_else(|| {
        (candidate.expect == Expect::Fails).then(|| {
            "not imported: the test expects this rule to be broken (Assert.False, Assert.Throws, AssertOnlyViolations), so it is not a rule to enforce".to_owned()
        })
    });
    (body, slice, reason)
}

/// A test that runs no rule, as a commented-out entry that stays with the incumbent.
fn query_item(query: &Query, name: String, path: &str) -> Item {
    let file = query.file.rsplit('/').next().unwrap_or(&query.file);
    Item {
        comments: vec![
            format!("{}()   [{file}:{}]", query.method, query.line),
            format!(
                "stays in {}: the test checks the architecture in C#, not with a rule",
                query.tool
            ),
        ],
        node: Node::Map(vec![
            ("name".to_owned(), Node::str(name)),
            (
                "comment".to_owned(),
                Node::str(format!("imported from {path}:{}", query.line)),
            ),
        ]),
        disabled: true,
    }
}

/// Maps every candidate to an item.
pub fn items(program: &Program, shown_dir: &str) -> Imported {
    let mut names = std::collections::BTreeSet::new();
    let mut imported = Imported {
        elements: Vec::new(),
        slices: Vec::new(),
        read: 0,
        enabled: 0,
        queried: 0,
    };
    let shown = |file: &str| {
        if file.starts_with(shown_dir) || shown_dir.is_empty() {
            file.to_owned()
        } else {
            format!("{shown_dir}/{file}")
        }
    };
    let at = |file: &str| program.files.iter().position(|(_, f)| f.shown == file);
    let mut entries: Vec<(Option<usize>, usize, Result<Candidate, Query>)> = program
        .candidates()
        .into_iter()
        .map(|c| (at(&c.file), c.line, Ok(c)))
        .chain(
            program
                .queries()
                .into_iter()
                .map(|q| (at(&q.file), q.line, Err(q))),
        )
        .collect();
    // Stable: the chains of one method keep their order, and a query sorts among them by line.
    entries.sort_by_key(|(file, line, _)| (*file, *line));
    for (_, _, entry) in entries {
        let candidate = match entry {
            Ok(candidate) => candidate,
            Err(query) => {
                imported.queried += 1;
                let name = unique_name(&kebab(&query.method), &mut names);
                imported
                    .elements
                    .push(query_item(&query, name, &shown(&query.file)));
                continue;
            }
        };
        imported.read += 1;
        let name = unique_name(&kebab(&candidate.method), &mut names);
        let path = shown(&candidate.file);
        let mut head = vec![
            ("name".to_owned(), Node::str(name)),
            (
                "comment".to_owned(),
                Node::str(format!("imported from {path}:{}", candidate.line)),
            ),
        ];
        let mut comments = chain_comment(&candidate);
        let (body, slice, reason) = mapped(program, &candidate);
        let disabled = reason.is_some();
        if let Some(reason) = reason {
            comments.push(reason);
        } else {
            imported.enabled += 1;
        }
        if let Some(body) = body {
            head.extend(body);
        }
        let item = Item {
            comments,
            node: Node::Map(head),
            disabled,
        };
        if slice {
            imported.slices.push(item);
        } else {
            imported.elements.push(item);
        }
    }
    imported
}

/// Imports the tests under the request's directory.
///
/// # Errors
/// [`ImportError`] when a file cannot be read or no C# file is found.
pub fn import(request: &Request) -> Result<Document, ImportError> {
    let tests: Vec<(PathBuf, String)> = types::walk(&request.dir, "cs")
        .into_iter()
        .map(|p| {
            let rel = p
                .strip_prefix(&request.dir)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            (p, rel)
        })
        .collect();
    if tests.is_empty() {
        return Err(ImportError::Invalid(format!(
            "{} holds no .cs file",
            request.shown
        )));
    }
    let root = types::solution_root(&request.dir);
    let mut declaration_dirs = vec![root.clone()];
    declaration_dirs.extend(request.sources.iter().cloned());
    let mut declarations = Vec::new();
    for dir in &declaration_dirs {
        for path in types::walk(dir, "cs") {
            if !path.starts_with(&request.dir) && !declarations.iter().any(|(p, _)| p == &path) {
                let shown = path.to_string_lossy().replace('\\', "/");
                declarations.push((path, shown));
            }
        }
    }
    let mut program = Program::read(&tests, &declarations, &root, &request.cwd)?;
    program.netarchtest_enhanced = tests.iter().any(|(path, _)| {
        types::project_of(path, &root).is_some_and(|(_, dir)| {
            types::projects_in(&dir)
                .iter()
                .any(|p| types::references_enhanced(p))
        })
    });
    if let Some(graph) = &request.graph {
        let shown = graph.to_string_lossy().into_owned();
        let text = std::fs::read_to_string(graph).map_err(|e| ImportError::Read {
            file: shown.clone(),
            reason: e.to_string(),
        })?;
        let document: rb_model::GraphDocument =
            serde_json::from_str(&text).map_err(|e| ImportError::Parse {
                file: shown,
                reason: e.to_string(),
            })?;
        for t in document.code.iter().flat_map(|c| c.types.iter()) {
            program.index.add_known(
                &t.full_name,
                t.namespace.as_deref().unwrap_or_default(),
                t.assembly.as_deref(),
            );
        }
    }
    Ok(document(&program, &request.shown))
}

/// The document for a program.
pub fn document(program: &Program, shown_dir: &str) -> Document {
    let imported = items(program, shown_dir);
    let (dotnet, loader_comments) = dotnet_block(program);
    let mut header = vec![
        format!("Imported from {shown_dir} by `rulebearing import archunit`."),
        "Each rule is preceded by the C# chain it came from; a chain that cannot be translated is kept, commented out, with the reason.".into(),
        format!(
            "{} rule chains read: {} imported, {} commented out.",
            imported.read,
            imported.enabled,
            imported.read - imported.enabled
        ),
    ];
    if imported.queried > 0 {
        header.push(format!(
            "{} tests run no rule (they check the architecture in C#) and stay, commented out.",
            imported.queried
        ));
    }
    if dotnet.is_none() && !loader_comments.is_empty() {
        header.push(String::new());
        header.extend(loader_comments.iter().cloned());
    }
    let mut body = vec![("$schema".to_owned(), Node::str(super::SCHEMA))];
    let mut key_comments = Vec::new();
    if let Some(block) = dotnet {
        key_comments.push(("languages".to_owned(), loader_comments));
        body.push(("languages".to_owned(), block));
    }
    let mut rules = Vec::new();
    if !imported.elements.is_empty() {
        rules.push(("elements", Node::List(imported.elements)));
    }
    if !imported.slices.is_empty() {
        rules.push(("slices", Node::List(imported.slices)));
    }
    body.push(("rules".to_owned(), Node::map(rules)));
    Document {
        header,
        body,
        key_comments,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_names_become_kebab_case() {
        for (name, expected) in [
            (
                "DomainShouldNotDependOnInfrastructure",
                "domain-should-not-depend-on-infrastructure",
            ),
            (
                "HolidayProvider_Should_Be_Internal",
                "holiday-provider-should-be-internal",
            ),
            ("IHolidayProviderTests", "i-holiday-provider-tests"),
            ("HTMLParserWorks", "html-parser-works"),
            ("Test1Case", "test1-case"),
            ("already-kebab", "already-kebab"),
        ] {
            assert_eq!(kebab(name), expected, "{name}");
        }
    }

    #[test]
    fn rule_names_are_unique_whatever_the_methods_are_called() -> Result<(), ImportError> {
        let mut used = std::collections::BTreeSet::new();
        let names: Vec<String> = ["foo", "foo", "foo-2", "foo", "bar"]
            .iter()
            .map(|b| unique_name(b, &mut used))
            .collect();
        assert_eq!(names, ["foo", "foo-2", "foo-2-2", "foo-3", "bar"]);
        let file = csharp::parse(
            r#"using static ArchUnitNET.Fluent.ArchRuleDefinition;
            class ArchTests {
                void Foo() {
                    Classes().That().HaveNameStartingWith("A").Should().BeSealed().Check(Architecture);
                    Classes().That().HaveNameStartingWith("B").Should().BeSealed().Check(Architecture);
                }
                void Foo_2() {
                    Classes().That().HaveNameStartingWith("C").Should().BeSealed().Check(Architecture);
                }
            }"#,
            "T.cs",
        )?;
        let program = Program::from_parts(vec![(PathBuf::from("T.cs"), file)], Index::default());
        let imported = items(&program, "");
        let names: Vec<String> = imported
            .elements
            .iter()
            .filter_map(|item| match &item.node {
                Node::Map(pairs) => pairs
                    .iter()
                    .find(|(k, _)| k == "name")
                    .map(|(_, v)| v.to_json().to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(names, [r#""foo""#, r#""foo-2""#, r#""foo-2-2""#]);
        Ok(())
    }

    #[test]
    fn folds_are_left_to_right_and_flat_for_one_connective() {
        let t = |k: &str| Node::map(vec![(k, Node::Bool(true))]);
        let mut fold = Fold::default();
        fold.push(t("a"));
        fold.connective("all");
        fold.push(t("b"));
        fold.connective("any");
        fold.push(t("c"));
        fold.connective("any");
        fold.push(t("d"));
        assert_eq!(
            fold.finish().map(|n| n.to_json()),
            Some(
                serde_json::json!({"any": [{"all": [{"a": true}, {"b": true}]}, {"c": true}, {"d": true}]})
            )
        );
    }

    /// NetArchTest 1.3.2 `Or_MultipleInstances_TreatedAsSeparateGroups` (`PredicateListTests` and
    /// `ConditionListTests`): `A.And().B().Or().C().And().D()` is `(A and B) or (C and D)`.
    #[test]
    fn net_arch_test_folds_and_runs_into_or_ed_groups() {
        use serde_json::json;
        let t = |k: &str| Node::map(vec![(k, Node::Bool(true))]);
        let fold_of = |ops: &[&'static str]| {
            let mut fold = Fold::grouped();
            fold.push(t("a"));
            for (op, k) in ops.iter().zip(["b", "c", "d"]) {
                fold.connective(op);
                fold.push(t(k));
            }
            fold.finish().map(|n| n.to_json())
        };
        assert_eq!(
            fold_of(&["all", "any", "all"]),
            Some(
                json!({"any": [{"all": [{"a": true}, {"b": true}]}, {"all": [{"c": true}, {"d": true}]}]})
            )
        );
        assert_eq!(
            fold_of(&["any", "any", "any"]),
            Some(json!({"any": [{"a": true}, {"b": true}, {"c": true}, {"d": true}]}))
        );
        assert_eq!(
            fold_of(&["all", "all", "all"]),
            Some(json!({"all": [{"a": true}, {"b": true}, {"c": true}, {"d": true}]}))
        );
        assert_eq!(
            fold_of(&["any", "all", "any"]),
            Some(json!({"any": [{"a": true}, {"all": [{"b": true}, {"c": true}]}, {"d": true}]}))
        );
        assert_eq!(Fold::grouped().finish(), None);
        let mut one = Fold::grouped();
        one.push(t("a"));
        assert_eq!(one.finish().map(|n| n.to_json()), Some(json!({"a": true})));
    }

    #[test]
    fn assertions_decide_what_a_test_expects() -> Result<(), ImportError> {
        let file = csharp::parse(
            r"class T { void M() {
                Assert.False(r.HasNoViolations(a));
                Assert.True(!r.HasNoViolations(a));
                r.HasNoViolations(a).Should().BeFalse();
                r.HasNoViolations(a).ShouldBeTrue();
                Assert.Throws<X>(() => r.Check(a));
                r.AssertOnlyViolations(h);
                r.Check(a);
                Assert.True(r.HasNoViolations(a) is false);
                Assert.True(r.HasNoViolations(a) == false);
            } }",
            "T.cs",
        )?;
        let mut found = Vec::new();
        for stmt in &file.types[0].methods[0].body {
            if let Stmt::Expr { expr, .. } = stmt {
                sinks(expr, Expect::Passes, &BTreeMap::new(), &mut found);
            }
        }
        let expects: Vec<Expect> = found.iter().map(|f| f.3).collect();
        let (fails, passes) = (Expect::Fails, Expect::Passes);
        assert_eq!(
            expects,
            [
                fails, fails, fails, passes, fails, fails, passes, fails, fails
            ]
        );
        Ok(())
    }

    #[test]
    fn net_arch_test_results_are_asserted_later() -> Result<(), ImportError> {
        let file = csharp::parse(
            "class T { void M() { var result = x.GetResult(); Assert.False(result.IsSuccessful); } void N() { var r = x.GetResult(); r.IsSuccessful.Should().BeTrue(); } }",
            "T.cs",
        )?;
        let m = &file.types[0].methods;
        assert_eq!(result_expectation("result", &m[0].body[1..]), Expect::Fails);
        assert_eq!(result_expectation("r", &m[1].body[1..]), Expect::Passes);
        assert_eq!(result_expectation("q", &m[1].body[1..]), Expect::Passes);
        Ok(())
    }

    /// A program over one test file and, optionally, files read only for their declarations.
    fn program_of(test: &str, others: &[&str]) -> Result<Program, ImportError> {
        let mut files = vec![(PathBuf::from("T.cs"), csharp::parse(test, "T.cs")?)];
        for (i, other) in others.iter().enumerate() {
            let shown = format!("O{i}.cs");
            files.push((PathBuf::from(&shown), csharp::parse(other, &shown)?));
        }
        let mut index = Index::default();
        for (_, file) in &files {
            index.add_source(file, None);
        }
        Ok(Program {
            files,
            index,
            tests: 1,
            netarchtest_enhanced: false,
        })
    }

    fn imported(program: &Program) -> String {
        super::super::yaml::render(&document(program, ""))
    }

    #[test]
    fn helpers_are_evaluated_with_their_arguments_bound() -> Result<(), ImportError> {
        let program = program_of(
            r#"using static ArchUnitNET.Fluent.ArchRuleDefinition;
            using static N.Utils;
            namespace N;
            class Order { }
            static class Utils { public static string nsof<T>() => typeof(T).Namespace; }
            class ArchTests {
                static IArchRule Rule(string prefix, string suffix = "Service", params string[] more) =>
                    Classes().That().HaveNameStartingWith(prefix).Should().HaveNameEndingWith(suffix);
                [Fact] void Defaulted() { Rule("A").Check(Architecture); }
                [Fact] void Named() { Rule(suffix: "X", prefix: "B").Check(Architecture); }
                [Fact] void Generic() { Classes().That().ResideInNamespace(nsof<Order>()).Should().BeSealed().Check(Architecture); }
                [Fact] void Inferred() { Classes().That().ResideInNamespace(nsof()).Should().BeSealed().Check(Architecture); }
                [Fact] void Unknown() { Rule(nope: "x").Check(Architecture); }
                [Fact] void TooMany() { Other("a", "b").Check(Architecture); }
                static IArchRule Other(string a) => Classes().Should().HaveName(a);
            }"#,
            &[],
        )?;
        let text = imported(&program);
        for expected in [
            "- name: defaulted",
            "haveNameStartingWith: A",
            "haveNameEndingWith: Service",
            "haveNameStartingWith: B",
            "haveNameEndingWith: X",
            "- name: generic",
            "resideInNamespace: \"N\"",
            "# not imported: `nsof()` calls the generic `nsof` without its type arguments, which the importer does not infer",
            "`Rule(nope: \"x\")` names `nope`, which `Rule` does not take",
            "`Other(\"a\", \"b\")` calls `Other` with 2 arguments, which no declaration of it takes",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        Ok(())
    }

    #[test]
    fn a_params_array_collects_the_rest() -> Result<(), ImportError> {
        let program = program_of(
            r#"using static ArchUnitNET.Fluent.ArchRuleDefinition;
            class ArchTests {
                static IArchRule Named(params string[] names) => Classes().Should().HaveName(names);
                [Fact] void Two() { Named("A", "B").Check(Architecture); }
                [Fact] void Array() { Named(new[] { "C", "D" }).Check(Architecture); }
                [Fact] void None() { Named().Check(Architecture); }
            }"#,
            &[],
        )?;
        let text = imported(&program);
        assert!(text.contains("haveName: [A, B]"), "{text}");
        assert!(text.contains("haveName: [C, D]"), "{text}");
        assert!(text.contains("`HaveName` names nothing"), "{text}");
        Ok(())
    }

    #[test]
    fn a_class_fixture_is_an_instance_of_its_class() -> Result<(), ImportError> {
        let program = program_of(
            r#"using NetArchTest.Rules;
            namespace N;
            public class Fix { public Types Types { get; } = Types.InCurrentDomain(); }
            public class A : IClassFixture<Fix> {
                private readonly Fix _fix;
                public A(Fix fix) { _fix = fix; }
                [Fact] public void Sealed() { Assert.True(_fix.Types.That().HaveNameEndingWith("S", StringComparison.Ordinal).Should().BeSealed().GetResult().IsSuccessful); }
            }
            public class B(Fix fix) {
                [Fact] public void NotFixture() { Assert.True(fix.Types.Should().BeSealed().GetResult().IsSuccessful); }
            }"#,
            &[],
        )?;
        let text = imported(&program);
        assert!(text.contains("- name: sealed\n"), "{text}");
        assert!(text.contains("haveNameEndingWith: S"), "{text}");
        assert!(
            text.contains("`fix` is a constructor parameter the test framework fills"),
            "{text}"
        );
        Ok(())
    }

    #[test]
    fn extension_methods_continue_the_chain_or_name_a_selection() -> Result<(), ImportError> {
        let program = program_of(
            r#"using System.Linq;
            using NetArchTest.Rules;
            namespace N;
            static class Ext {
                internal static string[] Full(this PredicateList p) => p.GetTypes().Select(t => t.FullName).ToList();
                internal static string[] Short(this PredicateList p) => p.GetTypes().Select(t => t.Name).ToArray();
                internal static ConditionList Twice(this PredicateList p) => p.Should().BeSealed();
                internal static ConditionList Twice(this PredicateList p, int n = 1) => p.Should().BeSealed();
            }
            class A {
                [Fact] void Full() { Assert.True(Types.InCurrentDomain().ShouldNot().HaveDependencyOnAny(Types.InCurrentDomain().That().ResideInNamespace("X").Full()).GetResult().IsSuccessful); }
                [Fact] void Short() { Assert.True(Types.InCurrentDomain().ShouldNot().HaveDependencyOnAny(Types.InCurrentDomain().That().ResideInNamespace("X").Short()).GetResult().IsSuccessful); }
                [Fact] void Overloaded() { Assert.True(Types.InCurrentDomain().That().AreSealed().Twice().GetResult().IsSuccessful); }
                [Fact] void WithCondition() { Assert.True(Types.InCurrentDomain().ShouldNot().HaveDependencyOnAny(Types.InCurrentDomain().Should().BeSealed().GetTypes().Select(t => t.FullName)).GetResult().IsSuccessful); }
            }"#,
            &[],
        )?;
        let text = imported(&program);
        assert!(text.contains("- areNestedIn:"), "{text}");
        assert!(text.contains("haveFullNameStartingWith: X"), "{text}");
        assert!(
            text.contains("`p.GetTypes().Select(t => t.Name)` cannot be read from source"),
            "{text}"
        );
        assert!(
            text.contains(
                "calls `Twice`, overloaded in the sources, which the importer does not resolve"
            ),
            "{text}"
        );
        assert!(
            text.contains("has a condition, so it is not a selection"),
            "{text}"
        );
        Ok(())
    }

    #[test]
    fn a_test_that_runs_no_rule_stays() -> Result<(), ImportError> {
        let program = program_of(
            r#"using System.Linq;
            using ArchUnitNET.Domain;
            using static ArchUnitNET.Fluent.ArchRuleDefinition;
            namespace N;
            class ArchTests {
                [Fact] void Counted() { Assert.Single(Architecture.Classes.Where(c => c.IsSealed)); }
                [Fact] void ThroughAHelper() { Run(); }
                [Fact] void ThroughALibraryName() { Assert.Single(Sealed()); }
                [Test] void HaveName() { Classes().Should().HaveName("A").Check(Architecture); }
                [Fact] void NamedLikeATest() { Assert.Single(HaveName()); }
                void Run() { Classes().Should().BeSealed().Check(Architecture); }
            }"#,
            &["namespace ArchUnitNET.Fluent { class Api { object Sealed() { return Check(); } } }"],
        )?;
        let queries: Vec<String> = program.queries().into_iter().map(|q| q.method).collect();
        // `Run` runs a rule; the library's own `Sealed` is not a helper; a test is not followed.
        assert_eq!(
            queries,
            ["Counted", "ThroughALibraryName", "NamedLikeATest"]
        );
        let text = imported(&program);
        assert!(
            text.contains("# stays in ArchUnitNET: the test checks the architecture in C#, not with a rule\n    # - name: counted"),
            "{text}"
        );
        assert!(text.contains("3 tests run no rule"), "{text}");
        // A file that uses neither library holds no architecture test.
        let plain = program_of(
            "class U { [Fact] void Adds() { Assert.Equal(2, 1 + 1); } }",
            &[],
        )?;
        assert!(plain.queries().is_empty());
        Ok(())
    }

    #[test]
    fn called_names_over_approximate_calls() {
        assert_eq!(
            called_names("a.B(c).D<E>(F) + G (1) - h[2] + i.J"),
            ["B", "D", "G"]
        );
        assert!(called_names("").is_empty());
    }

    #[test]
    fn library_members_win_over_extension_methods() {
        for (name, library) in [
            ("That", true),
            ("GetResult", true),
            ("FollowCustomCondition", true),
            ("BeSealed", true),
            ("HaveNameStartingWith", true),
            ("GetModuleTypes", false),
            ("NotHavePublicSetters", false),
        ] {
            assert_eq!(library_member(name), library, "{name}");
        }
    }

    #[test]
    fn net_arch_test_enhanced_edition_is_not_read_as_net_arch_test() -> Result<(), ImportError> {
        let mut program = program_of(
            r"using NetArchTest.Rules;
            class A { [Fact] void S() { Assert.True(Types.InCurrentDomain().Should().BeSealed().GetResult().IsSuccessful); }
                      [Fact] void C() { Assert.True(Types.InCurrentDomain().Should().MeetCustomRule(new R()).GetResult().IsSuccessful); } }",
            &[],
        )?;
        assert!(imported(&program).contains("- name: s\n"));
        program.netarchtest_enhanced = true;
        let text = imported(&program);
        assert!(
            text.contains("# not imported: the tests use NetArchTest.eNhancedEdition, which is not backwards compatible"),
            "{text}"
        );
        assert!(
            text.contains("stays in NetArchTest: custom predicate"),
            "{text}"
        );
        Ok(())
    }
}
