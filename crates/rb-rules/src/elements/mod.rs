//! Element rules: `ArchUnitNET`'s declarative rule family evaluated over the code layer.
//!
//! - Architecture: [The rule engine](../../../../docs/architecture.md#the-rule-engine)
//! - Source: [design § Element rules](../../../../docs/artifacts/design.md#element-rules-archunitnet-declarative)
//! - Plan: [Wave 2, Step 5](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#25-step-5-the-element-rule-engine-and-the-capability-table-2c)
//!   and § 1.4.3 (the evaluation flow)
//! - Decisions: [ADR-0007](../../../../docs/adr/0007-vacuous-rules-fail-by-default.md) (an empty
//!   selection is vacuous), [ADR-0010](../../../../docs/adr/0010-crate-layout-and-extractor-boundary.md)
//!   (no `match language`), [ADR-0014](../../../../docs/adr/0014-no-invented-cross-language-edges.md)
//!   (an unanswerable predicate is an error), [ADR-0015](../../../../docs/adr/0015-stable-violation-id.md)
//! - Requirement: [FR-RULE-03](../../../../docs/prd.md#fr-rule-03)
//! - Specification: `ArchUnitNET` 0.13.4's predicate and condition definitions, proven by
//!   conformance gate 2 (`tests/gate2.rs`)
//!
//! A rule selects objects of its kind, filters them by `where`, and applies `should` to each
//! selected object; every failing object is one result. [`concepts`] holds what each predicate
//! means, [`capability`] which language can answer it. The engine reads strings the document
//! carries and never asks which extractor wrote them.

pub mod capability;
pub mod concepts;

use std::collections::{BTreeMap, BTreeSet};

use rb_config::elements::{ElementRule, Expr, Kind, Objects, Operand, Selector, Test};
use rb_model::{
    AttributeElement, CodeLayer, GraphDocument, Language, MemberElement, Module, TypeElement,
};

/// Why an element rule could not be evaluated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ElementError {
    /// A name the rule gives is no object of the analysed code (`ArchUnitNET`'s
    /// `TypeDoesNotExistInArchitecture`).
    #[error(
        "rule `{rule}`: {name} does not exist in the analysed code; check the spelling, or that the assembly or package that defines it is part of the run"
    )]
    UnknownObject {
        /// The rule.
        rule: String,
        /// The name.
        name: String,
    },
    /// A key names a concept a language of the run cannot answer (ADR-0014).
    #[error(
        "rule `{rule}`: `{key}` has no meaning in {language} ({why}); scope the rule with select.language or remove the key"
    )]
    Unanswerable {
        /// The rule.
        rule: String,
        /// The key as written.
        key: String,
        /// The language.
        language: String,
        /// Why.
        why: String,
    },
    /// A regular expression does not compile.
    #[error("rule `{rule}`: `{pattern}` is not a valid pattern")]
    Pattern {
        /// The rule.
        rule: String,
        /// The pattern.
        pattern: String,
    },
    /// A diagram rule's file is missing or malformed (`ArchUnitNET`'s `IllegalDiagramException`,
    /// `ComponentIntersectionException`).
    #[error("rule `{rule}`: {message}")]
    Diagram {
        /// The rule.
        rule: String,
        /// What is wrong.
        message: String,
    },
    /// A type's namespace matches a slice pattern's prefix and postfix but cannot be cut into a
    /// slice name (`ArchUnitNET`'s `ArgumentException` "is not clearly assignable to a slice").
    #[error(
        "rule `{rule}`: \"{object}\" is not clearly assignable to a slice with the pattern: \"{pattern}\"; make the pattern's postfix occur after its prefix in every matching namespace"
    )]
    Slice {
        /// The rule.
        rule: String,
        /// The type.
        object: String,
        /// The pattern as `ArchUnitNET` rewrites it.
        pattern: String,
    },
}

