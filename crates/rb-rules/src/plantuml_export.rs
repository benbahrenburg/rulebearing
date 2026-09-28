//! Generating `PlantUML` diagrams: the elements `ArchUnitNET` writes and the builder that draws
//! them from types or slices, the reverse of [`crate::plantuml`].
//!
//! - Source: [design § Diagram rules](../../../docs/artifacts/design.md#diagram-rules)
//! - Coverage: [`ArchUnitNET` § `PlantUML`](../../../docs/artifacts/archunitnet-0.13.4-coverage.md#plantuml)
//! - Plan: [Wave 3, Step 9](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//! - Decisions: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md) (the
//!   upstream suite is the specification; the ported cases are
//!   `conformance/archunitnet/ported/PlantUmlFileBuilderTest.yaml` and
//!   `PlantUmlFluentComponentDiagramTests.yaml`),
//!   [ADR-0034](../../../docs/adr/0034-slices-group-types-or-modules-and-segments.md) (what a
//!   slice holds)
//! - Requirements: [FR-OUT-02](../../../docs/prd.md#fr-out-02), [FR-RULE-05](../../../docs/prd.md#fr-rule-05)
//! - Specification: `ArchUnitNET` 0.13.4 `Domain/PlantUml/Export` (`PlantUmlFileBuilder`,
//!   `PlantUmlDiagram`, `PlantUmlDependency`, `PlantUmlClass`, `PlantUmlInterface`,
//!   `PlantUmlNamespace`, `PlantUmlSlice`, `PlantUmlNameChecker`, `GenerationOptions`,
//!   `DependencyFilters`)
//!
//! The text is `ArchUnitNET`'s, byte for byte, with `\n` line ends (`Environment.NewLine` on the
//! platforms its tests assert on). Where `ArchUnitNET` orders by its loader's discovery order,
//! the caller passes the types and slices already ordered, and a type's dependency targets are
//! drawn in ordinal order of their full names. Two parts of the upstream API are not ported
//! because nothing in Rulebearing reaches them: `RenderOptions.OmitClassFields = false` (a class
//! is always drawn without fields, the upstream default) and
//! `GenerationOptions.IncludeNodesWithoutDependencies = false` (every node is drawn, the upstream
//! default). Where `ArchUnitNET` throws, the builder returns [`ExportError`] naming the .NET
//! exception.

use std::collections::{BTreeMap, BTreeSet};

use crate::elements::Architecture;
use crate::slices::Slicing;

/// The .NET exception `ArchUnitNET`'s generator raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportException {
    /// `IllegalComponentNameException`: a name that is empty or holds a forbidden character.
    IllegalComponentName,
    /// `ArgumentOutOfRangeException`: a parent namespace asked of a name without a `.`.
    ArgumentOutOfRange,
    /// `NullReferenceException`: a C4 slice drawn without its namespace.
    NullReference,
}

impl ExportException {
    /// The upstream exception's class name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::IllegalComponentName => "IllegalComponentNameException",
            Self::ArgumentOutOfRange => "ArgumentOutOfRangeException",
            Self::NullReference => "NullReferenceException",
        }
    }
}

/// Why a diagram cannot be generated: the upstream exception and its message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}: {}", .exception.name(), .message)]
pub struct ExportError {
    /// The exception `ArchUnitNET` raises.
    pub exception: ExportException,
    /// Its message.
    pub message: String,
}

fn fail<T>(exception: ExportException, message: impl Into<String>) -> Result<T, ExportError> {
    Err(ExportError {
        exception,
        message: message.into(),
    })
}

/// `PlantUmlNameChecker.ForbiddenCharacters`.
const FORBIDDEN: [char; 8] = ['[', ']', '\r', '\n', '\u{c}', '\u{7}', '\u{8}', '\u{b}'];

/// `PlantUmlNameChecker.AssertNoForbiddenCharacters`.
fn assert_no_forbidden(names: &[Option<&str>]) -> Result<(), ExportError> {
    if names.iter().flatten().any(|n| n.contains(FORBIDDEN)) {
        return fail(
            ExportException::IllegalComponentName,
            "PlantUml component names must not contain \"[\" or \"]\" or any of the escape characters \"\\r\", \"\\n\", \"\\f\", \"\\a\", \"\\b\", \"\\v\".",
        );
    }
    Ok(())
}

/// `PlantUmlNameChecker.AssertNotNullOrEmpty`.
fn assert_not_empty(names: &[&str]) -> Result<(), ExportError> {
    if names.iter().any(|n| n.is_empty()) {
        return fail(
            ExportException::IllegalComponentName,
            "PlantUml component names can't be null or empty.",
        );
    }
    Ok(())
}

/// `DependencyType`: how a dependency is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyType {
    /// `[a] --|> [b]`.
    OneToOne,
    /// `[a] "1" --|> "many" [b]`.
    OneToMany,
    /// `[a] -[#red]> b`, to a package.
    OneToPackage,
    /// `a -[#blue]> [b]`, from a package.
    PackageToOne,
    /// `a -[#green]> b`.
    PackageToPackage,
    /// `a --|> b` when both have the same parent namespace, else an arrow to or from the
    /// ancestor they share one with.
    OneToOneIfSameParentNamespace,
    /// `a ..> b` when both have the same parent namespace.
    PackageToPackageIfSameParentNamespace,
    /// `[a] --> [b]` when both are equally deep.
    OneToOneCompact,
    /// `[a] <-[#red]> [b]`.
    Circle,
    /// Nothing.
    NoDependency,
}

/// `PlantUmlDependency`: an arrow from `origin` to `target`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// Where the arrow starts.
    pub origin: String,
    /// Where it points.
    pub target: String,
    /// How it is drawn.
    pub kind: DependencyType,
}

/// `ns.Remove(ns.LastIndexOf("."))`.
fn parent_namespace(ns: &str) -> Result<&str, ExportError> {
    match ns.rfind('.') {
        Some(at) => Ok(&ns[..at]),
        None => fail(
            ExportException::ArgumentOutOfRange,
            format!("`{ns}` has no parent namespace (no `.`)"),
        ),
    }
}

/// `ns.Remove(0, ns.LastIndexOf(".") + 1)`.
fn child_namespace(ns: &str) -> &str {
    ns.rfind('.').map_or(ns, |at| &ns[at + 1..])
}

fn same_parent(origin: &str, target: &str) -> Result<bool, ExportError> {
    Ok(parent_namespace(origin)? == parent_namespace(target)?)
}

fn dots(name: &str) -> usize {
    name.matches('.').count()
}

impl Dependency {
    /// `new PlantUmlDependency(origin, target, dependencyType)`.
    ///
    /// # Errors
    /// `IllegalComponentNameException` for an empty name or one with a forbidden character.
    pub fn new(origin: &str, target: &str, kind: DependencyType) -> Result<Self, ExportError> {
        assert_no_forbidden(&[Some(origin), Some(target)])?;
        assert_not_empty(&[origin, target])?;
        Ok(Self {
            origin: origin.to_owned(),
            target: target.to_owned(),
            kind,
        })
    }

