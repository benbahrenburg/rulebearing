//! The `junit` output validates against the Jenkins xUnit plugin's `junit-10.xsd`, and the `trx`
//! output has the structure Visual Studio's `vstst.xsd` requires. No test reads the network.
//!
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//!   (XSD validation of `junit` and `trx`)
//! - Contract: [Wave 2 plan § 1.5](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#15-interfaces-and-contracts-this-wave-freezes)
//! - Requirement: [FR-OUT-02](../../../docs/prd.md#fr-out-02)
//!
//! No XSD validator written in Rust passes `cargo deny` and covers XML Schema, so this file holds
//! a small one, [`Xsd`], that reads the vendored `tests/schemas/junit-10.xsd` (provenance in
//! `tests/schemas/NOTICE`) and checks an instance against it: element declarations (named and by
//! `ref`), `sequence` and `choice` content with `minOccurs` and `maxOccurs`, `mixed` content,
//! `xs:string` elements, attribute declarations with `use="required"`, undeclared attributes, and
//! attributes whose type is a named `simpleType` restricted by a `pattern`. Any other schema
//! construct fails the test, so the validator cannot pass a document by skipping a rule it does
//! not know. `vstst.xsd` ships with Visual Studio under a licence that does not allow
//! redistribution, so it is not vendored; [`check_trx`] states the structure it requires
//! explicitly: the namespace, the element order `vstest` writes, each required attribute, the GUID
//! and outcome vocabularies, the counters, and that every result, definition and entry refer to
//! one another.
//!
//! The inputs are every dependency-cruiser `test/report` result in the gate 1 fixtures and a
//! Rulebearing result with element and slice violations, a known violation, a vacuous rule, an
//! expired entry, a ratchet and markup in names.

use std::collections::BTreeSet;
use std::error::Error;
use std::path::PathBuf;

use roxmltree::{Document, Node};
use serde_json::{Value, json};

const XS: &str = "http://www.w3.org/2001/XMLSchema";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A validator for the subset of XML Schema `junit-10.xsd` uses.
struct Xsd<'a> {
    schema: Node<'a, 'a>,
}

fn is_xs(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().namespace() == Some(XS) && node.tag_name().name() == name
}

fn xs_children<'a>(node: Node<'a, 'a>) -> impl Iterator<Item = Node<'a, 'a>> {
    node.children().filter(Node::is_element)
}

fn occurs(node: Node<'_, '_>, attribute: &str) -> Result<Option<usize>, String> {
    match node.attribute(attribute) {
        None => Ok(Some(1)),
        Some("unbounded") => Ok(None),
        Some(n) => n
            .parse()
            .map(Some)
            .map_err(|_| format!("{attribute}=\"{n}\" is not a number")),
    }
}

impl<'a> Xsd<'a> {
    fn new(document: &'a Document<'a>) -> Result<Self, String> {
        let schema = document.root_element();
        if !is_xs(schema, "schema") {
            return Err("not an XML Schema".into());
        }
        for child in xs_children(schema) {
            let known = ["element", "complexType", "simpleType"];
            if !known.iter().any(|k| is_xs(child, k)) {
                return Err(format!(
                    "unsupported top-level construct {:?}",
                    child.tag_name()
                ));
            }
        }
        Ok(Self { schema })
    }

    fn global(&self, kind: &str, name: &str) -> Result<Node<'a, 'a>, String> {
        xs_children(self.schema)
            .find(|n| is_xs(*n, kind) && n.attribute("name") == Some(name))
            .ok_or_else(|| format!("no global {kind} `{name}`"))
    }

    /// Validates `instance` as the root element.
    fn validate(&self, instance: Node<'_, '_>) -> Result<(), String> {
        let declaration = self.global("element", instance.tag_name().name())?;
        self.element(instance, declaration)
    }

    /// An element declaration by `ref`, resolved to the global one.
    fn resolve(&self, declaration: Node<'a, 'a>) -> Result<Node<'a, 'a>, String> {
        match declaration.attribute("ref") {
            Some(name) => self.global("element", name),
            None => Ok(declaration),
        }
    }