/// One object an element rule can select.
#[derive(Debug, Clone, Copy)]
pub enum Object<'a> {
    /// A type (or a module-level function).
    Type(&'a TypeElement),
    /// A member.
    Member(&'a MemberElement),
    /// A module of the module layer.
    Module(&'a Module),
}

impl<'a> Object<'a> {
    /// Whether the object is a type the code references but does not define.
    pub fn is_referenced(&self) -> bool {
        matches!(self, Self::Type(t) if t.referenced == Some(true))
    }

    /// The object's identity: a type's or member's full name, a module's source.
    pub fn key(&self) -> &'a str {
        match self {
            Self::Type(t) => &t.full_name,
            Self::Member(m) => m.full_name.as_deref().unwrap_or(&m.name),
            Self::Module(m) => &m.source,
        }
    }

    /// The simple name.
    pub fn name(&self) -> &'a str {
        match self {
            Self::Type(t) => &t.name,
            Self::Member(m) => &m.name,
            Self::Module(m) => m.source.rsplit('/').next().unwrap_or(&m.source),
        }
    }

    /// The file the object is declared in.
    pub fn file(&self) -> Option<&'a str> {
        match self {
            Self::Type(t) => t.location.file.as_deref(),
            Self::Member(m) => m.location.file.as_deref(),
            Self::Module(m) => Some(&m.source),
        }
    }

    /// The language the object came from.
    pub fn language(&self) -> Option<Language> {
        match self {
            Self::Type(t) => Some(t.location.language),
            Self::Member(m) => Some(m.location.language),
            Self::Module(m) => m.language,
        }
    }
}

/// The code layer indexed for evaluation.
#[derive(Debug)]
pub struct Architecture<'a> {
    /// Every type, by full name.
    pub types: BTreeMap<&'a str, &'a TypeElement>,
    /// Every member, in document order.
    pub members: Vec<&'a MemberElement>,
    /// Members by declaring type.
    pub members_of: BTreeMap<&'a str, Vec<&'a MemberElement>>,
    /// Attributes by the full name of what they are applied to.
    pub attributes_of: BTreeMap<&'a str, Vec<&'a AttributeElement>>,
    /// Called member full names by calling member.
    pub calls_from: BTreeMap<&'a str, BTreeSet<&'a str>>,
    /// Calling type full names by called member.
    pub callers_of: BTreeMap<&'a str, BTreeSet<&'a str>>,
    /// Every full name a dependency points at, loaded or not.
    pub referenced: BTreeSet<&'a str>,
    /// Every member's full name.
    pub member_names: BTreeSet<&'a str>,
    /// Every module's source.
    pub module_sources: BTreeSet<&'a str>,
    /// The module layer.
    pub modules: &'a [Module],
    /// Every language present in the code layer and the module layer.
    pub languages: BTreeSet<Language>,
    /// The folder a diagram path is relative to: the configuration's.
    pub base: std::path::PathBuf,
}

impl<'a> Architecture<'a> {
    /// Indexes a document's code layer and module layer.
    pub fn new(document: &'a GraphDocument) -> Self {
        static EMPTY: std::sync::OnceLock<CodeLayer> = std::sync::OnceLock::new();
        let code = document
            .code
            .as_ref()
            .unwrap_or_else(|| EMPTY.get_or_init(CodeLayer::default));
        let mut architecture = Self {
            types: code
                .types
                .iter()
                .map(|t| (t.full_name.as_str(), t))
                .collect(),
            members: code.members.iter().collect(),
            members_of: BTreeMap::new(),
            attributes_of: BTreeMap::new(),
            calls_from: BTreeMap::new(),
            callers_of: BTreeMap::new(),
            referenced: BTreeSet::new(),
            member_names: code
                .members
                .iter()
                .filter_map(|m| m.full_name.as_deref())
                .collect(),
            module_sources: document.modules.iter().map(|m| m.source.as_str()).collect(),
            modules: &document.modules,
            languages: BTreeSet::new(),
            base: std::path::PathBuf::from("."),
        };
        for member in &code.members {
            architecture
                .members_of
                .entry(member.declaring_type.as_str())
                .or_default()
                .push(member);
            architecture.languages.insert(member.location.language);
            architecture
                .referenced
                .extend(member.dependencies.iter().map(|d| d.target.as_str()));
        }
        for ty in &code.types {
            architecture.languages.insert(ty.location.language);
            architecture
                .referenced
                .extend(ty.dependencies.iter().map(|d| d.target.as_str()));
        }
        architecture
            .languages
            .extend(document.modules.iter().filter_map(|m| m.language));
        for attribute in &code.attributes {
            architecture
                .attributes_of
                .entry(attribute.target.as_str())
                .or_default()
                .push(attribute);
        }
        let declaring: BTreeMap<&str, &str> = code
            .members
            .iter()
            .filter_map(|m| {
                m.full_name
                    .as_deref()
                    .map(|f| (f, m.declaring_type.as_str()))
            })
            .collect();
        for call in &code.calls {
            architecture
                .calls_from
                .entry(call.from.as_str())
                .or_default()
                .insert(call.to.as_str());
            if let Some(caller_type) = declaring.get(call.from.as_str()) {
                architecture
                    .callers_of
                    .entry(call.to.as_str())
                    .or_default()
                    .insert(caller_type);
            }
        }
        architecture
    }

