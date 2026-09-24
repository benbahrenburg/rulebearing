//! Element, slice and diagram rules: `ArchUnitNET`'s three rule families as configuration.
//!
//! - Source: [design § Element rules](../../../docs/artifacts/design.md#element-rules-archunitnet-declarative),
//!   [§ Slice rules](../../../docs/artifacts/design.md#slice-rules),
//!   [§ Diagram rules](../../../docs/artifacts/design.md#diagram-rules)
//! - Coverage: [`ArchUnitNET` 0.13.4 coverage tab](../../../docs/artifacts/archunitnet-0.13.4-coverage.md)
//! - Decisions: [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md) (keys are
//!   `ArchUnitNET`'s method names in camelCase, nothing renamed),
//!   [ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md) (`allowEmpty`)
//! - Plan: [Wave 2, Steps 5 and 6](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#25-step-5-the-element-rule-engine-and-the-capability-table-2c)
//! - Requirements: [FR-RULE-03](../../../docs/prd.md#fr-rule-03), [FR-RULE-04](../../../docs/prd.md#fr-rule-04),
//!   [FR-RULE-05](../../../docs/prd.md#fr-rule-05)
//!
//! An element rule selects objects (`select.kind`, filtered by `select.where`) and states what
//! each must satisfy (`should`). Both sides are expressions over the same predicates, spelled as
//! `ArchUnitNET` spells them on each side: `arePublic` in `where`, `bePublic` in `should`, and every
//! negated twin (`areNotPublic`, `doNotHaveName`, `notBePublic`, `notHaveName`). Every key parses to
//! one [`Concept`] and a negation, so the predicate and condition spellings of one test cannot
//! disagree, which is what `ArchUnitNET`'s own suite checks pairwise.
//!
//! ```yaml
//! rules:
//!   elements:
//!     - name: services-are-sealed
//!       comment: "adr:0004"
//!       select: { kind: class, where: { haveNameEndingWith: Service } }
//!       should: { beSealed: true }
//! ```
//!
//! An expression is a mapping (every key must hold), a list (every item must hold), or one of the
//! combinators `all`, `any` and `not`. `ArchUnitNET`'s `A().And().B().Or().C()` is left-associative,
//! `any: [{ all: [A, B] }, C]`. An unknown key is a configuration error naming the nearest known
//! key.

use serde_json::{Map, Value};

use crate::ConfigError;
use rb_model::Severity;

/// What `select.kind` selects: `ArchUnitNET`'s `Types()`, `Classes()`, `Interfaces()`,
/// `Attributes()`, `Members()`, `FieldMembers()`, `MethodMembers()`, `PropertyMembers()`, plus the
/// function and module kinds the other languages have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// Every type.
    Type,
    /// Classes, attributes and records.
    Class,
    /// Interfaces.
    Interface,
    /// Attribute types.
    Attribute,
    /// Every member.
    Member,
    /// Fields.
    Field,
    /// Methods and constructors.
    Method,
    /// Properties.
    Property,
    /// Module-level functions (TypeScript, Python).
    Function,
    /// Modules of the module layer.
    Module,
}

impl Kind {
    /// The `select.kind` spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Attribute => "attribute",
            Self::Member => "member",
            Self::Field => "field",
            Self::Method => "method",
            Self::Property => "property",
            Self::Function => "function",
            Self::Module => "module",
        }
    }

    /// Every kind, in declaration order.
    pub const ALL: [Self; 10] = [
        Self::Type,
        Self::Class,
        Self::Interface,
        Self::Attribute,
        Self::Member,
        Self::Field,
        Self::Method,
        Self::Property,
        Self::Function,
        Self::Module,
    ];

    fn parse(text: &str) -> Result<Self, ConfigError> {
        Self::ALL
            .into_iter()
            .find(|k| k.as_str() == text)
            .ok_or_else(|| {
                ConfigError::Invalid(format!(
                    "`{text}` is not a select kind; use {}",
                    Self::ALL.map(Self::as_str).join(", ")
                ))
            })
    }

    /// Whether the kind selects members rather than types.
    pub const fn is_member(self) -> bool {
        matches!(
            self,
            Self::Member | Self::Field | Self::Method | Self::Property
        )
    }
}

/// What a key's value is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// `true` (the key holds) or `false` (its negation holds).
    Flag,
    /// One name or a list, any of which may match.
    Names,
    /// A regular expression.
    Pattern,
    /// Objects named by full name, or a nested selector.
    Objects,
    /// An attribute with positional arguments: `{ attribute, arguments: [...] }`.
    AttributeArguments,
    /// An attribute with named arguments: `{ attribute, arguments: { name: value } }`.
    AttributeNamedArguments,
    /// Positional argument values: `[...]`.
    ArgumentValues,
    /// Named argument values: `{ name: value }`.
    NamedArgumentValues,
    /// A `PlantUML` file path.
    Diagram,
}

/// One predicate or condition, whatever its spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(missing_docs)] // each variant is the ArchUnitNET method it is named after
pub enum Concept {
    /// `Are(...)` / `Be(...)`: the object is one of these.
    Identity,
    /// `Exist()`: the selection is not empty.
    Exist,
    Public,
    Private,
    Protected,
    Internal,
    ProtectedInternal,
    PrivateProtected,
    HaveName,
    HaveNameMatching,
    HaveNameStartingWith,
    HaveNameEndingWith,
    HaveNameContaining,
    HaveFullName,
    HaveFullNameMatching,
    HaveFullNameStartingWith,
    HaveFullNameEndingWith,
    HaveFullNameContaining,
    HaveAssemblyQualifiedName,
    HaveAssemblyQualifiedNameMatching,
    HaveAssemblyQualifiedNameStartingWith,
    HaveAssemblyQualifiedNameEndingWith,
    HaveAssemblyQualifiedNameContaining,
    ResideInNamespace,
    ResideInNamespaceMatching,
    ResideInAssembly,
    ResideInAssemblyMatching,
    DependOnAny,
    OnlyDependOn,
    CallAny,
    HaveAnyAttributes,
    OnlyHaveAttributes,
    HaveAttributeWithArguments,
    HaveAttributeWithNamedArguments,
    HaveAnyAttributesWithArguments,
    HaveAnyAttributesWithNamedArguments,
    AssignableTo,
    ImplementInterface,
    ImplementAnyInterfaces,
    Enums,
    Structs,
    ValueTypes,
    Nested,
    NestedIn,
    HaveMemberWithName,
    HaveFieldMemberWithName,
    HaveMethodMemberWithName,
    HavePropertyMemberWithName,
    Abstract,
    Sealed,
    Record,
    Immutable,
    Generic,
    DeclaredIn,
    Static,
    ReadOnly,
    Constructor,
    Virtual,
    HaveReturnType,
    HaveDependencyInMethodBodyTo,
    CalledBy,
    HaveGetter,
    HaveSetter,
    HaveInitOnlySetter,
    HavePublicGetter,
    HaveProtectedGetter,
    HaveInternalGetter,
    HaveProtectedInternalGetter,
    HavePrivateGetter,
    HavePrivateProtectedGetter,
    HavePublicSetter,
    HaveProtectedSetter,
    HaveInternalSetter,
    HaveProtectedInternalSetter,
    HavePrivateSetter,
    HavePrivateProtectedSetter,
    AdhereToPlantUmlDiagram,
}