    fn element(&self, instance: Node<'_, '_>, declaration: Node<'a, 'a>) -> Result<(), String> {
        let path = instance.tag_name().name();
        let simple = |instance: Node<'_, '_>| -> Result<(), String> {
            if instance.children().any(|c| c.is_element()) {
                return Err(format!("<{path}> is xs:string and may not hold elements"));
            }
            if instance.attributes().len() != 0 {
                return Err(format!(
                    "<{path}> is xs:string and may not carry attributes"
                ));
            }
            Ok(())
        };
        let complex = match declaration.attribute("type") {
            Some("xs:string") => return simple(instance),
            Some(name) => self.global("complexType", name)?,
            None => xs_children(declaration)
                .find(|n| is_xs(*n, "complexType"))
                .ok_or_else(|| {
                    format!("<{path}>: unsupported declaration without a complexType")
                })?,
        };
        self.attributes(instance, complex)?;
        let mixed = complex.attribute("mixed") == Some("true");
        if !mixed
            && instance
                .children()
                .any(|c| c.is_text() && !c.text().unwrap_or_default().trim().is_empty())
        {
            return Err(format!("<{path}> does not allow text"));
        }
        let children: Vec<Node<'_, '_>> = instance.children().filter(Node::is_element).collect();
        let model = xs_children(complex).find(|n| !is_xs(*n, "attribute"));
        match model {
            None => {
                if let Some(first) = children.first() {
                    return Err(format!(
                        "<{path}> allows no elements, found <{}>",
                        first.tag_name().name()
                    ));
                }
            }
            Some(model) => {
                let consumed = self.particle(&children, 0, model, path)?;
                if let Some(extra) = children.get(consumed) {
                    return Err(format!(
                        "<{path}>: <{}> is not allowed there",
                        extra.tag_name().name()
                    ));
                }
            }
        }
        for child in &children {
            let declaration = self
                .declaration_of(model, child.tag_name().name())?
                .ok_or_else(|| format!("<{path}>: <{}> is undeclared", child.tag_name().name()))?;
            self.element(*child, declaration)?;
        }
        Ok(())
    }

    /// The declaration a content model gives an element name.
    fn declaration_of(
        &self,
        model: Option<Node<'a, 'a>>,
        name: &str,
    ) -> Result<Option<Node<'a, 'a>>, String> {
        let Some(model) = model else {
            return Ok(None);
        };
        for particle in model.descendants().filter(|n| is_xs(*n, "element")) {
            let resolved = self.resolve(particle)?;
            if resolved.attribute("name") == Some(name) {
                return Ok(Some(resolved));
            }
        }
        Ok(None)
    }

    /// Matches `particle` against `children[at..]`, returning where it stopped.
    fn particle(
        &self,
        children: &[Node<'_, '_>],
        at: usize,
        particle: Node<'a, 'a>,
        path: &str,
    ) -> Result<usize, String> {
        let (min, max) = (
            occurs(particle, "minOccurs")?.unwrap_or(0),
            occurs(particle, "maxOccurs")?,
        );
        let mut position = at;
        let mut count = 0;
        loop {
            if max.is_some_and(|m| count >= m) {
                break;
            }
            let next = self.once(children, position, particle, path)?;
            match next {
                Some(next) if next > position => {
                    position = next;
                    count += 1;
                }
                // A match that consumed nothing (an empty sequence) satisfies it without looping.
                Some(_) => {
                    count = count.max(min);
                    break;
                }
                None => break,
            }
        }
        if count < min {
            return Err(format!(
                "<{path}>: {count} of {}, fewer than minOccurs {min}",
                describe(particle)
            ));
        }
        Ok(position)
    }

    /// One occurrence of `particle` at `children[at..]`: where it stopped, or `None`.
    fn once(
        &self,
        children: &[Node<'_, '_>],
        at: usize,
        particle: Node<'a, 'a>,
        path: &str,
    ) -> Result<Option<usize>, String> {
        if is_xs(particle, "element") {
            let name = self
                .resolve(particle)?
                .attribute("name")
                .unwrap_or_default();
            return Ok(children
                .get(at)
                .filter(|c| c.tag_name().name() == name)
                .map(|_| at + 1));
        }
        if is_xs(particle, "sequence") {
            let mut position = at;
            for item in xs_children(particle) {
                position = self.particle(children, position, item, path)?;
            }
            return Ok(Some(position));
        }
        if is_xs(particle, "choice") {
            for item in xs_children(particle) {
                let before = at;
                match self.particle(children, at, item, path) {
                    Ok(next) if next > before => return Ok(Some(next)),
                    Ok(_) | Err(_) => {}
                }
            }
            return Ok(None);
        }
        Err(format!(
            "<{path}>: unsupported content construct {}",
            describe(particle)
        ))
    }

