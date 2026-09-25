//! NetArchTest's predicates and conditions as element-rule terms, for `import archunit`.
//!
//! - Source: the mapping table in [conformance/netarchtest/README.md](../../../../../conformance/netarchtest/README.md#the-mapping),
//!   proven by the 326 gate 2 cases; this is a port of its C# source,
//!   `conformance/netarchtest/tools/Port/Rules.cs`, so a chain imports to the rule gate 2 checks
//! - Plan: [Wave 2, Step 11](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f)
//! - Decision: [ADR-0009](../../../../../docs/adr/0009-conformance-suites-as-specification.md)
//! - Requirement: [FR-CLI-04](../../../../../docs/prd.md#fr-cli-04)
//!
//! NetArchTest compares names ignoring case where `ArchUnitNET` does not; a case-insensitive
//! comparison is written as a pattern whose letters match either case. A predicate and its
//! condition share one function upstream, so one mapping serves `where` and `should`, spelled for
//! the side it is on.

use rb_config::elements::Side;

use super::archunit::Val;
use super::yaml::Node;

type Args<'a> = &'a [Result<Val, String>];

/// Verbs that are spelled as written on both sides (`haveName`, `doNotHaveName`).
const VERBS: &[&str] = &[
    "have",
    "reside",
    "depend",
    "only",
    "call",
    "implement",
    "exist",
    "adhere",
];

/// A concept's key on a side, as `rb-config` spells it.
pub fn key(concept: &str, side: Side, negated: bool) -> String {
    let verb = VERBS.iter().any(|v| concept.starts_with(v));
    let capital = concept.chars().next().map_or_else(String::new, |c| {
        c.to_ascii_uppercase().to_string() + &concept[1..]
    });
    match (side, verb, negated) {
        (Side::Where | Side::Should, true, false) => concept.to_owned(),
        (Side::Where, true, true) => format!("doNot{capital}"),
        (Side::Where, false, false) => format!("are{capital}"),
        (Side::Where, false, true) => format!("areNot{capital}"),
        (Side::Should, true, true) => format!("not{capital}"),
        (Side::Should, false, false) => format!("be{capital}"),
        (Side::Should, false, true) => format!("notBe{capital}"),
    }
}

/// One test.
pub fn test(concept: &str, side: Side, negated: bool, value: Node) -> Node {
    Node::Map(vec![(key(concept, side, negated), value)])
}

fn flag(concept: &str, side: Side, negated: bool) -> Node {
    test(concept, side, negated, Node::Bool(true))
}

/// Every item holds.
pub fn all(items: Vec<Node>) -> Node {
    Node::map(vec![("all", Node::list(items))])
}

/// Some item holds.
pub fn any(items: Vec<Node>) -> Node {
    Node::map(vec![("any", Node::list(items))])
}

/// The item does not hold.
pub fn not(item: Node) -> Node {
    Node::map(vec![("not", item)])
}

fn negated_if(node: Node, negated: bool) -> Node {
    if negated { not(node) } else { node }
}

fn selector(kind: &str, where_: Option<Node>) -> Node {
    let mut pairs = vec![("kind", Node::str(kind))];
    if let Some(w) = where_ {
        pairs.push(("where", w));
    }
    Node::map(pairs)
}

/// Escapes literal text for the JavaScript pattern dialect.
pub fn escape(text: &str) -> String {
    text.chars().fold(String::new(), |mut out, c| {
        if "\\^$.|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
        out
    })
}

/// The pattern with every letter outside an escape or a class matching either case; a letter
/// inside a character class is refused rather than translated.
///
/// # Errors
/// A sentence naming the pattern.
pub fn ignore_case(pattern: &str) -> Result<String, String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut in_class = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            out.push(c);
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if in_class {
            if c.is_alphabetic() {
                return Err(format!(
                    "`{pattern}` has a letter inside a character class, which cannot be made case-insensitive without a flag"
                ));
            }
            in_class = c != ']';
            out.push(c);
        } else if c == '[' {
            in_class = true;
            out.push(c);
        } else if c.is_ascii_alphabetic() {
            out.push('[');
            out.push(c.to_ascii_uppercase());
            out.push(c.to_ascii_lowercase());
            out.push(']');
        } else {
            out.push(c);
        }
        i += 1;
    }
    Ok(out)
}

