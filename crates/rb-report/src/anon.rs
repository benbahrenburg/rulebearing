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
//! Here the string is drawn from a generator seeded by the part itself, with upstream's ranges
//! (`a` to `y`, `0` to `8`, because `randomInt`'s upper bound is exclusive), so the same input is
//! anonymised byte for byte the same on every run; that is the one documented divergence. With a
//! word list long enough for the document, as upstream's specs use, the output is upstream's.

use std::collections::{BTreeMap, VecDeque};

use serde_json::{Map, Value};

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

/// splitmix64: a small, well-mixed generator; the stream for a part is seeded by the part.
struct Draw(u64);

impl Draw {
    fn seeded(text: &str) -> Self {
        // FNV-1a over the UTF-8 bytes.
        let seed = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
        Self(seed)
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
/// one (or a character without case), and `-`, `_` and `.` kept.
pub fn random_string(text: &str) -> String {
    let mut draw = Draw::seeded(text);
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
}

impl Anonymizer {
    /// An anonymiser drawing from `words`, used as given.
    pub fn new(words: impl IntoIterator<Item = String>) -> Self {
        Self {
            words: words.into_iter().collect(),
            cache: BTreeMap::new(),
        }
    }

    fn replace(&mut self, part: &str, index: usize) -> String {
        if index == 0 {
            self.words
                .pop_front()
                .filter(|w| !w.is_empty())
                .unwrap_or_else(|| random_string(part))
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
        self.field(&mut map, "from");
        self.field(&mut map, "to");
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

/// Renders `anon`, with `options` the `reporterOptions.anon` section. Always exits 0, as
/// upstream's reporter does.
pub fn render(result: &Value, options: Option<&Value>) -> Rendered {
    let words = sanitize_word_list(&word_list(result, options));
    let anonymised = Anonymizer::new(words).document(result);
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
        assert_eq!(random_string(""), "");
        assert_eq!(random_string("-"), "-");
        let s = random_string("better-someStuff_operator");
        let shape: String = s
            .chars()
            .map(|c| match c {
                'a'..='z' => 'a',
                'A'..='Z' => 'A',
                other => other,
            })
            .collect();
        assert_eq!(shape, "aaaaaa-aaaaAaaaa_aaaaaaaa");
        assert!(random_string("ü").chars().all(|c| c.is_ascii_lowercase()));
        assert!(random_string("Ü").chars().all(|c| c.is_ascii_uppercase()));
        assert!(random_string("@").chars().all(|c| c.is_ascii_uppercase()));
        assert!(random_string("1").chars().all(|c| c.is_ascii_digit()));
        assert_eq!(random_string("pulp2slurp"), random_string("pulp2slurp"));
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

    proptest! {
        #[test]
        fn random_strings_have_as_many_code_points(text in "\\PC{0,24}") {
            let s = random_string(&text);
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
