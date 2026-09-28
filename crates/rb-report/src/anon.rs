//! `anon`: the result document as `json` writes it, with every module name anonymised, so a
//! graph can be shared without its names. dependency-cruiser 18.2.0's `src/report/anon/`
//! (`index.mjs`, `anonymize-path.mjs`, `anonymize-path-element.mjs`, `random-string.mjs`), ported.
//!
//! - Specification: `test/report/anon/*.spec.mjs`, run unmodified by conformance gate 1 layer 3,
//!   and upstream's reporter over every `test/report` mock
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Coverage: [coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types),
//!   row `anon`; [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   row `reporterOptions.anon.wordlist`
//! - Plan: [Wave 3, Step 6](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01)
//!
//! Each path is split on `/` and each element on `.`; the part before the first dot of an element
//! that [`WHITELIST_RE`] does not match is replaced by the next word of the word list, and the
//! same part is replaced by the same word everywhere in the document. The word list is
//! `reporterOptions.anon.wordlist`, cleaned as upstream cleans it; upstream bundles no word list,
//! and its default is the empty one. When the words run out, a part is replaced by a string of
//! its shape: letters for letters with their case, digits for digits, separators kept.
//!
//! Upstream draws those strings from `crypto.randomInt`, so its output differs from run to run.
//! Here the string is drawn with upstream's ranges (`a` to `y`, `0` to `8`, because `randomInt`'s
//! upper bound is exclusive) from a generator keyed by a SHA-256 digest of the whole document
//! (after the strip below) and the part, so the same input is anonymised byte for byte the same on
//! every run, while a name cannot be recovered by anonymising candidate names, nor linked across
//! two documents, without the rest of the document. That is the one documented divergence. With a
//! word list long enough for the document, as upstream's specs use, the output is upstream's.
//!
//! Rulebearing's additions to the document ([ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md))
//! name modules, namespaces and types in fields upstream's anonymiser does not know (the code
//! layer, `namespaces`, `project`, `summary.affected`, `fix`, ...), so they are stripped first, as
//! `--strict-schema` strips them ([`crate::json::strip`]), and the output has upstream's shape.
//! What the additions say about names is still used: every identifier of a namespace, a type, an
//! assembly or a project in the code layer and on the modules is replaced wherever it appears in a
//! path, not only before the first dot (`src/Acme.Billing/Gateway.cs` loses `Billing` too), and the
//! object an element or slice violation names is replaced identifier by identifier. A document
//! without those additions, which is every document dependency-cruiser writes, is anonymised as
//! upstream anonymises it. The rule set, its comments, a violation's `unresolvedTo` and
//! `optionsUsed` are printed as upstream prints them.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::Rendered;

/// Path elements left as they are, verbatim from upstream's `anonymize-path.mjs`.
pub const WHITELIST_RE: &str = r"^(?:[.]+|~|bin|apps?|cli|src|libs?|configs?|components?|fixtures?|helpers?|i18n|index\.(?:jsx?|[mc]js|d\.ts|tsx?|vue|coffee|ls)|_?_?mocks?_?_?|node_modules|packages?|package\.json|scripts?|services?|sources?|specs?|_?_?tests?_?_?|types?|uti?ls?|tools)$";

/// A character's class, as `random-string.mjs` classifies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Number,
    Separator,
    Uppercase,
    NothingSpecial,
}

fn classify(c: char) -> CharClass {
    if c.is_ascii_digit() {
        CharClass::Number
    } else if matches!(c, '-' | '_' | '.') {
        CharClass::Separator
    } else if c.to_uppercase().eq(std::iter::once(c)) {
        // `char.toUpperCase() === char`: true for an upper case letter and for anything without
        // case, which upstream treats alike.
        CharClass::Uppercase
    } else {
        CharClass::NothingSpecial
    }
}

/// The key [`random_string`] draws with when there is no document: the protocol's unit calls.
pub const NO_DOCUMENT: [u8; 32] = [0; 32];

/// splitmix64: a small, well-mixed generator; the stream for a part is seeded by
/// `SHA-256(key || part)`.
struct Draw(u64);