fn strings(args: Args<'_>) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for arg in args {
        match arg {
            Ok(Val::Str(s)) => out.push(s.clone()),
            Ok(Val::List(items)) => {
                for item in items {
                    match item {
                        Val::Str(s) => out.push(s.clone()),
                        other => return Err(format!("{} is not a string", other.describe())),
                    }
                }
            }
            Ok(other) => return Err(format!("{} is not a string", other.describe())),
            Err(reason) => return Err(reason.clone()),
        }
    }
    Ok(out)
}

fn one_string(args: Args<'_>) -> Result<String, String> {
    match args.first() {
        Some(Ok(Val::Str(s))) => Ok(s.clone()),
        Some(Ok(other)) => Err(format!("{} is not a string", other.describe())),
        Some(Err(reason)) => Err(reason.clone()),
        None => Err("the call has no argument".into()),
    }
}

fn one_type(args: Args<'_>) -> Result<String, String> {
    match args.first() {
        Some(Ok(Val::Type(t))) => Ok(t.full.clone()),
        Some(Ok(other)) => Err(format!("{} is not a type", other.describe())),
        Some(Err(reason)) => Err(reason.clone()),
        None => Err("the call has no argument".into()),
    }
}

/// Whether a `StringComparison` argument makes the comparison ordinal (case-sensitive).
fn ordinal(args: Args<'_>) -> Result<bool, String> {
    match args.get(1) {
        None => Ok(false),
        Some(Ok(Val::Enum(e))) if e.ends_with(".Ordinal") => Ok(true),
        Some(Ok(Val::Enum(e))) if e.ends_with("IgnoreCase") => Ok(false),
        Some(Ok(other)) => Err(format!(
            "the comparison {} is neither ordinal nor case-insensitive",
            other.describe()
        )),
        Some(Err(reason)) => Err(reason.clone()),
    }
}

fn names(items: &[String]) -> Node {
    Node::strs(items)
}

/// `ResideInNamespaceMatching`: the namespace of a type that is not nested, the declaring type's
/// full name for a nested public or private type, and the empty namespace for any other.
fn namespace_matching(side: Side, pattern: &str, negated: bool) -> Result<Node, String> {
    let insensitive = ignore_case(pattern)?;
    let mut branches = vec![
        all(vec![
            flag("nested", side, true),
            test(
                "resideInNamespaceMatching",
                side,
                false,
                Node::str(insensitive.clone()),
            ),
        ]),
        all(vec![
            flag("nested", side, false),
            any(vec![
                flag("public", side, false),
                flag("private", side, false),
            ]),
            test(
                "nestedIn",
                side,
                false,
                selector(
                    "type",
                    Some(test(
                        "haveFullNameMatching",
                        Side::Where,
                        false,
                        Node::str(insensitive),
                    )),
                ),
            ),
        ]),
    ];
    let empty_matches = regex::Regex::new(&format!("(?i){pattern}"))
        .map_err(|e| format!("`{pattern}` is not a pattern the importer can check: {e}"))?
        .is_match("");
    if empty_matches {
        branches.push(all(vec![
            flag("nested", side, false),
            flag("public", side, true),
            flag("private", side, true),
        ]));
    }
    Ok(negated_if(any(branches), negated))
}

/// The types a dependency search names: a prefix of the full name in whole segments.
fn dependencies(entries: &[String]) -> Node {
    let mut distinct: Vec<String> = Vec::new();
    for e in entries {
        if !distinct.contains(e) {
            distinct.push(e.clone());
        }
    }
    let alternation = distinct
        .iter()
        .map(|e| escape(&e.replace('/', "+")))
        .collect::<Vec<_>>()
        .join("|");
    selector(
        "type",
        Some(test(
            "haveFullNameMatching",
            Side::Where,
            false,
            Node::str(format!("^(?:{alternation})(?:$|[.+])")),
        )),
    )
}

