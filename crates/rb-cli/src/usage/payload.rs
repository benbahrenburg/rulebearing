//! The usage-counts payload: one calendar month of one repository's counts, with every rule
//! name outside the public library replaced by a salted hash.
//!
//! - Architecture: [architecture § Security posture](../../../../docs/architecture.md#security-posture)
//! - Decision: [ADR-0076](../../../../docs/adr/0076-usage-counts-are-counted-and-printed-but-not-sent.md)
//! - Plan: [plan 0004 § 1.5](../../../../docs/plans/pending/0004-wave-4-reach.md#15-architecture-of-what-this-wave-builds)
//!   (the payload), [Step 13](../../../../docs/plans/pending/0004-wave-4-reach.md#25-steps-for-sub-wave-4e-usage-counts-and-the-adoption-review)
//! - Requirements: [FR-REACH-03](../../../../docs/prd.md#fr-reach-03), [NFR-SEC-01](../../../../docs/prd.md#nfr-sec-01)
//! - Schema: `schema/usage-counts-v1.json`, published in [docs/usage-counts.md](../../../../docs/usage-counts.md#the-payload-schema)
//!
//! The payload is built with sorted maps and printed pretty, so the same counts print the same
//! bytes on every machine. The file on disk is the payload itself: the hashing happens when a
//! rule is counted, so no rule name outside the library is ever written.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::OnceLock;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The value of `tool`.
pub const TOOL: &str = "rulebearing";

/// The prefix of a hashed rule key.
pub const HASHED: &str = "sha256:";

/// One month of counts, exactly as `--usage-counts print` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    /// Always [`TOOL`].
    pub tool: String,
    /// The version of the binary that last counted into this month.
    pub version: String,
    /// The calendar month, `YYYY-MM`.
    pub period: String,
    /// Runs per top-level subcommand.
    pub commands: BTreeMap<String, u64>,
    /// Per rule key ([`rule_key`]), the `cruise` runs in which the rule fired.
    pub rules: BTreeMap<String, u64>,
}

impl Payload {
    /// No counts yet for `period`.
    pub fn empty(period: &str) -> Self {
        Self {
            tool: TOOL.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            period: period.to_owned(),
            commands: BTreeMap::new(),
            rules: BTreeMap::new(),
        }
    }

    /// Adds one run of the subcommand `name`.
    pub fn count_command(&mut self, name: &str) {
        let count = self.commands.entry(name.to_owned()).or_insert(0);
        *count = count.saturating_add(1);
    }

    /// Adds one run in which the rule with key `key` fired.
    pub fn count_rule(&mut self, key: String) {
        let count = self.rules.entry(key).or_insert(0);
        *count = count.saturating_add(1);
    }

    /// The payload as printed: pretty JSON, keys in order, one trailing newline.
    pub fn to_text(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_default();
        text.push('\n');
        text
    }

    /// Reads a payload written by [`Payload::to_text`].
    ///
    /// # Errors
    /// Why the text is not a payload: not JSON of this shape, another tool, or a period that is
    /// not `YYYY-MM`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let payload: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if payload.tool != TOOL {
            return Err(format!("`tool` is `{}`, not `{TOOL}`", payload.tool));
        }
        if !is_period(&payload.period) {
            return Err(format!(
                "`period` is `{}`, not a calendar month (YYYY-MM)",
                payload.period
            ));
        }
        Ok(payload)
    }
}

/// The calendar month of `date`, `YYYY-MM`.
pub fn period(date: NaiveDate) -> String {
    date.format("%Y-%m").to_string()
}

/// Whether `text` is a calendar month as [`period`] writes it.
pub fn is_period(text: &str) -> bool {
    NaiveDate::parse_from_str(&format!("{text}-01"), "%Y-%m-%d")
        .is_ok_and(|date| period(date) == text)
}

/// The key a rule is counted under: its name when it is a rule of the public library
/// ([`is_library`]), otherwise `sha256:` and the hex SHA-256 of `salt` followed by the name.
/// The salt is a fixed 64 hex characters, so no two (salt, name) pairs hash the same input.
pub fn rule_key(name: &str, salt: &str) -> String {
    if is_library(name) {
        return name.to_owned();
    }
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(name.as_bytes());
    let mut key = String::from(HASHED);
    for byte in hasher.finalize() {
        let _ = write!(key, "{byte:02x}");
    }
    key
}