/// Every concept: its canonical name (the key with `are` / `be` and the negation removed), its
/// value kind, and whether it may appear in `where` (`false` for a condition-only concept).
pub const VOCABULARY: &[(&str, Concept, ValueKind, bool)] = &[
    ("", Concept::Identity, ValueKind::Objects, true),
    ("exist", Concept::Exist, ValueKind::Flag, false),
    ("public", Concept::Public, ValueKind::Flag, true),
    ("private", Concept::Private, ValueKind::Flag, true),
    ("protected", Concept::Protected, ValueKind::Flag, true),
    ("internal", Concept::Internal, ValueKind::Flag, true),
    (
        "protectedInternal",
        Concept::ProtectedInternal,
        ValueKind::Flag,
        true,
    ),
    (
        "privateProtected",
        Concept::PrivateProtected,
        ValueKind::Flag,
        true,
    ),
    ("haveName", Concept::HaveName, ValueKind::Names, true),
    (
        "haveNameMatching",
        Concept::HaveNameMatching,
        ValueKind::Pattern,
        true,
    ),
    (
        "haveNameStartingWith",
        Concept::HaveNameStartingWith,
        ValueKind::Names,
        true,
    ),
    (
        "haveNameEndingWith",
        Concept::HaveNameEndingWith,
        ValueKind::Names,
        true,
    ),
    (
        "haveNameContaining",
        Concept::HaveNameContaining,
        ValueKind::Names,
        true,
    ),
    (
        "haveFullName",
        Concept::HaveFullName,
        ValueKind::Names,
        true,
    ),
    (
        "haveFullNameMatching",
        Concept::HaveFullNameMatching,
        ValueKind::Pattern,
        true,
    ),
    (
        "haveFullNameStartingWith",
        Concept::HaveFullNameStartingWith,
        ValueKind::Names,
        true,
    ),
    (
        "haveFullNameEndingWith",
        Concept::HaveFullNameEndingWith,
        ValueKind::Names,
        true,
    ),
    (
        "haveFullNameContaining",
        Concept::HaveFullNameContaining,
        ValueKind::Names,
        true,
    ),
    (
        "haveAssemblyQualifiedName",
        Concept::HaveAssemblyQualifiedName,
        ValueKind::Names,
        true,
    ),
    (
        "haveAssemblyQualifiedNameMatching",
        Concept::HaveAssemblyQualifiedNameMatching,
        ValueKind::Pattern,
        true,
    ),
    (
        "haveAssemblyQualifiedNameStartingWith",
        Concept::HaveAssemblyQualifiedNameStartingWith,
        ValueKind::Names,
        true,
    ),
    (
        "haveAssemblyQualifiedNameEndingWith",
        Concept::HaveAssemblyQualifiedNameEndingWith,
        ValueKind::Names,
        true,
    ),
    (
        "haveAssemblyQualifiedNameContaining",
        Concept::HaveAssemblyQualifiedNameContaining,
        ValueKind::Names,
        true,
    ),
    (
        "resideInNamespace",
        Concept::ResideInNamespace,
        ValueKind::Names,
        true,
    ),
    (
        "resideInNamespaceMatching",
        Concept::ResideInNamespaceMatching,
        ValueKind::Pattern,
        true,
    ),
    (
        "resideInAssembly",
        Concept::ResideInAssembly,
        ValueKind::Names,
        true,
    ),
    (
        "resideInAssemblyMatching",
        Concept::ResideInAssemblyMatching,
        ValueKind::Pattern,
        true,
    ),
    (
        "dependOnAny",
        Concept::DependOnAny,
        ValueKind::Objects,
        true,
    ),
    (
        "onlyDependOn",
        Concept::OnlyDependOn,
        ValueKind::Objects,
        true,
    ),
    ("callAny", Concept::CallAny, ValueKind::Objects, true),
    (
        "haveAnyAttributes",
        Concept::HaveAnyAttributes,
        ValueKind::Objects,
        true,
    ),
    (
        "onlyHaveAttributes",
        Concept::OnlyHaveAttributes,
        ValueKind::Objects,
        true,
    ),
    (
        "haveAttributeWithArguments",
        Concept::HaveAttributeWithArguments,
        ValueKind::AttributeArguments,
        true,
    ),
    (
        "haveAttributeWithNamedArguments",
        Concept::HaveAttributeWithNamedArguments,
        ValueKind::AttributeNamedArguments,
        true,
    ),
    (
        "haveAnyAttributesWithArguments",
        Concept::HaveAnyAttributesWithArguments,
        ValueKind::ArgumentValues,
        true,
    ),
    (
        "haveAnyAttributesWithNamedArguments",
        Concept::HaveAnyAttributesWithNamedArguments,
        ValueKind::NamedArgumentValues,
        true,
    ),
    (
        "assignableTo",
        Concept::AssignableTo,
        ValueKind::Objects,
        true,
    ),
    (
        "implementInterface",
        Concept::ImplementInterface,
        ValueKind::Objects,
        true,
    ),
    (
        "implementAnyInterfaces",
        Concept::ImplementAnyInterfaces,
        ValueKind::Objects,
        true,
    ),
    ("enums", Concept::Enums, ValueKind::Flag, true),
    ("structs", Concept::Structs, ValueKind::Flag, true),
    ("valueTypes", Concept::ValueTypes, ValueKind::Flag, true),
    ("nested", Concept::Nested, ValueKind::Flag, true),
    ("nestedIn", Concept::NestedIn, ValueKind::Objects, true),
    (
        "haveMemberWithName",
        Concept::HaveMemberWithName,
        ValueKind::Names,
        true,
    ),
    (
        "haveFieldMemberWithName",
        Concept::HaveFieldMemberWithName,
        ValueKind::Names,
        true,
    ),
    (
        "haveMethodMemberWithName",
        Concept::HaveMethodMemberWithName,
        ValueKind::Names,
        true,
    ),
    (
        "havePropertyMemberWithName",
        Concept::HavePropertyMemberWithName,
        ValueKind::Names,
        true,
    ),
    ("abstract", Concept::Abstract, ValueKind::Flag, true),
    ("sealed", Concept::Sealed, ValueKind::Flag, true),
    ("record", Concept::Record, ValueKind::Flag, true),
    ("immutable", Concept::Immutable, ValueKind::Flag, true),
    ("generic", Concept::Generic, ValueKind::Flag, true),
    ("declaredIn", Concept::DeclaredIn, ValueKind::Objects, true),
    ("static", Concept::Static, ValueKind::Flag, true),
    ("readOnly", Concept::ReadOnly, ValueKind::Flag, true),
    ("constructor", Concept::Constructor, ValueKind::Flag, true),
    ("virtual", Concept::Virtual, ValueKind::Flag, true),
    (
        "haveReturnType",
        Concept::HaveReturnType,
        ValueKind::Objects,
        true,
    ),
    (
        "haveDependencyInMethodBodyTo",
        Concept::HaveDependencyInMethodBodyTo,
        ValueKind::Objects,
        true,
    ),
    ("calledBy", Concept::CalledBy, ValueKind::Objects, true),
    ("haveGetter", Concept::HaveGetter, ValueKind::Flag, true),
    ("haveSetter", Concept::HaveSetter, ValueKind::Flag, true),
    (
        "haveInitOnlySetter",
        Concept::HaveInitOnlySetter,
        ValueKind::Flag,
        true,
    ),
    (
        "havePublicGetter",
        Concept::HavePublicGetter,
        ValueKind::Flag,
        true,
    ),
    (
        "haveProtectedGetter",
        Concept::HaveProtectedGetter,
        ValueKind::Flag,
        true,
    ),
    (
        "haveInternalGetter",
        Concept::HaveInternalGetter,
        ValueKind::Flag,
        true,
    ),
    (
        "haveProtectedInternalGetter",
        Concept::HaveProtectedInternalGetter,
        ValueKind::Flag,
        true,
    ),
    (
        "havePrivateGetter",
        Concept::HavePrivateGetter,
        ValueKind::Flag,
        true,
    ),
    (
        "havePrivateProtectedGetter",
        Concept::HavePrivateProtectedGetter,
        ValueKind::Flag,
        true,
    ),
    (
        "havePublicSetter",
        Concept::HavePublicSetter,
        ValueKind::Flag,
        true,
    ),
    (
        "haveProtectedSetter",
        Concept::HaveProtectedSetter,
        ValueKind::Flag,
        true,
    ),
    (
        "haveInternalSetter",
        Concept::HaveInternalSetter,
        ValueKind::Flag,
        true,
    ),
    (
        "haveProtectedInternalSetter",
        Concept::HaveProtectedInternalSetter,
        ValueKind::Flag,
        true,
    ),
    (
        "havePrivateSetter",
        Concept::HavePrivateSetter,
        ValueKind::Flag,
        true,
    ),
    (
        "havePrivateProtectedSetter",
        Concept::HavePrivateProtectedSetter,
        ValueKind::Flag,
        true,
    ),
    (
        "adhereToPlantUmlDiagram",
        Concept::AdhereToPlantUmlDiagram,
        ValueKind::Diagram,
        false,
    ),
];