    /// Every object of `kind`, before any filter.
    pub fn of_kind(&self, kind: Kind) -> Vec<Object<'a>> {
        let member_kind = |m: &&&MemberElement| match kind {
            Kind::Member => true,
            Kind::Field => m.kind == "field",
            Kind::Method => m.kind == "method" || m.kind == "constructor",
            Kind::Property => m.kind == "property",
            _ => false,
        };
        match kind {
            Kind::Module => self.modules.iter().map(Object::Module).collect(),
            Kind::Member | Kind::Field | Kind::Method | Kind::Property => self
                .members
                .iter()
                .filter(member_kind)
                .map(|m| Object::Member(m))
                .collect(),
            _ => self
                .types
                .values()
                .filter(|t| match kind {
                    Kind::Type => t.kind != "function",
                    Kind::Class => matches!(t.kind.as_str(), "class" | "attribute"),
                    Kind::Interface => t.kind == "interface",
                    Kind::Attribute => t.kind == "attribute",
                    Kind::Function => t.kind == "function",
                    _ => false,
                })
                .map(|t| Object::Type(t))
                .collect(),
        }
    }

    /// Whether a name is an object of the analysed code or something its dependencies name:
    /// four set lookups, none a scan.
    fn knows(&self, name: &str) -> bool {
        self.types.contains_key(name)
            || self.referenced.contains(name)
            || self.member_names.contains(name)
            || self.module_sources.contains(name)
    }
}

/// One selected object's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectResult {
    /// The object's identity.
    pub object: String,
    /// Its file.
    pub file: Option<String>,
    /// Whether it satisfied `should`.
    pub passed: bool,
}

/// What evaluating one element rule found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The rule name.
    pub rule: String,
    /// One result per selected object, sorted by object.
    pub results: Vec<ObjectResult>,
    /// The selection was empty and the rule does not allow it.
    pub vacuous: bool,
    /// An `exist` / `notExist` condition failed on the selection as a whole.
    pub existence_failed: bool,
}

impl Outcome {
    /// The failing objects.
    pub fn failures(&self) -> impl Iterator<Item = &ObjectResult> {
        self.results.iter().filter(|r| !r.passed)
    }

    /// Whether the rule holds: every selected object passed, the selection was not vacuous, and no
    /// existence condition failed.
    pub fn holds(&self) -> bool {
        !self.vacuous && !self.existence_failed && self.results.iter().all(|r| r.passed)
    }
}

/// Resolved object keys, shared between the objects a test is applied to.
pub type Keys = std::rc::Rc<BTreeSet<String>>;

/// The evaluator of one rule's expressions against an architecture.
pub struct Evaluator<'r, 'a> {
    architecture: &'r Architecture<'a>,
    rule: &'r str,
    /// Selector results already computed, keyed by the selector's address (the rule's own,
    /// alive as long as the evaluator).
    cache: std::cell::RefCell<BTreeMap<usize, Keys>>,
    /// Name operands already checked to exist, by the names: each test checks its names once,
    /// not once per object.
    names: std::cell::RefCell<BTreeMap<Vec<String>, Keys>>,
    /// Diagrams already read, by path.
    pub(crate) diagrams:
        std::cell::RefCell<BTreeMap<String, std::rc::Rc<crate::plantuml::Association>>>,
}

impl<'r, 'a> Evaluator<'r, 'a> {
    /// An evaluator for `rule` over `architecture`.
    pub fn new(architecture: &'r Architecture<'a>, rule: &'r str) -> Self {
        Self {
            architecture,
            rule,
            cache: std::cell::RefCell::new(BTreeMap::new()),
            names: std::cell::RefCell::new(BTreeMap::new()),
            diagrams: std::cell::RefCell::new(BTreeMap::new()),
        }
    }

