//! Names: resolving a type reference across the loaded assemblies and spelling it as `ArchUnitNET`
//! does.
//!
//! - Plan: [Wave 2, Step 3](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#23-step-3-attribution-edge-projection-the-net-code-layer-and-defaults-2a)
//!   ("`fullName` and `assemblyQualifiedName` in `ArchUnitNET`'s spelling (verified by the ported
//!   name tests)")
//! - Specification: `ArchUnitNET` 0.13.4 reads assemblies with Mono.Cecil, so its names are Cecil's:
//!   `MonoCecilTypeExtensions.BuildFullName` (a type's `FullName` with `/` replaced by `+`; a
//!   generic parameter as `Declaring+<T>`), `MonoCecilMemberExtensions.BuildFullName` (a member's
//!   Cecil `FullName`, nested types still written with `/`), `BuildMethodMemberName`
//!   (`Name(Param,Param)`), and `Assembly.CreateQualifiedName(simpleName, fullName)`
//! - Decision: [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md) (nothing
//!   is renamed: a rule written for `ArchUnitNET` matches the same names here)
//!
//! | Name | Example |
//! | --- | --- |
//! | type full name | `Ns.Outer+Inner`, ``Ns.List`1`` |
//! | type in a signature | ``Ns.GenericClass`1<Ns.RegularClass>``, `System.Int32[]`, `System.String&` |
//! | member full name | `System.Void Ns.Outer/Inner::Run(System.Int32,System.String)` |
//! | method member name | `Run(System.Int32,System.String)` |
//! | assembly-qualified name | `Ns.Outer+Inner, MyAssembly` |

use std::collections::BTreeMap;

use crate::loader::{Loaded, Scope};
use crate::metadata::tables::id;
use crate::sig::{Token, TypeSig};

/// Where a type reference leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// A type defined in a loaded assembly: its index and `TypeDef` row.
    Def {
        /// Index into [`Universe::assemblies`].
        assembly: usize,
        /// 1-based `TypeDef` row.
        row: u32,
    },
    /// A type outside the loaded set.
    External {
        /// Its full name, nested types joined with `+`.
        full_name: String,
        /// The simple name of the assembly that defines it, when the reference says.
        assembly: Option<String>,
    },
}

/// Generic parameter names in scope when naming a signature: the declaring type's and method's.
#[derive(Debug, Clone, Copy, Default)]
pub struct Generics<'a> {
    /// The declaring type's full name and parameters (`!0`).
    pub type_: Option<(&'a str, &'a [String])>,
    /// The method's Cecil full name and parameters (`!!0`).
    pub method: Option<(&'a str, &'a [String])>,
}

/// Every assembly of a run, with indexes to resolve references between them.
#[derive(Debug)]
pub struct Universe<'a> {
    /// The loaded assemblies.
    pub assemblies: Vec<&'a Loaded>,
    by_name: BTreeMap<String, usize>,
    defs: Vec<BTreeMap<String, u32>>,
}

impl<'a> Universe<'a> {
    /// Indexes `assemblies` by simple name and each one's types by full name.
    pub fn new(assemblies: Vec<&'a Loaded>) -> Self {
        let mut by_name = BTreeMap::new();
        let mut defs = Vec::with_capacity(assemblies.len());
        for (index, assembly) in assemblies.iter().enumerate() {
            by_name
                .entry(assembly.identity.name.to_ascii_lowercase())
                .or_insert(index);
            defs.push(
                assembly
                    .types
                    .iter()
                    .map(|t| (t.full_name.clone(), t.row))
                    .collect(),
            );
        }
        Self {
            assemblies,
            by_name,
            defs,
        }
    }

    /// The loaded assembly with this simple name.
    pub fn assembly_named(&self, name: &str) -> Option<usize> {
        self.by_name.get(&name.to_ascii_lowercase()).copied()
    }

    /// The `TypeDef` row of `full_name` in assembly `index`.
    pub fn def_named(&self, index: usize, full_name: &str) -> Option<u32> {
        self.defs.get(index)?.get(full_name).copied()
    }

