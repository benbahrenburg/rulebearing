//! dependency-cruiser's `getCachedRegExp`: every pattern compiled once per run.
//!
//! - Decisions: [ADR-0016](../../../docs/adr/0016-linear-time-regex-and-strict-compat.md),
//!   [ADR-0028](../../../docs/adr/0028-backreferences-by-instantiation-on-the-linear-engine.md)
//! - Plan: [Wave 1, Step 5](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-5-matchers-and-restriction-evaluation-1b)
//!
//! A pattern that cannot compile never reaches the engine from a loaded configuration
//! (`rb-config` compiles every rule pattern at load time and refuses with exit 3); one that is
//! built at run time by substituting captures can still fail, and such a pattern matches
//! nothing, which is what an invalid `RegExp` would do after dependency-cruiser caught the error.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use rb_config::pattern::{self, Matcher};

fn cache() -> &'static Mutex<HashMap<String, Option<Arc<Matcher>>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<Matcher>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The compiled pattern, or `None` when it does not compile.
pub fn get(text: &str) -> Option<Arc<Matcher>> {
    let mut cache = cache().lock().unwrap_or_else(PoisonError::into_inner);
    cache
        .entry(text.to_owned())
        .or_insert_with(|| pattern::matcher(text).ok().map(Arc::new))
        .clone()
}

/// `getCachedRegExp(pattern).test(text)`.
pub fn test(pattern_text: &str, text: &str) -> bool {
    get(pattern_text).is_some_and(|m| m.is_match(text))
}

/// `extractGroups({ path: pattern }, text)`: the match and its participating groups, or nothing.
pub fn groups(pattern_text: &str, text: &str) -> Vec<String> {
    get(pattern_text).map_or_else(Vec::new, |m| m.groups(text))
}

/// `getCachedRegExp(pattern).exec(text)?.[0]`.
pub fn first_match(pattern_text: &str, text: &str) -> Option<String> {
    get(pattern_text).and_then(|m| m.find(text).map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_are_compiled_once_and_tested() {
        assert!(test("^src/", "src/a.ts"));
        assert!(!test("^src/", "lib/a.ts"));
        assert!(
            !test("a(?=b)", "ab"),
            "an uncompilable pattern matches nothing"
        );
        assert!(get("a(?=b)").is_none());
        let first = get("^x").map(|m| Arc::as_ptr(&m));
        let second = get("^x").map(|m| Arc::as_ptr(&m));
        assert_eq!(first, second);
        assert_eq!(groups("^(src)/(.+)$", "src/a"), ["src/a", "src", "a"]);
        assert!(groups("^src", "src").is_empty());
        assert_eq!(
            first_match("[a-z]+/[a-z]+", "1/src/app/x"),
            Some("src/app".to_owned())
        );
        assert_eq!(first_match("^z", "a"), None);
    }
}