impl Concept {
    /// The canonical name, the value kind and whether `where` may use it.
    pub fn entry(self) -> (&'static str, ValueKind, bool) {
        VOCABULARY
            .iter()
            .find(|(_, c, _, _)| *c == self)
            .map_or(("", ValueKind::Flag, false), |(n, _, k, w)| (n, *k, *w))
    }

    /// Every concept, in vocabulary order.
    pub fn all() -> impl Iterator<Item = Self> {
        VOCABULARY.iter().map(|(_, c, _, _)| *c)
    }
}

/// Which side of a rule an expression is on, which decides how keys are spelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// `select.where`: `ArchUnitNET`'s predicates (`arePublic`, `doNotHaveName`).
    Where,
    /// `should`: its conditions (`bePublic`, `notHaveName`).
    Should,
}

fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |c| {
        c.to_ascii_lowercase().to_string() + chars.as_str()
    })
}

/// Splits a key into its canonical concept name, its negation, and whether it named a nested
/// selector (`...That`): `areNotPublic` is `("public", true, false)`, `dependOnAnyTypesThat` is
/// `("dependOnAny", false, true)`.
pub fn split_key(key: &str, side: Side) -> (String, bool, bool) {
    let (mut rest, mut negated) = match side {
        Side::Where => {
            if let Some(r) = key.strip_prefix("areNot") {
                (lower_first(r), true)
            } else if let Some(r) = key.strip_prefix("doNot") {
                (lower_first(r), true)
            } else if let Some(r) = key.strip_prefix("are") {
                (lower_first(r), false)
            } else {
                (key.to_owned(), false)
            }
        }
        Side::Should => {
            if let Some(r) = key.strip_prefix("notBe") {
                (lower_first(r), true)
            } else if let Some(r) = key.strip_prefix("not") {
                (lower_first(r), true)
            } else if let Some(r) = key.strip_prefix("be") {
                (lower_first(r), false)
            } else {
                (key.to_owned(), false)
            }
        }
    };
    let mut selector = false;
    for suffix in ["TypesThat", "That"] {
        if let Some(r) = rest.strip_suffix(suffix) {
            rest = r.to_owned();
            selector = true;
            break;
        }
    }
    // `ArchUnitNET`'s plural and "no" spellings.
    let (base, flip) = match rest.as_str() {
        "constructors" => ("constructor".to_owned(), false),
        "noConstructors" | "noConstructor" => ("constructor".to_owned(), true),
        "haveNoGetter" => ("haveGetter".to_owned(), true),
        "haveNoSetter" => ("haveSetter".to_owned(), true),
        // The coverage tab's names for `ImplementAnyInterfaces` and `HaveInitSetter`.
        "implementAny" => ("implementAnyInterfaces".to_owned(), false),
        "haveInitSetter" => ("haveInitOnlySetter".to_owned(), false),
        "types" | "methodMembers" => (String::new(), false),
        _ => (rest.clone(), false),
    };
    negated ^= flip;
    (base, negated, selector)
}

/// Selected objects named by full name, or by a nested selector (`BeTypesThat`).
#[derive(Debug, Clone, PartialEq)]
pub enum Objects {
    /// Full names; any may match.
    Names(Vec<String>),
    /// Every object a selector selects.
    Selector(Box<Selector>),
}

/// A predicate or condition's value, by [`ValueKind`].
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// A flag, already folded into the test's negation.
    Flag,
    /// Names, any of which may match.
    Names(Vec<String>),
    /// A regular expression (JavaScript syntax, compiled by the engine).
    Pattern(String),
    /// Objects.
    Objects(Objects),
    /// An attribute and its arguments; `named` for named arguments.
    Attribute {
        /// The attribute, when named.
        attribute: Option<Objects>,
        /// Positional arguments, as literal text.
        positional: Vec<String>,
        /// Named arguments, as literal text.
        named: Vec<(String, String)>,
    },
    /// A diagram path.
    Diagram(String),
}

/// One predicate or condition applied.
#[derive(Debug, Clone, PartialEq)]
pub struct Test {
    /// The key as written, for messages and the violation id.
    pub key: String,
    /// What it tests.
    pub concept: Concept,
    /// Whether it is negated.
    pub negated: bool,
    /// Its value.
    pub operand: Operand,
}

/// An expression over tests.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Every item holds.
    All(Vec<Expr>),
    /// Some item holds.
    Any(Vec<Expr>),
    /// The item does not hold.
    Not(Box<Expr>),
    /// One test.
    Test(Test),
}

/// `select`: a kind, an optional language scope and an optional filter.
#[derive(Debug, Clone, PartialEq)]
pub struct Selector {
    /// What is selected.
    pub kind: Kind,
    /// Only objects of these languages (`select.language`); empty for every language. A rule
    /// using a key one language cannot answer is scoped with it
    /// ([ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md)).
    pub languages: Vec<rb_model::Language>,
    /// The filter, when there is one.
    pub where_: Option<Expr>,
    /// Also select the types the code references but does not define (`ArchUnitNET`'s
    /// `Types(true)`, `Classes(true)`, `Interfaces(true)`, `Attributes(true)`).
    pub include_referenced: bool,
}

/// One element rule.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementRule {
    /// The name.
    pub name: String,
    /// The comment.
    pub comment: Option<String>,
    /// The fix text.
    pub fix: Option<String>,
    /// The severity. Default `error`.
    pub severity: Severity,
    /// `ArchUnitNET`'s `Because(...)`.
    pub because: Option<String>,
    /// `WithoutRequiringPositiveResults()`: an empty selection is not vacuous.
    pub allow_empty: bool,
    /// What is selected.
    pub select: Selector,
    /// What every selected object must satisfy.
    pub should: Expr,
}

/// What a slice rule requires of its slices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceCondition {
    /// No slice depends on another.
    NotDependOnEachOther,
    /// The slice graph has no cycle.
    BeFreeOfCycles,
}

/// One slice rule.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceRule {
    /// The name.
    pub name: String,
    /// The comment.
    pub comment: Option<String>,
    /// The fix text.
    pub fix: Option<String>,
    /// The severity.
    pub severity: Severity,
    /// `Matching("X.(*)")`, or `MatchingWithPackages` with `(**)`.
    pub matching: String,
    /// The conditions, every one of which must hold.
    pub should: Vec<SliceCondition>,
    /// Slice names left out.
    pub ignore: Vec<String>,
    /// A pattern a slice name must match to take part.
    pub where_: Option<String>,
    /// Keep only the first this many segments of a slice name, so a package and everything
    /// below it form one slice: import-linter's `acyclic_siblings`, which `ArchUnitNET`'s
    /// patterns cannot say (a Rulebearing addition).
    pub segments: Option<usize>,
    /// The module imports taken out before the slices are joined (a Rulebearing addition,
    /// [ADR-0038](../../../docs/adr/0038-a-rule-narrows-the-graph-it-sees.md)); never
    /// `chainsThrough`, since a slice edge is one import.
    pub graph: Option<crate::model::GraphFilter>,
    /// An empty slicing is not vacuous.
    pub allow_empty: bool,
}

