//! `import archunit` against the gate 2 corpora: the C# chain beside every ported case imports
//! to the case's rule, and imported ArchUnitNET tests reproduce upstream's verdicts over the
//! fixture graphs.
//!
//! - Plan: [Wave 2, Step 11](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f)
//!   ("a round-trip test that an imported ArchUnitNET rule over `TestAssembly` reproduces the
//!   upstream expectation")
//! - Decision: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md) (the
//!   upstream suites are the specification)
//! - Source: [conformance/netarchtest/README.md](../../../conformance/netarchtest/README.md#the-mapping)
//! - Requirement: [FR-CLI-04](../../../docs/prd.md#fr-cli-04)
//!
//! The corpus test wraps each ported case's `csharp` chain in a small test class and imports it.
//! The chains name upstream's helper objects (`helper.RegularClass`); the test declares a helper
//! whose fields follow upstream's convention, `X = Architecture.GetClassOfType(typeof(X))` for a
//! type named X in the case's graphs, so the importer's own field resolution runs. A chain whose
//! helper value is not a type (a member, an argument constant) stays unresolved, which is the
//! importer refusing to guess; every chain it does import must equal the ported rule.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use rb_cli::cmd::import::archunit::{Chain, Expect, Mapped, Program, Root};
use rb_cli::cmd::import::csharp;
use rb_cli::cmd::import::types::Index;
use rb_cli::cmd::import::yaml::Node;
use rb_config::elements::{Concept, Expr, Objects, Operand, Selector};
use rb_model::GraphDocument;
use serde_json::{Value, json};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The graphs of a suite, loaded once each.
struct Graphs {
    suite: PathBuf,
    loaded: BTreeMap<String, GraphDocument>,
}

impl Graphs {
    fn new(suite: &str) -> Self {
        Self {
            suite: root().join("conformance").join(suite),
            loaded: BTreeMap::new(),
        }
    }

    fn get(&mut self, assembly: &str) -> Result<&GraphDocument, Box<dyn Error>> {
        if !self.loaded.contains_key(assembly) {
            let path = self.suite.join("graphs").join(format!("{assembly}.json"));
            let graph: GraphDocument = serde_json::from_str(&std::fs::read_to_string(path)?)?;
            self.loaded.insert(assembly.to_owned(), graph);
        }
        self.loaded
            .get(assembly)
            .ok_or_else(|| format!("no graph {assembly}").into())
    }

    /// One document over several assemblies' graphs, as gate 2 joins them.
    fn document(&mut self, assemblies: &[String]) -> Result<GraphDocument, Box<dyn Error>> {
        let mut document = GraphDocument::default();
        let mut code = rb_model::CodeLayer::default();
        for assembly in assemblies {
            let graph = self.get(assembly)?.clone();
            document.modules.extend(graph.modules);
            if let Some(layer) = graph.code {
                code.merge(layer);
            }
        }
        code.normalise();
        document.code = Some(code);
        Ok(document)
    }
}

/// An index over every type of the assemblies' graphs, and the namespaces they hold.
fn index_of(
    graphs: &mut Graphs,
    assemblies: &[String],
) -> Result<(Index, BTreeSet<String>), Box<dyn Error>> {
    let mut index = Index::default();
    let mut namespaces = BTreeSet::new();
    for assembly in assemblies {
        let graph = graphs.get(assembly)?;
        for t in graph.code.iter().flat_map(|c| c.types.iter()) {
            let namespace = t.namespace.clone().unwrap_or_default();
            index.add_known(&t.full_name, &namespace, t.assembly.as_deref());
            if !namespace.is_empty() {
                namespaces.insert(namespace);
            }
        }
    }
    Ok((index, namespaces))
}

/// The chain without the call that ran it or the assertion around it.
fn bare_chain(csharp: &str) -> String {
    let flat = csharp.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut end = flat.len();
    for sink in [
        ".HasNoViolations(",
        ".Check(",
        ".GetObjects(",
        ".AssertNoViolations(",
        ".AssertOnlyViolations(",
        ".AssertAnyViolations(",
        ".GetResult(",
        ".GetTypes(",
    ] {
        if let Some(at) = flat.find(sink) {
            end = end.min(at);
        }
    }
    flat[..end].to_owned()
}