/// The `where` term of a `Types.In...` root, when it narrows the types.
///
/// # Errors
/// A sentence when the root cannot be translated.
pub fn root(method: &str, args: Args<'_>) -> Result<Option<Node>, String> {
    let assembly = |val: &Val| match val {
        Val::Assembly { name, .. } => Ok(name.clone()),
        other => Err(format!("{} is not an assembly", other.describe())),
    };
    match method {
        "InAssembly" | "InAssemblies" => {
            let mut found = Vec::new();
            for arg in args {
                match arg {
                    Ok(Val::List(items)) => {
                        for item in items {
                            found.push(assembly(item)?);
                        }
                    }
                    Ok(other) => found.push(assembly(other)?),
                    Err(reason) => return Err(reason.clone()),
                }
            }
            if found.is_empty() {
                return Err(format!("`Types.{method}` names no assembly"));
            }
            let value = if found.len() == 1 {
                Node::str(found.remove(0))
            } else {
                names(&found)
            };
            Ok(Some(test("resideInAssembly", Side::Where, false, value)))
        }
        "InNamespace" => Ok(Some(test(
            "haveFullNameStartingWith",
            Side::Where,
            false,
            Node::str(one_string(args)?),
        ))),
        "InCurrentDomain" => Ok(None),
        other => Err(format!(
            "`Types.{other}` reads assemblies from files the rules cannot name"
        )),
    }
}

/// The spelling of a call on its side: negated or not, and the concept it names.
fn split(method: &str, side: Side) -> (bool, String) {
    match side {
        Side::Where => {
            if let Some(rest) = method.strip_prefix("DoNot") {
                (true, rest.to_owned())
            } else if let Some(rest) = method.strip_prefix("AreNot") {
                (true, rest.to_owned())
            } else if let Some(rest) = method.strip_prefix("Are") {
                (false, rest.to_owned())
            } else {
                (false, method.to_owned())
            }
        }
        Side::Should => {
            if method == "HaveDependenciesOtherThan" {
                (true, "OnlyHaveDependenciesOn".to_owned())
            } else if let Some(rest) = method.strip_prefix("NotBe") {
                (true, rest.to_owned())
            } else if let Some(rest) = method.strip_prefix("Not") {
                (true, rest.to_owned())
            } else if let Some(rest) = method.strip_prefix("Be") {
                (false, rest.to_owned())
            } else {
                (false, method.to_owned())
            }
        }
    }
}

/// One NetArchTest predicate or condition as a term.
///
/// # Errors
/// A sentence naming why the call has no element-rule equivalent.
pub fn term(method: &str, args: Args<'_>, side: Side) -> Result<Node, String> {
    let (negated, concept) = split(method, side);
    if let Some(node) = name_term(&concept, args, side, negated)? {
        return Ok(node);
    }
    type_term(method, &concept, args, side, negated)
}

/// The name and namespace concepts, or `None` for another concept.
fn name_term(
    concept: &str,
    args: Args<'_>,
    side: Side,
    negated: bool,
) -> Result<Option<Node>, String> {
    let node = match concept {
        "ResideInNamespace" => test(
            "haveFullNameStartingWith",
            side,
            negated,
            Node::str(one_string(args)?),
        ),
        "ResideInNamespaceMatching" => namespace_matching(side, &one_string(args)?, negated)?,
        "ResideInNamespaceStartingWith" => {
            namespace_matching(side, &format!("^{}", one_string(args)?), negated)?
        }
        "ResideInNamespaceEndingWith" => {
            namespace_matching(side, &format!("{}$", one_string(args)?), negated)?
        }
        "ResideInNamespaceContaining" => {
            namespace_matching(side, &format!("^.*{}.*$", one_string(args)?), negated)?
        }
        "HaveName" => test("haveName", side, negated, Node::str(one_string(args)?)),
        "HaveNameStartingWith" => {
            let start = one_string(args)?;
            if ordinal(args)? {
                test("haveNameStartingWith", side, negated, Node::str(start))
            } else {
                test(
                    "haveNameMatching",
                    side,
                    negated,
                    Node::str(format!("^{}", ignore_case(&escape(&start))?)),
                )
            }
        }
        "HaveNameEndingWith" => {
            let end = one_string(args)?;
            if ordinal(args)? {
                test("haveNameEndingWith", side, negated, Node::str(end))
            } else {
                test(
                    "haveNameMatching",
                    side,
                    negated,
                    Node::str(format!("{}$", ignore_case(&escape(&end))?)),
                )
            }
        }
        "HaveNameMatching" => test(
            "haveNameMatching",
            side,
            negated,
            Node::str(ignore_case(&one_string(args)?)?),
        ),
        _ => return Ok(None),
    };
    Ok(Some(node))
}