    /// `OriginCountOfDots`.
    #[must_use]
    pub fn origin_dots(&self) -> usize {
        dots(&self.origin)
    }

    /// `TargetCountOfDots`.
    #[must_use]
    pub fn target_dots(&self) -> usize {
        dots(&self.target)
    }

    /// `GetPlantUmlString`: the line, or nothing when this kind draws nothing here.
    ///
    /// # Errors
    /// `ArgumentOutOfRangeException` where `ArchUnitNET` asks a name without a `.` for its parent.
    pub fn render(&self) -> Result<String, ExportError> {
        let (o, t) = (self.origin.as_str(), self.target.as_str());
        let (od, td) = (self.origin_dots(), self.target_dots());
        Ok(match self.kind {
            DependencyType::OneToOne => format!("[{o}] --|> [{t}]\n"),
            DependencyType::OneToMany => format!("[{o}] \"1\" --|> \"many\" [{t}]\n"),
            DependencyType::OneToPackage => format!("[{o}] -[#red]> {}\n", child_namespace(t)),
            DependencyType::PackageToOne => format!("{} -[#blue]> [{t}]\n", child_namespace(o)),
            DependencyType::PackageToPackage => {
                format!("{} -[#green]> {}\n", child_namespace(o), child_namespace(t))
            }
            DependencyType::OneToOneCompact if od == td => format!("[{o}] --> [{t}]\n"),
            DependencyType::Circle => format!("[{o}] <-[#red]> [{t}]\n"),
            DependencyType::PackageToPackageIfSameParentNamespace
                if od == td && (od == 0 || same_parent(o, t)?) =>
            {
                format!("{} ..> {}\n", child_namespace(o), child_namespace(t))
            }
            DependencyType::OneToOneIfSameParentNamespace => {
                if od == td && (od == 0 || same_parent(o, t)?) {
                    format!("{o} --|> {t}\n")
                } else if od < td {
                    let mut tmp = parent_namespace(t)?;
                    while od < dots(tmp) {
                        tmp = parent_namespace(tmp)?;
                    }
                    if tmp != o && same_parent(tmp, o)? {
                        format!("{o} --> {}\n", child_namespace(tmp))
                    } else {
                        String::new()
                    }
                } else {
                    let mut tmp = parent_namespace(o)?;
                    while dots(tmp) > td {
                        tmp = parent_namespace(tmp)?;
                    }
                    if tmp != t && same_parent(tmp, t)? {
                        format!("{} -> {t}\n", child_namespace(tmp))
                    } else {
                        String::new()
                    }
                }
            }
            DependencyType::OneToOneCompact
            | DependencyType::PackageToPackageIfSameParentNamespace
            | DependencyType::NoDependency => String::new(),
        })
    }
}

/// `PlantUmlSlice`: a slice drawn as a component, inside its packages when it has a namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceNode {
    /// The slice's description.
    pub name: String,
    /// The namespace (`MatchingWithPackages`' prefix), ending in `.`.
    pub namespace: Option<String>,
    /// A background colour, without the `#`.
    pub color: Option<String>,
    /// Drawn as C4 containers and boundaries (`UseS4Style`).
    pub c4: bool,
}

impl SliceNode {
    /// `new PlantUmlSlice(name, nameSpace, color)`.
    ///
    /// # Errors
    /// `IllegalComponentNameException` for an empty name or a forbidden character.
    pub fn new(
        name: &str,
        namespace: Option<&str>,
        color: Option<&str>,
    ) -> Result<Self, ExportError> {
        assert_no_forbidden(&[Some(name), namespace])?;
        assert_not_empty(&[name])?;
        Ok(Self {
            name: name.to_owned(),
            namespace: namespace.map(str::to_owned),
            color: color.map(str::to_owned),
            c4: false,
        })
    }

    fn render(&self) -> Result<String, ExportError> {
        if self.c4 {
            return self.render_c4();
        }
        let mut out = String::new();
        let Some(namespace) = &self.namespace else {
            out.push('[');
            out.push_str(&self.name);
            out.push(']');
            if let Some(color) = &self.color {
                out.push_str(" #");
                out.push_str(color);
            }
            out.push('\n');
            return Ok(out);
        };
        let Some(package) = namespace
            .len()
            .checked_sub(1)
            .and_then(|end| namespace.get(..end))
        else {
            return fail(
                ExportException::ArgumentOutOfRange,
                "a slice's namespace is empty",
            );
        };
        out.push_str("package ");
        out.push_str(package);
        // Each `.` in the name beyond the namespace opens one more package.
        let mut parts: Vec<&str> = self
            .name
            .get(namespace.len()..)
            .unwrap_or_default()
            .split('.')
            .collect();
        let name = parts.pop().unwrap_or_default();
        for part in &parts {
            out.push_str(" {\n");
            out.push_str("package ");
            out.push_str(part);
        }
        let depth = parts.len() + 1;
        if name.is_empty() {
            match &self.color {
                Some(color) => {
                    out.push_str(" #");
                    out.push_str(color);
                    out.push_str(" {\n");
                }
                None => out.push_str(" {\n"),
            }
        } else {
            out.push_str(" {\n");
            out.push('[');
            out.push_str(name);
            out.push_str("] as ");
            out.push_str(&self.name);
            if let Some(color) = &self.color {
                out.push_str(" #");
                out.push_str(color);
            }
            out.push('\n');
        }
        for _ in 0..depth {
            out.push_str("}\n");
        }
        out.push('\n');
        Ok(out)
    }

    fn render_c4(&self) -> Result<String, ExportError> {
        let Some(namespace) = &self.namespace else {
            return fail(
                ExportException::NullReference,
                format!(
                    "C4Style draws slice `{}` inside its namespace, and it has none: slice with MatchingWithPackages",
                    self.name
                ),
            );
        };
        let Some(package) = namespace
            .len()
            .checked_sub(1)
            .and_then(|end| namespace.get(..end))
        else {
            return fail(
                ExportException::ArgumentOutOfRange,
                "a slice's namespace is empty",
            );
        };
        let boundary = |part: &str| format!("Boundary({part}, {part}) ");
        let mut out = boundary(package);
        let mut parts: Vec<&str> = self
            .name
            .get(namespace.len()..)
            .unwrap_or_default()
            .split('.')
            .collect();
        let name = parts.pop().unwrap_or_default();
        for part in &parts {
            out.push_str(" {\n");
            out.push_str(&boundary(part));
        }
        let depth = parts.len() + 1;
        out.push_str(" {\n");
        if !name.is_empty() {
            let container = format!("Container({}, {name})\n", self.name);
            out.push_str(&container);
        }
        for _ in 0..depth {
            out.push_str("}\n");
        }
        out.push('\n');
        Ok(out)
    }
}