/// Upstream's `LogicalConjunctionTests` fields the chains of that file name, as upstream declares
/// them (ArchUnitNET 0.13.4, Apache-2.0).
const LOGICAL_CONJUNCTION: &str = "
        private static readonly Class ThisClass = Architecture.GetClassOfType(typeof(global::ArchUnitNETTests.Fluent.Syntax.Elements.LogicalConjunctionTests));
        private static readonly Class OtherClass = Architecture.GetClassOfType(typeof(global::ArchUnitNETTests.Fluent.Syntax.Elements.OtherClassForLogicalConjunctionTest));
        private static readonly string ThisClassName = ThisClass.Name;
        private static readonly string OtherClassName = OtherClass.Name;
        private static readonly IArchRule ThisClassExists = Classes().That().Are(ThisClass).Should().Exist();
        private static readonly IArchRule ThisClassDoesNotExist = Classes().That().Are(ThisClass).Should().NotExist();
";

/// The helper fields a chain names: `X` for a type named X, `XSystemType` for its `typeof`.
fn helper_fields(chain: &str, types: &BTreeMap<String, Vec<String>>) -> String {
    let mut fields = String::from(
        "        public readonly string NonExistentObjectName = \"NotTheNameOfAnyObject\";\n",
    );
    let mut seen = BTreeSet::new();
    for piece in chain.split("helper.").skip(1) {
        let name: String = piece
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !seen.insert(name.clone()) {
            continue;
        }
        let (type_name, system) = name
            .strip_suffix("SystemType")
            .map_or((name.as_str(), false), |t| (t, true));
        let Some([full]) = types.get(type_name).map(Vec::as_slice) else {
            continue;
        };
        let dotted = full.replace('+', ".");
        if system {
            let _ = writeln!(
                fields,
                "        public Type {name} = typeof(global::{dotted});"
            );
        } else {
            let _ = writeln!(
                fields,
                "        public Class {name} = Architecture.GetClassOfType(typeof(global::{dotted}));"
            );
        }
    }
    fields
}

/// An expression with `all` inside `all` and `any` inside `any` flattened (both fold orders
/// mean the same), and an assembly named by its simple name (`resideInAssembly` matches either).
fn normal(expr: &Expr) -> Expr {
    let flat = |items: &[Expr], any: bool| {
        let mut out = Vec::new();
        for item in items.iter().map(normal) {
            match item {
                Expr::Any(inner) if any => out.extend(inner),
                Expr::All(inner) if !any => out.extend(inner),
                other => out.push(other),
            }
        }
        out
    };
    match expr {
        Expr::All(items) => Expr::All(flat(items, false)),
        Expr::Any(items) => Expr::Any(flat(items, true)),
        Expr::Not(inner) => Expr::Not(Box::new(normal(inner))),
        Expr::Test(test) => {
            let mut test = test.clone();
            test.operand = match &test.operand {
                Operand::Names(names) if test.concept == Concept::ResideInAssembly => {
                    Operand::Names(
                        names
                            .iter()
                            .map(|n| n.split(',').next().unwrap_or(n).to_owned())
                            .collect(),
                    )
                }
                Operand::Objects(Objects::Selector(selector)) => {
                    Operand::Objects(Objects::Selector(Box::new(normal_selector(selector))))
                }
                other => other.clone(),
            };
            Expr::Test(test)
        }
    }
}

fn normal_selector(selector: &Selector) -> Selector {
    let mut selector = selector.clone();
    selector.where_ = selector.where_.as_ref().map(normal);
    selector
}

/// The ported rule and the imported body as parsed element rules, compared on what they select
/// and require.
fn same_element(ported: &Value, body: &[(String, Node)]) -> Result<bool, Box<dyn Error>> {
    let mut mine = vec![("name".to_owned(), Node::str("case"))];
    mine.extend(body.iter().cloned());
    let mine = rb_config::elements::parse_elements(&json!([Node::Map(mine).to_json()]))?;
    let mut theirs = ported.clone();
    theirs["name"] = json!("case");
    let theirs = rb_config::elements::parse_elements(&json!([theirs]))?;
    // Every imported rule is scoped to .NET; the ported cases run over .NET-only graphs, where
    // the scope changes nothing, so it is asserted here and left out of the comparison.
    if mine[0].select.languages != [rb_model::Language::Dotnet] {
        return Ok(false);
    }
    let mut mine_select = mine[0].select.clone();
    mine_select
        .languages
        .clone_from(&theirs[0].select.languages);
    Ok(
        normal_selector(&mine_select) == normal_selector(&theirs[0].select)
            && normal(&mine[0].should) == normal(&theirs[0].should),
    )
}