/// The type, attribute and dependency concepts.
fn type_term(
    method: &str,
    concept: &str,
    args: Args<'_>,
    side: Side,
    negated: bool,
) -> Result<Node, String> {
    let node = match concept {
        "HaveCustomAttribute" => test(
            "haveAnyAttributes",
            side,
            negated,
            names(&[one_type(args)?]),
        ),
        "HaveCustomAttributeOrInherit" => test(
            "haveAnyAttributes",
            side,
            negated,
            selector(
                "type",
                Some(test(
                    "assignableTo",
                    Side::Where,
                    false,
                    names(&[one_type(args)?]),
                )),
            ),
        ),
        "Inherit" => {
            let t = one_type(args)?;
            negated_if(
                all(vec![
                    test("assignableTo", side, false, names(std::slice::from_ref(&t))),
                    test("", side, true, names(&[t])),
                ]),
                negated,
            )
        }
        "ImplementInterface" => test(
            "implementInterface",
            side,
            negated,
            names(&[one_type(args)?]),
        ),
        "Classes" => test("", side, !negated, selector("interface", None)),
        "Interfaces" => test("", side, negated, selector("interface", None)),
        "Abstract" | "Generic" | "Static" | "Nested" | "Public" | "Sealed" => {
            flag(&concept.to_ascii_lowercase(), side, negated)
        }
        "NestedPublic" => negated_if(
            all(vec![
                flag("nested", side, false),
                flag("public", side, false),
            ]),
            negated,
        ),
        "NestedPrivate" => negated_if(
            all(vec![
                flag("nested", side, false),
                flag("private", side, false),
            ]),
            negated,
        ),
        "HaveDependencyOn" | "HaveDependencyOnAny" => {
            test("dependOnAny", side, negated, dependencies(&strings(args)?))
        }
        "HaveDependencyOnAll" => {
            let entries = strings(args)?;
            let mut distinct: Vec<String> = Vec::new();
            for e in entries {
                if !distinct.contains(&e) {
                    distinct.push(e);
                }
            }
            negated_if(
                all(distinct
                    .iter()
                    .map(|e| {
                        test(
                            "dependOnAny",
                            side,
                            false,
                            dependencies(std::slice::from_ref(e)),
                        )
                    })
                    .collect()),
                negated,
            )
        }
        "OnlyHaveDependenciesOn" | "OnlyHaveDependencyOn" => {
            test("onlyDependOn", side, negated, dependencies(&strings(args)?))
        }
        _ => return Err(unsupported(method, concept)),
    };
    Ok(node)
}