/// One thing a diagram draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Element {
    /// `PlantUmlClass`: `class "name" {}`.
    Class(String),
    /// `PlantUmlInterface`: `interface "name" {}`.
    Interface(String),
    /// `PlantUmlNamespace`: `namespace name {}`.
    Namespace(String),
    /// `PlantUmlSlice`.
    Slice(SliceNode),
    /// `PlantUmlDependency`.
    Dependency(Dependency),
}

impl Element {
    /// `new PlantUmlClass(name)`.
    ///
    /// # Errors
    /// `IllegalComponentNameException` for an empty name or a forbidden character.
    pub fn class(name: &str) -> Result<Self, ExportError> {
        assert_no_forbidden(&[Some(name)])?;
        assert_not_empty(&[name])?;
        Ok(Self::Class(name.to_owned()))
    }

    /// `new PlantUmlInterface(name)`.
    ///
    /// # Errors
    /// As [`Element::class`].
    pub fn interface(name: &str) -> Result<Self, ExportError> {
        assert_no_forbidden(&[Some(name)])?;
        assert_not_empty(&[name])?;
        Ok(Self::Interface(name.to_owned()))
    }

    /// `new PlantUmlNamespace(name)`.
    ///
    /// # Errors
    /// As [`Element::class`].
    pub fn namespace(name: &str) -> Result<Self, ExportError> {
        assert_no_forbidden(&[Some(name)])?;
        assert_not_empty(&[name])?;
        Ok(Self::Namespace(name.to_owned()))
    }

    /// `GetPlantUmlString`.
    ///
    /// # Errors
    /// As [`Dependency::render`], and [`ExportException::NullReference`] for a C4 slice without a
    /// namespace.
    pub fn render(&self) -> Result<String, ExportError> {
        match self {
            Self::Class(name) => Ok(format!("class \"{name}\" {{\n}}\n")),
            Self::Interface(name) => Ok(format!("interface \"{name}\" {{\n}}\n")),
            Self::Namespace(name) => Ok(format!("namespace {name} {{\n}}\n")),
            Self::Slice(slice) => slice.render(),
            Self::Dependency(dependency) => dependency.render(),
        }
    }

    /// `PlantUmlDiagram`'s order: namespaces, then slices, classes, interfaces, then the rest.
    fn rank(&self) -> u8 {
        match self {
            Self::Namespace(_) => 0,
            Self::Slice(_) => 1,
            Self::Class(_) => 2,
            Self::Interface(_) => 3,
            Self::Dependency(_) => 4,
        }
    }
}

/// `PlantUmlDiagram`'s header: the C4 library it includes and `HIDE_STEREOTYPE()`.
pub const HEADER: &str = "@startuml\n\n!include https://raw.githubusercontent.com/plantuml-stdlib/C4-PlantUML/master/C4_Container.puml\n\nHIDE_STEREOTYPE()\n\n";

/// `PlantUmlDiagram`'s last line.
pub const FOOTER: &str = "@enduml\n";

/// Which dependencies a builder draws: `GenerationOptions.DependencyFilter` as a predicate over
/// the origin's and the target's full names.
pub type DependencyFilter<'f> = &'f dyn Fn(&str, &str) -> bool;

/// `GenerationOptions`.
#[derive(Clone, Copy, Default)]
pub struct GenerationOptions<'f> {
    /// `DependencyFilter`: the dependencies drawn; all when absent.
    pub dependency_filter: Option<DependencyFilter<'f>>,
    /// `IncludeDependenciesToOther`: from types, also draw dependencies on types outside the
    /// selection.
    pub include_dependencies_to_other: bool,
    /// `LimitDependencies`: from slices, draw only the dependencies between slices at the same
    /// depth (and, with packages, under the same parent).
    pub limit_dependencies: bool,
    /// `C4Style`: from slices with packages, draw C4 containers and boundaries.
    pub c4_style: bool,
}

impl std::fmt::Debug for GenerationOptions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GenerationOptions")
            .field("dependency_filter", &self.dependency_filter.is_some())
            .field(
                "include_dependencies_to_other",
                &self.include_dependencies_to_other,
            )
            .field("limit_dependencies", &self.limit_dependencies)
            .field("c4_style", &self.c4_style)
            .finish()
    }
}

impl GenerationOptions<'_> {
    fn keeps(&self, origin: &str, target: &str) -> bool {
        self.dependency_filter.is_none_or(|f| f(origin, target))
    }
}

/// `DependencyFilters.IgnoreDependenciesToParents`: a nested type's dependency on the type it
/// is nested in (by full-name prefix) is left out.
#[must_use]
pub fn ignore_dependencies_to_parents(origin: &str, target: &str) -> bool {
    no_parents(origin, target) || !origin.starts_with(target)
}

/// `DependencyFilters.IgnoreDependenciesToChildren`.
#[must_use]
pub fn ignore_dependencies_to_children(origin: &str, target: &str) -> bool {
    no_parents(origin, target) || !target.starts_with(origin)
}

/// `DependencyFilters.IgnoreDependenciesToChildrenAndParents`.
#[must_use]
pub fn ignore_dependencies_to_children_and_parents(origin: &str, target: &str) -> bool {
    no_parents(origin, target) || (!target.starts_with(origin) && !origin.starts_with(target))
}

fn no_parents(origin: &str, target: &str) -> bool {
    !origin.contains('.') && !target.contains('.')
}

/// A type to draw: its full name, whether it is an interface, and the full names it depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportType {
    /// `FullName`.
    pub full_name: String,
    /// `type is Interface`.
    pub interface: bool,
    /// The full names of `Dependencies`' targets.
    pub dependencies: Vec<String>,
}

/// A slice to draw (`Slice`, or `Namespace`, which is one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSlice {
    /// `Description`.
    pub description: String,
    /// `NameSpace`: the prefix `MatchingWithPackages` keeps, ending in `.`.
    pub namespace: Option<String>,
    /// `CountOfAsteriskInPattern`: the number of `(*)` in the pattern, none for `(**)`.
    pub asterisks: Option<usize>,
    /// `slice is Namespace`.
    pub is_namespace: bool,
    /// The full names of `Types`.
    pub types: BTreeSet<String>,
    /// `Dependencies`: (origin, target) full names of every dependency of its types.
    pub dependencies: Vec<(String, String)>,
}

/// `PlantUmlFileBuilder`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Builder {
    elements: Vec<Element>,
    dependencies: Vec<Dependency>,
}

impl Builder {
    /// An empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The elements added so far, dependencies included, in the order they were added.
    #[must_use]
    pub fn elements(&self) -> &[Element] {
        &self.elements
    }

