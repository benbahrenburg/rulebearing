//! Which type a C# name means: the declarations `import archunit` has read, the assembly each
//! belongs to, and C#'s lookup order for a name written in a namespace with using directives.
//!
//! - Plan: [Wave 2, Step 11](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f)
//!   ("`typeof(X)` becomes the full name, resolved from using directives and namespace
//!   declarations; a type whose namespace cannot be determined is emitted commented out")
//! - Requirement: [FR-CLI-04](../../../../../docs/prd.md#fr-cli-04)
//!
//! A written name is looked up as C# does: inside the enclosing types, then in the enclosing
//! namespaces from the innermost out, then through the using directives (the file's and every
//! `global using`), where two candidates are an ambiguity rather than a choice. A name found
//! nowhere is an error naming it, never a guess: the importer comments the rule out with that
//! reason. A type's assembly is the `AssemblyName` of the nearest `.csproj` above its file, or
//! the project file's name, and that project's folder is where the loader finds its build.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::csharp::{SourceFile, TypeDecl, TypeName, Usings, arity_name};

/// What the importer knows of a type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypeInfo {
    /// The metadata full name, as the graph spells it (`Ns.Outer+Inner`, `` Ns.List`1 ``).
    pub full: String,
    /// The simple name, with arity (`` List`1 ``), as `Type.Name` gives it.
    pub name: String,
    /// The namespace.
    pub namespace: String,
    /// The simple assembly name, when known.
    pub assembly: Option<String>,
}

/// The C# keywords that name types.
const KEYWORDS: &[(&str, &str)] = &[
    ("bool", "System.Boolean"),
    ("byte", "System.Byte"),
    ("sbyte", "System.SByte"),
    ("char", "System.Char"),
    ("decimal", "System.Decimal"),
    ("double", "System.Double"),
    ("float", "System.Single"),
    ("int", "System.Int32"),
    ("uint", "System.UInt32"),
    ("long", "System.Int64"),
    ("ulong", "System.UInt64"),
    ("short", "System.Int16"),
    ("ushort", "System.UInt16"),
    ("object", "System.Object"),
    ("string", "System.String"),
    ("void", "System.Void"),
    ("nint", "System.IntPtr"),
    ("nuint", "System.UIntPtr"),
];

/// Every type declaration read, by C# path, and every project's assembly.
#[derive(Debug, Clone, Default)]
pub struct Index {
    by_path: BTreeMap<String, BTreeSet<TypeInfo>>,
    /// Assembly name to its project folder, relative to the working directory.
    projects: BTreeMap<String, String>,
    /// Source file (absolute) to its assembly name.
    file_assembly: BTreeMap<PathBuf, String>,
    /// `global using` directives from every file read.
    pub global_usings: Usings,
}

impl Index {
    /// Adds a type under its C# path. The same full name read twice is one type: an assembly
    /// learnt from either reading is kept.
    fn insert(&mut self, path: String, info: TypeInfo) {
        let set = self.by_path.entry(path).or_default();
        if let Some(existing) = set.iter().find(|t| t.full == info.full).cloned() {
            if existing.assembly.is_some() || info.assembly.is_none() {
                return;
            }
            set.remove(&existing);
        }
        set.insert(info);
    }

    /// Adds a declaration from a source file.
    pub fn add_decl(&mut self, decl: &TypeDecl, assembly: Option<&str>) {
        let info = TypeInfo {
            name: arity_name(&decl.name, decl.arity),
            namespace: decl.namespace.clone(),
            full: decl.full_name(),
            assembly: assembly.map(str::to_owned),
        };
        self.insert(decl.dotted(), info);
    }

    /// Adds a type known by its metadata full name (from a graph document, say).
    pub fn add_known(&mut self, full: &str, namespace: &str, assembly: Option<&str>) {
        let name = full.rsplit(['.', '+']).next().unwrap_or(full).to_owned();
        let info = TypeInfo {
            full: full.to_owned(),
            name,
            namespace: namespace.to_owned(),
            assembly: assembly.map(str::to_owned),
        };
        self.insert(full.replace('+', "."), info);
    }