impl Draw {
    fn seeded(key: &[u8; 32], text: &str) -> Self {
        let digest = Sha256::new()
            .chain_update(key)
            .chain_update(text.as_bytes())
            .finalize();
        let mut seed = [0_u8; 8];
        seed.copy_from_slice(&digest[..8]);
        Self(u64::from_le_bytes(seed))
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// `randomInt(0, bound)`: `0` up to `bound - 1`.
    fn below(&mut self, bound: u8) -> u8 {
        u8::try_from(self.next() % u64::from(bound)).unwrap_or(0)
    }
}

/// `getRandomString(text)`: a string of the same shape, one character per code point: a digit
/// for a digit, a lower case letter for a lower case one, an upper case letter for an upper case
/// one (or a character without case), and `-`, `_` and `.` kept; drawn with `key`, the digest of
/// the document being anonymised ([`document_key`]).
pub fn random_string(key: &[u8; 32], text: &str) -> String {
    let mut draw = Draw::seeded(key, text);
    text.chars()
        .map(|c| match classify(c) {
            CharClass::Separator => c,
            CharClass::Number => char::from(b'0' + draw.below(9)),
            CharClass::Uppercase => char::from(b'A' + draw.below(25)),
            CharClass::NothingSpecial => char::from(b'a' + draw.below(25)),
        })
        .collect()
}

/// What anonymises one document: the words not used yet and the parts replaced so far
/// (upstream's module-level `ALREADY_USED_WORDS`, which lives for one report).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Anonymizer {
    /// The words left, taken from the front.
    pub words: VecDeque<String>,
    /// Each part replaced so far, with what replaced it.
    pub cache: BTreeMap<String, String>,
    /// The key random strings are drawn with ([`document_key`]).
    pub key: [u8; 32],
    /// Identifiers of namespaces, types, assemblies and projects, replaced wherever they appear
    /// in a path ([`name_identifiers`]).
    pub names: BTreeSet<String>,
}

impl Anonymizer {
    /// An anonymiser drawing from `words`, used as given, with no document key and no names.
    pub fn new(words: impl IntoIterator<Item = String>) -> Self {
        Self {
            words: words.into_iter().collect(),
            cache: BTreeMap::new(),
            key: NO_DOCUMENT,
            names: BTreeSet::new(),
        }
    }

    fn replace(&mut self, part: &str, index: usize) -> String {
        if index == 0 {
            let key = self.key;
            self.words
                .pop_front()
                .filter(|w| !w.is_empty())
                .unwrap_or_else(|| random_string(&key, part))
        } else {
            part.to_owned()
        }
    }

    fn replace_cached(&mut self, part: &str, index: usize) -> String {
        if let Some(done) = self.cache.get(part) {
            return done.clone();
        }
        let replaced = self.replace(part, index);
        self.cache.insert(part.to_owned(), replaced.clone());
        replaced
    }

    /// `anonymizePathElement(element, words, whitelist, cached)`: the element unchanged when
    /// `whitelist` (a pattern) matches it, else its part before the first dot replaced.
    pub fn path_element(&mut self, element: &str, whitelist: &str, cached: bool) -> String {
        if rb_rules::patterns::test(whitelist, element) {
            return element.to_owned();
        }
        element
            .split('.')
            .enumerate()
            .map(|(index, part)| {
                // A namespace, type or project identifier is replaced after a dot too.
                let index = if self.names.contains(part) { 0 } else { index };
                if cached {
                    self.replace_cached(part, index)
                } else {
                    self.replace(part, index)
                }
            })
            .collect::<Vec<_>>()
            .join(".")
    }

    /// `anonymizePath(path, words, whitelist)`: each `/`-separated element anonymised.
    pub fn path(&mut self, path: &str, whitelist: &str) -> String {
        path.split('/')
            .map(|element| self.path_element(element, whitelist, true))
            .collect::<Vec<_>>()
            .join("/")
    }

    /// A name that is not a path (the object an element or slice violation names, such as
    /// `System.Void Acme.Billing.Gateway::Charge()`): each run of letters, digits and `_` replaced,
    /// every other character kept.
    pub fn name(&mut self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut run = String::new();
        for c in text.chars().chain(std::iter::once('\0')) {
            if c.is_alphanumeric() || c == '_' {
                run.push(c);
                continue;
            }
            if !run.is_empty() {
                let replaced = if rb_rules::patterns::test(WHITELIST_RE, &run) {
                    run.clone()
                } else {
                    self.replace_cached(&run, 0)
                };
                out.push_str(&replaced);
                run.clear();
            }
            if c != '\0' {
                out.push(c);
            }
        }
        out
    }

    /// A string at `key` of `object` anonymised with [`WHITELIST_RE`]; any other value is left as
    /// it is, where upstream would stop with a `TypeError`.
    fn field(&mut self, object: &mut Map<String, Value>, key: &str) {
        if let Some(Value::String(text)) = object.get(key) {
            let anonymised = self.path(&text.clone(), WHITELIST_RE);
            object.insert(key.to_owned(), Value::String(anonymised));
        }
    }

    /// `anonymizeMiniDependencyArray(list)`: each entry with its `name` anonymised; a missing list
    /// is the empty one.
    fn mini_dependencies(&mut self, list: Option<&Value>) -> Value {
        let items = list.and_then(Value::as_array).cloned().unwrap_or_default();
        Value::Array(items.into_iter().map(|item| self.named(item)).collect())
    }

    fn named(&mut self, item: Value) -> Value {
        match item {
            Value::Object(mut map) => {
                self.field(&mut map, "name");
                Value::Object(map)
            }
            other => other,
        }
    }