/// One diagram rule.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagramRule {
    /// The name.
    pub name: String,
    /// The comment.
    pub comment: Option<String>,
    /// The fix text.
    pub fix: Option<String>,
    /// The severity.
    pub severity: Severity,
    /// The objects the diagram's components are matched against.
    pub select: Selector,
    /// The `.puml` file, relative to the configuration.
    pub adhere_to: String,
}

impl DiagramRule {
    /// The element rule the diagram rule means: its `select` should adhere to the diagram.
    #[must_use]
    pub fn as_element_rule(&self) -> ElementRule {
        ElementRule {
            name: self.name.clone(),
            comment: self.comment.clone(),
            fix: self.fix.clone(),
            severity: self.severity,
            because: None,
            allow_empty: false,
            select: self.select.clone(),
            should: Expr::Test(Test {
                key: "adhereToPlantUmlDiagram".into(),
                concept: Concept::AdhereToPlantUmlDiagram,
                negated: false,
                operand: Operand::Diagram(self.adhere_to.clone()),
            }),
        }
    }
}

fn invalid(context: &str, message: impl std::fmt::Display) -> ConfigError {
    ConfigError::Invalid(format!("{context}: {message}"))
}

/// The Levenshtein distance between two keys, for the nearest-key suggestion.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let current = row[j + 1];
            row[j + 1] = (previous + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(row[j + 1] + 1);
            previous = current;
        }
    }
    row[b.len()]
}

/// Every key spelling valid on `side`: the nearest-key suggestions, the schema and the generated
/// reference read it.
pub fn spellings(side: Side) -> Vec<String> {
    let mut out = Vec::new();
    for (name, _, _, in_where) in VOCABULARY {
        if side == Side::Where && !in_where {
            continue;
        }
        let capital = {
            let mut c = name.chars();
            c.next().map_or_else(String::new, |f| {
                f.to_ascii_uppercase().to_string() + c.as_str()
            })
        };
        // A name that starts with a verb is spelled as written (`haveName`, `doNotHaveName`);
        // the rest take `are` / `be` (`arePublic`, `notBePublic`).
        let flag_like = ![
            "have",
            "reside",
            "depend",
            "only",
            "callAny",
            "implement",
            "exist",
            "adhere",
        ]
        .iter()
        .any(|verb| name.starts_with(verb));
        match (side, flag_like) {
            (Side::Where, true) => {
                out.push(format!("are{capital}"));
                out.push(format!("areNot{capital}"));
            }
            (Side::Where, false) => {
                out.push((*name).to_owned());
                out.push(format!("doNot{capital}"));
            }
            (Side::Should, true) => {
                out.push(format!("be{capital}"));
                out.push(format!("notBe{capital}"));
            }
            (Side::Should, false) => {
                out.push((*name).to_owned());
                out.push(format!("not{capital}"));
            }
        }
    }
    // `ArchUnitNET`'s plural and "no" spellings, which `split_key` folds into the concepts above.
    let extra: &[&str] = match side {
        Side::Where => &[
            "areConstructors",
            "areNoConstructors",
            "haveNoGetter",
            "haveNoSetter",
            "implementAny",
            "haveInitSetter",
        ],
        Side::Should => &[
            "beNoConstructor",
            "haveNoGetter",
            "haveNoSetter",
            "implementAny",
            "haveInitSetter",
        ],
    };
    out.extend(extra.iter().map(|s| (*s).to_owned()));
    out
}

fn suggestion(key: &str, side: Side) -> String {
    spellings(side)
        .into_iter()
        .min_by_key(|s| distance(key, s))
        .map(|s| format!("; did you mean `{s}`?"))
        .unwrap_or_default()
}

fn text_list(value: &Value, context: &str) -> Result<Vec<String>, ConfigError> {
    match value {
        Value::String(s) => Ok(vec![s.clone()]),
        Value::Array(items) => items
            .iter()
            .map(|v| scalar_text(v).ok_or_else(|| invalid(context, "every item must be a string")))
            .collect(),
        _ => Err(invalid(context, "must be a string or a list of strings")),
    }
}

/// A literal as the text the code layer records (`True`, `7`, `abc`).
fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(if *b { "True" } else { "False" }.to_owned()),
        Value::Number(n) => Some(n.to_string()),
        Value::Null => Some("null".to_owned()),
        _ => None,
    }
}

fn objects(value: &Value, context: &str) -> Result<Objects, ConfigError> {
    match value {
        Value::Object(map) if map.contains_key("kind") => {
            Ok(Objects::Selector(Box::new(parse_selector(value, context)?)))
        }
        other => Ok(Objects::Names(text_list(other, context)?)),
    }
}

fn operand(kind: ValueKind, value: &Value, context: &str) -> Result<Operand, ConfigError> {
    Ok(match kind {
        ValueKind::Flag => Operand::Flag,
        ValueKind::Names => Operand::Names(text_list(value, context)?),
        ValueKind::Pattern => match value {
            Value::String(s) => Operand::Pattern(s.clone()),
            _ => return Err(invalid(context, "must be a regular expression string")),
        },
        ValueKind::Objects => Operand::Objects(objects(value, context)?),
        ValueKind::Diagram => match value {
            Value::String(s) => Operand::Diagram(s.clone()),
            _ => return Err(invalid(context, "must be the path of a .puml file")),
        },
        ValueKind::ArgumentValues => Operand::Attribute {
            attribute: None,
            positional: match value {
                Value::Array(items) => items
                    .iter()
                    .map(|v| scalar_text(v).unwrap_or_else(|| v.to_string()))
                    .collect(),
                other => vec![scalar_text(other).unwrap_or_else(|| other.to_string())],
            },
            named: Vec::new(),
        },
        ValueKind::NamedArgumentValues => Operand::Attribute {
            attribute: None,
            positional: Vec::new(),
            named: named_arguments(value, context)?,
        },
        ValueKind::AttributeArguments | ValueKind::AttributeNamedArguments => {
            let Value::Object(map) = value else {
                return Err(invalid(context, "must be { attribute, arguments }"));
            };
            let attribute = map
                .get("attribute")
                .map(|a| objects(a, context))
                .transpose()?;
            let arguments = map.get("arguments").unwrap_or(&Value::Null);
            if kind == ValueKind::AttributeArguments {
                let positional = match arguments {
                    Value::Array(items) => items
                        .iter()
                        .map(|v| scalar_text(v).unwrap_or_else(|| v.to_string()))
                        .collect(),
                    Value::Null => Vec::new(),
                    other => vec![scalar_text(other).unwrap_or_else(|| other.to_string())],
                };
                Operand::Attribute {
                    attribute,
                    positional,
                    named: Vec::new(),
                }
            } else {
                Operand::Attribute {
                    attribute,
                    positional: Vec::new(),
                    named: named_arguments(arguments, context)?,
                }
            }
        }
    })
}

fn named_arguments(value: &Value, context: &str) -> Result<Vec<(String, String)>, ConfigError> {
    match value {
        Value::Object(map) => Ok(map
            .iter()
            .map(|(k, v)| (k.clone(), scalar_text(v).unwrap_or_else(|| v.to_string())))
            .collect()),
        Value::Null => Ok(Vec::new()),
        _ => Err(invalid(
            context,
            "named arguments must be a mapping of name to value",
        )),
    }
}