fn same_slice(ported: &Value, body: &[(String, Node)]) -> Result<bool, Box<dyn Error>> {
    let mut mine = vec![("name".to_owned(), Node::str("case"))];
    mine.extend(body.iter().cloned());
    let mine = rb_config::elements::parse_slices(&json!([Node::Map(mine).to_json()]))?;
    let mut theirs = ported.clone();
    theirs["name"] = json!("case");
    let theirs = rb_config::elements::parse_slices(&json!([theirs]))?;
    Ok(mine[0].matching == theirs[0].matching && mine[0].should == theirs[0].should)
}

/// What importing a corpus found.
#[derive(Default)]
struct Tally {
    chains: usize,
    imported: usize,
    unresolved: usize,
    mismatches: Vec<String>,
}

/// Imports one synthetic test class and returns the mapping of its single chain.
fn import_one(source: &str, index: Index) -> Result<Option<(Chain, Mapped)>, Box<dyn Error>> {
    let file = csharp::parse(source, "Case.cs")?;
    let program = Program::from_parts(vec![(PathBuf::from("Case.cs"), file)], index);
    let candidates = program.candidates();
    let Some(candidate) = candidates.into_iter().next() else {
        return Ok(None);
    };
    Ok(candidate.chain.ok().map(|chain| {
        let mapped = program.map(&chain);
        (chain, mapped)
    }))
}

/// The index over one architecture's graphs, its namespaces, and its types by simple name.
struct SuiteIndex {
    index: Index,
    namespaces: BTreeSet<String>,
    by_name: BTreeMap<String, Vec<String>>,
}

impl SuiteIndex {
    fn new(graphs: &mut Graphs, assemblies: &[String]) -> Result<Self, Box<dyn Error>> {
        let (index, namespaces) = index_of(graphs, assemblies)?;
        let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for assembly in assemblies {
            for t in graphs
                .get(assembly)?
                .code
                .iter()
                .flat_map(|c| c.types.iter())
            {
                if !t.full_name.contains('`') {
                    let names = by_name.entry(t.name.clone()).or_default();
                    if !names.contains(&t.full_name) {
                        names.push(t.full_name.clone());
                    }
                }
            }
        }
        Ok(Self {
            index,
            namespaces,
            by_name,
        })
    }
}

/// `using` directives for every namespace.
fn usings(namespaces: &BTreeSet<String>) -> String {
    namespaces.iter().fold(String::new(), |mut out, n| {
        let _ = writeln!(out, "using {n};");
        out
    })
}

/// A test class running one ported chain, with the helper and fields it names.
fn archunit_source(chain: &str, stem: &str, suite: &SuiteIndex) -> String {
    let conjunction = if stem == "LogicalConjunctionTests" {
        LOGICAL_CONJUNCTION
    } else {
        ""
    };
    format!(
        "{}using static ArchUnitNET.Fluent.ArchRuleDefinition;\nnamespace Corpus\n{{\n    public class Helper\n    {{\n{}    }}\n    public class Case\n    {{\n{conjunction}        public void Run()\n        {{\n            var helper = new Helper();\n            {chain}.Check(Architecture);\n        }}\n    }}\n}}\n",
        usings(&suite.namespaces),
        helper_fields(chain, &suite.by_name)
    )
}