    fn dependency(&mut self, dependency: Value) -> Value {
        let Value::Object(mut map) = dependency else {
            return dependency;
        };
        self.field(&mut map, "resolved");
        self.field(&mut map, "module");
        let cycle = self.mini_dependencies(map.get("cycle"));
        map.insert("cycle".into(), cycle);
        Value::Object(map)
    }

    fn reaches(&mut self, reaches: Value) -> Value {
        let Value::Object(mut map) = reaches else {
            return reaches;
        };
        if let Some(Value::Array(modules)) = map.get("modules").cloned() {
            let anonymised: Vec<Value> = modules
                .into_iter()
                .map(|module| match module {
                    Value::Object(mut m) => {
                        self.field(&mut m, "source");
                        let via = self.mini_dependencies(m.get("via"));
                        m.insert("via".into(), via);
                        Value::Object(m)
                    }
                    other => other,
                })
                .collect();
            map.insert("modules".into(), Value::Array(anonymised));
        }
        Value::Object(map)
    }

    fn module(&mut self, module: Value) -> Value {
        let Value::Object(mut map) = module else {
            return module;
        };
        if let Some(Value::Array(dependencies)) = map.get("dependencies").cloned() {
            let anonymised = dependencies
                .into_iter()
                .map(|d| self.dependency(d))
                .collect();
            map.insert("dependencies".into(), Value::Array(anonymised));
        }
        self.field(&mut map, "source");
        if crate::truthy(map.get("dependents"))
            && let Some(Value::Array(dependents)) = map.get("dependents").cloned()
        {
            let anonymised = dependents
                .into_iter()
                .map(|d| match d {
                    Value::String(path) => Value::String(self.path(&path, WHITELIST_RE)),
                    other => other,
                })
                .collect();
            map.insert("dependents".into(), Value::Array(anonymised));
        }
        if crate::truthy(map.get("reaches"))
            && let Some(Value::Array(reaches)) = map.get("reaches").cloned()
        {
            let anonymised = reaches.into_iter().map(|r| self.reaches(r)).collect();
            map.insert("reaches".into(), Value::Array(anonymised));
        }
        Value::Object(map)
    }

    fn folder(&mut self, folder: Value) -> Value {
        let Value::Object(mut map) = folder else {
            return folder;
        };
        self.field(&mut map, "name");
        if crate::truthy(map.get("dependencies"))
            && let Some(Value::Array(dependencies)) = map.get("dependencies").cloned()
        {
            let anonymised = dependencies
                .into_iter()
                .map(|dependency| match dependency {
                    Value::Object(mut d) => {
                        self.field(&mut d, "name");
                        if crate::truthy(d.get("cycle")) {
                            let cycle = self.mini_dependencies(d.get("cycle"));
                            d.insert("cycle".into(), cycle);
                        }
                        Value::Object(d)
                    }
                    other => other,
                })
                .collect();
            map.insert("dependencies".into(), Value::Array(anonymised));
        }
        if crate::truthy(map.get("dependents"))
            && let Some(Value::Array(dependents)) = map.get("dependents").cloned()
        {
            let anonymised = dependents.into_iter().map(|d| self.named(d)).collect();
            map.insert("dependents".into(), Value::Array(anonymised));
        }
        Value::Object(map)
    }

    fn violation(&mut self, violation: Value) -> Value {
        let Value::Object(mut map) = violation else {
            return violation;
        };
        let object = matches!(
            map.get("type").and_then(Value::as_str),
            Some("element" | "slice")
        );
        if object {
            // An element or slice violation's ends are a file or an object's name.
            for key in ["from", "to"] {
                if let Some(Value::String(text)) = map.get(key).cloned() {
                    let anonymised = if text.contains('/') {
                        self.path(&text, WHITELIST_RE)
                    } else {
                        self.name(&text)
                    };
                    map.insert(key.into(), Value::String(anonymised));
                }
            }
        } else {
            self.field(&mut map, "from");
            self.field(&mut map, "to");
        }
        let cycle = self.mini_dependencies(map.get("cycle"));
        map.insert("cycle".into(), cycle);
        if crate::truthy(map.get("via")) {
            let via = self.mini_dependencies(map.get("via"));
            map.insert("via".into(), via);
        }
        Value::Object(map)
    }

    /// `anonymize(result, words)`: the modules, then the folders, then the violations, in that
    /// order, which decides which part gets which word.
    pub fn document(&mut self, result: &Value) -> Value {
        let mut out = result.clone();
        if let Some(Value::Array(modules)) = out.get("modules").cloned() {
            let anonymised = modules.into_iter().map(|m| self.module(m)).collect();
            out["modules"] = Value::Array(anonymised);
        }
        if crate::truthy(out.get("folders"))
            && let Some(Value::Array(folders)) = out.get("folders").cloned()
        {
            let anonymised = folders.into_iter().map(|f| self.folder(f)).collect();
            out["folders"] = Value::Array(anonymised);
        }
        if let Some(Value::Array(violations)) = out
            .get("summary")
            .and_then(|s| s.get("violations"))
            .cloned()
        {
            let anonymised = violations.into_iter().map(|v| self.violation(v)).collect();
            out["summary"]["violations"] = Value::Array(anonymised);
        }
        out
    }
}