/// Parses one expression on `side`.
///
/// # Errors
/// [`ConfigError::Invalid`] for an unknown key (naming the nearest), a key used on the wrong
/// side, or a value of the wrong shape.
pub fn parse_expr(value: &Value, side: Side, context: &str) -> Result<Expr, ConfigError> {
    match value {
        Value::Array(items) => Ok(Expr::All(
            items
                .iter()
                .map(|v| parse_expr(v, side, context))
                .collect::<Result<_, _>>()?,
        )),
        Value::Object(map) => {
            let mut parts = Vec::with_capacity(map.len());
            for (key, value) in map {
                parts.push(parse_entry(key, value, side, context)?);
            }
            Ok(if parts.len() == 1 {
                parts.remove(0)
            } else {
                Expr::All(parts)
            })
        }
        _ => Err(invalid(
            context,
            "an expression must be a mapping or a list",
        )),
    }
}

fn parse_entry(key: &str, value: &Value, side: Side, context: &str) -> Result<Expr, ConfigError> {
    let here = format!("{context}.{key}");
    match key {
        "all" | "any" => {
            let Value::Array(items) = value else {
                return Err(invalid(&here, "must be a list of expressions"));
            };
            let items = items
                .iter()
                .map(|v| parse_expr(v, side, &here))
                .collect::<Result<_, _>>()?;
            return Ok(if key == "all" {
                Expr::All(items)
            } else {
                Expr::Any(items)
            });
        }
        "not" => return Ok(Expr::Not(Box::new(parse_expr(value, side, &here)?))),
        "getterVisibility" | "setterVisibility" => return accessor_visibility(key, value, &here),
        _ => {}
    }
    let (base, mut negated, selector) = split_key(key, side);
    let Some((_, concept, kind, in_where)) = VOCABULARY.iter().find(|(n, _, _, _)| *n == base)
    else {
        return Err(invalid(
            context,
            format!(
                "`{key}` is not an element {} key{}",
                side_word(side),
                suggestion(key, side)
            ),
        ));
    };
    if side == Side::Where && !in_where {
        return Err(invalid(
            context,
            format!("`{key}` is a condition; it belongs in `should`, not `select.where`"),
        ));
    }
    if selector && !matches!(value, Value::Object(m) if m.contains_key("kind")) {
        return Err(invalid(&here, "must be a selector `{ kind, where }`"));
    }
    if *kind == ValueKind::Flag {
        match value {
            Value::Bool(b) => negated ^= !*b,
            _ => return Err(invalid(&here, "must be true or false")),
        }
    }
    Ok(Expr::Test(Test {
        key: key.to_owned(),
        concept: *concept,
        negated,
        operand: operand(*kind, value, &here)?,
    }))
}

/// `getterVisibility: public`, `setterVisibility: private` and so on: the coverage tab's form of
/// `HavePublicGetter` ... `HavePrivateProtectedSetter`.
fn accessor_visibility(key: &str, value: &Value, here: &str) -> Result<Expr, ConfigError> {
    let getter = key == "getterVisibility";
    let concept = match (getter, value.as_str()) {
        (true, Some("public")) => Concept::HavePublicGetter,
        (true, Some("protected")) => Concept::HaveProtectedGetter,
        (true, Some("internal")) => Concept::HaveInternalGetter,
        (true, Some("protected-internal")) => Concept::HaveProtectedInternalGetter,
        (true, Some("private")) => Concept::HavePrivateGetter,
        (true, Some("private-protected")) => Concept::HavePrivateProtectedGetter,
        (false, Some("public")) => Concept::HavePublicSetter,
        (false, Some("protected")) => Concept::HaveProtectedSetter,
        (false, Some("internal")) => Concept::HaveInternalSetter,
        (false, Some("protected-internal")) => Concept::HaveProtectedInternalSetter,
        (false, Some("private")) => Concept::HavePrivateSetter,
        (false, Some("private-protected")) => Concept::HavePrivateProtectedSetter,
        _ => {
            return Err(invalid(
                here,
                "must be public, protected, internal, protected-internal, private or private-protected",
            ));
        }
    };
    Ok(Expr::Test(Test {
        key: key.to_owned(),
        concept,
        negated: false,
        operand: Operand::Flag,
    }))
}

const fn side_word(side: Side) -> &'static str {
    match side {
        Side::Where => "predicate",
        Side::Should => "condition",
    }
}

/// Parses `select: { kind, where }`.
///
/// # Errors
/// [`ConfigError::Invalid`] when `kind` is missing or unknown or `where` is malformed.
pub fn parse_selector(value: &Value, context: &str) -> Result<Selector, ConfigError> {
    let Value::Object(map) = value else {
        return Err(invalid(context, "`select` must be { kind, where }"));
    };
    for key in map.keys() {
        if !matches!(
            key.as_str(),
            "kind" | "where" | "language" | "includeReferenced"
        ) {
            return Err(invalid(
                context,
                format!(
                    "`{key}` is not a select key; use kind, language, where and includeReferenced"
                ),
            ));
        }
    }
    let kind = match map.get("kind") {
        Some(Value::String(k)) => Kind::parse(k)?,
        _ => return Err(invalid(context, "`select.kind` is required")),
    };
    let where_ = map
        .get("where")
        .map(|w| parse_expr(w, Side::Where, &format!("{context}.where")))
        .transpose()?;
    let languages = match map.get("language") {
        None => Vec::new(),
        Some(value) => text_list(value, &format!("{context}.language"))?
            .iter()
            .map(|l| {
                l.parse::<rb_model::Language>().map_err(|_| {
                    invalid(
                        context,
                        format!(
                            "`{l}` is not a language; use typescript, javascript, dotnet or python"
                        ),
                    )
                })
            })
            .collect::<Result<_, _>>()?,
    };
    let include_referenced = match map.get("includeReferenced") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => {
            return Err(invalid(
                context,
                "`select.includeReferenced` must be true or false",
            ));
        }
    };
    Ok(Selector {
        kind,
        languages,
        where_,
        include_referenced,
    })
}

fn severity(map: &Map<String, Value>, context: &str) -> Result<Severity, ConfigError> {
    map.get("severity").map_or(Ok(Severity::Error), |v| {
        v.as_str()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| invalid(context, "`severity` must be error, warn, info or ignore"))
    })
}

fn text(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn rule_map<'a>(
    value: &'a Value,
    family: &str,
    index: usize,
) -> Result<(&'a Map<String, Value>, String, String), ConfigError> {
    let Value::Object(map) = value else {
        return Err(ConfigError::Invalid(format!(
            "rules.{family}[{index}] must be an object"
        )));
    };
    let Some(name) = text(map, "name") else {
        return Err(ConfigError::Invalid(format!(
            "rules.{family}[{index}] needs a `name`"
        )));
    };
    let context = format!("rules.{family}[{name}]");
    Ok((map, name, context))
}

fn check_keys(
    map: &Map<String, Value>,
    allowed: &[&str],
    context: &str,
) -> Result<(), ConfigError> {
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(invalid(
                context,
                format!("`{key}` is not a key here; use {}", allowed.join(", ")),
            ));
        }
    }
    Ok(())
}

/// Parses `rules.elements`.
///
/// # Errors
/// [`ConfigError::Invalid`] naming the rule and the key.
pub fn parse_elements(value: &Value) -> Result<Vec<ElementRule>, ConfigError> {
    let Value::Array(items) = value else {
        return Err(ConfigError::Invalid("rules.elements must be a list".into()));
    };
    let mut rules = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let (map, name, context) = rule_map(item, "elements", index)?;
        check_keys(
            map,
            &[
                "name",
                "comment",
                "fix",
                "severity",
                "because",
                "allowEmpty",
                "select",
                "should",
                "examples",
                "expires",
                "owner",
            ],
            &context,
        )?;
        let select = parse_selector(
            map.get("select")
                .ok_or_else(|| invalid(&context, "`select` is required"))?,
            &format!("{context}.select"),
        )?;
        let should = parse_expr(
            map.get("should")
                .ok_or_else(|| invalid(&context, "`should` is required"))?,
            Side::Should,
            &format!("{context}.should"),
        )?;
        rules.push(ElementRule {
            comment: text(map, "comment"),
            fix: text(map, "fix"),
            severity: severity(map, &context)?,
            because: text(map, "because"),
            allow_empty: map
                .get("allowEmpty")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            name,
            select,
            should,
        });
    }
    Ok(rules)
}