    /// The full name (`+` for nesting) of `TypeRef` row `row` of assembly `index`, and the simple
    /// name of the assembly it points at. A nesting chain longer than the table is cut off.
    fn type_ref_name(&self, index: usize, row: u32) -> (String, Option<String>) {
        let Some(assembly) = self.assemblies.get(index) else {
            return (String::new(), None);
        };
        let mut parts = Vec::new();
        let mut current = row;
        let mut target = None;
        let mut namespace = String::new();
        for _ in 0..=assembly.type_refs.len() {
            let Some(r) = (current as usize)
                .checked_sub(1)
                .and_then(|i| assembly.type_refs.get(i))
            else {
                break;
            };
            parts.push(r.name.clone());
            namespace.clone_from(&r.namespace);
            match r.scope {
                Scope::Enclosing(outer) => current = outer,
                Scope::Assembly(a) => {
                    target = assembly
                        .assembly_refs
                        .get(a)
                        .map(|a| a.identity.name.clone());
                    break;
                }
                Scope::Module | Scope::ModuleRef => {
                    target = Some(assembly.identity.name.clone());
                    break;
                }
                Scope::Unknown => break,
            }
        }
        parts.reverse();
        let joined = parts.join("+");
        let full_name = if namespace.is_empty() {
            joined
        } else {
            format!("{namespace}.{joined}")
        };
        (full_name, target)
    }

    /// Resolves a `TypeDef` or `TypeRef` token of assembly `index`. A `TypeSpec` is not a single
    /// type; use [`Universe::spec`] and name its signature.
    pub fn resolve(&self, index: usize, token: Token) -> Resolved {
        match token.table {
            id::TYPE_DEF => Resolved::Def {
                assembly: index,
                row: token.row,
            },
            id::TYPE_REF => {
                let (full_name, target) = self.type_ref_name(index, token.row);
                let loaded = target
                    .as_deref()
                    .and_then(|name| self.assembly_named(name))
                    .or_else(|| target.is_none().then_some(index));
                match loaded.and_then(|a| self.def_named(a, &full_name).map(|row| (a, row))) {
                    Some((assembly, row)) => Resolved::Def { assembly, row },
                    None => Resolved::External {
                        full_name,
                        assembly: target,
                    },
                }
            }
            _ => Resolved::External {
                full_name: String::new(),
                assembly: None,
            },
        }
    }

    /// The signature a `TypeSpec` token stands for.
    pub fn spec(&self, index: usize, token: Token) -> Option<&TypeSig> {
        (token.table == id::TYPE_SPEC)
            .then(|| {
                self.assemblies
                    .get(index)?
                    .type_specs
                    .get((token.row as usize).checked_sub(1)?)
            })
            .flatten()
    }

    /// `ArchUnitNET`'s full name of a resolved type (`+` for nesting).
    pub fn full_name(&self, resolved: &Resolved) -> String {
        match resolved {
            Resolved::Def { assembly, row } => self
                .assemblies
                .get(*assembly)
                .and_then(|a| a.type_at(*row))
                .map(|t| t.full_name.clone())
                .unwrap_or_default(),
            Resolved::External { full_name, .. } => full_name.clone(),
        }
    }

    /// The simple name of the assembly that defines a resolved type.
    pub fn assembly_of(&self, resolved: &Resolved) -> Option<String> {
        match resolved {
            Resolved::Def { assembly, .. } => self
                .assemblies
                .get(*assembly)
                .map(|a| a.identity.name.clone()),
            Resolved::External { assembly, .. } => assembly.clone(),
        }
    }

    /// The full name of a token of assembly `index` as it appears inside a signature: a type
    /// token by its full name, a `TypeSpec` by its signature's spelling.
    pub fn token_name(
        &self,
        index: usize,
        token: Token,
        generics: Generics<'_>,
        cecil: bool,
    ) -> String {
        if let Some(spec) = self.spec(index, token) {
            return self.sig_name(index, spec, generics, cecil);
        }
        let name = self.full_name(&self.resolve(index, token));
        if cecil { name.replace('+', "/") } else { name }
    }