    /// `WithElements`.
    #[must_use]
    pub fn with_elements(mut self, elements: impl IntoIterator<Item = Element>) -> Self {
        self.elements.extend(elements);
        self
    }

    /// `WithDependenciesFrom(IEnumerable<IType>, GenerationOptions)`: one class or interface per
    /// type, and an arrow per dependency on another type (in the selection, unless
    /// `IncludeDependenciesToOther`), each once, targets in ordinal order.
    ///
    /// # Errors
    /// `IllegalComponentNameException` for a type or target name `PlantUML` cannot hold.
    pub fn with_types(
        mut self,
        types: &[ExportType],
        options: &GenerationOptions<'_>,
    ) -> Result<Self, ExportError> {
        let mut selected: Vec<&ExportType> = Vec::new();
        for ty in types {
            if !selected.iter().any(|s| s.full_name == ty.full_name) {
                selected.push(ty);
            }
        }
        let names: BTreeSet<&str> = selected.iter().map(|t| t.full_name.as_str()).collect();
        let mut nodes = Vec::new();
        for ty in &selected {
            let targets: BTreeSet<&str> = ty
                .dependencies
                .iter()
                .map(String::as_str)
                .filter(|target| {
                    options.keeps(&ty.full_name, target)
                        && *target != ty.full_name
                        && (options.include_dependencies_to_other || names.contains(target))
                })
                .collect();
            for target in targets {
                self.dependencies.push(Dependency::new(
                    &ty.full_name,
                    target,
                    DependencyType::OneToOne,
                )?);
            }
            nodes.push(if ty.interface {
                Element::interface(&ty.full_name)?
            } else {
                Element::class(&ty.full_name)?
            });
        }
        self.elements.extend(nodes);
        self.elements
            .extend(self.dependencies.iter().cloned().map(Element::Dependency));
        Ok(self)
    }

    /// `WithDependenciesFrom(IEnumerable<Slice>, GenerationOptions)`: one component per slice
    /// (a namespace block per namespace, nothing for a slice whose name prefixes another's when
    /// it has packages), and the arrows `ArchUnitNET` draws between them for the options.
    ///
    /// # Errors
    /// `IllegalComponentNameException` for a name `PlantUML` cannot hold.
    pub fn with_slices(
        mut self,
        slices: &[ExportSlice],
        options: &GenerationOptions<'_>,
    ) -> Result<Self, ExportError> {
        let mut list: Vec<&ExportSlice> = Vec::new();
        for slice in slices {
            if !list.iter().any(|s| s.description == slice.description) {
                list.push(slice);
            }
        }
        remove_pattern_inappropriate(&mut list, '.');
        let mut nodes: Vec<(&ExportSlice, Element)> = Vec::new();
        for slice in &list {
            let package = is_package(&list, slice);
            let targets = select_dependencies(&list, slice, options);
            if slice.is_namespace {
                nodes.push((slice, Element::namespace(&slice.description)?));
            } else if !package {
                let mut node =
                    SliceNode::new(&slice.description, slice.namespace.as_deref(), None)?;
                node.c4 = options.c4_style;
                nodes.push((slice, Element::Slice(node)));
            }
            let kind = if !options.limit_dependencies {
                if package {
                    DependencyType::PackageToOne
                } else {
                    DependencyType::OneToOne
                }
            } else if slice.namespace.is_some() {
                if package {
                    DependencyType::PackageToPackageIfSameParentNamespace
                } else {
                    DependencyType::OneToOneIfSameParentNamespace
                }
            } else {
                DependencyType::OneToOneCompact
            };
            for target in targets {
                let dependency = Dependency::new(&slice.description, &target.description, kind)?;
                // With packages, a slice's arrow to a slice whose name contains the target's is
                // kept only towards a deeper target or when no other slice's name contains it.
                let drawn = kind != DependencyType::OneToOneIfSameParentNamespace
                    || dependency.origin_dots() < dependency.target_dots()
                    || list.iter().all(|s| {
                        s.description == dependency.target
                            || !s.description.contains(&dependency.target)
                    });
                if drawn {
                    self.dependencies.push(dependency);
                }
            }
        }
        let mut elements = handle_nodes(nodes)?;
        if !options.limit_dependencies {
            self.remove_duplicates_when_showing_packages(&list);
            self.replace_circles();
        }
        self.remove_duplicated_arrows()?;
        elements.sort_by_key(|element| match element {
            Element::Namespace(name) => i64::try_from(name.len()).unwrap_or(i64::MAX),
            _ => -1,
        });
        self.elements.extend(elements);
        self.elements
            .extend(self.dependencies.iter().cloned().map(Element::Dependency));
        Ok(self)
    }

    /// `AsString()`: the header, the elements in `PlantUmlDiagram`'s order, `@enduml`.
    ///
    /// # Errors
    /// As [`Element::render`].
    pub fn render(&self) -> Result<String, ExportError> {
        let mut body = String::new();
        for element in self.ordered() {
            body.push_str(&element.render()?);
        }
        Ok(format!("{HEADER}{body}{FOOTER}"))
    }

    /// The elements in `PlantUmlDiagram`'s order: a stable sort by kind.
    #[must_use]
    pub fn ordered(&self) -> Vec<&Element> {
        let mut ordered: Vec<&Element> = self.elements.iter().collect();
        ordered.sort_by_key(|e| e.rank());
        ordered
    }

    /// `RemoveDuplicateDependenciesWhenShowingPackages`, when every slice has a namespace.
    fn remove_duplicates_when_showing_packages(&mut self, list: &[&ExportSlice]) {
        if list.iter().any(|s| s.namespace.is_none()) {
            return;
        }
        let deps = &mut self.dependencies;
        let mut j = deps.len();
        while j > 0 {
            j -= 1;
            let (origin, target, kind) =
                (deps[j].origin.clone(), deps[j].target.clone(), deps[j].kind);
            if list
                .iter()
                .any(|s| s.description.contains(&target) && s.description != target)
            {
                if origin.contains(&target) {
                    deps.remove(j);
                } else {
                    deps[j].kind = if kind == DependencyType::PackageToOne {
                        DependencyType::PackageToPackage
                    } else {
                        DependencyType::OneToPackage
                    };
                }
                continue;
            }
            if kind == DependencyType::PackageToOne && target.contains(&origin) {
                deps.remove(j);
            }
        }
        let mut i = deps.len();
        while i > 0 {
            i -= 1;
            let this = deps[i].clone();
            if matches!(
                this.kind,
                DependencyType::PackageToOne | DependencyType::PackageToPackage
            ) && deps.iter().any(|d| {
                this.target == d.target
                    && d.origin.contains(&this.origin)
                    && d.origin != this.origin
            }) {
                deps.remove(i);
                continue;
            }
            if matches!(
                this.kind,
                DependencyType::OneToPackage | DependencyType::PackageToPackage
            ) && deps.iter().any(|d| {
                d.target.contains(&this.target)
                    && d.target != this.target
                    && d.origin.contains(&this.origin)
            }) {
                deps.remove(i);
                continue;
            }
            if this.target.contains(&this.origin) && this.target != this.origin {
                deps.remove(i);
            }
        }
    }