    /// Records the project that builds an assembly.
    pub fn add_project(&mut self, assembly: &str, dir: &str) {
        self.projects
            .entry(assembly.to_owned())
            .or_insert_with(|| dir.to_owned());
    }

    /// The folder of the project that builds `assembly`.
    pub fn project_of(&self, assembly: &str) -> Option<&str> {
        self.projects.get(assembly).map(String::as_str)
    }

    /// Records a source file's assembly.
    pub fn add_file(&mut self, file: &Path, assembly: &str) {
        self.file_assembly
            .insert(file.to_path_buf(), assembly.to_owned());
    }

    /// The assembly a source file is compiled into.
    pub fn assembly_of_file(&self, file: &Path) -> Option<&str> {
        self.file_assembly.get(file).map(String::as_str)
    }

    fn one(&self, path: &str) -> Option<&BTreeSet<TypeInfo>> {
        self.by_path.get(path)
    }

    /// Resolves a written type name.
    ///
    /// # Errors
    /// A sentence naming the type when it is not found or is ambiguous.
    pub fn resolve(
        &self,
        name: &TypeName,
        namespace: &str,
        enclosing: &[String],
        usings: &Usings,
    ) -> Result<TypeInfo, String> {
        if let [(single, 0)] = name.segments.as_slice()
            && let Some((_, full)) = KEYWORDS.iter().find(|(k, _)| k == single)
        {
            return Ok(TypeInfo {
                full: (*full).to_owned(),
                name: full.trim_start_matches("System.").to_owned(),
                namespace: "System".into(),
                assembly: None,
            });
        }
        let mut segments: Vec<String> = name
            .segments
            .iter()
            .map(|(n, a)| arity_name(n, *a))
            .collect();
        if !name.global
            && let Some((_, target)) = usings
                .aliases
                .iter()
                .chain(self.global_usings.aliases.iter())
                .find(|(alias, _)| Some(alias) == segments.first())
        {
            let mut replaced: Vec<String> = target.split('.').map(str::to_owned).collect();
            replaced.extend(segments.drain(1..));
            segments = replaced;
        }
        let written = segments.join(".");
        let unique = |set: &BTreeSet<TypeInfo>| -> Result<TypeInfo, String> {
            match set.len() {
                1 => set.iter().next().cloned().ok_or_else(String::new),
                _ => Err(format!(
                    "`{}` names types in several assemblies ({})",
                    name.text,
                    set.iter()
                        .map(|t| t.assembly.clone().unwrap_or_default())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            }
        };
        if name.global {
            return self.one(&written).map_or_else(
                || {
                    Err(format!(
                        "`{}` is not declared in the sources read",
                        name.text
                    ))
                },
                unique,
            );
        }
        // Enclosing types, innermost first, then the namespaces from the innermost out.
        let mut scopes: Vec<String> = enclosing.iter().rev().cloned().collect();
        let mut ns = namespace.to_owned();
        loop {
            scopes.push(ns.clone());
            match ns.rfind('.') {
                Some(at) => ns.truncate(at),
                None if !ns.is_empty() => ns.clear(),
                None => break,
            }
        }
        for scope in &scopes {
            let path = if scope.is_empty() {
                written.clone()
            } else {
                format!("{scope}.{written}")
            };
            if let Some(set) = self.one(&path) {
                return unique(set);
            }
        }
        self.through_usings(name, &written, &segments, usings)
    }

    /// A written name found through the using directives: one candidate, or an error naming
    /// the ambiguity or the missing namespace.
    fn through_usings(
        &self,
        name: &TypeName,
        written: &str,
        segments: &[String],
        usings: &Usings,
    ) -> Result<TypeInfo, String> {
        let mut found = BTreeSet::new();
        for using in usings
            .namespaces
            .iter()
            .chain(usings.statics.iter())
            .chain(self.global_usings.namespaces.iter())
            .chain(self.global_usings.statics.iter())
        {
            if let Some(set) = self.one(&format!("{using}.{written}")) {
                found.extend(set.iter().cloned());
            }
        }
        match found.len() {
            // `System.IDisposable` written out: the platform's namespaces are not in the sources,
            // and a name written from `System` or `Microsoft` names its namespace itself.
            0 if segments.len() > 1 && matches!(segments[0].as_str(), "System" | "Microsoft") => {
                Ok(TypeInfo {
                    full: written.to_owned(),
                    name: segments.last().cloned().unwrap_or_default(),
                    namespace: segments[..segments.len() - 1].join("."),
                    assembly: None,
                })
            }
            0 => Err(format!(
                "the namespace of `{}` cannot be determined from the sources read (pass the folder that declares it with --sources)",
                name.text
            )),
            1 => found.into_iter().next().ok_or_else(String::new),
            _ => Err(format!(
                "`{}` is ambiguous between {}",
                name.text,
                found
                    .iter()
                    .map(|t| t.full.clone())
                    .collect::<Vec<_>>()
                    .join(" and ")
            )),
        }
    }
}

/// Folders never read.
const SKIPPED: &[&str] = &["bin", "obj", "node_modules", "packages", "TestResults"];

/// Every file under `dir` with extension `ext`, sorted, skipping build output and hidden folders.
pub fn walk(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !name.starts_with('.') && !SKIPPED.contains(&name.as_str()) {
                    stack.push(path);
                }
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(ext))
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// The folder whose declarations resolve names by default: the nearest folder at or above `dir`
/// holding a `.sln` or `.slnx`, not leaving the git repository; `dir` itself when there is none.
pub fn solution_root(dir: &Path) -> PathBuf {
    for candidate in dir.ancestors() {
        let has_solution = std::fs::read_dir(candidate).is_ok_and(|entries| {
            entries.flatten().any(|e| {
                e.path().extension().is_some_and(|x| {
                    x.eq_ignore_ascii_case("sln") || x.eq_ignore_ascii_case("slnx")
                })
            })
        });
        if has_solution {
            return candidate.to_path_buf();
        }
        if candidate.join(".git").exists() {
            break;
        }
    }
    dir.to_path_buf()
}

/// The project file above `file`: its assembly name and folder.
pub fn project_of(file: &Path, stop: &Path) -> Option<(String, PathBuf)> {
    for dir in file.ancestors().skip(1) {
        let mut projects: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension()
                            .is_some_and(|x| x.eq_ignore_ascii_case("csproj"))
                    })
                    .collect()
            })
            .unwrap_or_default();
        projects.sort();
        if let Some(project) = projects.first() {
            let text = std::fs::read_to_string(project).unwrap_or_default();
            let name = assembly_name(&text).unwrap_or_else(|| {
                project
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
            return Some((name, dir.to_path_buf()));
        }
        if dir == stop {
            break;
        }
    }
    None
}

