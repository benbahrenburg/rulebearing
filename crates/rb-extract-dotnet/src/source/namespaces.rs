//! The names every file wrote, resolved against the types every file declares, the way the C#
//! compiler looks a name up, and projected onto file modules with edges marked `approximate`.
//!
//! - Plan: [Wave 3, Step 14](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)
//!   (namespace-level edges projected to files, every dependency `approximate: true`, every module
//!   `attribution: source`)
//! - Decision: [ADR-0011](../../../../docs/adr/0011-read-dotnet-assemblies-not-source.md),
//!   [ADR-0014](../../../../docs/adr/0014-no-invented-cross-language-edges.md)
//! - Architecture: [§ The graph document](../../../../docs/architecture.md#the-graph-document)
//! - Requirement: [FR-EXT-DN-04](../../../../docs/prd.md#fr-ext-dn-04)
//!
//! A simple name is looked up as C# 12 § 7.6 (namespace and type names) orders it: the nested
//! types of each enclosing type, innermost first; then, for each namespace from the innermost
//! out, the types it declares, then the aliases and the namespaces its `using` directives import
//! at that level; the compilation unit's directives and the project's `global using` directives
//! at the global level. A dotted name continues from what its first segment found: a namespace's
//! types and sub-namespaces, a type's nested types, and a type is where it stops when the rest is
//! a member. Generic arity must match. What only a compiler knows (overload resolution,
//! extension methods, members inherited from a base, `var`) is not known, which is why every edge
//! is `approximate`.
//!
//! A type declared in several files (a partial type) lands in one of them, as a compiled build
//! attributes it: the part that declares a constructor when exactly one does, else the file named
//! after the type, else the first by path. A name that resolves to no declared type is not an
//! edge. Namespaces imported by a `using` directive and declared nowhere in the repository are
//! external modules named by the namespace, classified as compiled mode classifies an assembly
//! name: `package` when a package reference provides it, `framework` for `System.*` and
//! `Microsoft.*`, else `undetermined`, since without a build nothing says whether it resolves.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

use rb_model::{
    Attribution, Dependency, DependencyKind, DependencyType, Language, Module, ModuleSystem,
};

use super::tree_sitter::{FileFacts, Reference, TypeKind, Using};
use crate::discover::PackageRef;
use crate::edges::{is_framework, providing_package};

/// FNV-1a over the bytes written: the index's keys are short names hashed millions of times on a
/// large solution, where the standard library's `SipHash`, built to resist crafted keys a local
/// graph never sees, was most of the resolution's time.
#[derive(Debug, Default, Clone, Copy)]
struct Fnv(u64);

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0100_0000_01b3;
        let mut hash = if self.0 == 0 { OFFSET } else { self.0 };
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
        self.0 = hash;
    }
}

/// A hash map keyed with [`Fnv`].
type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<Fnv>>;

/// A hash set keyed with [`Fnv`].
type FastSet<K> = HashSet<K, BuildHasherDefault<Fnv>>;

/// A project as source mode needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectInfo {
    /// The project file, relative to the root, `/`-separated.
    pub path: String,
    /// A test project: its edges also carry `test-only`.
    pub is_test: bool,
    /// Its package references, direct and through its project references.
    pub package_refs: Vec<PackageRef>,
    /// The projects whose types it sees, itself and its project references transitively, sorted;
    /// empty for no restriction (no project file read).
    pub sees: Vec<usize>,
}

/// One parsed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// The module source: relative to the root, `/`-separated.
    pub source: String,
    /// The project it compiles in, an index into the projects.
    pub project: Option<usize>,
    /// What it declares and writes.
    pub facts: FileFacts,
}

/// One declared type, all its partial parts together.
#[derive(Debug, Clone)]
struct TypeEntry {
    /// The metadata name: `Ns.Outer+Inner`, with `` `N `` for a generic arity.
    full: String,
    /// The dotted container key its nested types are indexed under: `Ns.Outer.Inner`.
    dotted: String,
    kind: TypeKind,
    arity: u32,
    /// The file the type lands in.
    file: usize,
    /// The project that file compiles in.
    project: Option<usize>,
    /// The namespace or type it is declared in: its dotted key without its own name.
    container: String,
    /// Its constants and enum members, every part's: a compiled build inlines their values.
    constants: FastSet<String>,
}

/// What a name has resolved to so far.
enum Found {
    Namespace(String),
    Types(Vec<usize>),
}

/// (project, dotted key, arity) to (full name, kind, parts: (file, declaration, constructor)):
/// a partial type's parts are one compilation's, so two projects' types of one name are two.
type Declared =
    BTreeMap<(Option<usize>, String, u32), (String, TypeKind, Vec<(usize, usize, bool)>)>;

/// Every type and namespace the files declare.
struct Index {
    types: Vec<TypeEntry>,
    /// (container, simple name) to entries: the container is a namespace or a type's dotted key.
    members: FastMap<String, FastMap<String, Vec<usize>>>,
    /// Every declared namespace and each of its prefixes.
    namespaces: FastSet<String>,
    /// (file, declaration index) to the entry the declaration is a part of.
    parts: FastMap<(usize, usize), usize>,
    /// Each namespace's sub-namespaces by their last segment, so a lookup allocates nothing.
    children: FastMap<String, FastSet<String>>,
    /// Each extension method name to the types declaring one of that name.
    extensions: FastMap<String, Vec<usize>>,
}

fn join(left: &str, right: &str) -> String {
    if left.is_empty() {
        right.to_owned()
    } else {
        format!("{left}.{right}")
    }
}

fn parent_namespace(namespace: &str) -> &str {
    namespace.rsplit_once('.').map_or("", |(parent, _)| parent)
}