    /// `ReplaceCirclesWithAppropriateDependencyType`: of two opposite arrows, the later goes and
    /// the earlier becomes a circle.
    fn replace_circles(&mut self) {
        let deps = &mut self.dependencies;
        let mut i = deps.len();
        while i > 0 {
            i -= 1;
            for j in (0..i).rev() {
                if deps[i].target != deps[j].origin || deps[i].origin != deps[j].target {
                    continue;
                }
                deps.remove(i);
                deps[j].kind = DependencyType::Circle;
                break;
            }
        }
    }

    /// `RemoveDuplicatedArrowsIfExist`: of two arrows drawn the same, the later goes.
    fn remove_duplicated_arrows(&mut self) -> Result<(), ExportError> {
        let rendered: Vec<String> = self
            .dependencies
            .iter()
            .map(Dependency::render)
            .collect::<Result<_, _>>()?;
        let mut keep = vec![true; rendered.len()];
        for i in (0..rendered.len()).rev() {
            if (0..i).rev().any(|j| rendered[j] == rendered[i]) {
                keep[i] = false;
            }
        }
        let mut index = 0;
        self.dependencies.retain(|_| {
            index += 1;
            keep[index - 1]
        });
        Ok(())
    }

    /// The arrows drawn, in order.
    #[must_use]
    pub fn dependencies(&self) -> &[Dependency] {
        &self.dependencies
    }
}

/// `RemovePatternInappropriateSlices()`: with every slice's pattern counting its `(*)`, a slice
/// whose name (beyond its namespace) holds as many separators as the pattern has `(*)` is
/// dropped. `ArchUnitNET` counts `.`; a path pattern's slices count `/`.
pub fn remove_pattern_inappropriate<S: std::borrow::Borrow<ExportSlice>>(
    list: &mut Vec<S>,
    separator: char,
) {
    let count = |name: &str| name.matches(separator).count();
    if list.iter().any(|s| s.borrow().asterisks.is_none()) {
        return;
    }
    // Upstream drops a slice when `dots(name) - dots(namespace) >= asterisks`.
    list.retain(|s| {
        let s = s.borrow();
        let namespace = s.namespace.as_deref().map_or(0, count);
        s.asterisks
            .is_none_or(|n| count(&s.description) < namespace + n)
    });
}

/// `IsPackage`: a slice with a namespace whose name another slice's name starts with.
fn is_package(list: &[&ExportSlice], slice: &ExportSlice) -> bool {
    slice.namespace.is_some()
        && list.iter().any(|s| {
            s.description != slice.description && s.description.starts_with(&slice.description)
        })
}

/// `SelectDependencies`: the other slices holding a target of one of the slice's dependencies.
fn select_dependencies<'s>(
    list: &[&'s ExportSlice],
    slice: &ExportSlice,
    options: &GenerationOptions<'_>,
) -> Vec<&'s ExportSlice> {
    list.iter()
        .copied()
        .filter(|target| {
            target.description != slice.description
                && slice
                    .dependencies
                    .iter()
                    .any(|(o, t)| options.keeps(o, t) && target.types.contains(t))
        })
        .collect()
}

/// `HandleNodes`: the nodes, then for each namespace every parent namespace not yet drawn.
fn handle_nodes(nodes: Vec<(&ExportSlice, Element)>) -> Result<Vec<Element>, ExportError> {
    let namespaces: Vec<String> = nodes
        .iter()
        .filter(|(slice, _)| slice.is_namespace)
        .map(|(slice, _)| slice.description.clone())
        .collect();
    let mut elements: Vec<Element> = nodes.into_iter().map(|(_, e)| e).collect();
    for name in namespaces {
        let mut parents = Vec::new();
        let mut current = name.as_str();
        loop {
            parents.push(current);
            match current.rfind('.') {
                Some(at) => current = &current[..at],
                None => break,
            }
        }
        for parent in parents.into_iter().rev() {
            let drawn = elements
                .iter()
                .any(|e| matches!(e, Element::Namespace(n) if n == parent));
            if !drawn {
                elements.push(Element::namespace(parent)?);
            }
        }
    }
    Ok(elements)
}

/// The architecture's loaded types, by ordinal full name, as `WithDependenciesFrom(types)` takes
/// them: `referenced` types are dependency targets, not types of the architecture.
#[must_use]
pub fn export_types(architecture: &Architecture<'_>) -> Vec<ExportType> {
    architecture
        .types
        .values()
        .filter(|t| t.referenced != Some(true))
        .map(|t| ExportType {
            full_name: t.full_name.clone(),
            interface: t.kind == "interface",
            dependencies: t.dependencies.iter().map(|d| d.target.clone()).collect(),
        })
        .collect()
}

/// The architecture's namespaces (`Architecture.Namespaces`): one per namespace of a loaded
/// type, by ordinal name, each holding its loaded types.
#[must_use]
pub fn export_namespaces(architecture: &Architecture<'_>) -> Vec<ExportSlice> {
    let mut by_name: BTreeMap<&str, ExportSlice> = BTreeMap::new();
    for ty in architecture
        .types
        .values()
        .filter(|t| t.referenced != Some(true))
    {
        let name = ty.namespace.as_deref().unwrap_or_default();
        let slice = by_name.entry(name).or_insert_with(|| ExportSlice {
            description: name.to_owned(),
            namespace: None,
            asterisks: None,
            is_namespace: true,
            types: BTreeSet::new(),
            dependencies: Vec::new(),
        });
        slice.types.insert(ty.full_name.clone());
        slice.dependencies.extend(
            ty.dependencies
                .iter()
                .map(|d| (ty.full_name.clone(), d.target.clone())),
        );
    }
    by_name.into_values().collect()
}