/// Whether `name` is the name of a rule in a preset the binary carries.
pub fn is_library(name: &str) -> bool {
    library().contains(name)
}

/// The names of the rules in every preset the binary carries: `rulebearing:*` (the language
/// presets and the framework presets) and `dependency-cruiser/configs/*`. These are public, so
/// they are counted by name.
pub fn library() -> &'static BTreeSet<String> {
    static LIBRARY: OnceLock<BTreeSet<String>> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let mut names = BTreeSet::new();
        for (_, text) in rb_config::extends::NATIVE_PRESETS {
            if let Ok(preset) = serde_yaml::from_str::<Value>(text) {
                names.extend(preset_rule_names(&preset));
            }
        }
        for (_, text) in rb_config::js::DEPENDENCY_CRUISER_PRESETS {
            names.extend(cruiser_rule_names(text));
        }
        names
    })
}

/// The `name` of every rule in a native preset's rule lists.
pub fn preset_rule_names(preset: &Value) -> BTreeSet<String> {
    let rules = preset.get("rules");
    let dependencies = rules.and_then(|r| r.get("dependencies"));
    let lists = ["forbidden", "required", "allowed"]
        .iter()
        .flat_map(|list| [preset.get(*list), dependencies.and_then(|d| d.get(*list))])
        .chain(
            [
                "ratchets",
                "layers",
                "independence",
                "elements",
                "slices",
                "diagrams",
            ]
            .iter()
            .map(|family| rules.and_then(|r| r.get(*family))),
        );
    lists
        .flatten()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|rule| rule.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

/// The `name: "..."` of every rule in a vendored dependency-cruiser preset's source.
pub fn cruiser_rule_names(source: &str) -> BTreeSet<String> {
    source
        .lines()
        .filter_map(|line| line.trim().strip_prefix("name:"))
        .filter_map(|rest| {
            let rest = rest.trim().trim_end_matches(',');
            rest.strip_prefix('"')
                .and_then(|r| r.strip_suffix('"'))
                .or_else(|| rest.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')))
        })
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::json;

    const SALT: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    #[test]
    fn the_empty_payload_is_the_plans_shape_with_nothing_counted() {
        let payload = Payload::empty("2026-11");
        let value: Value = serde_json::from_str(&payload.to_text()).unwrap_or_default();
        assert_eq!(
            value,
            json!({ "tool": "rulebearing", "version": env!("CARGO_PKG_VERSION"), "period": "2026-11", "commands": {}, "rules": {} })
        );
        assert!(payload.to_text().ends_with("}\n"));
    }

    #[test]
    fn counting_adds_one_per_call_and_prints_in_key_order() {
        let mut payload = Payload::empty("2026-11");
        payload.count_command("cruise");
        payload.count_command("can-import");
        payload.count_command("cruise");
        payload.count_rule("no-circular".into());
        payload.count_rule(rule_key("mine", SALT));
        assert_eq!(payload.commands["cruise"], 2);
        assert_eq!(payload.commands["can-import"], 1);
        assert_eq!(payload.rules["no-circular"], 1);
        let text = payload.to_text();
        let can = text.find("can-import").unwrap_or(usize::MAX);
        let cruise = text.find("\"cruise\"").unwrap_or(0);
        assert!(can < cruise, "{text}");
        payload.commands.insert("x".into(), u64::MAX);
        payload.count_command("x");
        assert_eq!(payload.commands["x"], u64::MAX, "saturates");
        payload.rules.insert("y".into(), u64::MAX);
        payload.count_rule("y".into());
        assert_eq!(payload.rules["y"], u64::MAX, "saturates");
    }

    #[test]
    fn a_payload_round_trips_and_a_foreign_file_is_refused() {
        let mut payload = Payload::empty("2026-01");
        payload.count_command("cruise");
        assert_eq!(Payload::parse(&payload.to_text()), Ok(payload.clone()));
        let other = payload.to_text().replace("\"rulebearing\"", "\"other\"");
        assert!(Payload::parse(&other).is_err_and(|e| e.contains("`tool` is `other`")));
        let month = payload.to_text().replace("2026-01", "2026-13");
        assert!(Payload::parse(&month).is_err_and(|e| e.contains("not a calendar month")));
        assert!(Payload::parse("{}").is_err());
        let extra = payload
            .to_text()
            .replace("\"tool\"", "\"path\": \"/home\", \"tool\"");
        assert!(Payload::parse(&extra).is_err(), "no field beyond the five");
    }

    #[test]
    fn periods_are_calendar_months() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap_or_default();
        assert_eq!(period(date), "2026-03");
        for good in ["2026-01", "2026-12", "1999-07"] {
            assert!(is_period(good), "{good}");
        }
        for bad in [
            "2026-13",
            "2026-00",
            "2026-1",
            "26-01",
            "2026-01-01",
            "",
            "2026/01",
        ] {
            assert!(!is_period(bad), "{bad}");
        }
    }

    #[test]
    fn library_rules_are_named_and_every_other_rule_is_a_salted_hash() {
        assert_eq!(rule_key("no-circular", SALT), "no-circular");
        assert_eq!(rule_key("not-to-unresolvable", SALT), "not-to-unresolvable");
        let key = rule_key("payments-not-to-ledger", SALT);
        assert!(key.starts_with(HASHED), "{key}");
        assert_eq!(key.len(), HASHED.len() + 64);
        assert!(!key.contains("payments"), "the name never appears");
        // A fixed vector: the key is part of the published schema's contract.
        assert_eq!(
            rule_key("a", SALT),
            format!("{HASHED}{}", {
                let digest = Sha256::digest(format!("{SALT}a").as_bytes());
                digest.iter().fold(String::new(), |mut s, b| {
                    let _ = write!(s, "{b:02x}");
                    s
                })
            })
        );
        let other_salt = "f".repeat(64);
        assert_ne!(
            rule_key("payments-not-to-ledger", SALT),
            rule_key("payments-not-to-ledger", &other_salt),
            "the same name in two repositories hashes differently"
        );
    }

    #[test]
    fn the_library_is_every_rule_in_the_carried_presets() {
        let library = library();
        for name in [
            "no-circular",
            "no-orphans",
            "not-to-unresolvable",
            "no-deprecated-core",
            "not-to-deprecated",
            "no-duplicate-dep-types",
            "no-non-package-json",
            "nextjs-no-import-of-api-routes",
            "django-views-only-from-urls",
            "slices-are-independent",
        ] {
            assert!(library.contains(name), "{name} in {library:?}");
        }
        assert!(!library.contains("payments-not-to-ledger"));
        assert!(library.iter().all(|n| !n.is_empty()));
    }

    #[test]
    fn rule_names_come_from_rule_lists_only() {
        let preset = json!({
            "forbidden": [{ "name": "top" }],
            "rules": {
                "dependencies": { "forbidden": [{ "name": "dep" }], "required": [{ "name": "req" }], "allowed": [{ "name": "ok" }] },
                "ratchets": [{ "name": "r" }],
                "layers": [{ "name": "l", "layers": [{ "name": "not-a-rule" }] }],
                "independence": [{ "name": "i" }],
                "elements": [{ "name": "e" }],
                "slices": [{ "name": "s" }],
                "diagrams": [{ "name": "d" }, { "comment": "unnamed" }]
            },
            "options": { "name": "not-a-rule-either" }
        });
        let names: Vec<String> = preset_rule_names(&preset).into_iter().collect();
        assert_eq!(
            names,
            ["d", "dep", "e", "i", "l", "ok", "r", "req", "s", "top"]
        );
        assert!(preset_rule_names(&json!({})).is_empty());
    }

    #[test]
    fn cruiser_rule_names_read_both_quote_styles() {
        let source = "module.exports = {\n  name: \"no-circular\",\n  name: 'no-orphans',\n  // name: unquoted\n  severity: \"warn\",\n};\n";
        let names: Vec<String> = cruiser_rule_names(source).into_iter().collect();
        assert_eq!(names, ["no-circular", "no-orphans"]);
    }

    proptest! {
        #[test]
        fn a_hashed_key_never_carries_the_name(name in "[a-z][a-z0-9-]{3,40}") {
            prop_assume!(!is_library(&name));
            let key = rule_key(&name, SALT);
            prop_assert!(key.starts_with(HASHED));
            prop_assert!(!key.contains(&name));
            prop_assert!(key[HASHED.len()..].chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
            prop_assert_eq!(key.clone(), rule_key(&name, SALT), "deterministic");
        }

        #[test]
        fn any_counts_round_trip(commands in proptest::collection::btree_map("[a-z][a-z-]{0,12}", 1_u64..1000, 0..6)) {
            let mut payload = Payload::empty("2026-11");
            payload.commands = commands;
            prop_assert_eq!(Payload::parse(&payload.to_text()), Ok(payload));
        }
    }
}