    fn attributes(&self, instance: Node<'_, '_>, complex: Node<'a, 'a>) -> Result<(), String> {
        let path = instance.tag_name().name();
        let declared: Vec<Node<'a, 'a>> = xs_children(complex)
            .filter(|n| is_xs(*n, "attribute"))
            .collect();
        for attribute in instance.attributes() {
            let Some(declaration) = declared
                .iter()
                .find(|d| d.attribute("name") == Some(attribute.name()))
            else {
                return Err(format!(
                    "<{path}>: attribute `{}` is undeclared",
                    attribute.name()
                ));
            };
            match declaration.attribute("type") {
                None | Some("xs:string") => {}
                Some(name) => self.simple_value(name, attribute.value(), path)?,
            }
        }
        for declaration in &declared {
            let name = declaration.attribute("name").unwrap_or_default();
            if declaration.attribute("use") == Some("required")
                && instance.attribute(name).is_none()
            {
                return Err(format!("<{path}>: required attribute `{name}` is missing"));
            }
        }
        Ok(())
    }

    fn simple_value(&self, type_name: &str, value: &str, path: &str) -> Result<(), String> {
        let simple = self.global("simpleType", type_name)?;
        let restriction = xs_children(simple)
            .find(|n| is_xs(*n, "restriction") && n.attribute("base") == Some("xs:string"))
            .ok_or_else(|| format!("simpleType {type_name}: unsupported definition"))?;
        for facet in xs_children(restriction) {
            if !is_xs(facet, "pattern") {
                return Err(format!(
                    "simpleType {type_name}: unsupported facet {}",
                    describe(facet)
                ));
            }
            let pattern = facet.attribute("value").unwrap_or_default();
            let regex =
                regex::Regex::new(&format!("^(?:{pattern})$")).map_err(|e| e.to_string())?;
            if !regex.is_match(value) {
                return Err(format!(
                    "<{path}>: `{value}` does not match {type_name} ({pattern})"
                ));
            }
        }
        Ok(())
    }
}

fn describe(node: Node<'_, '_>) -> String {
    format!(
        "xs:{}{}",
        node.tag_name().name(),
        node.attribute("ref")
            .or(node.attribute("name"))
            .map(|n| format!(" {n}"))
            .unwrap_or_default()
    )
}

fn junit_schema() -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/schemas/junit-10.xsd"),
    )?)
}

fn upstream_results() -> Result<Vec<(String, Value)>, Box<dyn Error>> {
    let fixtures = root().join("conformance/dependency-cruiser/fixtures");
    let index: Value = serde_json::from_str(&std::fs::read_to_string(
        fixtures.join("report-json/INDEX.json"),
    )?)?;
    let mut files: Vec<String> = Vec::new();
    for entry in index["valid"].as_array().into_iter().flatten() {
        files.extend(entry.as_str().map(str::to_owned));
    }
    for entry in index["invalid"].as_array().into_iter().flatten() {
        files.extend(entry["file"].as_str().map(str::to_owned));
    }
    let mut out = Vec::new();
    for file in files {
        let value: Value = serde_json::from_str(&std::fs::read_to_string(fixtures.join(&file))?)?;
        out.push((file, value));
    }
    Ok(out)
}

fn rulebearing_result() -> Value {
    json!({
        "modules": [{ "source": "src/a.ts", "dependencies": [{ "resolved": "src/b.ts", "line": 4, "column": 1 }] }],
        "summary": {
            "violations": [
                { "type": "dependency", "from": "src/a.ts", "to": "src/b.ts", "rule": { "name": "no-b", "severity": "error" }, "id": "RB-4f2a9c1e" },
                { "type": "element", "from": "src/A.cs", "to": "S.A<T>", "rule": { "name": "sealed & \"final\"", "severity": "error" }, "id": "RB-00000002" },
                { "type": "element", "from": "src/B.cs", "to": "S.B", "rule": { "name": "sealed & \"final\"", "severity": "error" }, "id": "RB-00000003" },
                { "type": "slice", "from": "Slice1", "to": "Slice2", "rule": { "name": "apart", "severity": "warn" }, "id": "RB-00000004" },
                { "type": "dependency", "from": "src/c.ts", "to": "src/b.ts", "rule": { "name": "no-b", "severity": "ignore" }, "id": "RB-00000005" }
            ],
            "error": 3, "warn": 1, "info": 0, "ignore": 1, "totalCruised": 3,
            "inspected": { "typescript": { "files": 2, "assemblies": 0, "modules": 2 }, "dotnet": { "files": 2, "assemblies": 1, "modules": 2 } },
            "ruleSetUsed": {
                "forbidden": [{ "name": "no-b", "severity": "error", "fix": "Go through <index>." }, { "name": "dead" }],
                "elements": [{ "name": "sealed & \"final\"", "severity": "error" }],
                "slices": [{ "name": "apart", "severity": "warn" }]
            },
            "vacuousRules": [{ "name": "dead", "side": "from" }],
            "ratchets": [{ "name": "budget", "budget": "b.json", "count": 3, "ceiling": 2, "status": "exceeded" }],
            "expired": [{ "name": "RB-9", "expires": "2026-01-01", "kind": "knownViolation" }]
        }
    })
}