/// `sanitizeWordList(words)`: every UTF-16 code unit outside `a-z`, `A-Z` and `-` replaced by
/// `_`, then the empty words and those [`WHITELIST_RE`] matches dropped. A word that is not a
/// string is dropped, where upstream would stop with a `TypeError`.
pub fn sanitize_word_list(words: &[Value]) -> Vec<String> {
    words
        .iter()
        .filter_map(Value::as_str)
        .map(|word| {
            word.encode_utf16()
                .map(|unit| match u8::try_from(unit) {
                    Ok(b) if b.is_ascii_alphabetic() || b == b'-' => char::from(b),
                    _ => '_',
                })
                .collect::<String>()
        })
        .filter(|word| !word.is_empty() && !rb_rules::patterns::test(WHITELIST_RE, word))
        .collect()
}

/// The word list: `options.wordlist` when it is given and truthy, else
/// `summary.optionsUsed.reporterOptions.anon.wordlist`, else none.
pub fn word_list(result: &Value, options: Option<&Value>) -> Vec<Value> {
    let given = options
        .and_then(|o| o.get("wordlist"))
        .filter(|w| crate::truthy(Some(w)));
    let list = given.or_else(|| {
        result
            .get("summary")
            .and_then(|s| s.get("optionsUsed"))
            .and_then(|o| o.get("reporterOptions"))
            .and_then(|r| r.get("anon"))
            .and_then(|a| a.get("wordlist"))
    });
    list.and_then(Value::as_array).cloned().unwrap_or_default()
}

/// Each identifier (a run of letters, digits and `_`) of `text`.
fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
}

/// The identifiers of every namespace, type, assembly and project the document's additions name:
/// the code layer's types, members and calls, and the modules' `namespaces` and `project`.
pub fn name_identifiers(result: &Value) -> BTreeSet<String> {
    fn strings<'a>(texts: &mut Vec<&'a str>, value: Option<&'a Value>) {
        match value {
            Some(Value::String(s)) => texts.push(s),
            Some(Value::Array(items)) => texts.extend(items.iter().filter_map(Value::as_str)),
            _ => {}
        }
    }
    let mut texts: Vec<&str> = Vec::new();
    let code = result.get("code");
    for t in code.map_or(&[][..], |c| rb_rules::js::array(c, "types")) {
        for key in [
            "fullName",
            "name",
            "namespace",
            "assembly",
            "assemblyFullName",
            "assemblyQualifiedName",
            "baseType",
            "baseTypes",
            "interfaces",
            "nestedIn",
        ] {
            strings(&mut texts, t.get(key));
        }
    }
    for m in code.map_or(&[][..], |c| rb_rules::js::array(c, "members")) {
        for key in ["declaringType", "name", "fullName", "returnType"] {
            strings(&mut texts, m.get(key));
        }
    }
    for c in code.map_or(&[][..], |c| rb_rules::js::array(c, "calls")) {
        for key in ["from", "to"] {
            strings(&mut texts, c.get(key));
        }
    }
    for m in rb_rules::js::array(result, "modules") {
        strings(&mut texts, m.get("namespaces"));
        strings(&mut texts, m.get("project"));
    }
    texts
        .into_iter()
        .flat_map(identifiers)
        .filter(|id| !rb_rules::patterns::test(WHITELIST_RE, id))
        .map(str::to_owned)
        .collect()
}

/// The key the document's random strings are drawn with: SHA-256 of the stripped document as
/// JSON, so it depends on every name the document holds and on nothing else.
pub fn document_key(stripped: &Value) -> [u8; 32] {
    let mut key = [0_u8; 32];
    key.copy_from_slice(&Sha256::digest(stripped.to_string().as_bytes()));
    key
}