/// The namespaces a directive written in `base` names its target from: `base` and each
/// enclosing namespace, innermost first, then the global namespace (C# 12 § 14.5.2, a
/// `using` inside a namespace declaration resolves as a name written there would).
fn outward(base: &str) -> impl Iterator<Item = &str> {
    let mut next = Some(base);
    std::iter::from_fn(move || {
        let current = next?;
        next = (!current.is_empty()).then(|| parent_namespace(current));
        Some(current)
    })
}

/// The namespace a `using N;` written in `base` imports, when the repository declares it.
fn imported_namespace(index: &Index, base: &str, target: &[String]) -> Option<String> {
    let written = target.join(".");
    outward(base)
        .map(|namespace| join(namespace, &written))
        .find(|candidate| index.namespaces.contains(candidate))
}

fn metadata_name(namespace: &str, outer: &[(String, u32)], name: &str, arity: u32) -> String {
    let mut full = namespace.to_owned();
    for (i, (part, part_arity)) in outer
        .iter()
        .map(|(n, a)| (n.as_str(), *a))
        .chain(std::iter::once((name, arity)))
        .enumerate()
    {
        if i == 0 {
            if !full.is_empty() {
                full.push('.');
            }
        } else {
            full.push('+');
        }
        full.push_str(part);
        if part_arity > 0 {
            full.push('`');
            full.push_str(&part_arity.to_string());
        }
    }
    full
}

fn dotted_key(namespace: &str, outer: &[(String, u32)], name: &str) -> String {
    let mut key = namespace.to_owned();
    for part in outer.iter().map(|(n, _)| n.as_str()).chain([name]) {
        key = join(&key, part);
    }
    key
}

impl Index {
    fn build(files: &[SourceFile]) -> Self {
        let mut parts: Declared = BTreeMap::new();
        let mut namespaces = FastSet::default();
        namespaces.insert(String::new());
        for (file_index, file) in files.iter().enumerate() {
            for scope in &file.facts.scopes {
                let mut namespace = scope.namespace.as_str();
                while !namespace.is_empty() && namespaces.insert(namespace.to_owned()) {
                    namespace = parent_namespace(namespace);
                }
            }
            for (declaration_index, declaration) in file.facts.declarations.iter().enumerate() {
                if declaration.name.is_empty() {
                    continue;
                }
                let dotted = dotted_key(
                    &declaration.namespace,
                    &declaration.outer,
                    &declaration.name,
                );
                let full = metadata_name(
                    &declaration.namespace,
                    &declaration.outer,
                    &declaration.name,
                    declaration.arity,
                );
                let entry = parts
                    .entry((file.project, dotted, declaration.arity))
                    .or_insert_with(|| (full, declaration.kind, Vec::new()));
                entry
                    .2
                    .push((file_index, declaration_index, declaration.constructor));
            }
        }
        let mut types = Vec::new();
        let mut members: FastMap<String, FastMap<String, Vec<usize>>> = FastMap::default();
        let mut by_part = FastMap::default();
        for ((_, dotted, arity), (full, kind, declared)) in parts {
            let name = dotted.rsplit('.').next().unwrap_or(&dotted).to_owned();
            let container = dotted
                .rsplit_once('.')
                .map_or(String::new(), |(c, _)| c.to_owned());
            let landing: Vec<(usize, bool)> = declared.iter().map(|(f, _, c)| (*f, *c)).collect();
            let file = landing_file(files, &name, &landing);
            let mut constants = FastSet::default();
            for (part_file, declaration, _) in &declared {
                by_part.insert((*part_file, *declaration), types.len());
                if let Some(part) = files[*part_file].facts.declarations.get(*declaration) {
                    constants.extend(part.constants.iter().cloned());
                }
            }
            members
                .entry(container.clone())
                .or_default()
                .entry(name)
                .or_default()
                .push(types.len());
            types.push(TypeEntry {
                full,
                dotted,
                kind,
                arity,
                file,
                project: files[file].project,
                container,
                constants,
            });
        }
        let mut children: FastMap<String, FastSet<String>> = FastMap::default();
        for namespace in namespaces.iter().filter(|n| !n.is_empty()) {
            let (parent, last) = namespace
                .rsplit_once('.')
                .unwrap_or(("", namespace.as_str()));
            children
                .entry(parent.to_owned())
                .or_default()
                .insert(last.to_owned());
        }
        let mut extensions: FastMap<String, Vec<usize>> = FastMap::default();
        for (file_index, file) in files.iter().enumerate() {
            for (d, declaration) in file.facts.declarations.iter().enumerate() {
                let Some(&entry) = by_part.get(&(file_index, d)) else {
                    continue;
                };
                for name in &declaration.extensions {
                    let owners = extensions.entry(name.clone()).or_default();
                    if !owners.contains(&entry) {
                        owners.push(entry);
                    }
                }
            }
        }
        Self {
            types,
            members,
            namespaces,
            parts: by_part,
            children,
            extensions,
        }
    }