    /// A signature type spelled as Cecil's `FullName` (`cecil`: nested types with `/`, generic
    /// parameters by name) or as `ArchUnitNET`'s type name (nested with `+`).
    pub fn sig_name(
        &self,
        index: usize,
        sig: &TypeSig,
        generics: Generics<'_>,
        cecil: bool,
    ) -> String {
        match sig {
            TypeSig::Primitive(name) => (*name).to_owned(),
            TypeSig::Named { token, .. } => self.token_name(index, *token, generics, cecil),
            TypeSig::GenericInst { base, args } => {
                let args: Vec<String> = args
                    .iter()
                    .map(|a| self.sig_name(index, a, generics, cecil))
                    .collect();
                format!(
                    "{}<{}>",
                    self.sig_name(index, base, generics, cecil),
                    args.join(",")
                )
            }
            TypeSig::SzArray(element) => {
                format!("{}[]", self.sig_name(index, element, generics, cecil))
            }
            TypeSig::Array { element, rank } => format!(
                "{}[{}]",
                self.sig_name(index, element, generics, cecil),
                ",".repeat((*rank as usize).saturating_sub(1))
            ),
            TypeSig::Ptr(inner) => format!("{}*", self.sig_name(index, inner, generics, cecil)),
            TypeSig::ByRef(inner) => format!("{}&", self.sig_name(index, inner, generics, cecil)),
            TypeSig::Pinned(inner) => self.sig_name(index, inner, generics, cecil),
            TypeSig::Var(n) => generic_name(generics.type_, *n, "!", cecil),
            TypeSig::MVar(n) => generic_name(generics.method, *n, "!!", cecil),
            TypeSig::FnPtr(method) => {
                let params: Vec<String> = method
                    .params
                    .iter()
                    .map(|p| self.sig_name(index, p, generics, cecil))
                    .collect();
                format!(
                    "method {} *({})",
                    self.sig_name(index, &method.ret, generics, cecil),
                    params.join(",")
                )
            }
            TypeSig::Modified {
                required,
                modifier,
                inner,
            } => format!(
                "{} {}({})",
                self.sig_name(index, inner, generics, cecil),
                if *required { "modreq" } else { "modopt" },
                self.token_name(index, *modifier, generics, cecil)
            ),
        }
    }
}

/// A generic parameter's name: its declared name for Cecil (`T`), `Declaring+<T>` for
/// `ArchUnitNET`'s type name, or the positional form when the name is not known.
fn generic_name(scope: Option<(&str, &[String])>, n: u32, prefix: &str, cecil: bool) -> String {
    match scope.and_then(|(owner, names)| names.get(n as usize).map(|name| (owner, name))) {
        Some((_, name)) if cecil => name.clone(),
        Some((owner, name)) => format!("{owner}+<{name}>"),
        None => format!("{prefix}{n}"),
    }
}

/// `Assembly.CreateQualifiedName(assembly, fullName)`: `Full.Name, Assembly`.
pub fn assembly_qualified_name(full_name: &str, assembly: &str) -> String {
    format!("{full_name}, {assembly}")
}

/// Whether a name is one the C# compiler generates (`<Main>b__0_0`, `<>c`, `!0`): the test
/// `ArchUnitNET`'s `HasCompilerGeneratedName` applies to members and types.
pub fn is_compiler_generated_name(name: &str) -> bool {
    name.starts_with('<') || name.starts_with('!')
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn generic_parameters_are_named_per_spelling() {
        let names = ["T".to_owned(), "U".to_owned()];
        let scope = Some(("Ns.Box`2", &names[..]));
        assert_eq!(generic_name(scope, 1, "!", true), "U");
        assert_eq!(generic_name(scope, 0, "!", false), "Ns.Box`2+<T>");
        assert_eq!(generic_name(scope, 5, "!", true), "!5");
        assert_eq!(generic_name(None, 0, "!!", false), "!!0");
    }

    #[test]
    fn qualified_names_and_compiler_generated_names() {
        assert_eq!(
            assembly_qualified_name("Ns.Outer+Inner", "MyAssembly"),
            "Ns.Outer+Inner, MyAssembly"
        );
        for (name, generated) in [
            ("<Main>b__0_0", true),
            ("<>c", true),
            ("!0", true),
            ("Main", false),
            ("", false),
        ] {
            assert_eq!(is_compiler_generated_name(name), generated, "{name:?}");
        }
    }

    proptest! {
        #[test]
        fn a_qualified_name_starts_with_the_full_name(full in "[A-Za-z.+`0-9]{1,24}", asm in "[A-Za-z.]{1,12}") {
            let qualified = assembly_qualified_name(&full, &asm);
            prop_assert!(qualified.starts_with(&full));
            prop_assert!(qualified.ends_with(&asm));
        }
    }
}