/// Renders `anon`, with `options` the `reporterOptions.anon` section. Always exits 0, as
/// upstream's reporter does. Rulebearing's additions are stripped first (see the module
/// documentation).
pub fn render(result: &Value, options: Option<&Value>) -> Rendered {
    let words = sanitize_word_list(&word_list(result, options));
    let names = name_identifiers(result);
    let mut stripped = result.clone();
    crate::json::strip(&mut stripped);
    let mut anonymizer = Anonymizer::new(words);
    anonymizer.key = document_key(&stripped);
    anonymizer.names = names;
    let anonymised = anonymizer.document(&stripped);
    let mut output = serde_json::to_string_pretty(&anonymised).unwrap_or_default();
    output.push('\n');
    Rendered {
        output,
        exit_code: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::json;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn paths_as_upstream_s_specs_have_them() {
        let mut a = Anonymizer::default();
        assert_eq!(a.path("", WHITELIST_RE), "");
        assert_eq!(a.path("////", WHITELIST_RE), "////");
        let mut a = Anonymizer::new(words(&["foo", "bar", "baz"]));
        assert_eq!(
            a.path("src/tien/kleine/geitjes/index.ts", WHITELIST_RE),
            "src/foo/bar/baz/index.ts"
        );
        let mut a = Anonymizer::new(words(&[
            "aap", "noot", "mies", "wim", "zus", "jet", "heide",
        ]));
        for (path, expected) in [
            (
                "src/tien/kleine/geitjes/index.ts",
                "src/aap/noot/mies/index.ts",
            ),
            (
                "src/tien/kleine/geitjes/tien.ts",
                "src/aap/noot/mies/aap.ts",
            ),
            (
                "shwoop/tien/grote/geiten/index.ts",
                "wim/aap/zus/jet/index.ts",
            ),
            (
                "test/tien/kleine/geitjes/tien.spec.ts",
                "test/aap/noot/mies/aap.spec.ts",
            ),
        ] {
            assert_eq!(a.path(path, WHITELIST_RE), expected);
        }
        assert_eq!(a.words, ["heide"]);
    }

    #[test]
    fn elements_uncached_and_cached() {
        let mut a = Anonymizer::new(words(&["aap", "noot"]));
        assert_eq!(a.path_element("", "^$", false), "");
        assert_eq!(a.path_element("one", "^$", false), "aap");
        assert_eq!(a.path_element("two", "^$", false), "noot");
        let three = a.path_element("three", "^$", false);
        assert_eq!(three.len(), 5);
        assert!(three.chars().all(|c| c.is_ascii_lowercase()));
        assert!(a.cache.is_empty(), "uncached calls cache nothing");
        let mut a = Anonymizer::new(words(&["aap", "noot"]));
        assert_eq!(a.path_element("package", "^packages?$", false), "package");
        assert_eq!(
            a.path_element("thing.spec.js", "^packages?$", false),
            "aap.spec.js"
        );
        let mut a = Anonymizer::default();
        let first = a.path_element("yudelyo", "^$", true);
        assert_eq!(a.path_element("yudelyo", "^$", true), first);
        assert_ne!(a.path_element("yoyudel", "^$", true), first);
        assert_eq!(a.cache.get("yudelyo"), Some(&first));
    }

    #[test]
    fn random_strings_keep_the_shape() {
        assert_eq!(random_string(&NO_DOCUMENT, ""), "");
        assert_eq!(random_string(&NO_DOCUMENT, "-"), "-");
        let s = random_string(&NO_DOCUMENT, "better-someStuff_operator");
        let shape: String = s
            .chars()
            .map(|c| match c {
                'a'..='z' => 'a',
                'A'..='Z' => 'A',
                other => other,
            })
            .collect();
        assert_eq!(shape, "aaaaaa-aaaaAaaaa_aaaaaaaa");
        assert!(
            random_string(&NO_DOCUMENT, "ü")
                .chars()
                .all(|c| c.is_ascii_lowercase())
        );
        assert!(
            random_string(&NO_DOCUMENT, "Ü")
                .chars()
                .all(|c| c.is_ascii_uppercase())
        );
        assert!(
            random_string(&NO_DOCUMENT, "@")
                .chars()
                .all(|c| c.is_ascii_uppercase())
        );
        assert!(
            random_string(&NO_DOCUMENT, "1")
                .chars()
                .all(|c| c.is_ascii_digit())
        );
        assert_eq!(
            random_string(&NO_DOCUMENT, "pulp2slurp"),
            random_string(&NO_DOCUMENT, "pulp2slurp")
        );
        assert_eq!(classify('ß'), CharClass::NothingSpecial);
    }

    #[test]
    fn word_lists_are_cleaned_and_found() {
        assert_eq!(
            sanitize_word_list(&[
                json!("foo"),
                json!("b4r"),
                json!("src"),
                json!(""),
                json!(3),
                json!("😀")
            ]),
            ["foo", "b_r", "__"]
        );
        let result = json!({ "summary": { "optionsUsed": { "reporterOptions": { "anon": { "wordlist": ["x"] } } } } });
        assert_eq!(word_list(&result, None), [json!("x")]);
        assert_eq!(
            word_list(&result, Some(&json!({ "wordlist": ["y"] }))),
            [json!("y")]
        );
        assert_eq!(word_list(&result, Some(&json!({}))), [json!("x")]);
        assert!(word_list(&json!({}), None).is_empty());
    }

    #[test]
    fn a_document_in_upstream_s_order() {
        let result = json!({
            "modules": [{
                "source": "lib/alpha.js",
                "dependencies": [{ "resolved": "lib/beta.js", "module": "./beta" }],
                "dependents": ["gamma.js"],
                "reaches": [{ "asDefinedInRule": "r", "modules": [{ "source": "delta.js", "via": [{ "name": "alpha.js" }] }] }],
            }],
            "folders": [{ "name": "lib/epsilon", "dependencies": [{ "name": "zeta", "cycle": [{ "name": "eta" }] }], "dependents": [{ "name": "theta" }] }],
            "summary": { "violations": [{ "from": "lib/alpha.js", "to": "iota.js", "via": [{ "name": "kappa" }], "rule": { "name": "r" } }] }
        });
        let words = words(&[
            "w1", "w2", "w3", "w4", "w5", "w6", "w7", "w8", "w9", "w10", "w11",
        ]);
        let mut anonymizer = Anonymizer::new(words);
        let out = anonymizer.document(&result);
        // The dependencies come before the module's own source; `./beta` reuses `beta`'s word.
        assert_eq!(
            out,
            json!({
                "modules": [{
                    "source": "lib/w2.js",
                    "dependencies": [{ "resolved": "lib/w1.js", "module": "./w1", "cycle": [] }],
                    "dependents": ["w3.js"],
                    "reaches": [{ "asDefinedInRule": "r", "modules": [{ "source": "w4.js", "via": [{ "name": "w2.js" }] }] }],
                }],
                "folders": [{ "name": "lib/w5", "dependencies": [{ "name": "w6", "cycle": [{ "name": "w7" }] }], "dependents": [{ "name": "w8" }] }],
                "summary": { "violations": [{ "from": "lib/w2.js", "to": "w9.js", "via": [{ "name": "w10" }], "rule": { "name": "r" }, "cycle": [] }] }
            })
        );
        assert_eq!(anonymizer.words, ["w11"]);
        // Keys keep their places; an added `cycle` goes last, as the object spread puts it.
        let printed = serde_json::to_string(&out["summary"]["violations"][0]).unwrap_or_default();
        assert_eq!(
            printed,
            r#"{"from":"lib/w2.js","to":"w9.js","via":[{"name":"w10"}],"rule":{"name":"r"},"cycle":[]}"#
        );
        let printed = serde_json::to_string(&out["modules"][0]).unwrap_or_default();
        assert!(
            printed.starts_with(r#"{"source":"lib/w2.js","dependencies":"#),
            "{printed}"
        );
        assert!(
            printed.contains(r#"{"asDefinedInRule":"r","modules":"#),
            "{printed}"
        );
    }

    #[test]
    fn renders_json_deterministically() {
        let result = json!({ "modules": [{ "source": "secret/thing.ts", "dependencies": [] }], "summary": { "violations": [] } });
        let rendered = render(&result, Some(&json!({ "wordlist": ["public"] })));
        assert_eq!(rendered.exit_code, 0);
        assert!(
            rendered.output.contains("\"source\": \"public/"),
            "{}",
            rendered.output
        );
        assert!(rendered.output.ends_with("}\n"));
        let random = render(&result, None);
        assert_eq!(
            render(&result, None),
            random,
            "the same input, the same strings"
        );
        assert!(!random.output.contains("secret"));
    }

    /// Every string anywhere in `value`, keys included.
    fn strings(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(s) => out.push(s.clone()),
            Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
            Value::Object(map) => {
                for (k, v) in map {
                    out.push(k.clone());
                    strings(v, out);
                }
            }
            _ => {}
        }
    }

    /// The names of `secret` that survive in the output, as whole identifiers or inside one.
    fn survivors(output: &str, secret: &[&str]) -> Vec<String> {
        let parsed: Value = serde_json::from_str(output).unwrap_or(Value::Null);
        let mut all = Vec::new();
        strings(&parsed, &mut all);
        secret
            .iter()
            .filter(|name| {
                let lower = name.to_lowercase();
                all.iter().any(|s| s.to_lowercase().contains(&lower))
            })
            .map(|n| (*n).to_owned())
            .collect()
    }

    /// A .NET-shaped result: modules with Rulebearing's additions, a code layer, an element
    /// violation naming a member, the `--affected` receipt and a `fix` everywhere one can be.
    fn dotnet() -> Value {
        json!({
            "modules": [
                { "source": "src/Acme.Billing/Payments/Gateway.cs", "language": "dotnet",
                  "project": "Acme.Billing.dll", "namespaces": ["Acme.Billing.Payments"], "attribution": "pdb",
                  "dependencies": [
                    { "module": "Acme.Billing.Ledger.Entry", "resolved": "src/Acme.Billing/Ledger/Entry.cs",
                      "dependencyKind": "body", "member": "Acme.Billing.Ledger.Entry.Post", "line": 3, "column": 1,
                      "valid": false, "rules": [{ "name": "no-cross", "severity": "error" }] },
                    { "module": "System.Object", "resolved": "System.Runtime", "coreModule": true, "valid": true } ] },
                { "source": "src/Acme.Billing/Ledger/Entry.cs", "language": "dotnet", "project": "Acme.Billing.dll",
                  "namespaces": ["Acme.Billing.Ledger"], "dependencies": [] },
                { "source": "System.Runtime", "language": "dotnet", "coreModule": true, "dependencies": [] },
            ],
            "code": {
                "types": [
                    { "fullName": "Acme.Billing.Payments.Gateway", "name": "Gateway", "namespace": "Acme.Billing.Payments",
                      "assembly": "Acme.Billing", "file": "src/Acme.Billing/Payments/Gateway.cs", "baseType": "System.Object" },
                    { "fullName": "Acme.Billing.Ledger.Entry", "name": "Entry", "namespace": "Acme.Billing.Ledger",
                      "assembly": "Acme.Billing", "file": "src/Acme.Billing/Ledger/Entry.cs" },
                    { "fullName": "System.Object", "name": "Object", "namespace": "System", "referenced": true },
                    { "fullName": "System.Runtime.Remoting", "name": "Remoting", "namespace": "System.Runtime", "referenced": true },
                ],
                "members": [{ "declaringType": "Acme.Billing.Payments.Gateway", "name": "Charge()",
                              "fullName": "System.Void Acme.Billing.Payments.Gateway::Charge()" }],
                "calls": [{ "from": "System.Void Acme.Billing.Payments.Gateway::Charge()",
                            "to": "System.Void Acme.Billing.Ledger.Entry::Post()" }],
            },
            "summary": {
                "violations": [
                    { "from": "src/Acme.Billing/Payments/Gateway.cs", "to": "src/Acme.Billing/Ledger/Entry.cs",
                      "type": "dependency", "rule": { "name": "no-cross", "severity": "error" },
                      "id": "RB-1", "fix": "Call Acme.Billing.Ledger through its service", "decision": "adr:0001" },
                    { "from": "src/Acme.Billing/Payments/Gateway.cs",
                      "to": "System.Void Acme.Billing.Payments.Gateway::Charge()", "type": "element",
                      "rule": { "name": "sealed", "severity": "warn" }, "fix": "Seal Gateway" },
                    { "from": "Payments", "to": "Ledger", "type": "slice", "rule": { "name": "slices", "severity": "error" } },
                ],
                "error": 2, "warn": 1, "info": 0, "totalCruised": 3,
                "affected": { "revision": "HEAD", "changed": ["src/Acme.Billing/Payments/Gateway.cs"],
                              "closure": ["src/Acme.Billing/Payments/Gateway.cs"] },
                "plugins": ["reporters/Acme.Billing.cjs"],
                "inspected": { "dotnet": { "assemblies": ["Acme.Billing.dll"] } },
                "ruleSetUsed": {
                    "forbidden": [{ "name": "no-cross", "severity": "error", "fix": "Leave Ledger to Acme.Billing",
                                    "from": { "path": "^src/" }, "to": { "path": "\\.cs$" } }],
                    "elements": [{ "name": "sealed", "select": { "namespace": "Acme.Billing.Payments" } }],
                },
                "optionsUsed": {},
            }
        })
    }

    const DOTNET_NAMES: &[&str] = &[
        "Acme", "Billing", "Payments", "Gateway", "Ledger", "Entry", "Post", "Charge", "System",
        "Object", "Runtime", "Remoting",
    ];

    #[test]
    fn no_name_of_a_dotnet_document_survives() {
        let rendered = render(&dotnet(), None);
        assert_eq!(
            survivors(&rendered.output, DOTNET_NAMES),
            Vec::<String>::new(),
            "{}",
            rendered.output
        );
        // With a word list as well: every name is a word, none is left.
        let words: Vec<String> = (0..40)
            .map(|i| format!("word{}", char::from(b'a' + i % 26)))
            .collect();
        let listed = render(&dotnet(), Some(&json!({ "wordlist": words })));
        assert_eq!(
            survivors(&listed.output, DOTNET_NAMES),
            Vec::<String>::new(),
            "{}",
            listed.output
        );
        // Upstream's shape: every addition is gone, `cycle` added where upstream adds it.
        let parsed: Value = serde_json::from_str(&rendered.output).unwrap_or(Value::Null);
        assert!(parsed.get("code").is_none());
        for key in ["affected", "plugins", "inspected"] {
            assert!(parsed["summary"].get(key).is_none(), "{key}");
        }
        assert!(parsed["modules"][0].get("namespaces").is_none());
        assert!(
            parsed["modules"][0]["dependencies"][0]
                .get("member")
                .is_none()
        );
        assert!(parsed["summary"]["violations"][0].get("fix").is_none());
        assert!(parsed["summary"]["ruleSetUsed"].get("elements").is_none());
        assert_eq!(parsed["summary"]["violations"][0]["cycle"], json!([]));
        // The file keeps its extension; the member keeps its punctuation.
        let element = parsed["summary"]["violations"][1]["to"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        assert!(
            element.contains(' ') && element.ends_with("()") && element.contains("::"),
            "{element}"
        );
        let file = parsed["modules"][0]["source"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        assert!(
            file.starts_with("src/")
                && std::path::Path::new(&file)
                    .extension()
                    .is_some_and(|e| e == "cs"),
            "{file}"
        );
        // The same name gets the same replacement everywhere: the module and the violation agree.
        assert_eq!(
            parsed["summary"]["violations"][0]["from"].as_str(),
            Some(file.as_str())
        );
    }

    #[test]
    fn the_affected_receipt_and_fix_of_a_typescript_run_do_not_leak() {
        let result = json!({
            "modules": [
                { "source": "src/index.ts", "dependencies": [
                    { "module": "./secretbilling/paymentgateway", "resolved": "src/secretbilling/paymentgateway.ts",
                      "valid": false, "rules": [{ "name": "no-billing", "severity": "error" }] }] },
                { "source": "src/secretbilling/paymentgateway.ts", "dependencies": [] },
            ],
            "summary": {
                "violations": [{ "from": "src/index.ts", "to": "src/secretbilling/paymentgateway.ts",
                                 "type": "dependency", "rule": { "name": "no-billing", "severity": "error" },
                                 "fix": "Move the import of secretbilling/paymentgateway into src/secretbilling/api" }],
                "affected": { "revision": "HEAD", "changed": ["src/secretbilling/paymentgateway.ts"],
                              "closure": ["src/secretbilling/paymentgateway.ts", "src/index.ts"] },
                "ruleSetUsed": { "forbidden": [{ "name": "no-billing", "severity": "error",
                    "fix": "Move the import of secretbilling/paymentgateway into src/secretbilling/api",
                    "from": {}, "to": { "path": "^src/[^/]+/" } }] },
            }
        });
        let out = render(&result, None).output;
        assert_eq!(
            survivors(&out, &["secretbilling", "paymentgateway"]),
            Vec::<String>::new(),
            "{out}"
        );
    }

    #[test]
    fn random_strings_are_keyed_by_the_whole_document() {
        let one = json!({ "modules": [{ "source": "secretbilling.ts", "dependencies": [] }], "summary": { "violations": [] } });
        let two = json!({ "modules": [{ "source": "secretbilling.ts", "dependencies": [] },
                                      { "source": "other.ts", "dependencies": [] }], "summary": { "violations": [] } });
        let first = |r: &Value| -> String {
            let parsed: Value =
                serde_json::from_str(&render(r, None).output).unwrap_or(Value::Null);
            parsed["modules"][0]["source"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        };
        // Not a public function of the name: the unkeyed draw is not what the report prints, and
        // the same name in another document gets another string.
        assert_ne!(
            first(&one),
            format!("{}.ts", random_string(&NO_DOCUMENT, "secretbilling"))
        );
        assert_ne!(first(&one), first(&two));
        // Two runs over the same input still agree.
        assert_eq!(first(&one), first(&one));
        assert_eq!(first(&one).len(), "secretbilling.ts".len());
        // The key is the digest of the stripped document, so an addition does not move it.
        let mut with_fix = one.clone();
        with_fix["summary"]["plugins"] = json!(["x.cjs"]);
        assert_eq!(first(&with_fix), first(&one));
        assert_ne!(document_key(&one), document_key(&two));
        assert_ne!(
            random_string(&document_key(&one), "abc"),
            random_string(&document_key(&two), "abc")
        );
    }

    #[test]
    fn names_come_from_the_code_layer_and_the_modules() {
        let names = name_identifiers(&dotnet());
        for name in DOTNET_NAMES {
            assert!(names.contains(*name), "{name}");
        }
        assert!(!names.contains("src"), "whitelisted words are not names");
        assert!(name_identifiers(&json!({ "modules": [{ "source": "a.ts" }] })).is_empty());
        let mut a = Anonymizer::new(words(&["w1", "w2", "w3"]));
        a.names = ["Books".to_owned()].into_iter().collect();
        assert_eq!(
            a.path("src/River.Books/x.cs", WHITELIST_RE),
            "src/w1.w2/w3.cs"
        );
        assert_eq!(
            a.name("System.Void River.Books::Go()"),
            a.name("System.Void River.Books::Go()")
        );
        let mut b = Anonymizer::new(words(&["w1", "w2"]));
        assert_eq!(b.name("test::Alpha()"), "test::w1()");
        assert_eq!(b.name("Alpha.Beta"), "w1.w2");
        assert_eq!(b.name(""), "");
    }

    proptest! {
        #[test]
        fn random_strings_have_as_many_code_points(text in "\\PC{0,24}") {
            let s = random_string(&NO_DOCUMENT, &text);
            prop_assert_eq!(s.chars().count(), text.chars().count());
            for (a, b) in s.chars().zip(text.chars()) {
                if matches!(b, '-' | '_' | '.') {
                    prop_assert_eq!(a, b);
                } else {
                    prop_assert!(a.is_ascii_alphanumeric());
                }
                prop_assert!(!matches!(a, 'z' | 'Z' | '9'));
            }
        }

        #[test]
        fn whitelisted_paths_stay(path in "(src|bin|lib|test|index\\.ts)(/(src|bin|lib|test|index\\.ts)){0,4}") {
            let mut a = Anonymizer::default();
            prop_assert_eq!(a.path(&path, WHITELIST_RE), path);
        }
    }
}