/// A slice rule's `graph`, checked as a dependency rule's is, without `chainsThrough`.
fn slice_graph(
    value: &Value,
    name: &str,
    context: &str,
) -> Result<crate::model::GraphFilter, ConfigError> {
    crate::normalize::check_graph(value, &format!("{context}.graph"))?;
    let mut graph: crate::model::GraphFilter = serde_json::from_value(value.clone())
        .map_err(|e| invalid(context, format!("`graph`: {e}")))?;
    if graph.chains_through.is_some() {
        return Err(invalid(
            context,
            "`graph.chainsThrough` restricts the chains of a reachability rule; a slice edge is one import, so there is no chain to restrict",
        ));
    }
    crate::normalize::normalise_graph(&mut graph);
    for (at, text) in crate::normalize::graph_patterns(&graph) {
        crate::pattern::matcher(text).map_err(|source| ConfigError::Pattern {
            rule: name.to_owned(),
            at: at.to_owned(),
            source,
        })?;
    }
    Ok(graph)
}

/// Parses `rules.slices`.
///
/// # Errors
/// [`ConfigError::Invalid`] naming the rule and the key.
pub fn parse_slices(value: &Value) -> Result<Vec<SliceRule>, ConfigError> {
    let Value::Array(items) = value else {
        return Err(ConfigError::Invalid("rules.slices must be a list".into()));
    };
    let mut rules = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let (map, name, context) = rule_map(item, "slices", index)?;
        check_keys(
            map,
            &[
                "name",
                "comment",
                "fix",
                "severity",
                "matching",
                "should",
                "ignore",
                "where",
                "segments",
                "graph",
                "allowEmpty",
                "expires",
                "owner",
            ],
            &context,
        )?;
        let graph = map
            .get("graph")
            .map(|value| slice_graph(value, &name, &context))
            .transpose()?;
        let segments = match map.get("segments") {
            None => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .filter(|n| *n > 0)
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| invalid(&context, "`segments` must be a whole number from 1"))?,
            ),
        };
        let Some(matching) = text(map, "matching") else {
            return Err(invalid(
                &context,
                "`matching` is required, for example \"MyApp.(*)\"",
            ));
        };
        let conditions = text_list(
            map.get("should").unwrap_or(&Value::Null),
            &format!("{context}.should"),
        )
        .map_err(|_| {
            invalid(
                &context,
                "`should` must be notDependOnEachOther, beFreeOfCycles or a list of them",
            )
        })?;
        let should = conditions
            .iter()
            .map(|c| match c.as_str() {
                "notDependOnEachOther" => Ok(SliceCondition::NotDependOnEachOther),
                "beFreeOfCycles" => Ok(SliceCondition::BeFreeOfCycles),
                other => Err(invalid(&context, format!("`{other}` is not a slice condition; use notDependOnEachOther or beFreeOfCycles"))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ignore = map
            .get("ignore")
            .map(|v| text_list(v, &format!("{context}.ignore")))
            .transpose()?
            .unwrap_or_default();
        rules.push(SliceRule {
            comment: text(map, "comment"),
            fix: text(map, "fix"),
            severity: severity(map, &context)?,
            where_: text(map, "where"),
            allow_empty: map
                .get("allowEmpty")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            name,
            matching,
            should,
            ignore,
            segments,
            graph,
        });
    }
    Ok(rules)
}

/// Parses `rules.diagrams`.
///
/// # Errors
/// [`ConfigError::Invalid`] naming the rule and the key.
pub fn parse_diagrams(value: &Value) -> Result<Vec<DiagramRule>, ConfigError> {
    let Value::Array(items) = value else {
        return Err(ConfigError::Invalid("rules.diagrams must be a list".into()));
    };
    let mut rules = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let (map, name, context) = rule_map(item, "diagrams", index)?;
        check_keys(
            map,
            &[
                "name", "comment", "fix", "severity", "select", "adhereTo", "expires", "owner",
            ],
            &context,
        )?;
        let select = parse_selector(
            map.get("select")
                .ok_or_else(|| invalid(&context, "`select` is required"))?,
            &format!("{context}.select"),
        )?;
        let Some(adhere_to) = text(map, "adhereTo") else {
            return Err(invalid(&context, "`adhereTo` is required: the .puml file"));
        };
        rules.push(DiagramRule {
            comment: text(map, "comment"),
            fix: text(map, "fix"),
            severity: severity(map, &context)?,
            name,
            select,
            adhere_to,
        });
    }
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test(expr: &Expr) -> Option<&Test> {
        match expr {
            Expr::Test(t) => Some(t),
            _ => None,
        }
    }

    #[test]
    fn predicate_and_condition_spellings_meet_in_one_concept() {
        let pairs = [
            ("arePublic", "bePublic", "public", false),
            ("areNotPublic", "notBePublic", "public", true),
            ("haveName", "haveName", "haveName", false),
            ("doNotHaveName", "notHaveName", "haveName", true),
            (
                "resideInNamespace",
                "resideInNamespace",
                "resideInNamespace",
                false,
            ),
            ("areNestedIn", "beNestedIn", "nestedIn", false),
            ("areConstructors", "beConstructor", "constructor", false),
            ("areNoConstructors", "beNoConstructor", "constructor", true),
            ("are", "be", "", false),
            ("areNot", "notBe", "", true),
            ("doNotDependOnAny", "notDependOnAny", "dependOnAny", true),
            (
                "doNotImplementAnyInterfaces",
                "notImplementAnyInterfaces",
                "implementAnyInterfaces",
                true,
            ),
        ];
        for (predicate, condition, base, negated) in pairs {
            assert_eq!(
                split_key(predicate, Side::Where),
                (base.to_owned(), negated, false),
                "{predicate}"
            );
            assert_eq!(
                split_key(condition, Side::Should),
                (base.to_owned(), negated, false),
                "{condition}"
            );
        }
        assert_eq!(
            split_key("haveNoGetter", Side::Should),
            ("haveGetter".to_owned(), true, false)
        );
        assert_eq!(
            split_key("haveNoSetter", Side::Where),
            ("haveSetter".to_owned(), true, false)
        );
        assert_eq!(
            split_key("dependOnAnyTypesThat", Side::Should),
            ("dependOnAny".to_owned(), false, true)
        );
        assert_eq!(
            split_key("beTypesThat", Side::Should),
            (String::new(), false, true)
        );
        assert_eq!(
            split_key("beMethodMembersThat", Side::Should),
            (String::new(), false, true)
        );
        assert_eq!(
            split_key("implementAnyInterfacesThat", Side::Where),
            ("implementAnyInterfaces".to_owned(), false, true)
        );
        assert_eq!(
            split_key("exist", Side::Should),
            ("exist".to_owned(), false, false)
        );
        assert_eq!(
            split_key("notExist", Side::Should),
            ("exist".to_owned(), true, false)
        );
    }

    #[test]
    fn every_concept_has_one_vocabulary_row() {
        let mut seen = std::collections::BTreeSet::new();
        for concept in Concept::all() {
            assert!(seen.insert(concept), "{concept:?} twice");
            let (name, _, _) = concept.entry();
            assert!(
                VOCABULARY.iter().filter(|(n, _, _, _)| *n == name).count() == 1,
                "{name}"
            );
        }
        assert!(!Concept::Exist.entry().2, "exist is a condition only");
        assert_eq!(
            Concept::AdhereToPlantUmlDiagram.entry().1,
            ValueKind::Diagram
        );
    }

    #[test]
    fn expressions_parse_with_combinators_and_flags() -> Result<(), ConfigError> {
        let expr = parse_expr(
            &json!({ "any": [{ "arePublic": true, "haveNameEndingWith": "Service" }, { "not": { "areSealed": true } }] }),
            Side::Where,
            "t",
        )?;
        let Expr::Any(items) = &expr else {
            return Err(ConfigError::Invalid(format!("{expr:?}")));
        };
        assert!(matches!(&items[0], Expr::All(parts) if parts.len() == 2));
        assert!(matches!(&items[1], Expr::Not(_)));
        let negative = parse_expr(&json!({ "arePublic": false }), Side::Where, "t")?;
        assert_eq!(
            test(&negative).map(|t| (t.concept, t.negated)),
            Some((Concept::Public, true))
        );
        let list = parse_expr(
            &json!([{ "areSealed": true }, { "areNested": true }]),
            Side::Where,
            "t",
        )?;
        assert!(matches!(list, Expr::All(ref parts) if parts.len() == 2));
        Ok(())
    }

    #[test]
    fn operands_take_every_value_shape() -> Result<(), ConfigError> {
        let names = parse_expr(&json!({ "haveName": ["A", "B"] }), Side::Should, "t")?;
        assert_eq!(
            test(&names).map(|t| t.operand.clone()),
            Some(Operand::Names(vec!["A".into(), "B".into()]))
        );
        let selector = parse_expr(
            &json!({ "dependOnAnyTypesThat": { "kind": "class", "where": { "areSealed": true } } }),
            Side::Should,
            "t",
        )?;
        assert!(
            matches!(test(&selector).map(|t| &t.operand), Some(Operand::Objects(Objects::Selector(s))) if s.kind == Kind::Class && s.where_.is_some())
        );
        let attribute = parse_expr(
            &json!({ "haveAttributeWithArguments": { "attribute": "A", "arguments": ["x", 1, true] } }),
            Side::Where,
            "t",
        )?;
        assert_eq!(
            test(&attribute).map(|t| t.operand.clone()),
            Some(Operand::Attribute {
                attribute: Some(Objects::Names(vec!["A".into()])),
                positional: vec!["x".into(), "1".into(), "True".into()],
                named: vec![]
            })
        );
        let named_values = parse_expr(
            &json!({ "haveAnyAttributesWithNamedArguments": { "Level": 2 } }),
            Side::Where,
            "t",
        )?;
        assert_eq!(
            test(&named_values).map(|t| t.operand.clone()),
            Some(Operand::Attribute {
                attribute: None,
                positional: vec![],
                named: vec![("Level".into(), "2".into())]
            })
        );
        let with_named = parse_expr(
            &json!({ "haveAttributeWithNamedArguments": { "attribute": "A", "arguments": { "N": "v" } } }),
            Side::Where,
            "t",
        )?;
        assert!(
            matches!(test(&with_named).map(|t| &t.operand), Some(Operand::Attribute { named, .. }) if named.len() == 1)
        );
        let values = parse_expr(
            &json!({ "haveAnyAttributesWithArguments": "x" }),
            Side::Where,
            "t",
        )?;
        assert!(
            matches!(test(&values).map(|t| &t.operand), Some(Operand::Attribute { positional, .. }) if positional == &["x"])
        );
        let pattern = parse_expr(&json!({ "haveNameMatching": "^A" }), Side::Where, "t")?;
        assert_eq!(
            test(&pattern).map(|t| t.operand.clone()),
            Some(Operand::Pattern("^A".into()))
        );
        let diagram = parse_expr(
            &json!({ "adhereToPlantUmlDiagram": "d.puml" }),
            Side::Should,
            "t",
        )?;
        assert_eq!(
            test(&diagram).map(|t| t.operand.clone()),
            Some(Operand::Diagram("d.puml".into()))
        );
        Ok(())
    }

    #[test]
    fn mistakes_are_named_with_a_suggestion() {
        let unknown = parse_expr(
            &json!({ "arePublc": true }),
            Side::Where,
            "rules.elements[r].select.where",
        );
        let message = unknown.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            message.contains("`arePublc`") && message.contains("did you mean `arePublic`"),
            "{message}"
        );
        let condition_in_where = parse_expr(&json!({ "exist": true }), Side::Where, "t");
        assert!(
            condition_in_where
                .err()
                .is_some_and(|e| e.to_string().contains("belongs in `should`"))
        );
        for (value, why) in [
            (json!({ "arePublic": "yes" }), "true or false"),
            (json!({ "haveName": 3 }), "string"),
            (json!({ "haveNameMatching": ["a"] }), "regular expression"),
            (json!({ "dependOnAnyTypesThat": "X" }), "selector"),
            (json!({ "any": {} }), "list"),
            (json!("x"), "mapping or a list"),
            (
                json!({ "haveAttributeWithArguments": "A" }),
                "{ attribute, arguments }",
            ),
        ] {
            let message = parse_expr(&value, Side::Where, "t")
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(message.contains(why), "{value}: {message}");
        }
        assert!(distance("kitten", "sitting") == 3 && distance("", "abc") == 3);
    }

    #[test]
    fn referenced_types_are_selected_only_when_asked() -> Result<(), ConfigError> {
        let plain = parse_selector(&json!({ "kind": "type" }), "t")?;
        assert!(
            !plain.include_referenced,
            "referenced types are left out by default"
        );
        let referenced =
            parse_selector(&json!({ "kind": "type", "includeReferenced": true }), "t")?;
        assert!(referenced.include_referenced);
        assert!(
            parse_selector(&json!({ "kind": "type", "includeReferenced": "yes" }), "t").is_err()
        );
        Ok(())
    }

    /// Every key the `ArchUnitNET` coverage tab's "Rulebearing key" column names for an element
    /// predicate or condition, read from the tab so the two cannot drift apart.
    fn coverage_tab_keys() -> Vec<String> {
        let tab = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/artifacts/archunitnet-0.13.4-coverage.md"),
        )
        .unwrap_or_default();
        let mut keys = Vec::new();
        for section in tab.split("\n## ") {
            let title = section.lines().next().unwrap_or_default();
            if ![
                "Predicates and conditions",
                "Type predicates",
                "Class and attribute",
                "Member predicates",
            ]
            .iter()
            .any(|t| title.starts_with(t))
            {
                continue;
            }
            let rows = section.lines().filter(|l| {
                l.starts_with("| ") && !l.starts_with("| ---") && !l.contains("Rulebearing key")
            });
            for line in rows {
                let column = line.split('|').nth(2).unwrap_or_default();
                for token in column.split('`').skip(1).step_by(2) {
                    keys.push(token.to_owned());
                }
            }
        }
        keys
    }

    #[test]
    fn accessor_visibility_is_the_have_getter_and_setter_family() -> Result<(), ConfigError> {
        let concept = |value: Value| -> Result<Option<Concept>, ConfigError> {
            Ok(test(&parse_expr(&value, Side::Where, "t")?).map(|t| t.concept))
        };
        assert_eq!(
            concept(json!({ "getterVisibility": "public" }))?,
            concept(json!({ "havePublicGetter": true }))?
        );
        assert_eq!(
            concept(json!({ "setterVisibility": "private-protected" }))?,
            Some(Concept::HavePrivateProtectedSetter)
        );
        assert!(parse_expr(&json!({ "getterVisibility": "friend" }), Side::Should, "t").is_err());
        assert_eq!(
            concept(json!({ "implementAny": ["I"] }))?,
            Some(Concept::ImplementAnyInterfaces)
        );
        assert_eq!(
            concept(json!({ "haveInitSetter": true }))?,
            Some(Concept::HaveInitOnlySetter)
        );
        Ok(())
    }

    #[test]
    fn every_key_the_coverage_tab_names_parses() {
        let keys = coverage_tab_keys();
        assert!(
            keys.len() > 60,
            "{} keys read from the coverage tab",
            keys.len()
        );
        let mut refused = Vec::new();
        for token in &keys {
            // `be...`, `haveFullName*`: families the rows spell out elsewhere; a diagram rule.
            if token.contains("...") || token.ends_with('*') || token.starts_with("adhereTo") {
                continue;
            }
            let (key, value) = match token.split_once(": ") {
                Some((key, value)) => (key, json!(value)),
                None => (token.as_str(), json!(true)),
            };
            let (base, _, _) = split_key(key, Side::Where);
            let in_where = VOCABULARY
                .iter()
                .find(|(n, ..)| *n == base)
                .is_none_or(|(_, _, _, w)| *w);
            let sides: &[Side] = if key.starts_with("are") || key.starts_with("declaredIn") {
                &[Side::Where]
            } else if key.starts_with("be") || !in_where {
                &[Side::Should]
            } else {
                &[Side::Where, Side::Should]
            };
            for side in sides {
                // The value's shape is checked by the schema; here, that the key is known.
                let parsed = parse_expr(&json!({ key: value.clone() }), *side, "t")
                    .or_else(|_| parse_expr(&json!({ key: "X" }), *side, "t"))
                    .or_else(|_| parse_expr(&json!({ key: ["X"] }), *side, "t"))
                    .or_else(|_| parse_expr(&json!({ key: { "kind": "type" } }), *side, "t"));
                if let Err(e) = parsed {
                    refused.push(format!("{key} ({side:?}): {e}"));
                }
            }
        }
        assert!(
            refused.is_empty(),
            "coverage-tab keys that do not parse: {refused:#?}"
        );
    }

    #[test]
    fn a_slice_rule_narrows_its_graph() -> Result<(), ConfigError> {
        let plain = parse_slices(
            &json!([{ "name": "s", "matching": "app.(*)", "should": "beFreeOfCycles" }]),
        )?;
        assert_eq!(plain[0].graph, None);
        let narrowed = parse_slices(
            &json!([{ "name": "s", "matching": "app.(*)", "should": "beFreeOfCycles",
            "graph": { "ignore": [{ "from": ["^a", "^b"], "to": "^c" }], "dependencyTypesNot": ["type-only"] } }]),
        )?;
        let graph = narrowed[0].graph.clone().unwrap_or_default();
        assert_eq!(
            graph.ignore[0].from,
            Some(rb_model::options::Patterns::One("^a|^b".into())),
            "joined as a rule's patterns are"
        );
        assert_eq!(
            graph.dependency_types_not,
            Some(vec![rb_model::DependencyType::TypeOnly])
        );
        for (graph, needle) in [
            (
                json!({ "chainsThrough": "^a" }),
                "a slice edge is one import",
            ),
            (json!({}), "slices[s].graph` removes nothing"),
            (json!({ "ignore": [{}] }), "has neither `from` nor `to`"),
            (
                json!({ "dependencyTypesNot": ["type-onl"] }),
                "`graph`: `type-onl` is not a valid dependency type",
            ),
            (json!({ "modulesNot": "(?=x)" }), "graph.modulesNot"),
        ] {
            let error = parse_slices(&json!([{ "name": "s", "matching": "app.(*)", "should": "beFreeOfCycles", "graph": graph }]))
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(error.contains(needle), "{needle}: {error}");
        }
        Ok(())
    }

    #[test]
    fn slice_segments_are_a_whole_number_from_one() -> Result<(), ConfigError> {
        let slices = parse_slices(
            &json!([{ "name": "s", "matching": "App.(*)", "should": "beFreeOfCycles" }]),
        )?;
        assert_eq!(slices[0].segments, None);
        let squashed = parse_slices(
            &json!([{ "name": "s", "matching": "App.(*)", "should": "beFreeOfCycles", "segments": 1 }]),
        )?;
        assert_eq!(squashed[0].segments, Some(1));
        for bad in [json!(0), json!("one"), json!(-1)] {
            assert!(
                parse_slices(&json!([{ "name": "s", "matching": "App.(*)", "should": "beFreeOfCycles", "segments": bad }]))
                    .is_err()
            );
        }

        Ok(())
    }

    #[test]
    fn rules_parse_with_their_metadata() -> Result<(), ConfigError> {
        let elements = parse_elements(&json!([{
            "name": "services-are-sealed", "comment": "adr:0004", "severity": "warn", "because": "b", "allowEmpty": true,
            "select": { "kind": "class", "where": { "haveNameEndingWith": "Service" } },
            "should": { "beSealed": true }
        }]))?;
        let rule = &elements[0];
        assert_eq!(
            (rule.name.as_str(), rule.severity, rule.allow_empty),
            ("services-are-sealed", Severity::Warn, true)
        );
        assert_eq!(rule.because.as_deref(), Some("b"));
        assert_eq!(rule.select.kind, Kind::Class);
        assert!(rule.select.languages.is_empty());
        let scoped = parse_selector(
            &json!({ "kind": "class", "language": ["dotnet", "python"] }),
            "t",
        )?;
        assert_eq!(
            scoped.languages,
            [rb_model::Language::Dotnet, rb_model::Language::Python]
        );
        assert!(parse_selector(&json!({ "kind": "class", "language": "cobol" }), "t").is_err());
        let slices = parse_slices(
            &json!([{ "name": "s", "matching": "App.(*)", "should": ["notDependOnEachOther", "beFreeOfCycles"], "ignore": "App.Shared" }]),
        )?;
        assert_eq!(
            slices[0].should,
            [
                SliceCondition::NotDependOnEachOther,
                SliceCondition::BeFreeOfCycles
            ]
        );
        assert_eq!(slices[0].ignore, ["App.Shared"]);
        let diagrams = parse_diagrams(
            &json!([{ "name": "d", "select": { "kind": "type" }, "adhereTo": "docs/c.puml" }]),
        )?;
        assert_eq!(diagrams[0].adhere_to, "docs/c.puml");
        for (bad, family) in [
            (
                json!([{ "name": "x", "select": { "kind": "type" } }]),
                "elements",
            ),
            (json!([{ "name": "x", "should": {} }]), "elements"),
            (
                json!([{ "select": { "kind": "type" }, "should": {} }]),
                "elements",
            ),
            (
                json!([{ "name": "x", "select": { "kind": "nope" }, "should": {} }]),
                "elements",
            ),
            (
                json!([{ "name": "x", "select": { "where": {} }, "should": {} }]),
                "elements",
            ),
            (
                json!([{ "name": "x", "select": { "kind": "type", "extra": 1 }, "should": {} }]),
                "elements",
            ),
            (
                json!([{ "name": "x", "select": { "kind": "type" }, "should": {}, "typo": 1 }]),
                "elements",
            ),
            (
                json!([{ "name": "x", "select": { "kind": "type" }, "should": {}, "severity": "loud" }]),
                "elements",
            ),
            (json!({}), "elements"),
            (json!([1]), "elements"),
            (
                json!([{ "name": "s", "should": "beFreeOfCycles" }]),
                "slices",
            ),
            (
                json!([{ "name": "s", "matching": "A.(*)", "should": "shine" }]),
                "slices",
            ),
            (json!({}), "slices"),
            (
                json!([{ "name": "d", "select": { "kind": "type" } }]),
                "diagrams",
            ),
            (json!([{ "name": "d", "adhereTo": "x" }]), "diagrams"),
            (json!({}), "diagrams"),
        ] {
            let result = match family {
                "elements" => parse_elements(&bad).map(|_| ()),
                "slices" => parse_slices(&bad).map(|_| ()),
                _ => parse_diagrams(&bad).map(|_| ()),
            };
            assert!(result.is_err(), "{family}: {bad}");
        }
        assert!(Kind::Method.is_member() && !Kind::Class.is_member());
        assert_eq!(Kind::ALL.len(), 10);
        Ok(())
    }
}