/// The slices of a [`Slicing`], by ordinal name, as `WithDependenciesFrom(slices)` takes them.
#[must_use]
pub fn export_slices(slicing: &Slicing) -> Vec<ExportSlice> {
    slicing
        .slices
        .iter()
        .map(|(name, members)| ExportSlice {
            description: name.clone(),
            namespace: slicing.namespace.clone(),
            asterisks: slicing.asterisks,
            is_namespace: false,
            types: members.clone(),
            dependencies: members
                .iter()
                .flat_map(|member| {
                    slicing
                        .dependencies
                        .get(member)
                        .into_iter()
                        .flatten()
                        .map(move |target| (member.clone(), target.clone()))
                })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dep(o: &str, t: &str, kind: DependencyType) -> Dependency {
        Dependency {
            origin: o.into(),
            target: t.into(),
            kind,
        }
    }

    fn drawn(o: &str, t: &str, kind: DependencyType) -> String {
        dep(o, t, kind).render().unwrap_or_else(|e| e.to_string())
    }

    #[test]
    fn every_dependency_type_draws_as_upstream() {
        use DependencyType::*;
        let table = [
            ("a", "b", OneToOne, "[a] --|> [b]\n"),
            ("a", "b", OneToMany, "[a] \"1\" --|> \"many\" [b]\n"),
            ("a", "x.b", OneToPackage, "[a] -[#red]> b\n"),
            ("x.a", "b", PackageToOne, "a -[#blue]> [b]\n"),
            ("x.a", "y.b", PackageToPackage, "a -[#green]> b\n"),
            ("a.b", "a.c", OneToOneCompact, "[a.b] --> [a.c]\n"),
            ("a.b", "c", OneToOneCompact, ""),
            ("a", "b", Circle, "[a] <-[#red]> [b]\n"),
            ("a", "b", PackageToPackageIfSameParentNamespace, "a ..> b\n"),
            (
                "p.a",
                "p.b",
                PackageToPackageIfSameParentNamespace,
                "a ..> b\n",
            ),
            ("p.a", "q.b", PackageToPackageIfSameParentNamespace, ""),
            ("p.a", "b", PackageToPackageIfSameParentNamespace, ""),
            ("a", "b", OneToOneIfSameParentNamespace, "a --|> b\n"),
            (
                "p.a",
                "p.b",
                OneToOneIfSameParentNamespace,
                "p.a --|> p.b\n",
            ),
            (
                "p.a",
                "q.b",
                OneToOneIfSameParentNamespace,
                "ArgumentOutOfRangeException: `p` has no parent namespace (no `.`)",
            ),
            ("p.a", "p.b.c", OneToOneIfSameParentNamespace, "p.a --> b\n"),
            ("p.a", "p.a.c", OneToOneIfSameParentNamespace, ""),
            ("p.a", "q.b.c", OneToOneIfSameParentNamespace, ""),
            ("p.b.c", "p.a", OneToOneIfSameParentNamespace, "b -> p.a\n"),
            ("p.a.c", "p.a", OneToOneIfSameParentNamespace, ""),
            ("q.b.c", "p.a", OneToOneIfSameParentNamespace, ""),
            ("a", "b", NoDependency, ""),
        ];
        for (o, t, kind, want) in table {
            assert_eq!(drawn(o, t, kind), want, "{o} {t} {kind:?}");
        }
        // Upstream asks a name without a `.` for its parent and throws.
        let error = dep("a", "b.c", OneToOneIfSameParentNamespace).render();
        assert_eq!(
            error.map_err(|e| e.exception),
            Err(ExportException::ArgumentOutOfRange)
        );
        let error = dep("b.c", "a", OneToOneIfSameParentNamespace).render();
        assert_eq!(
            error.map_err(|e| e.exception),
            Err(ExportException::ArgumentOutOfRange)
        );
    }

    #[test]
    fn names_are_checked_as_upstream() {
        for bad in ["[", "]", "\r", "\n", "\u{c}", "\u{7}", "\u{8}", "\u{b}"] {
            for result in [
                Dependency::new(bad, "a", DependencyType::OneToOne).map(|_| ()),
                Dependency::new("a", bad, DependencyType::OneToOne).map(|_| ()),
                Element::class(bad).map(|_| ()),
                Element::interface(bad).map(|_| ()),
                Element::namespace(bad).map(|_| ()),
                SliceNode::new(bad, None, None).map(|_| ()),
                SliceNode::new("a", Some(bad), None).map(|_| ()),
            ] {
                assert_eq!(
                    result.map_err(|e| e.exception),
                    Err(ExportException::IllegalComponentName),
                    "{bad:?}"
                );
            }
        }
        for result in [
            Dependency::new("", "a", DependencyType::OneToOne).map(|_| ()),
            Dependency::new("a", "", DependencyType::OneToOne).map(|_| ()),
            Element::class("").map(|_| ()),
            Element::interface("").map(|_| ()),
            Element::namespace("").map(|_| ()),
            SliceNode::new("", None, None).map(|_| ()),
        ] {
            let error = result.err();
            assert_eq!(
                error.as_ref().map(|e| e.message.as_str()),
                Some("PlantUml component names can't be null or empty.")
            );
        }
        assert!(Element::class("a b\t%").is_ok());
        assert_eq!(
            ExportError {
                exception: ExportException::NullReference,
                message: "m".into()
            }
            .to_string(),
            "NullReferenceException: m"
        );
    }

    #[test]
    fn slices_draw_inside_their_packages() -> Result<(), ExportError> {
        let plain = SliceNode::new("Books", None, None)?;
        assert_eq!(plain.render()?, "[Books]\n");
        let coloured = SliceNode::new("Books", None, Some("99ffd1"))?;
        assert_eq!(coloured.render()?, "[Books] #99ffd1\n");
        let nested = SliceNode::new("A.B.C", Some("A."), None)?;
        assert_eq!(
            nested.render()?,
            "package A {\npackage B {\n[C] as A.B.C\n}\n}\n\n"
        );
        let nested_coloured = SliceNode::new("A.B", Some("A."), Some("99ffd1"))?;
        assert_eq!(
            nested_coloured.render()?,
            "package A {\n[B] as A.B #99ffd1\n}\n\n"
        );
        let package = SliceNode::new("A.B.", Some("A."), None)?;
        assert_eq!(package.render()?, "package A {\npackage B {\n}\n}\n\n");
        let package_coloured = SliceNode::new("A.B.", Some("A."), Some("99ffd1"))?;
        assert_eq!(
            package_coloured.render()?,
            "package A {\npackage B #99ffd1 {\n}\n}\n\n"
        );
        let mut c4 = SliceNode::new("A.B.C", Some("A."), None)?;
        c4.c4 = true;
        assert_eq!(
            c4.render()?,
            "Boundary(A, A)  {\nBoundary(B, B)  {\nContainer(A.B.C, C)\n}\n}\n\n"
        );
        let mut c4_package = SliceNode::new("A.", Some("A."), None)?;
        c4_package.c4 = true;
        assert_eq!(c4_package.render()?, "Boundary(A, A)  {\n}\n\n");
        let mut no_namespace = SliceNode::new("A", None, None)?;
        no_namespace.c4 = true;
        assert_eq!(
            no_namespace.render().map_err(|e| e.exception),
            Err(ExportException::NullReference)
        );
        for c4 in [false, true] {
            let mut empty = SliceNode::new("A", Some(""), None)?;
            empty.c4 = c4;
            assert_eq!(
                empty.render().map_err(|e| e.exception),
                Err(ExportException::ArgumentOutOfRange)
            );
        }
        Ok(())
    }

    #[test]
    fn elements_draw_in_the_diagrams_order() -> Result<(), ExportError> {
        let builder = Builder::new().with_elements([
            Element::Dependency(dep("a", "b", DependencyType::OneToOne)),
            Element::interface("I")?,
            Element::class("C")?,
            Element::Slice(SliceNode::new("S", None, None)?),
            Element::namespace("N")?,
        ]);
        assert_eq!(
            builder.render()?,
            format!(
                "{HEADER}namespace N {{\n}}\n[S]\nclass \"C\" {{\n}}\ninterface \"I\" {{\n}}\n[a] --|> [b]\n{FOOTER}"
            )
        );
        assert_eq!(builder.elements().len(), 5);
        Ok(())
    }

    fn ty(name: &str, interface: bool, deps: &[&str]) -> ExportType {
        ExportType {
            full_name: name.into(),
            interface,
            dependencies: deps.iter().map(|d| (*d).to_owned()).collect(),
        }
    }

    #[test]
    fn types_draw_their_dependencies_once_each() -> Result<(), ExportError> {
        let types = [
            ty("N.B", false, &["N.A", "N.B", "N.A", "System.Object"]),
            ty("N.A", true, &["N.B"]),
            ty("N.B", false, &[]),
        ];
        let inside = Builder::new().with_types(&types, &GenerationOptions::default())?;
        assert_eq!(
            inside.render()?,
            format!(
                "{HEADER}class \"N.B\" {{\n}}\ninterface \"N.A\" {{\n}}\n[N.B] --|> [N.A]\n[N.A] --|> [N.B]\n{FOOTER}"
            )
        );
        let other = GenerationOptions {
            include_dependencies_to_other: true,
            ..GenerationOptions::default()
        };
        let all = Builder::new().with_types(&types, &other)?;
        assert_eq!(all.dependencies().len(), 3);
        assert_eq!(all.dependencies()[1].target, "System.Object");
        let only_b = |_: &str, target: &str| target == "N.B";
        let filtered = GenerationOptions {
            dependency_filter: Some(&only_b),
            ..GenerationOptions::default()
        };
        let some = Builder::new().with_types(&types, &filtered)?;
        let pairs: Vec<(&str, &str)> = some
            .dependencies()
            .iter()
            .map(|d| (d.origin.as_str(), d.target.as_str()))
            .collect();
        assert_eq!(pairs, [("N.A", "N.B")]);
        assert!(format!("{filtered:?}").contains("dependency_filter: true"));
        Ok(())
    }

    fn slice(
        name: &str,
        namespace: Option<&str>,
        asterisks: Option<usize>,
        deps: &[(&str, &str)],
    ) -> ExportSlice {
        ExportSlice {
            description: name.into(),
            namespace: namespace.map(str::to_owned),
            asterisks,
            is_namespace: false,
            types: [format!("{name}.T")].into(),
            dependencies: deps
                .iter()
                .map(|(o, t)| ((*o).to_owned(), (*t).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn slices_draw_one_arrow_or_circle_per_pair() -> Result<(), ExportError> {
        let slices = [
            slice("A", None, Some(1), &[("A.T", "B.T"), ("A.T", "C.T")]),
            slice("B", None, Some(1), &[("B.T", "A.T")]),
            slice("C", None, Some(1), &[]),
            slice("C.D", None, Some(1), &[("C.D.T", "A.T")]),
        ];
        let builder = Builder::new().with_slices(&slices, &GenerationOptions::default())?;
        assert_eq!(
            builder.render()?,
            format!("{HEADER}[A]\n[B]\n[C]\n[A] <-[#red]> [B]\n[A] --|> [C]\n{FOOTER}"),
            "C.D is as deep as the pattern's one (*), so it is dropped"
        );
        let limited = GenerationOptions {
            limit_dependencies: true,
            ..GenerationOptions::default()
        };
        let compact = Builder::new().with_slices(&slices, &limited)?;
        let kinds: Vec<DependencyType> = compact.dependencies().iter().map(|d| d.kind).collect();
        assert_eq!(
            kinds,
            [
                DependencyType::OneToOneCompact,
                DependencyType::OneToOneCompact,
                DependencyType::OneToOneCompact
            ],
            "no circles when limited"
        );
        Ok(())
    }

    #[test]
    fn namespaces_draw_with_their_parents() -> Result<(), ExportError> {
        let mut deep = slice("P.Q.R", None, None, &[("P.Q.R.T", "S.T")]);
        deep.is_namespace = true;
        let mut other = slice("S", None, None, &[]);
        other.is_namespace = true;
        let builder = Builder::new().with_slices(&[deep, other], &GenerationOptions::default())?;
        assert_eq!(
            builder.render()?,
            format!(
                "{HEADER}namespace S {{\n}}\nnamespace P {{\n}}\nnamespace P.Q {{\n}}\nnamespace P.Q.R {{\n}}\n[P.Q.R] --|> [S]\n{FOOTER}"
            )
        );
        Ok(())
    }

    #[test]
    fn packages_turn_arrows_into_package_arrows() -> Result<(), ExportError> {
        let slices = [
            slice("N.A", Some("N."), Some(2), &[("N.A.T", "N.B.T")]),
            slice("N.A.X", Some("N."), Some(2), &[("N.A.X.T", "N.B.T")]),
            slice("N.B", Some("N."), Some(2), &[("N.B.T", "N.A.X.T")]),
        ];
        let builder = Builder::new().with_slices(&slices, &GenerationOptions::default())?;
        let drawn: Vec<(String, String, DependencyType)> = builder
            .dependencies()
            .iter()
            .map(|d| (d.origin.clone(), d.target.clone(), d.kind))
            .collect();
        assert_eq!(
            drawn,
            [("N.A.X".to_owned(), "N.B".to_owned(), DependencyType::Circle)]
        );
        let limited = GenerationOptions {
            limit_dependencies: true,
            c4_style: true,
            ..GenerationOptions::default()
        };
        let c4 = Builder::new().with_slices(&slices, &limited)?;
        let text = c4.render()?;
        assert!(text.contains("Container(N.A.X, X)"), "{text}");
        assert!(
            !text.contains("Container(N.A, A)"),
            "N.A is a package: {text}"
        );
        Ok(())
    }

    #[test]
    fn inappropriate_slices_go_only_when_every_pattern_counts() {
        let mut list = vec![
            slice("A", None, Some(1), &[]),
            slice("A.B", None, Some(1), &[]),
            slice("N.A.B", Some("N."), Some(2), &[]),
            slice("N.A.B.C", Some("N."), Some(2), &[]),
        ];
        remove_pattern_inappropriate(&mut list, '.');
        let names: Vec<&str> = list.iter().map(|s| s.description.as_str()).collect();
        assert_eq!(names, ["A", "N.A.B"]);
        let mut mixed = vec![
            slice("A.B", None, Some(1), &[]),
            slice("C", None, None, &[]),
        ];
        remove_pattern_inappropriate(&mut mixed, '.');
        let mut paths = vec![
            slice("lib/x.ts", None, Some(1), &[]),
            slice("index.ts", None, Some(1), &[]),
        ];
        remove_pattern_inappropriate(&mut paths, '/');
        let names: Vec<&str> = paths.iter().map(|s| s.description.as_str()).collect();
        assert_eq!(
            names,
            ["index.ts"],
            "a path counts `/`, not the extension's `.`"
        );
        assert_eq!(mixed.len(), 2, "a (**) slice keeps every slice");
    }

    fn pairs(builder: &Builder) -> Vec<(String, String, DependencyType)> {
        builder
            .dependencies()
            .iter()
            .map(|d| (d.origin.clone(), d.target.clone(), d.kind))
            .collect()
    }

    fn packaged(name: &str, deps: &[(&str, &str)]) -> ExportSlice {
        slice(name, Some("N."), None, deps)
    }

    #[test]
    fn limited_arrows_with_packages_skip_a_target_another_slice_contains() -> Result<(), ExportError>
    {
        use DependencyType::OneToOneIfSameParentNamespace as Same;
        let limited = GenerationOptions {
            limit_dependencies: true,
            ..GenerationOptions::default()
        };
        // Deeper target: kept although `N.N.B.C` contains its name.
        let deeper = [
            packaged("N.A", &[("N.A.T", "N.B.C.T")]),
            packaged("N.B.C", &[]),
            packaged("N.N.B.C", &[]),
        ];
        let built = Builder::new().with_slices(&deeper, &limited)?;
        assert_eq!(pairs(&built), [("N.A".into(), "N.B.C".into(), Same)]);
        // As deep: dropped, because `N.N.B` contains `N.B`.
        let level = [
            packaged("N.A", &[("N.A.T", "N.B.T")]),
            packaged("N.B", &[]),
            packaged("N.N.B", &[]),
        ];
        assert!(
            Builder::new()
                .with_slices(&level, &limited)?
                .dependencies()
                .is_empty()
        );
        // As deep with no slice containing the target: kept.
        let plain = [packaged("N.A", &[("N.A.T", "N.B.T")]), packaged("N.B", &[])];
        assert_eq!(
            pairs(&Builder::new().with_slices(&plain, &limited)?),
            [("N.A".into(), "N.B".into(), Same)]
        );
        Ok(())
    }

    #[test]
    fn slices_come_before_namespaces_of_any_length() -> Result<(), ExportError> {
        let mut namespace = slice("A", None, None, &[]);
        namespace.is_namespace = true;
        let builder = Builder::new().with_slices(
            &[namespace, slice("S", None, None, &[])],
            &GenerationOptions::default(),
        )?;
        assert!(matches!(builder.elements()[0], Element::Slice(_)));
        assert!(matches!(builder.elements()[1], Element::Namespace(_)));
        Ok(())
    }

    #[test]
    fn showing_packages_rewrites_and_drops_arrows_as_upstream() -> Result<(), ExportError> {
        use DependencyType::*;
        let draw =
            |slices: &[ExportSlice]| -> Result<Vec<(String, String, DependencyType)>, ExportError> {
                Ok(pairs(
                    &Builder::new().with_slices(slices, &GenerationOptions::default())?,
                ))
            };
        // A package's arrow to a slice becomes package-to-package when another slice contains
        // the target's name; a slice's arrow to it, one-to-package.
        assert_eq!(
            draw(&[
                packaged("N.A", &[("N.A.T", "N.C.T")]),
                packaged("N.A.X", &[]),
                packaged("N.C", &[]),
                packaged("N.D", &[("N.D.T", "N.C.T")]),
                packaged("N.N.C", &[]),
            ])?,
            [
                ("N.A".into(), "N.C".into(), PackageToPackage),
                ("N.D".into(), "N.C".into(), OneToPackage),
            ]
        );
        // An arrow into a slice whose name holds the origin's is dropped.
        assert_eq!(
            draw(&[
                packaged("N.A", &[("N.A.T", "N.A.X.T")]),
                packaged("N.A.X", &[]),
                packaged("N.B", &[("N.B.T", "N.A.X.T")]),
            ])?,
            [("N.B".into(), "N.A.X".into(), OneToOne)]
        );
        // A package arrow that a deeper origin also draws to the same target is dropped.
        assert_eq!(
            draw(&[
                packaged("N.A", &[("N.A.T", "N.B.T")]),
                packaged("N.A.X", &[("N.A.X.T", "N.B.T")]),
                packaged("N.B", &[]),
            ])?,
            [("N.A.X".into(), "N.B".into(), OneToOne)]
        );
        // A package's arrow to an unrelated slice stays a package arrow.
        assert_eq!(
            draw(&[
                packaged("N.A", &[("N.A.T", "N.B.T")]),
                packaged("N.A.X", &[]),
                packaged("N.B", &[]),
            ])?,
            [("N.A".into(), "N.B".into(), PackageToOne)]
        );
        // A slice's arrow to a slice whose name contains its own (not as a prefix) is dropped.
        assert!(
            draw(&[
                packaged("N.B", &[("N.B.T", "N.AN.B.T")]),
                packaged("N.AN.B", &[]),
            ])?
            .is_empty()
        );
        // Upstream drops such an arrow only in its second pass, so it still takes a package
        // arrow into a part of its target down with it first.
        assert!(
            draw(&[
                packaged("N.BQ", &[("N.BQ.T", "N.AN.BQ.T"), ("N.BQ.T", "N.AN.T")]),
                packaged("N.AN.BQ", &[]),
                packaged("N.AN", &[]),
            ])?
            .is_empty()
        );
        // Without a namespace on every slice nothing is rewritten.
        assert_eq!(
            draw(&[
                slice("A", None, None, &[("A.T", "C.T")]),
                slice("C", None, None, &[]),
                slice("NC", None, None, &[]),
            ])?,
            [("A".into(), "C".into(), OneToOne)]
        );
        Ok(())
    }

    #[test]
    fn upstreams_named_filters() {
        assert!(
            !ignore_dependencies_to_parents("A.B", "A"),
            "only one end has a dot"
        );
        assert!(!ignore_dependencies_to_parents("A.B+C", "A.B"));
        assert!(ignore_dependencies_to_parents("A.B", "A.B+C"));
        assert!(
            ignore_dependencies_to_parents("AB", "A"),
            "no dots on either side"
        );
        assert!(!ignore_dependencies_to_children("A.B", "A.B+C"));
        assert!(ignore_dependencies_to_children("A.B+C", "A.B"));
        assert!(!ignore_dependencies_to_children_and_parents("A.B", "A.B+C"));
        assert!(!ignore_dependencies_to_children_and_parents("A.B+C", "A.B"));
        assert!(ignore_dependencies_to_children_and_parents("A.B", "A.C"));
        assert!(ignore_dependencies_to_children_and_parents("A", "AB"));
    }
}