    /// The architecture evaluated against.
    pub fn architecture(&self) -> &'r Architecture<'a> {
        self.architecture
    }

    /// The objects a selector selects.
    ///
    /// # Errors
    /// [`ElementError`] from a test in `where`.
    pub fn select(&self, selector: &Selector) -> Result<Vec<Object<'a>>, ElementError> {
        self.select_among(selector, selector.include_referenced)
    }

    /// The objects a selector selects, referenced types among them when `referenced`.
    fn select_among(
        &self,
        selector: &Selector,
        referenced: bool,
    ) -> Result<Vec<Object<'a>>, ElementError> {
        let mut selected = Vec::new();
        for object in self.architecture.of_kind(selector.kind) {
            if !referenced && object.is_referenced() {
                continue;
            }
            if !selector.languages.is_empty()
                && !object
                    .language()
                    .is_some_and(|l| selector.languages.contains(&l))
            {
                continue;
            }
            let keep = match &selector.where_ {
                Some(expr) => self.holds(&object, expr)?,
                None => true,
            };
            if keep {
                selected.push(object);
            }
        }
        Ok(selected)
    }

    /// The keys of the objects an operand names, checked to exist.
    ///
    /// # Errors
    /// [`ElementError::UnknownObject`] for a name nothing in the run has.
    pub fn resolve(&self, objects: &Objects) -> Result<Keys, ElementError> {
        match objects {
            Objects::Names(names) => {
                if let Some(hit) = self.names.borrow().get(names) {
                    return Ok(std::rc::Rc::clone(hit));
                }
                if let Some(name) = names.iter().find(|n| !self.architecture.knows(n)) {
                    return Err(ElementError::UnknownObject {
                        rule: self.rule.to_owned(),
                        name: name.clone(),
                    });
                }
                let keys = std::rc::Rc::new(names.iter().cloned().collect());
                self.names
                    .borrow_mut()
                    .insert(names.clone(), std::rc::Rc::clone(&keys));
                Ok(keys)
            }
            Objects::Selector(selector) => {
                let address = std::ptr::from_ref::<Selector>(selector) as usize;
                if let Some(hit) = self.cache.borrow().get(&address) {
                    return Ok(std::rc::Rc::clone(hit));
                }
                // A nested selector filters what the object relates to, as `ArchUnitNET`'s
                // `ComplexCondition` filters dependency targets: referenced types count.
                let keys: Keys = std::rc::Rc::new(
                    self.select_among(selector, true)?
                        .iter()
                        .map(|o| o.key().to_owned())
                        .collect(),
                );
                self.cache
                    .borrow_mut()
                    .insert(address, std::rc::Rc::clone(&keys));
                Ok(keys)
            }
        }
    }

    /// How many operands this evaluator has resolved, for the tests that prove each is
    /// resolved once.
    #[cfg(test)]
    pub(crate) fn resolved(&self) -> usize {
        self.cache.borrow().len() + self.names.borrow().len()
    }

    /// Whether `object` satisfies `expr`.
    ///
    /// # Errors
    /// [`ElementError`] from a test.
    pub fn holds(&self, object: &Object<'a>, expr: &Expr) -> Result<bool, ElementError> {
        Ok(match expr {
            Expr::All(items) => {
                for item in items {
                    if !self.holds(object, item)? {
                        return Ok(false);
                    }
                }
                true
            }
            Expr::Any(items) => {
                for item in items {
                    if self.holds(object, item)? {
                        return Ok(true);
                    }
                }
                false
            }
            Expr::Not(inner) => !self.holds(object, inner)?,
            Expr::Test(test) => concepts::test(self, object, test)?,
        })
    }

    /// The rule being evaluated.
    pub fn rule(&self) -> &str {
        self.rule
    }

    pub(crate) fn pattern_error(&self, pattern: &str) -> ElementError {
        ElementError::Pattern {
            rule: self.rule.to_owned(),
            pattern: pattern.to_owned(),
        }
    }
}