fn inputs() -> Result<Vec<(String, Value)>, Box<dyn Error>> {
    let mut all = upstream_results()?;
    assert!(all.len() > 100, "the gate 1 fixtures are present");
    all.push(("rulebearing".into(), rulebearing_result()));
    Ok(all)
}

fn render(output_type: &str, result: &Value) -> Result<String, Box<dyn Error>> {
    let options = rb_report::ReportOptions {
        timestamp: "2026-09-21T14:13:20.000".into(),
        ..rb_report::ReportOptions::default()
    };
    Ok(rb_report::render(output_type, result, &options)?.output)
}

#[test]
fn every_junit_report_validates_against_junit_10_xsd() -> Result<(), Box<dyn Error>> {
    let schema_text = junit_schema()?;
    let schema_document = Document::parse(&schema_text)?;
    let xsd = Xsd::new(&schema_document)?;
    let mut failures = Vec::new();
    let mut cases = 0;
    for (name, input) in inputs()? {
        let output = render("junit", &input)?;
        let document = Document::parse(&output).map_err(|e| format!("{name}: {e}"))?;
        cases += document
            .descendants()
            .filter(|n| n.has_tag_name("testcase"))
            .count();
        if let Err(e) = xsd.validate(document.root_element()) {
            failures.push(format!("{name}: {e}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(cases > 100, "{cases} test cases");
    Ok(())
}

#[test]
fn the_junit_validator_rejects_what_the_schema_forbids() -> Result<(), Box<dyn Error>> {
    let schema_text = junit_schema()?;
    let schema_document = Document::parse(&schema_text)?;
    let xsd = Xsd::new(&schema_document)?;
    let check = |xml: &str| -> Result<(), String> {
        let document = Document::parse(xml).map_err(|e| e.to_string())?;
        xsd.validate(document.root_element())
    };
    assert_eq!(
        check(
            r#"<testsuites><testsuite name="a" tests="1" failures="0" errors="0"><testcase name="t"/></testsuite></testsuites>"#
        ),
        Ok(())
    );
    let rejected = [
        (
            r#"<testsuites><testsuite name="a" tests="1" failures="0"/></testsuites>"#,
            "required attribute `errors`",
        ),
        (
            r#"<testsuites><testsuite name="a" tests="1" failures="0" errors="0" colour="red"/></testsuites>"#,
            "`colour` is undeclared",
        ),
        (
            r#"<testsuites><testcase name="t"/></testsuites>"#,
            "not allowed there",
        ),
        (
            r#"<testsuites time="1.2.3"/>"#,
            "does not match SUREFIRE_TIME",
        ),
        (r"<testsuites>text</testsuites>", "does not allow text"),
        (
            r#"<testsuites><testsuite name="a" tests="1" failures="0" errors="0"><testcase name="t"><system-out><b/></system-out></testcase></testsuite></testsuites>"#,
            "xs:string",
        ),
        (
            r#"<testsuites><testsuite name="a" tests="1" failures="0" errors="0"><properties><property name="x"/></properties></testsuite></testsuites>"#,
            "required attribute `value`",
        ),
        ("<testcases/>", "no global element"),
    ];
    for (xml, expected) in rejected {
        let error = check(xml).err().unwrap_or_default();
        assert!(error.contains(expected), "{xml}: {error}");
    }
    Ok(())
}

const GUID: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";
const OUTCOMES: &[&str] = &[
    "Error",
    "Failed",
    "Timeout",
    "Aborted",
    "Passed",
    "PassedButRunAborted",
    "NotRunnable",
    "NotExecuted",
    "Disconnected",
    "Warning",
    "Completed",
    "InProgress",
    "Pending",
    "Inconclusive",
];
const TRX: &str = "http://microsoft.com/schemas/VisualStudio/TeamTest/2010";

fn required<'a>(node: Node<'a, 'a>, names: &[&str]) -> Result<Vec<&'a str>, String> {
    names
        .iter()
        .map(|n| {
            node.attribute(*n).ok_or_else(|| {
                format!(
                    "<{}> lacks the required attribute `{n}`",
                    node.tag_name().name()
                )
            })
        })
        .collect()
}

fn one<'a>(parent: Node<'a, 'a>, name: &str) -> Result<Node<'a, 'a>, String> {
    let found: Vec<Node<'a, 'a>> = parent
        .children()
        .filter(|c| c.has_tag_name((TRX, name)))
        .collect();
    match found.as_slice() {
        [only] => Ok(*only),
        _ => Err(format!(
            "<{}> holds {} <{name}>, not one",
            parent.tag_name().name(),
            found.len()
        )),
    }
}

/// The structure `vstst.xsd` requires of a `TestRun` as `vstest` writes it.
#[expect(
    clippy::too_many_lines,
    reason = "one function states the whole structure vstst.xsd requires, in document order"
)]
fn check_trx(text: &str) -> Result<(), String> {
    let document = Document::parse(text).map_err(|e| e.to_string())?;
    let guid = regex::Regex::new(GUID).map_err(|e| e.to_string())?;
    let is_guid = |value: &str, what: &str| -> Result<(), String> {
        if guid.is_match(value) {
            Ok(())
        } else {
            Err(format!("{what} `{value}` is not a GUID"))
        }
    };
    let run = document.root_element();
    if !run.has_tag_name((TRX, "TestRun")) {
        return Err(format!(
            "the root is {:?}, not TestRun in {TRX}",
            run.tag_name()
        ));
    }
    let [id, _] = required(run, &["id", "name"])?[..] else {
        return Err("TestRun".into());
    };
    is_guid(id, "TestRun id")?;
    let order: Vec<&str> = run
        .children()
        .filter(Node::is_element)
        .map(|c| c.tag_name().name())
        .collect();
    if order
        != [
            "Times",
            "Results",
            "TestDefinitions",
            "TestEntries",
            "TestLists",
            "ResultSummary",
        ]
    {
        return Err(format!("TestRun holds {order:?}"));
    }
    required(
        one(run, "Times")?,
        &["creation", "queuing", "start", "finish"],
    )?;
    let lists: BTreeSet<&str> = one(run, "TestLists")?
        .children()
        .filter(Node::is_element)
        .map(|l| required(l, &["name", "id"]).map(|v| v[1]))
        .collect::<Result<_, _>>()?;
    for list in &lists {
        is_guid(list, "TestList id")?;
    }
    let mut definitions = BTreeSet::new();
    for test in one(run, "TestDefinitions")?
        .children()
        .filter(Node::is_element)
    {
        if !test.has_tag_name((TRX, "UnitTest")) {
            return Err(format!(
                "TestDefinitions holds <{}>",
                test.tag_name().name()
            ));
        }
        let values = required(test, &["name", "id", "storage"])?;
        is_guid(values[1], "UnitTest id")?;
        let execution = required(one(test, "Execution")?, &["id"])?[0];
        is_guid(execution, "Execution id")?;
        required(
            one(test, "TestMethod")?,
            &["codeBase", "className", "name", "adapterTypeName"],
        )?;
        definitions.insert((values[1], execution));
    }
    let mut results = BTreeSet::new();
    let mut tally = [0u64; 3];
    for result in one(run, "Results")?.children().filter(Node::is_element) {
        if !result.has_tag_name((TRX, "UnitTestResult")) {
            return Err(format!("Results holds <{}>", result.tag_name().name()));
        }
        let values = required(
            result,
            &[
                "executionId",
                "testId",
                "testName",
                "computerName",
                "testType",
                "outcome",
                "testListId",
                "duration",
                "startTime",
                "endTime",
            ],
        )?;
        for (index, what) in [
            (0, "executionId"),
            (1, "testId"),
            (4, "testType"),
            (6, "testListId"),
        ] {
            is_guid(values[index], what)?;
        }
        if !OUTCOMES.contains(&values[5]) {
            return Err(format!(
                "outcome `{}` is not in vstst.xsd's TestOutcome",
                values[5]
            ));
        }
        if !lists.contains(values[6]) {
            return Err(format!("testListId {} names no TestList", values[6]));
        }
        tally[match values[5] {
            "Passed" => 0,
            "Failed" => 1,
            _ => 2,
        }] += 1;
        if let Some(output) = result.children().find(|c| c.has_tag_name((TRX, "Output"))) {
            for child in output.children().filter(Node::is_element) {
                match child.tag_name().name() {
                    "StdOut" => {}
                    "ErrorInfo" => {
                        one(child, "Message")?;
                    }
                    other => return Err(format!("Output holds <{other}>")),
                }
            }
        }
        results.insert((values[1], values[0]));
    }
    if results != definitions {
        return Err("each UnitTestResult must have its UnitTest and Execution, and no more".into());
    }
    let mut entries = BTreeSet::new();
    for entry in one(run, "TestEntries")?.children().filter(Node::is_element) {
        if !entry.has_tag_name((TRX, "TestEntry")) {
            return Err(format!("TestEntries holds <{}>", entry.tag_name().name()));
        }
        let values = required(entry, &["testId", "executionId", "testListId"])?;
        if !lists.contains(values[2]) {
            return Err(format!(
                "TestEntry testListId {} names no TestList",
                values[2]
            ));
        }
        entries.insert((values[0], values[1]));
    }
    if entries != definitions {
        return Err("each test needs one TestEntry".into());
    }
    let summary = one(run, "ResultSummary")?;
    let outcome = required(summary, &["outcome"])?[0];
    if !OUTCOMES.contains(&outcome) {
        return Err(format!("ResultSummary outcome `{outcome}`"));
    }
    let counters = one(summary, "Counters")?;
    let names = [
        "total",
        "executed",
        "passed",
        "failed",
        "error",
        "timeout",
        "aborted",
        "inconclusive",
        "passedButRunAborted",
        "notRunnable",
        "notExecuted",
        "disconnected",
        "warning",
        "completed",
        "inProgress",
        "pending",
    ];
    let values: Vec<u64> = required(counters, &names)?
        .iter()
        .map(|v| {
            v.parse::<u64>()
                .map_err(|_| format!("counter `{v}` is not an int"))
        })
        .collect::<Result<_, _>>()?;
    let total = results.len() as u64;
    if values[..5] != [total, total, tally[0], tally[1], tally[2]] {
        return Err(format!(
            "Counters {:?} do not count the results {tally:?} of {total}",
            &values[..5]
        ));
    }
    Ok(())
}

#[test]
fn every_trx_report_has_the_structure_vstst_xsd_requires() -> Result<(), Box<dyn Error>> {
    let mut failures = Vec::new();
    for (name, input) in inputs()? {
        if let Err(e) = check_trx(&render("trx", &input)?) {
            failures.push(format!("{name}: {e}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let rulebearing = render("trx", &rulebearing_result())?;
    assert!(
        rulebearing.contains("outcome=\"Error\""),
        "the vacuous rule and the expired entry"
    );
    assert!(rulebearing.contains("<StackTrace>RB-00000002 src/A.cs -&gt; S.A&lt;T&gt;\nRB-00000003 src/B.cs -&gt; S.B</StackTrace>"), "one line per object");
    Ok(())
}

#[test]
fn the_trx_checks_reject_what_vstst_xsd_forbids() -> Result<(), Box<dyn Error>> {
    let good = render("trx", &rulebearing_result())?;
    assert_eq!(check_trx(&good), Ok(()));
    let broken = [
        (
            good.replace(
                "outcome=\"Failed\" testListId",
                "outcome=\"Broken\" testListId",
            ),
            "TestOutcome",
        ),
        (
            good.replacen(" computerName=\"rulebearing\"", "", 1),
            "computerName",
        ),
        (good.replace(TRX, "urn:other"), "not TestRun"),
        (
            good.replacen("<TestEntry ", "<TestEntryX ", 1),
            "TestEntries holds <TestEntryX>",
        ),
        (
            good.replacen("<Execution id=\"", "<Execution id=\"0", 1),
            "not a GUID",
        ),
    ];
    for (text, expected) in &broken {
        let error = check_trx(text).err().unwrap_or_default();
        assert!(error.contains(expected), "{expected}: {error}");
    }
    let miscounted = good.replacen("passed=\"", "passed=\"9", 1);
    assert!(check_trx(&miscounted).is_err_and(|e| e.contains("Counters")));
    Ok(())
}