/// `<AssemblyName>` in a project file, when it is a literal.
fn assembly_name(project: &str) -> Option<String> {
    let start = project.find("<AssemblyName>")? + "<AssemblyName>".len();
    let end = project[start..].find("</AssemblyName>")? + start;
    let name = project[start..end].trim();
    (!name.is_empty() && !name.contains("$(")).then(|| name.to_owned())
}

impl Index {
    /// Adds a file's declarations, in `assembly`, and its `global using` directives.
    pub fn add_source(&mut self, file: &SourceFile, assembly: Option<&str>) {
        for decl in &file.types {
            self.add_decl(decl, assembly);
        }
        let globals = &file.global_usings;
        self.global_usings
            .namespaces
            .extend(globals.namespaces.iter().cloned());
        self.global_usings
            .statics
            .extend(globals.statics.iter().cloned());
        self.global_usings
            .aliases
            .extend(globals.aliases.iter().cloned());
    }
}

/// Adds every declaration of `files` to `index`, with the assemblies of their projects.
pub fn index_files(index: &mut Index, files: &[(PathBuf, SourceFile)], stop: &Path, cwd: &Path) {
    for (path, file) in files {
        let project = project_of(path, stop);
        if let Some((assembly, dir)) = &project {
            let shown = dir
                .strip_prefix(cwd)
                .map_or_else(
                    |_| dir.to_string_lossy().into_owned(),
                    |r| r.to_string_lossy().into_owned(),
                )
                .replace('\\', "/");
            index.add_project(assembly, &shown);
            index.add_file(path, assembly);
        }
        index.add_source(file, project.as_ref().map(|(a, _)| a.as_str()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::import::csharp::parse;

    fn name(text: &str) -> TypeName {
        let global = text.starts_with("global::");
        let body = text.trim_start_matches("global::");
        TypeName {
            segments: body
                .split('.')
                .map(|s| match s.split_once('<') {
                    Some((n, rest)) => (n.to_owned(), rest.matches(',').count() + 1),
                    None => (s.to_owned(), 0),
                })
                .collect(),
            global,
            text: text.to_owned(),
        }
    }

    fn index() -> Result<Index, crate::cmd::import::ImportError> {
        let mut index = Index::default();
        let a = parse(
            "namespace App.Domain { public class Order { public class Line {} } public class Repo<T> {} }\nnamespace App.Web { public class Order {} }\nnamespace Lib { public class Order {} public class Only {} }",
            "a.cs",
        )?;
        for decl in &a.types {
            index.add_decl(decl, Some("App"));
        }
        index.add_known("Ext.Thing+Nested", "Ext", None);
        index.add_known("Ext.Thing+Nested", "Ext", Some("Ext"));
        index.add_known("Ext.Thing+Nested", "Ext", None);
        Ok(index)
    }

    #[test]
    fn names_resolve_in_csharp_order() -> Result<(), crate::cmd::import::ImportError> {
        let index = index()?;
        let usings = Usings {
            namespaces: vec!["Lib".into(), "App.Domain".into()],
            statics: Vec::new(),
            aliases: vec![("D".into(), "App.Domain".into())],
        };
        let resolve = |text: &str, ns: &str| index.resolve(&name(text), ns, &[], &usings);
        assert_eq!(
            resolve("Order", "App.Web.Controllers").map(|t| t.full),
            Ok("App.Web.Order".into())
        );
        assert!(resolve("Order", "Tests").is_err_and(|e| e.contains("ambiguous")));
        assert_eq!(
            resolve("Only", "Tests").map(|t| t.full),
            Ok("Lib.Only".into())
        );
        assert_eq!(
            resolve("Order.Line", "App.Domain").map(|t| t.full),
            Ok("App.Domain.Order+Line".into())
        );
        assert_eq!(
            resolve("D.Order", "Tests").map(|t| t.full),
            Ok("App.Domain.Order".into())
        );
        assert_eq!(
            resolve("Domain.Repo<>", "App").map(|t| t.full),
            Ok("App.Domain.Repo`1".into())
        );
        assert_eq!(
            resolve("Repo<>", "Tests").map(|t| t.full),
            Ok("App.Domain.Repo`1".into())
        );
        assert_eq!(
            resolve("global::Ext.Thing.Nested", "X").map(|t| (t.full, t.assembly)),
            Ok(("Ext.Thing+Nested".into(), Some("Ext".into())))
        );
        assert!(resolve("global::Nope.X", "X").is_err());
        assert!(resolve("Missing", "X").is_err_and(|e| e.contains("cannot be determined")));
        assert!(resolve("Lib.Missing", "X").is_err());
        assert_eq!(
            resolve("System.IDisposable", "X").map(|t| (t.full, t.namespace)),
            Ok(("System.IDisposable".into(), "System".into()))
        );
        assert_eq!(
            resolve("string", "X").map(|t| t.full),
            Ok("System.String".into())
        );
        let nested = index.resolve(
            &name("Line"),
            "App.Domain",
            &["App.Domain.Order".into()],
            &usings,
        );
        assert_eq!(nested.map(|t| t.full), Ok("App.Domain.Order+Line".into()));
        Ok(())
    }

    #[test]
    fn assembly_names_come_from_the_project() {
        assert_eq!(
            assembly_name("<Project><PropertyGroup><AssemblyName> My.App </AssemblyName>"),
            Some("My.App".into())
        );
        assert_eq!(assembly_name("<AssemblyName>$(Name)</AssemblyName>"), None);
        assert_eq!(assembly_name("<Project/>"), None);
    }
}