/// Whether an expression mentions `exist` or `notExist`: `ArchUnitNET` then stops requiring a
/// positive result (`AddObjectCondition.Exist`, `NotExist` set `RequirePositiveResults = false`).
fn mentions_existence(expr: &Expr) -> bool {
    match expr {
        Expr::All(items) | Expr::Any(items) => items.iter().any(mentions_existence),
        Expr::Not(inner) => mentions_existence(inner),
        Expr::Test(test) => test.concept == rb_config::elements::Concept::Exist,
    }
}

/// What a condition says about an empty selection, `ICondition.CheckEmpty` folded as
/// `ConditionManager.CheckEmpty` folds it: `exist` is false, `notExist` true, every other
/// condition true.
fn empty_verdict(expr: &Expr) -> bool {
    match expr {
        Expr::All(items) => items.iter().all(empty_verdict),
        Expr::Any(items) => items.iter().any(empty_verdict),
        // Only `not: exist` is a condition of its own (`notExist`); any other negation is the
        // negated condition, whose empty verdict is true like every other.
        Expr::Not(inner) if mentions_existence(inner) => !empty_verdict(inner),
        Expr::Test(test) if test.concept == rb_config::elements::Concept::Exist => test.negated,
        Expr::Not(_) | Expr::Test(_) => true,
    }
}

/// The diagram paths of every `adhereToPlantUmlDiagram` test in `expr`.
fn diagram_paths<'x>(expr: &'x Expr, out: &mut Vec<&'x str>) {
    match expr {
        Expr::All(items) | Expr::Any(items) => {
            for item in items {
                diagram_paths(item, out);
            }
        }
        Expr::Not(inner) => diagram_paths(inner, out),
        Expr::Test(Test {
            operand: Operand::Diagram(path),
            ..
        }) => out.push(path),
        Expr::Test(_) => {}
    }
}

/// Evaluates one element rule.
///
/// An empty selection is vacuous ([ADR-0007](../../../../docs/adr/0007-vacuous-rules-fail-by-default.md),
/// `ArchUnitNET`'s positive-result requirement) unless the rule allows it or its conditions
/// mention `exist` / `notExist`; then the conditions' empty verdict decides, and a false one is
/// [`Outcome::existence_failed`] ("There are no objects matching the criteria").
///
/// # Errors
/// [`ElementError`]: an unknown name, an unanswerable key, a bad pattern.
pub fn evaluate(
    architecture: &Architecture<'_>,
    rule: &ElementRule,
) -> Result<Outcome, ElementError> {
    capability::validate(architecture, rule)?;
    let evaluator = Evaluator::new(architecture, &rule.name);
    // `AdhereToPlantUmlDiagram` reads and associates its diagram when the rule is built, so a
    // malformed diagram fails the rule even when nothing is selected.
    let mut diagrams = Vec::new();
    diagram_paths(&rule.should, &mut diagrams);
    for path in diagrams {
        crate::plantuml::load(&evaluator, path)?;
    }
    let selected = evaluator.select(&rule.select)?;
    let existence = mentions_existence(&rule.should);
    let mut results = Vec::with_capacity(selected.len());
    for object in &selected {
        results.push(ObjectResult {
            object: object.key().to_owned(),
            file: object.file().map(str::to_owned),
            passed: evaluator.holds(object, &rule.should)?,
        });
    }
    results.sort_by(|a, b| a.object.cmp(&b.object));
    // Two objects can share a key (members without a full name, say): one result for the key,
    // failing when either fails, so a duplicate never hides a failure.
    results.dedup_by(|later, kept| {
        let same = later.object == kept.object;
        if same {
            kept.passed &= later.passed;
        }
        same
    });
    let empty = selected.is_empty();
    Ok(Outcome {
        rule: rule.name.clone(),
        vacuous: empty && !rule.allow_empty && !existence,
        existence_failed: empty && existence && !empty_verdict(&rule.should),
        results,
    })
}

/// The operand keys of a test, resolved; `Names` operands are returned as written.
pub(crate) fn operand_keys(
    evaluator: &Evaluator<'_, '_>,
    operand: &Operand,
) -> Result<Keys, ElementError> {
    match operand {
        Operand::Objects(objects) => evaluator.resolve(objects),
        Operand::Names(names) => Ok(std::rc::Rc::new(names.iter().cloned().collect())),
        _ => Ok(Keys::default()),
    }
}

#[cfg(test)]
mod tests {
    use rb_config::elements::parse_elements;
    use rb_model::{CodeLayer, ElementDependency, Language, Location, TypeElement};
    use serde_json::json;