/// Why a NetArchTest call has no element-rule equivalent.
fn unsupported(method: &str, concept: &str) -> String {
    match concept {
        "Immutable" | "Mutable" => format!(
            "`{method}` is NetArchTest's immutability (no public setter, every field non-public, readonly or const), which is not ArchUnitNET's `BeImmutable`"
        ),
        "OnlyHaveNullableMembers"
        | "HaveSomeNonNullableMembers"
        | "OnlyHaveNonNullableMembers"
        | "HaveSomeNullableMembers" => {
            format!("`{method}` asks member nullability, which the graph does not record")
        }
        _ => format!("`{method}` has no element-rule equivalent"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::import::types::TypeInfo;

    fn json(node: &Node) -> serde_json::Value {
        node.to_json()
    }

    fn s(text: &str) -> Val {
        Val::Str(text.into())
    }

    fn t(full: &str) -> Val {
        Val::Type(TypeInfo {
            full: full.into(),
            name: full.rsplit('.').next().unwrap_or(full).into(),
            namespace: String::new(),
            assembly: None,
        })
    }

    #[test]
    fn keys_are_spelled_per_side() {
        let table = [
            ("haveName", Side::Where, false, "haveName"),
            ("haveName", Side::Where, true, "doNotHaveName"),
            ("haveName", Side::Should, true, "notHaveName"),
            ("abstract", Side::Where, false, "areAbstract"),
            ("abstract", Side::Where, true, "areNotAbstract"),
            ("abstract", Side::Should, false, "beAbstract"),
            ("abstract", Side::Should, true, "notBeAbstract"),
            ("", Side::Where, true, "areNot"),
            ("", Side::Should, false, "be"),
        ];
        for (concept, side, negated, expected) in table {
            assert_eq!(key(concept, side, negated), expected);
        }
    }

    #[test]
    fn case_insensitive_patterns() {
        assert_eq!(ignore_case("Some"), Ok("[Ss][Oo][Mm][Ee]".into()));
        assert_eq!(ignore_case("\\d+x"), Ok("\\d+[Xx]".into()));
        assert_eq!(ignore_case("[0-9]"), Ok("[0-9]".into()));
        assert!(ignore_case("[a-z]").is_err());
        assert_eq!(escape("a.b+c"), "a\\.b\\+c");
    }

    #[test]
    fn the_readme_table_maps() -> Result<(), String> {
        assert_eq!(
            json(&term("ResideInNamespace", &[Ok(s("A.B"))], Side::Where)?),
            serde_json::json!({"haveFullNameStartingWith": "A.B"})
        );
        assert_eq!(
            json(&term(
                "DoNotResideInNamespace",
                &[Ok(s("A.B"))],
                Side::Where
            )?),
            serde_json::json!({"doNotHaveFullNameStartingWith": "A.B"})
        );
        assert_eq!(
            json(&term("HaveNameStartingWith", &[Ok(s("Cl"))], Side::Should)?),
            serde_json::json!({"haveNameMatching": "^[Cc][Ll]"})
        );
        assert_eq!(
            json(&term(
                "HaveNameEndingWith",
                &[Ok(s("X")), Ok(Val::Enum("StringComparison.Ordinal".into()))],
                Side::Should
            )?),
            serde_json::json!({"haveNameEndingWith": "X"})
        );
        assert_eq!(
            json(&term("BeClasses", &[], Side::Should)?),
            serde_json::json!({"notBe": {"kind": "interface"}})
        );
        assert_eq!(
            json(&term("AreNotClasses", &[], Side::Where)?),
            serde_json::json!({"are": {"kind": "interface"}})
        );
        assert_eq!(
            json(&term("NotBeNestedPublic", &[], Side::Should)?),
            serde_json::json!({"not": {"all": [{"beNested": true}, {"bePublic": true}]}})
        );
        assert_eq!(
            json(&term("Inherit", &[Ok(t("A.B"))], Side::Where)?),
            serde_json::json!({"all": [{"areAssignableTo": ["A.B"]}, {"areNot": ["A.B"]}]})
        );
        assert_eq!(
            json(&term(
                "HaveDependencyOnAll",
                &[Ok(s("A")), Ok(s("B")), Ok(s("A"))],
                Side::Where
            )?)["all"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            json(&term(
                "HaveDependenciesOtherThan",
                &[Ok(s("System"))],
                Side::Should
            )?),
            serde_json::json!({"notOnlyDependOn": {"kind": "type", "where": {"haveFullNameMatching": "^(?:System)(?:$|[.+])"}}})
        );
        let matching = json(&term(
            "ResideInNamespaceContaining",
            &[Ok(s("x"))],
            Side::Where,
        )?);
        assert_eq!(matching["any"].as_array().map(Vec::len), Some(2));
        let everything = json(&term(
            "ResideInNamespaceMatching",
            &[Ok(s(".*"))],
            Side::Where,
        )?);
        assert_eq!(everything["any"].as_array().map(Vec::len), Some(3));
        let prefix = json(&term(
            "ResideInNamespaceStartingWith",
            &[Ok(s("x"))],
            Side::Where,
        )?);
        assert_eq!(prefix["any"].as_array().map(Vec::len), Some(2));
        assert!(term("BeImmutable", &[], Side::Should).is_err());
        assert!(term("OnlyHaveNullableMembers", &[], Side::Should).is_err());
        assert!(term("HaveSourceFile", &[], Side::Should).is_err());
        assert!(term("HaveName", &[Err("no".into())], Side::Where).is_err());
        assert!(term("HaveName", &[Ok(t("X"))], Side::Where).is_err());
        Ok(())
    }

    #[test]
    fn roots() -> Result<(), String> {
        let asm = Ok(Val::Assembly {
            name: "A".into(),
            project: None,
        });
        assert_eq!(
            root("InAssembly", std::slice::from_ref(&asm))?.map(|n| json(&n)),
            Some(serde_json::json!({"resideInAssembly": "A"}))
        );
        assert_eq!(
            root(
                "InAssemblies",
                &[Ok(Val::List(vec![
                    Val::Assembly {
                        name: "A".into(),
                        project: None
                    },
                    Val::Assembly {
                        name: "B".into(),
                        project: None
                    }
                ]))]
            )?
            .map(|n| json(&n)),
            Some(serde_json::json!({"resideInAssembly": ["A", "B"]}))
        );
        assert_eq!(root("InCurrentDomain", &[])?, None);
        assert!(root("FromFile", &[]).is_err());
        assert!(root("InAssembly", &[]).is_err());
        assert!(root("InAssembly", &[Ok(s("x"))]).is_err());
        Ok(())
    }
}