    /// The entries named `name` in `container` with `arity`.
    fn lookup(&self, container: &str, name: &str, arity: u32) -> Vec<usize> {
        self.members
            .get(container)
            .and_then(|names| names.get(name))
            .map(|entries| {
                entries
                    .iter()
                    .copied()
                    .filter(|&e| self.types[e].arity == arity)
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// The file a type declared in `declared` lands in: the one part with a constructor, else the
/// file named after the type, else the first by path.
fn landing_file(files: &[SourceFile], name: &str, declared: &[(usize, bool)]) -> usize {
    let with_constructor: BTreeSet<usize> = declared
        .iter()
        .filter(|(_, constructor)| *constructor)
        .map(|(file, _)| *file)
        .collect();
    if with_constructor.len() == 1
        && let Some(file) = with_constructor.first()
    {
        return *file;
    }
    let all: BTreeSet<usize> = declared.iter().map(|(file, _)| *file).collect();
    let named = format!("{name}.cs");
    all.iter()
        .copied()
        .find(|&file| {
            files[file]
                .source
                .rsplit('/')
                .next()
                .is_some_and(|f| f.eq_ignore_ascii_case(&named))
        })
        .or_else(|| {
            all.iter()
                .copied()
                .min_by(|&a, &b| files[a].source.cmp(&files[b].source))
        })
        .unwrap_or(0)
}

/// One lookup level: a namespace and the directives written at it.
struct Level<'f> {
    namespace: String,
    usings: Vec<&'f Using>,
}

/// What an alias names, resolved once per file.
enum Aliased {
    Types(Vec<usize>),
    Namespace(String),
    Outside,
}

/// A lookup level with its directives resolved: the namespace, the aliases, and the containers
/// whose types the directives import (a namespace for `using N;`, a type's nested-type key for
/// `using static T;`).
struct Prepared {
    namespace: String,
    aliases: Vec<(String, Aliased)>,
    imports: Vec<String>,
}

/// The lookup levels for a name written in `scope`, innermost namespace first; the global level
/// carries the compilation unit's directives and the project's global ones.
fn levels_of<'f>(facts: &'f FileFacts, scope: usize, global: &[&'f Using]) -> Vec<Level<'f>> {
    let mut out = Vec::new();
    let mut declaration = Some(scope);
    let mut namespace = facts
        .scopes
        .get(scope)
        .map(|s| s.namespace.clone())
        .unwrap_or_default();
    loop {
        let mut usings = Vec::new();
        while let Some(d) = declaration {
            let Some(current) = facts.scopes.get(d) else {
                declaration = None;
                break;
            };
            if d == 0 || current.namespace != namespace {
                break;
            }
            usings.extend(current.usings.iter());
            declaration = current.parent;
        }
        if namespace.is_empty() {
            if let Some(unit) = facts.scopes.first() {
                usings.extend(unit.usings.iter());
            }
            usings.extend(global.iter().copied());
            out.push(Level {
                namespace: String::new(),
                usings,
            });
            return out;
        }
        out.push(Level {
            namespace: namespace.clone(),
            usings,
        });
        namespace = parent_namespace(&namespace).to_owned();
    }
}

struct Resolver<'a> {
    index: &'a Index,
    /// The projects the file being resolved sees; `None` for no restriction.
    sees: Option<&'a [usize]>,
}

impl Resolver<'_> {
    /// [`Index::lookup`], keeping the types the file's project sees.
    fn lookup(&self, container: &str, name: &str, arity: u32) -> Vec<usize> {
        let mut found = self.index.lookup(container, name, arity);
        if let Some(sees) = self.sees {
            found.retain(|&t| {
                self.index.types[t]
                    .project
                    .is_none_or(|p| sees.binary_search(&p).is_ok())
            });
        }
        found
    }

    /// Resolves a level's directives once, for every reference written at that level.
    fn prepare(&self, level: Level<'_>) -> Prepared {
        let mut aliases = Vec::new();
        let mut imports = Vec::new();
        for using in level.usings {
            let segments: Vec<(String, u32)> =
                using.target.iter().map(|t| (t.clone(), 0)).collect();
            match &using.alias {
                Some(alias) => {
                    // The first alias of a name at a level is the one C# binds.
                    if aliases.iter().any(|(a, _): &(String, Aliased)| a == alias) {
                        continue;
                    }
                    let target = if let Some(types) = self.resolve_from(&level.namespace, &segments)
                    {
                        Aliased::Types(types)
                    } else if let Some(namespace) =
                        imported_namespace(self.index, &level.namespace, &using.target)
                    {
                        Aliased::Namespace(namespace)
                    } else {
                        Aliased::Outside
                    };
                    aliases.push((alias.clone(), target));
                }
                None if using.is_static => {
                    // `using static T;` brings T's nested types into scope.
                    for owner in self
                        .resolve_from(&level.namespace, &segments)
                        .unwrap_or_default()
                    {
                        imports.push(self.index.types[owner].dotted.clone());
                    }
                }
                None => imports.push(
                    imported_namespace(self.index, &level.namespace, &using.target)
                        .unwrap_or_else(|| using.target.join(".")),
                ),
            }
        }
        Prepared {
            namespace: level.namespace,
            aliases,
            imports,
        }
    }

    /// The types whose extension method `name` a call on a value could be, found as C# looks
    /// extension methods up (C# 12 § 12.8.10.3): at each level from the innermost namespace out,
    /// the static classes that namespace declares and those its directives bring into scope
    /// (imported namespaces, `using static` types); the first level with a candidate decides.
    /// Without the receiver's type every candidate there is taken.
    fn extension_owners(&self, name: &str, levels: &[Prepared]) -> Vec<usize> {
        let Some(owners) = self.index.extensions.get(name) else {
            return Vec::new();
        };
        let visible: Vec<usize> = owners
            .iter()
            .copied()
            .filter(|&t| {
                self.sees.is_none_or(|sees| {
                    self.index.types[t]
                        .project
                        .is_none_or(|p| sees.binary_search(&p).is_ok())
                })
            })
            .collect();
        for level in levels {
            let found: Vec<usize> = visible
                .iter()
                .copied()
                .filter(|&t| {
                    let entry = &self.index.types[t];
                    entry.container == level.namespace
                        || level
                            .imports
                            .iter()
                            .any(|i| *i == entry.container || *i == entry.dotted)
                })
                .collect();
            if !found.is_empty() {
                return found;
            }
        }
        Vec::new()
    }

    /// A directive's target written in namespace `base`: looked up from `base` outward, the
    /// first namespace where its first segment is a type or a namespace deciding.
    fn resolve_from(&self, base: &str, segments: &[(String, u32)]) -> Option<Vec<usize>> {
        let (first, rest) = segments.split_first()?;
        let found = outward(base)
            .find_map(|namespace| self.in_namespace(namespace, first, !rest.is_empty()))?;
        self.continue_with(found, rest)
    }

    /// A dotted name from the global namespace (`global::`).
    fn resolve_global(&self, segments: &[(String, u32)]) -> Option<Vec<usize>> {
        let (first, rest) = segments.split_first()?;
        let found = self.in_namespace("", first, !rest.is_empty())?;
        self.continue_with(found, rest)
    }

    /// `name` as a member of namespace `namespace`: a type, or (when more follows) a namespace.
    fn in_namespace(
        &self,
        namespace: &str,
        (name, arity): &(String, u32),
        more: bool,
    ) -> Option<Found> {
        let types = self.lookup(namespace, name, *arity);
        if !types.is_empty() {
            return Some(Found::Types(types));
        }
        let is_namespace = more
            && *arity == 0
            && self
                .index
                .children
                .get(namespace)
                .is_some_and(|children| children.contains(name.as_str()));
        is_namespace.then(|| Found::Namespace(join(namespace, name)))
    }

    fn continue_with(&self, mut found: Found, rest: &[(String, u32)]) -> Option<Vec<usize>> {
        for (i, segment) in rest.iter().enumerate() {
            let more = i + 1 < rest.len();
            found = match found {
                Found::Namespace(namespace) => self.in_namespace(&namespace, segment, more)?,
                Found::Types(types) => {
                    let nested: Vec<usize> = types
                        .iter()
                        .flat_map(|&t| {
                            self.lookup(&self.index.types[t].dotted, &segment.0, segment.1)
                        })
                        .collect();
                    if nested.is_empty() {
                        // A constant or an enum member is inlined where it is read, so
                        // `Type.Constant` leaves no reference to the type; any other member
                        // follows the type.
                        let inlined = types
                            .iter()
                            .all(|&t| self.index.types[t].constants.contains(&segment.0));
                        return Some(if inlined { Vec::new() } else { types });
                    }
                    Found::Types(nested)
                }
            };
        }
        match found {
            Found::Types(types) => Some(types),
            Found::Namespace(_) => None,
        }
    }

    /// The first segment of a name, looked up from where it was written.
    fn first(
        &self,
        containers: &[String],
        levels: &[Prepared],
        segment: &(String, u32),
        more: bool,
    ) -> Option<Found> {
        let (name, arity) = segment;
        // The nested types of each enclosing type, innermost first. A type naming itself or an
        // enclosing type is found one container out, or at its namespace's level.
        for container in containers {
            let types = self.lookup(container, name, *arity);
            if !types.is_empty() {
                return Some(Found::Types(types));
            }
        }
        for level in levels {
            if let Some(found) = self.in_namespace(&level.namespace, segment, more) {
                return Some(found);
            }
            if *arity == 0
                && let Some((_, target)) = level.aliases.iter().find(|(alias, _)| alias == name)
            {
                return match target {
                    Aliased::Types(types) => Some(Found::Types(types.clone())),
                    Aliased::Namespace(namespace) => Some(Found::Namespace(namespace.clone())),
                    // An alias of something outside the repository.
                    Aliased::Outside => None,
                };
            }
            let mut imported: Vec<usize> = level
                .imports
                .iter()
                .flat_map(|container| self.lookup(container, name, *arity))
                .collect();
            if !imported.is_empty() {
                imported.sort_unstable();
                imported.dedup();
                return Some(Found::Types(imported));
            }
        }
        None
    }

    /// The types a reference names, when it names any the repository declares.
    fn resolve(
        &self,
        reference: &Reference,
        containers: &[String],
        levels: &[Prepared],
    ) -> Vec<usize> {
        let segments = &reference.segments;
        match reference.qualifier.as_deref() {
            Some("global") => return self.resolve_global(segments).unwrap_or_default(),
            Some(_) => return Vec::new(),
            None => {}
        }
        let Some((first, rest)) = segments.split_first() else {
            return Vec::new();
        };
        let attribute = reference.kind == super::tree_sitter::Position::Attribute;
        let mut candidates = vec![first.clone()];
        if attribute && !first.0.ends_with("Attribute") && rest.is_empty() {
            candidates.insert(0, (format!("{}Attribute", first.0), first.1));
        }
        for candidate in &candidates {
            if let Some(found) = self.first(containers, levels, candidate, !rest.is_empty())
                && let Some(types) = self.continue_with(found, rest)
            {
                return types;
            }
        }
        Vec::new()
    }
}

/// The first reference of one grouped edge: line, column, and the first target in sort order.
type First = (u32, u32, String);

/// From file to (target file, kind) to the first reference.
type Edges = BTreeMap<usize, BTreeMap<(usize, DependencyKind), First>>;

/// Records one edge, keeping the earliest position and the first target name.
fn add_edge(
    edges: &mut Edges,
    from: usize,
    target: &TypeEntry,
    kind: DependencyKind,
    at: (u32, u32),
) {
    if target.file == from {
        return;
    }
    let first = edges
        .entry(from)
        .or_default()
        .entry((target.file, kind))
        .or_insert_with(|| (at.0, at.1, target.full.clone()));
    if at < (first.0, first.1) {
        first.0 = at.0;
        first.1 = at.1;
    }
    if target.full < first.2 {
        first.2.clone_from(&target.full);
    }
}

/// Every file's resolved edges. A reference written at member level in a part of a type that
/// lands elsewhere is the landing file's, as a compiled build attributes it; each such part with
/// code refers to its type, and so to the landing file.
fn resolve_edges(files: &[SourceFile], projects: &[ProjectInfo], index: &Index) -> Edges {
    // Per project: its files' `global using` directives.
    let mut globals: BTreeMap<Option<usize>, Vec<&Using>> = BTreeMap::new();
    for file in files {
        for using in file.facts.scopes.first().iter().flat_map(|s| &s.usings) {
            if using.global {
                globals.entry(file.project).or_default().push(using);
            }
        }
    }
    let empty = Vec::new();
    let mut edges = Edges::new();
    for (file_index, file) in files.iter().enumerate() {
        let global = globals.get(&file.project).unwrap_or(&empty);
        // A file sees the types of its project and of the projects it references, as its
        // compilation does.
        let resolver = Resolver {
            index,
            sees: file
                .project
                .and_then(|p| projects.get(p))
                .map(|p| p.sees.as_slice())
                .filter(|sees| !sees.is_empty()),
        };
        for (d, declaration) in file.facts.declarations.iter().enumerate() {
            if let Some(&entry) = index.parts.get(&(file_index, d))
                && declaration.bodies
            {
                let at = (declaration.line, 1);
                add_edge(
                    &mut edges,
                    file_index,
                    &index.types[entry],
                    DependencyKind::Body,
                    at,
                );
            }
        }
        resolve_file(&mut edges, &resolver, (file_index, file), global);
    }
    edges
}

/// Each declaration's enclosing containers, innermost first, as dotted keys.
fn enclosing_containers(facts: &FileFacts) -> Vec<Vec<String>> {
    facts
        .declarations
        .iter()
        .map(|d| {
            let mut chain: Vec<(String, u32)> = d.outer.clone();
            chain.push((d.name.clone(), d.arity));
            (1..=chain.len())
                .rev()
                .filter_map(|n| {
                    chain[..n]
                        .split_last()
                        .map(|(own, rest)| dotted_key(&d.namespace, rest, &own.0))
                })
                .collect()
        })
        .collect()
}

/// One file's references and calls, resolved into `edges`. A reference or call written at member
/// level in a part of a type that lands elsewhere is the landing file's.
fn resolve_file(
    edges: &mut Edges,
    resolver: &Resolver<'_>,
    (file_index, file): (usize, &SourceFile),
    global: &[&Using],
) {
    let index = resolver.index;
    let scoped: Vec<Vec<Prepared>> = (0..file.facts.scopes.len())
        .map(|scope| {
            levels_of(&file.facts, scope, global)
                .into_iter()
                .map(|level| resolver.prepare(level))
                .collect()
        })
        .collect();
    let enclosing = enclosing_containers(&file.facts);
    let from_of = |declaration: Option<usize>, member: bool| {
        let landing = declaration
            .and_then(|d| index.parts.get(&(file_index, d)))
            .map(|&entry| index.types[entry].file);
        match landing {
            Some(file) if member => file,
            _ => file_index,
        }
    };
    for reference in &file.facts.references {
        let Some(levels) = scoped.get(reference.scope) else {
            continue;
        };
        let from = from_of(reference.enclosing, reference.member);
        let containers = reference
            .enclosing
            .and_then(|e| enclosing.get(e))
            .map_or(&[][..], Vec::as_slice);
        for target in resolver.resolve(reference, containers, levels) {
            let entry = &index.types[target];
            let kind = reference.kind.dependency_kind(entry.kind);
            add_edge(edges, from, entry, kind, (reference.line, reference.column));
        }
    }
    for call in &file.facts.calls {
        let Some(levels) = scoped.get(call.scope) else {
            continue;
        };
        let from = from_of(call.enclosing, call.member);
        for owner in resolver.extension_owners(&call.name, levels) {
            let at = (call.line, call.column);
            add_edge(edges, from, &index.types[owner], DependencyKind::Body, at);
        }
    }
}

/// One file's edges to other files, with compiled mode's dependency types.
fn file_dependencies(
    files: &[SourceFile],
    file: &SourceFile,
    project: Option<&ProjectInfo>,
    grouped: BTreeMap<(usize, DependencyKind), First>,
) -> Vec<Dependency> {
    let mut kinds_by_target: BTreeMap<usize, BTreeSet<DependencyKind>> = BTreeMap::new();
    for (target, kind) in grouped.keys() {
        kinds_by_target.entry(*target).or_default().insert(*kind);
    }
    let mut dependencies = Vec::new();
    for ((target, kind), (line, column, name)) in grouped {
        let target_file = &files[target];
        let mut types = vec![if target_file.project == file.project {
            DependencyType::Local
        } else {
            DependencyType::Project
        }];
        if project.is_some_and(|p| p.is_test) {
            types.push(DependencyType::TestOnly);
        }
        if kinds_by_target
            .get(&target)
            .is_some_and(|k| k.iter().all(|k| *k == DependencyKind::Signature))
        {
            types.push(DependencyType::SignatureOnly);
        }
        let mut dependency = Dependency::new(name, target_file.source.clone(), ModuleSystem::Clr);
        dependency.dependency_types = types;
        dependency.followable = true;
        dependency.line = Some(line);
        dependency.column = Some(column);
        dependency.dependency_kind = Some(kind);
        dependency.approximate = Some(true);
        dependencies.push(dependency);
    }
    dependencies
}

/// The namespaces a file's directives import that no file declares, each an edge to an
/// external module, classified; `externals` collects each module's classification.
fn external_imports(
    file: &SourceFile,
    project: Option<&ProjectInfo>,
    index: &Index,
    externals: &mut BTreeMap<String, DependencyType>,
) -> Vec<Dependency> {
    let mut imports: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    for scope in &file.facts.scopes {
        for using in &scope.usings {
            let target = using.target.join(".");
            if target.is_empty()
                || imported_namespace(index, &scope.namespace, &using.target).is_some()
            {
                continue;
            }
            let head = if using.is_static || using.alias.is_some() {
                // A type outside the repository; its namespace is the external module.
                let (namespace, _) = target.rsplit_once('.').unwrap_or((target.as_str(), ""));
                namespace.to_owned()
            } else {
                target
            };
            let head_segments: Vec<String> = head.split('.').map(str::to_owned).collect();
            if head.is_empty()
                || imported_namespace(index, &scope.namespace, &head_segments).is_some()
            {
                continue;
            }
            imports.entry(head).or_insert((using.line, using.column));
        }
    }
    let packages = project.map_or(&[][..], |p| p.package_refs.as_slice());
    imports
        .into_iter()
        .map(|(namespace, (line, column))| {
            let kind = if providing_package(&namespace, packages).is_some() {
                DependencyType::Package
            } else if is_framework(&namespace) {
                DependencyType::Framework
            } else {
                DependencyType::Undetermined
            };
            externals.entry(namespace.clone()).or_insert(kind);
            let mut types = vec![kind];
            if project.is_some_and(|p| p.is_test) {
                types.push(DependencyType::TestOnly);
            }
            let mut dependency = Dependency::new(namespace.clone(), namespace, ModuleSystem::Clr);
            dependency.dependency_types = types;
            dependency.core_module = kind == DependencyType::Framework;
            dependency.line = Some(line);
            dependency.column = Some(column);
            dependency.dependency_kind = Some(DependencyKind::Import);
            dependency.approximate = Some(true);
            dependency
        })
        .collect()
}

/// A file's module: its project, its types' namespaces, `attribution: source`.
fn file_module(
    file: &SourceFile,
    project: Option<&ProjectInfo>,
    mut dependencies: Vec<Dependency>,
) -> Module {
    dependencies.sort_by(|a, b| {
        (&a.resolved, a.dependency_kind, &a.module).cmp(&(
            &b.resolved,
            b.dependency_kind,
            &b.module,
        ))
    });
    let mut module = Module::new(file.source.clone());
    module.language = Some(Language::Dotnet);
    module.project = project.map(|p| p.path.clone());
    let namespaces: BTreeSet<String> = file
        .facts
        .declarations
        .iter()
        .map(|d| d.namespace.clone())
        .collect();
    if !namespaces.is_empty() {
        module.namespaces = Some(namespaces.into_iter().collect());
    }
    module.attribution = Some(Attribution::Source);
    module.followable = Some(true);
    module.dependencies = dependencies;
    module
}

/// Builds the modules: one per file that declares a type or holds top-level statements, with
/// its resolved edges, and one per external namespace a `using` directive names.
pub fn modules(files: &[SourceFile], projects: &[ProjectInfo]) -> Vec<Module> {
    let index = Index::build(files);
    let mut edges = resolve_edges(files, projects, &index);
    let mut modules = Vec::new();
    let mut externals: BTreeMap<String, DependencyType> = BTreeMap::new();
    for (file_index, file) in files.iter().enumerate() {
        if file.facts.declarations.is_empty() && !file.facts.top_level {
            continue;
        }
        let project = file.project.and_then(|p| projects.get(p));
        let grouped = edges.remove(&file_index).unwrap_or_default();
        let mut dependencies = file_dependencies(files, file, project, grouped);
        dependencies.extend(external_imports(file, project, &index, &mut externals));
        modules.push(file_module(file, project, dependencies));
    }
    let sources: HashSet<String> = modules.iter().map(|m| m.source.clone()).collect();
    for (name, kind) in externals {
        if sources.contains(&name) {
            continue;
        }
        let mut module = Module::new(name);
        module.language = Some(Language::Dotnet);
        module.followable = Some(false);
        module.core_module = Some(kind == DependencyType::Framework);
        module.could_not_resolve = Some(false);
        module.dependency_types = Some(vec![kind]);
        modules.push(module);
    }
    modules.sort_by(|a, b| a.source.cmp(&b.source));
    modules
}

#[cfg(test)]
mod tests {
    use super::super::tree_sitter::{CSharpParser, ParseError};
    use super::*;

    fn files(sources: &[(&str, Option<usize>, &str)]) -> Result<Vec<SourceFile>, ParseError> {
        let mut parser = CSharpParser::new()?;
        sources
            .iter()
            .map(|(source, project, text)| {
                Ok(SourceFile {
                    source: (*source).to_owned(),
                    project: *project,
                    facts: parser.parse(text)?,
                })
            })
            .collect()
    }

    fn edges(modules: &[Module]) -> BTreeSet<(String, String, String)> {
        modules
            .iter()
            .flat_map(|m| {
                m.dependencies.iter().map(|d| {
                    (
                        m.source.clone(),
                        d.resolved.clone(),
                        d.dependency_kind.map_or("", |k| k.as_str()).to_owned(),
                    )
                })
            })
            .collect()
    }

    fn has(modules: &[Module], from: &str, to: &str) -> bool {
        edges(modules).iter().any(|(f, t, _)| f == from && t == to)
    }

    #[test]
    fn usings_nesting_and_the_enclosing_namespace_resolve() -> Result<(), ParseError> {
        let found = files(&[
            (
                "a/Order.cs",
                Some(0),
                "using Shop.Customers;\nnamespace Shop.Orders;\npublic class Order { Customer c; Line l; }",
            ),
            (
                "a/Line.cs",
                Some(0),
                "namespace Shop.Orders { public class Line {} }",
            ),
            (
                "b/Customer.cs",
                Some(1),
                "namespace Shop.Customers { public class Customer {} }",
            ),
            (
                "b/Other.cs",
                Some(1),
                "namespace Elsewhere { public class Line {} }",
            ),
        ])?;
        let projects = vec![ProjectInfo::default(), ProjectInfo::default()];
        let modules = modules(&found, &projects);
        assert!(has(&modules, "a/Order.cs", "b/Customer.cs"));
        assert!(has(&modules, "a/Order.cs", "a/Line.cs"));
        assert!(!has(&modules, "a/Order.cs", "b/Other.cs"));
        let customer = modules
            .iter()
            .find(|m| m.source == "a/Order.cs")
            .and_then(|m| {
                m.dependencies
                    .iter()
                    .find(|d| d.resolved == "b/Customer.cs")
            });
        assert_eq!(
            customer.map(|d| (d.dependency_types.clone(), d.approximate, d.dependency_kind)),
            Some((
                vec![DependencyType::Project],
                Some(true),
                Some(DependencyKind::Field)
            ))
        );
        Ok(())
    }

    #[test]
    fn global_usings_reach_every_file_of_their_project_only() -> Result<(), ParseError> {
        let found = files(&[
            ("p/GlobalUsings.cs", Some(0), "global using Lib;"),
            ("p/Use.cs", Some(0), "namespace P; class Use { Thing t; }"),
            ("q/Use.cs", Some(1), "namespace Q; class Use { Thing t; }"),
            (
                "lib/Thing.cs",
                Some(2),
                "namespace Lib; public class Thing {}",
            ),
        ])?;
        let modules = modules(
            &found,
            &[
                ProjectInfo::default(),
                ProjectInfo::default(),
                ProjectInfo::default(),
            ],
        );
        assert!(has(&modules, "p/Use.cs", "lib/Thing.cs"));
        assert!(!has(&modules, "q/Use.cs", "lib/Thing.cs"));
        // A file of directives only is not a module.
        assert!(!modules.iter().any(|m| m.source == "p/GlobalUsings.cs"));
        Ok(())
    }

    #[test]
    fn aliases_statics_global_qualifiers_and_nested_types_resolve() -> Result<(), ParseError> {
        let found = files(&[
            (
                "Use.cs",
                None,
                "using T = Lib.Deep.Target;\nusing static Lib.Holder;\nclass Use { T a; Inner b; global::Lib.Other c; Lib.Holder.Inner d; }",
            ),
            ("Target.cs", None, "namespace Lib.Deep { class Target {} }"),
            (
                "Holder.cs",
                None,
                "namespace Lib { static class Holder { public class Inner {} } }",
            ),
            ("Other.cs", None, "namespace Lib { class Other {} }"),
        ])?;
        let modules = modules(&found, &[]);
        assert!(has(&modules, "Use.cs", "Target.cs"));
        assert!(has(&modules, "Use.cs", "Holder.cs"));
        assert!(has(&modules, "Use.cs", "Other.cs"));
        Ok(())
    }

    #[test]
    fn attributes_find_the_suffixed_type_and_bases_split_by_kind() -> Result<(), ParseError> {
        let found = files(&[
            (
                "Use.cs",
                None,
                "namespace N; [Audit] class Use : Base, IThing {}",
            ),
            (
                "Audit.cs",
                None,
                "namespace N; class AuditAttribute : System.Attribute {}",
            ),
            ("Base.cs", None, "namespace N; class Base {}"),
            ("IThing.cs", None, "namespace N; interface IThing {}"),
        ])?;
        let all = edges(&modules(&found, &[]));
        assert!(all.contains(&("Use.cs".into(), "Audit.cs".into(), "attribute".into())));
        assert!(all.contains(&("Use.cs".into(), "Base.cs".into(), "inherits".into())));
        assert!(all.contains(&("Use.cs".into(), "IThing.cs".into(), "implements".into())));
        Ok(())
    }

    #[test]
    fn arity_must_match_and_unknown_names_are_not_edges() -> Result<(), ParseError> {
        let found = files(&[
            (
                "Use.cs",
                None,
                "namespace N; class Use { Box<int> a; Missing m; }",
            ),
            ("Box.cs", None, "namespace N; class Box {}"),
            ("Box1.cs", None, "namespace N; class Box<T> {}"),
        ])?;
        let modules = modules(&found, &[]);
        assert!(has(&modules, "Use.cs", "Box1.cs"));
        assert!(!has(&modules, "Use.cs", "Box.cs"));
        let generic = modules
            .iter()
            .flat_map(|m| &m.dependencies)
            .find(|d| d.resolved == "Box1.cs");
        assert_eq!(generic.map(|d| d.module.as_str()), Some("N.Box`1"));
        Ok(())
    }

    #[test]
    fn a_partial_type_lands_in_the_part_with_its_constructor() -> Result<(), ParseError> {
        let found = files(&[
            (
                "Order.Lines.cs",
                None,
                "namespace N; partial class Order { void Add() {} }",
            ),
            (
                "Order.cs",
                None,
                "namespace N; partial class Order { public Order() {} }",
            ),
            ("Use.cs", None, "namespace N; class Use { Order o; }"),
        ])?;
        let modules = modules(&found, &[]);
        assert!(has(&modules, "Use.cs", "Order.cs"));
        assert!(!has(&modules, "Use.cs", "Order.Lines.cs"));
        Ok(())
    }

    #[test]
    fn external_usings_are_classified_as_compiled_mode_classifies_assemblies()
    -> Result<(), ParseError> {
        let found = files(&[(
            "Use.cs",
            Some(0),
            "using System.Text;\nusing Serilog.Sinks;\nusing Acme.Unknown;\nusing N;\nnamespace N; class Use {}",
        )])?;
        let projects = vec![ProjectInfo {
            path: "p.csproj".into(),
            is_test: false,
            sees: Vec::new(),
            package_refs: vec![PackageRef {
                id: "Serilog".into(),
                version: None,
            }],
        }];
        let modules = modules(&found, &projects);
        let kinds: BTreeMap<String, Vec<DependencyType>> = modules
            .iter()
            .flat_map(|m| &m.dependencies)
            .map(|d| (d.resolved.clone(), d.dependency_types.clone()))
            .collect();
        assert_eq!(
            kinds.get("System.Text"),
            Some(&vec![DependencyType::Framework])
        );
        assert_eq!(
            kinds.get("Serilog.Sinks"),
            Some(&vec![DependencyType::Package])
        );
        assert_eq!(
            kinds.get("Acme.Unknown"),
            Some(&vec![DependencyType::Undetermined])
        );
        // A namespace the repository declares is no external module.
        assert!(!kinds.contains_key("N"));
        let external = modules.iter().find(|m| m.source == "Serilog.Sinks");
        assert_eq!(external.and_then(|m| m.followable), Some(false));
        assert!(
            modules
                .iter()
                .all(|m| m.dependencies.iter().all(|d| d.approximate == Some(true)))
        );
        Ok(())
    }

    #[test]
    fn a_using_inside_a_namespace_names_its_target_from_there_outward() -> Result<(), ParseError> {
        let found = files(&[
            (
                "Use.cs",
                None,
                "namespace App.Orders.Data;\nusing Common.Rules;\nusing Alias = Common.Rules.Rule;\nclass Use { Rule a; Alias b; }",
            ),
            (
                "Rule.cs",
                None,
                "namespace App.Common.Rules; public class Rule {}",
            ),
        ])?;
        let modules = modules(&found, &[]);
        assert!(has(&modules, "Use.cs", "Rule.cs"));
        // Nothing external is invented for the relative name.
        assert!(!modules.iter().any(|m| m.source == "Common.Rules"));
        Ok(())
    }

    #[test]
    fn a_file_sees_only_the_types_its_project_and_its_references_declare() -> Result<(), ParseError>
    {
        // Two copies of one library, as a repository with several solutions has them.
        let found = files(&[
            (
                "a/Use.cs",
                Some(0),
                "namespace App; class Use { Lib.Thing t; }",
            ),
            (
                "a-lib/Thing.cs",
                Some(1),
                "namespace Lib; public class Thing {}",
            ),
            (
                "b-lib/Thing.cs",
                Some(2),
                "namespace Lib; public class Thing {}",
            ),
            (
                "b/Use.cs",
                Some(3),
                "namespace App; class Other { Lib.Thing t; }",
            ),
        ])?;
        let project = |sees: Vec<usize>| ProjectInfo {
            sees,
            ..ProjectInfo::default()
        };
        let projects = vec![
            project(vec![0, 1]),
            project(vec![1]),
            project(vec![2]),
            project(vec![2, 3]),
        ];
        let modules = modules(&found, &projects);
        assert!(has(&modules, "a/Use.cs", "a-lib/Thing.cs"));
        assert!(!has(&modules, "a/Use.cs", "b-lib/Thing.cs"));
        assert!(has(&modules, "b/Use.cs", "b-lib/Thing.cs"));
        assert!(!has(&modules, "b/Use.cs", "a-lib/Thing.cs"));
        Ok(())
    }

    #[test]
    fn an_extension_method_call_lands_in_the_class_in_scope_that_declares_it()
    -> Result<(), ParseError> {
        let found = files(&[
            (
                "Use.cs",
                None,
                "using Lib.Extensions;\nnamespace App; class Use { void M(Thing t) { t.Shout(); t?.Whisper(); t.Unknown(); } }",
            ),
            (
                "Loud.cs",
                None,
                "namespace Lib.Extensions; public static class Loud { public static void Shout(this Thing t) {} }",
            ),
            (
                "Quiet.cs",
                None,
                "namespace App; public static class Quiet { public static void Whisper(this Thing t) {} }",
            ),
            (
                "Far.cs",
                None,
                "namespace Elsewhere; public static class Far { public static void Shout(this Thing t) {} }",
            ),
        ])?;
        let modules = modules(&found, &[]);
        assert!(has(&modules, "Use.cs", "Loud.cs"), "an imported namespace");
        assert!(
            has(&modules, "Use.cs", "Quiet.cs"),
            "the enclosing namespace"
        );
        assert!(
            !has(&modules, "Use.cs", "Far.cs"),
            "a namespace not in scope"
        );
        Ok(())
    }

    #[test]
    fn a_constant_or_an_enum_member_read_is_no_edge() -> Result<(), ParseError> {
        let found = files(&[
            (
                "Use.cs",
                None,
                "namespace N; class Use { string a = Paths.Root; int b = (int)Tier.Gold; void M() { Helper.Run(); } }",
            ),
            (
                "Paths.cs",
                None,
                "namespace N; static class Paths { public const string Root = \"/\"; }",
            ),
            ("Tier.cs", None, "namespace N; enum Tier { Basic, Gold }"),
            (
                "Helper.cs",
                None,
                "namespace N; static class Helper { public static void Run() {} }",
            ),
        ])?;
        let modules = modules(&found, &[]);
        assert!(!has(&modules, "Use.cs", "Paths.cs"));
        assert!(!has(&modules, "Use.cs", "Tier.cs"));
        assert!(has(&modules, "Use.cs", "Helper.cs"));
        Ok(())
    }

    #[test]
    fn outward_names_every_enclosing_namespace_then_the_global_one() {
        assert_eq!(
            outward("A.B.C").collect::<Vec<_>>(),
            ["A.B.C", "A.B", "A", ""]
        );
        assert_eq!(outward("").collect::<Vec<_>>(), [""]);
    }

    #[test]
    fn fnv_is_fnv_1a() {
        // The published FNV-1a 64-bit vectors: the empty string and "a".
        let mut empty = Fnv::default();
        empty.write(b"");
        assert_eq!(empty.finish(), 0xcbf2_9ce4_8422_2325);
        let mut a = Fnv::default();
        a.write(b"a");
        assert_eq!(a.finish(), 0xaf63_dc4c_8601_ec8c);
        let mut split = Fnv::default();
        split.write(b"a");
        split.write(b"b");
        let mut whole = Fnv::default();
        whole.write(b"ab");
        assert_eq!(split.finish(), whole.finish());
    }

    #[test]
    fn metadata_names_nest_with_plus_and_carry_arity() {
        assert_eq!(
            metadata_name("A.B", &[("Outer".into(), 1)], "Inner", 2),
            "A.B.Outer`1+Inner`2"
        );
        assert_eq!(metadata_name("", &[], "Program", 0), "Program");
    }
}