    use super::*;

    /// `App.Service` depends on `App.Repository` and on `Lib.Client`, a referenced type.
    fn document() -> GraphDocument {
        let at = || Location::in_file(Language::Dotnet, Some("a.cs".to_owned()));
        let dependency = |target: &str| ElementDependency {
            target: target.to_owned(),
            kind: "body".to_owned(),
            member: None,
            line: None,
            form: None,
        };
        let mut service = TypeElement::new("App.Service", "Service", "class", at());
        service.dependencies = vec![dependency("App.Repository"), dependency("Lib.Client")];
        let repository = TypeElement::new("App.Repository", "Repository", "class", at());
        let mut client = TypeElement::new(
            "Lib.Client",
            "Client",
            "class",
            Location::in_file(Language::Dotnet, None),
        );
        client.referenced = Some(true);
        client.namespace = Some("Lib".to_owned());
        GraphDocument {
            code: Some(CodeLayer {
                types: vec![service, repository, client],
                ..CodeLayer::default()
            }),
            ..GraphDocument::default()
        }
    }

    fn outcome(rule: serde_json::Value) -> Result<Outcome, Box<dyn std::error::Error>> {
        let document = document();
        let architecture = Architecture::new(&document);
        let mut rule = rule;
        rule["name"] = json!("r");
        Ok(evaluate(
            &architecture,
            &parse_elements(&json!([rule]))?[0],
        )?)
    }

    fn keys(outcome: &Outcome) -> Vec<(&str, bool)> {
        outcome
            .results
            .iter()
            .map(|r| (r.object.as_str(), r.passed))
            .collect()
    }

    #[test]
    fn referenced_types_are_selected_only_when_asked() -> Result<(), Box<dyn std::error::Error>> {
        let plain = outcome(json!({ "select": { "kind": "class" }, "should": { "exist": true } }))?;
        assert_eq!(
            keys(&plain),
            [("App.Repository", true), ("App.Service", true)]
        );
        let wide = outcome(json!({
            "select": { "kind": "class", "includeReferenced": true },
            "should": { "exist": true }
        }))?;
        assert_eq!(
            keys(&wide),
            [
                ("App.Repository", true),
                ("App.Service", true),
                ("Lib.Client", true)
            ]
        );
        Ok(())
    }

    #[test]
    fn nested_selectors_see_referenced_types_and_only_depend_on_does_not()
    -> Result<(), Box<dyn std::error::Error>> {
        let depends = outcome(json!({
            "select": { "kind": "class", "where": { "are": ["App.Service"] } },
            "should": { "dependOnAnyTypesThat": { "kind": "type", "where": { "resideInNamespace": "Lib" } } }
        }))?;
        assert_eq!(keys(&depends), [("App.Service", true)]);
        // Only the dependency on the architecture's own `App.Repository` is judged.
        let only = outcome(json!({
            "select": { "kind": "class", "where": { "are": ["App.Service"] } },
            "should": { "onlyDependOn": ["App.Repository"] }
        }))?;
        assert_eq!(keys(&only), [("App.Service", true)]);
        Ok(())
    }