#[test]
fn every_archunitnet_chain_the_importer_reads_is_its_ported_rule() -> Result<(), Box<dyn Error>> {
    let mut graphs = Graphs::new("archunitnet");
    let mut tally = Tally::default();
    let mut indexes: BTreeMap<Vec<String>, SuiteIndex> = BTreeMap::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(root().join("conformance/archunitnet/ported"))?
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    for file in files {
        let suite: Value = serde_yaml::from_str(&std::fs::read_to_string(&file)?)?;
        let stem = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        for case in suite["cases"].as_array().into_iter().flatten() {
            let (Some(csharp), Some(rule)) = (case["csharp"].as_str(), case.get("rule")) else {
                continue;
            };
            let family = case["family"].as_str().unwrap_or("element");
            if family == "diagram" {
                continue;
            }
            let assemblies: Vec<String> = case["architecture"]
                .as_array()
                .or_else(|| suite["architecture"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|a| a.as_str().map(str::to_owned))
                .collect();
            if !indexes.contains_key(&assemblies) {
                let index = SuiteIndex::new(&mut graphs, &assemblies)?;
                indexes.insert(assemblies.clone(), index);
            }
            let Some(suite_index) = indexes.get(&assemblies) else {
                continue;
            };
            let chain = bare_chain(csharp);
            tally.chains += 1;
            let source = archunit_source(&chain, &stem, suite_index);
            let id = format!("{stem}.{}", case["id"].as_str().unwrap_or("?"));
            let Some((_, mapped)) = import_one(&source, suite_index.index.clone())? else {
                tally.unresolved += 1;
                continue;
            };
            let same = match (&mapped, family) {
                (Mapped::Element(body), "element") => same_element(rule, body)?,
                (Mapped::Slice(body), "slice") => same_slice(rule, body)?,
                (Mapped::Unmapped(_) | Mapped::Custom, _) => {
                    tally.unresolved += 1;
                    continue;
                }
                _ => false,
            };
            if same {
                tally.imported += 1;
            } else {
                tally.mismatches.push(format!(
                    "{id}: {chain}\n  imported {mapped:?}\n  ported {rule}"
                ));
            }
        }
    }
    println!(
        "ArchUnitNET corpus: {} chains, {} imported to their ported rule, {} not read",
        tally.chains, tally.imported, tally.unresolved
    );
    assert!(
        tally.mismatches.is_empty(),
        "{}",
        tally.mismatches.join("\n")
    );
    // The floor is what the importer reads today; it may only rise.
    assert!(
        tally.imported >= IMPORTED_FLOOR,
        "{} imported, below the floor {IMPORTED_FLOOR}",
        tally.imported
    );
    Ok(())
}

/// The ArchUnitNET chains the importer reads today (a ratchet).
const IMPORTED_FLOOR: usize = 727;

/// The NetArchTest chains the importer reads today (a ratchet).
const NETARCHTEST_FLOOR: usize = 70;

/// My `select.where` with NetArchTest's root term (`resideInAssembly`) taken off, as the ported
/// rules leave it out: each case's graph is its one assembly.
fn without_root(body: &[(String, Node)]) -> Vec<(String, Node)> {
    body.iter()
        .map(|(key, value)| {
            if key != "select" {
                return (key.clone(), value.clone());
            }
            let Node::Map(pairs) = value else {
                return (key.clone(), value.clone());
            };
            let mut select = Vec::new();
            for (k, v) in pairs {
                if k != "where" {
                    select.push((k.clone(), v.clone()));
                    continue;
                }
                match v {
                    Node::Map(w) if w.len() == 1 && w[0].0 == "resideInAssembly" => {}
                    Node::Map(w) if w.len() == 1 && w[0].0 == "all" => {
                        if let Node::List(items) = &w[0].1 {
                            let rest: Vec<_> = items.iter().skip(1).cloned().collect();
                            match rest.len() {
                                0 => {}
                                1 => select.push((k.clone(), rest[0].node.clone())),
                                _ => select
                                    .push((k.clone(), Node::map(vec![("all", Node::List(rest))]))),
                            }
                        }
                    }
                    other => select.push((k.clone(), other.clone())),
                }
            }
            (key.clone(), Node::Map(select))
        })
        .collect()
}

#[test]
fn every_netarchtest_chain_the_importer_reads_is_its_ported_rule() -> Result<(), Box<dyn Error>> {
    let mut graphs = Graphs::new("netarchtest");
    let assemblies = vec!["NetArchTest.TestStructure".to_owned()];
    let (index, namespaces) = index_of(&mut graphs, &assemblies)?;
    let usings = usings(&namespaces);
    let mut tally = Tally::default();
    let mut files: Vec<PathBuf> = std::fs::read_dir(root().join("conformance/netarchtest/ported"))?
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    for file in files {
        let suite: Value = serde_yaml::from_str(&std::fs::read_to_string(&file)?)?;
        for case in suite["cases"].as_array().into_iter().flatten() {
            let (Some(csharp), Some(rule)) = (case["csharp"].as_str(), case.get("rule")) else {
                continue;
            };
            if !csharp.contains(".GetResult()") || !csharp.starts_with("Types.") {
                continue;
            }
            tally.chains += 1;
            let chain = bare_chain(csharp);
            let source = format!(
                "using System;\nusing System.Reflection;\n{usings}namespace Corpus\n{{\n    public class Case\n    {{\n        public void Run()\n        {{\n            var result = {chain}.GetResult();\n        }}\n    }}\n}}\n"
            );
            let id = case["id"].as_str().unwrap_or("?");
            match import_one(&source, index.clone())? {
                Some((chain_read, Mapped::Element(body))) => {
                    assert!(matches!(chain_read.root, Root::NetArchTest { .. }), "{id}");
                    if same_element(rule, &without_root(&body))? {
                        tally.imported += 1;
                    } else {
                        tally.mismatches.push(format!(
                            "{id}: {chain}\n  imported {body:?}\n  ported {rule}"
                        ));
                    }
                }
                _ => tally.unresolved += 1,
            }
        }
    }
    println!(
        "NetArchTest corpus: {} chains, {} imported to their ported rule, {} not read",
        tally.chains, tally.imported, tally.unresolved
    );
    assert!(
        tally.mismatches.is_empty(),
        "{}",
        tally.mismatches.join("\n")
    );
    assert!(
        tally.imported >= NETARCHTEST_FLOOR,
        "{} imported, below the floor {NETARCHTEST_FLOOR}",
        tally.imported
    );
    Ok(())
}

/// A round-trip verdict: the line, what upstream expects, whether the imported rule held.
type Verdict = (usize, Expect, bool);

/// Imports a round-trip fixture over the graphs of `index_assemblies` and evaluates each
/// imported rule over `architecture`, returning `(line, expected, held)` per chain.
fn round_trip(
    fixture: &str,
    index_assemblies: &[&str],
    architecture: &[&str],
) -> Result<Vec<Verdict>, Box<dyn Error>> {
    let mut graphs = Graphs::new("archunitnet");
    let owned = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let (index, _) = index_of(&mut graphs, &owned(index_assemblies))?;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/import/archunit/roundtrip")
        .join(fixture);
    let file = csharp::parse(&std::fs::read_to_string(&path)?, fixture)?;
    let program = Program::from_parts(vec![(path, file)], index);
    let document = graphs.document(&owned(architecture))?;
    let arch = rb_rules::elements::Architecture::new(&document);
    let mut out = Vec::new();
    for candidate in program.candidates() {
        let chain = candidate
            .chain
            .map_err(|e| format!("line {}: {e}", candidate.line))?;
        let held = match program.map(&chain) {
            Mapped::Element(body) => {
                let mut rule = vec![("name".to_owned(), Node::str("imported"))];
                rule.extend(body);
                let parsed =
                    rb_config::elements::parse_elements(&json!([Node::Map(rule).to_json()]))?;
                rb_rules::elements::evaluate(&arch, &parsed[0])?.holds()
            }
            Mapped::Slice(body) => {
                let mut rule = vec![("name".to_owned(), Node::str("imported"))];
                rule.extend(body);
                let parsed =
                    rb_config::elements::parse_slices(&json!([Node::Map(rule).to_json()]))?;
                rb_rules::slices::evaluate(&arch, &parsed[0])?
                    .failures
                    .is_empty()
            }
            other => return Err(format!("line {}: {other:?}", candidate.line).into()),
        };
        out.push((candidate.line, candidate.expect, held));
    }
    Ok(out)
}

#[test]
fn imported_archunitnet_tests_reproduce_upstream_over_the_test_assemblies()
-> Result<(), Box<dyn Error>> {
    // SlicesTests over TestAssembly: `(**)` has a cycle (Assert.Throws and Assert.False), `(**)..`
    // has none (Assert.True).
    let slices = round_trip("SlicesTests.cs", &["TestAssembly"], &["TestAssembly"])?;
    assert_eq!(slices.len(), 3, "{slices:?}");
    // DependenciesToOtherAssembliesTests over ArchUnitNETTests, the TestAssembly types referenced:
    // five rules upstream checks, and three it expects to throw.
    let dependencies = round_trip(
        "DependenciesToOtherAssembliesTests.cs",
        &["ArchUnitNETTests", "TestAssembly"],
        &["ArchUnitNETTests"],
    )?;
    assert_eq!(dependencies.len(), 8, "{dependencies:?}");
    let expected: Vec<Expect> = dependencies.iter().map(|d| d.1).collect();
    assert_eq!(
        expected.iter().filter(|e| **e == Expect::Fails).count(),
        3,
        "{dependencies:?}"
    );
    for (line, expect, held) in slices.iter().chain(&dependencies) {
        assert_eq!(
            *held,
            *expect == Expect::Passes,
            "line {line}: upstream expects {expect:?}, the imported rule held: {held}"
        );
    }
    Ok(())
}