    #[test]
    fn names_are_known_by_set_lookups_and_each_operand_is_resolved_once()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut document = document();
        let at = Location::in_file(Language::Dotnet, Some("a.cs".to_owned()));
        let mut run = rb_model::MemberElement::new("App.Service", "Run", "method", at);
        run.full_name = Some("App.Service::Run()".to_owned());
        if let Some(code) = document.code.as_mut() {
            code.members.push(run);
        }
        document.modules.push(Module::new("src/a.cs"));
        let architecture = Architecture::new(&document);
        assert_eq!(
            architecture.member_names,
            BTreeSet::from(["App.Service::Run()"])
        );
        assert_eq!(architecture.module_sources, BTreeSet::from(["src/a.cs"]));
        let evaluator = Evaluator::new(&architecture, "r");
        for known in [
            "App.Service",
            "Lib.Client",
            "App.Service::Run()",
            "src/a.cs",
        ] {
            let names = Objects::Names(vec![known.to_owned()]);
            assert_eq!(
                evaluator.resolve(&names)?.iter().collect::<Vec<_>>(),
                [known]
            );
        }
        let unknown = Objects::Names(vec!["App.Service".to_owned(), "Nope".to_owned()]);
        assert_eq!(
            evaluator.resolve(&unknown),
            Err(ElementError::UnknownObject {
                rule: "r".into(),
                name: "Nope".into()
            })
        );
        let rules = parse_elements(&json!([{ "name": "r",
            "select": { "kind": "class" },
            "should": { "dependOnAny": ["App.Repository"] } }]))?;
        let evaluator = Evaluator::new(&architecture, "r");
        let selected = evaluator.select(&rules[0].select)?;
        assert_eq!(selected.len(), 2);
        let verdicts = selected
            .iter()
            .map(|o| evaluator.holds(o, &rules[0].should))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(verdicts, [false, true], "Repository, then Service");
        assert_eq!(evaluator.resolved(), 1, "one operand, resolved once");
        let Expr::Test(Test {
            operand: Operand::Objects(objects),
            ..
        }) = &rules[0].should
        else {
            return Err("not a test".into());
        };
        assert!(std::rc::Rc::ptr_eq(
            &evaluator.resolve(objects)?,
            &evaluator.resolve(objects)?
        ));
        Ok(())
    }

    #[test]
    fn two_objects_with_one_key_fail_when_either_fails() -> Result<(), Box<dyn std::error::Error>> {
        let at = || Location::in_file(Language::Dotnet, Some("a.cs".to_owned()));
        // Neither member has a full name, so both are known by the simple name `f`; the
        // failing one comes second in document order.
        let mut fixed = rb_model::MemberElement::new("A", "f", "field", at());
        fixed.r#static = Some(true);
        let loose = rb_model::MemberElement::new("B", "f", "field", at());
        let document = GraphDocument {
            code: Some(CodeLayer {
                members: vec![fixed, loose],
                ..CodeLayer::default()
            }),
            ..GraphDocument::default()
        };
        let architecture = Architecture::new(&document);
        let rules = parse_elements(&json!([{ "name": "r",
            "select": { "kind": "field" }, "should": { "beStatic": true } }]))?;
        let outcome = evaluate(&architecture, &rules[0])?;
        assert_eq!(keys(&outcome), [("f", false)]);
        let rules = parse_elements(&json!([{ "name": "r",
            "select": { "kind": "field" }, "should": { "haveName": "f" } }]))?;
        assert_eq!(keys(&evaluate(&architecture, &rules[0])?), [("f", true)]);
        Ok(())
    }

    #[test]
    fn exist_and_not_exist_follow_archunitnet() -> Result<(), Box<dyn std::error::Error>> {
        let select = json!({ "kind": "class", "where": { "are": ["App.Service"] } });
        let empty = json!({ "kind": "class", "where": { "haveName": "Nothing" } });
        let cases = [
            (select.clone(), json!({ "exist": true }), true),
            (select.clone(), json!({ "notExist": true }), false),
            (
                select.clone(),
                json!({ "any": [{ "notExist": true }, { "haveName": "Service" }] }),
                true,
            ),
            (
                select,
                json!({ "all": [{ "exist": true }, { "notExist": true }] }),
                false,
            ),
            (empty.clone(), json!({ "exist": true }), false),
            (empty.clone(), json!({ "notExist": true }), true),
            (
                empty.clone(),
                json!({ "any": [{ "exist": true }, { "notExist": true }] }),
                true,
            ),
            (
                empty.clone(),
                json!({ "all": [{ "notExist": true }, { "not": { "exist": true } }] }),
                true,
            ),
            (
                empty.clone(),
                json!({ "all": [{ "notExist": true }, { "not": { "haveName": "X" } }] }),
                true,
            ),
            (
                empty,
                json!({ "all": [{ "exist": true }, { "haveName": "X" }] }),
                false,
            ),
        ];
        for (select, should, holds) in cases {
            let result = outcome(json!({ "select": select, "should": should }))?;
            assert_eq!(result.holds(), holds, "{should}");
            assert!(
                !result.vacuous,
                "{should}: a rule with exist is never vacuous"
            );
        }
        let vacuous = outcome(json!({
            "select": { "kind": "class", "where": { "haveName": "Nothing" } },
            "should": { "haveName": "X" }
        }))?;
        assert!(vacuous.vacuous && !vacuous.holds());
        Ok(())
    }
}
