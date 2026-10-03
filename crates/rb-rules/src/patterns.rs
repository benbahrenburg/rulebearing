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

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use rb_config::pattern::{self, Matcher};

type Cache = HashMap<String, Option<Arc<Matcher>>>;

/// Every pattern the run has compiled, shared by every thread, so each compiles once.
fn shared() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

thread_local! {
    /// The patterns this thread has used, in front of [`shared`], so the engine's parallel
    /// stages neither wait for the lock nor share a reference count.
    static LOCAL: RefCell<Cache> = RefCell::new(HashMap::new());
}

/// The compiled pattern from the shared cache, compiled there on first use.
fn compiled(text: &str) -> Option<Arc<Matcher>> {
    let mut cache = shared().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(found) = cache.get(text) {
        return found.clone();
    }
    let found = pattern::matcher(text).ok().map(Arc::new);
    insert(&mut cache, SHARED_LIMIT, text, found.clone());
    found
}

/// How many patterns a thread's cache holds before it starts over.
const LOCAL_LIMIT: usize = 4096;

/// How many patterns the shared cache holds before it starts over.
const SHARED_LIMIT: usize = 4096;

/// Adds a pattern to a cache, emptying it first when it holds `limit` patterns. A pattern with a
/// capture substituted is a new text for each captured value, so a process that evaluates for
/// hours (`guard --watch`) or for millions of inputs (a fuzz target) would otherwise keep every
/// matcher it ever compiled. A matcher still in use elsewhere lives on through its `Arc`.
fn insert(cache: &mut Cache, limit: usize, text: &str, found: Option<Arc<Matcher>>) {
    if cache.len() >= limit {
        cache.clear();
    }
    cache.insert(text.to_owned(), found);
}

/// Runs `f` with the compiled pattern, or `None` when it does not compile, looked up in this
/// thread's cache without allocating once the thread has seen it.
fn with<R>(text: &str, f: impl FnOnce(Option<&Arc<Matcher>>) -> R) -> R {
    LOCAL.with(|local| {
        if let Ok(cache) = local.try_borrow()
            && let Some(found) = cache.get(text)
        {
            return f(found.as_ref());
        }
        let found = compiled(text);
        let result = f(found.as_ref());
        if let Ok(mut cache) = local.try_borrow_mut() {
            insert(&mut cache, LOCAL_LIMIT, text, found);
        }
        result
    })
}

/// The compiled pattern, or `None` when it does not compile.
pub fn get(text: &str) -> Option<Arc<Matcher>> {
    with(text, |found| found.map(Arc::clone))
}

/// `getCachedRegExp(pattern).test(text)`.
pub fn test(pattern_text: &str, text: &str) -> bool {
    with(pattern_text, |m| m.is_some_and(|m| m.is_match(text)))
}

/// `extractGroups({ path: pattern }, text)`: the match and its participating groups, or nothing.
pub fn groups(pattern_text: &str, text: &str) -> Vec<String> {
    with(pattern_text, |m| {
        m.map_or_else(Vec::new, |m| m.groups(text))
    })
}

/// `getCachedRegExp(pattern).exec(text)?.[0]`.
pub fn first_match(pattern_text: &str, text: &str) -> Option<String> {
    with(pattern_text, |m| {
        m.and_then(|m| m.find(text).map(str::to_owned))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_s_cache_starts_over_at_its_limit() {
        // On a thread of its own, so no other test's patterns are in the cache.
        let held = std::thread::spawn(|| {
            for i in 0..LOCAL_LIMIT {
                assert!(test(&format!("^limit-{i}$"), &format!("limit-{i}")));
            }
            let full = LOCAL.with(|local| local.borrow().len());
            assert!(test("^one-more$", "one-more"));
            let after = LOCAL.with(|local| local.borrow().len());
            // A pattern seen before the cache started over still answers.
            assert!(test("^limit-0$", "limit-0"));
            (full, after)
        })
        .join();
        assert_eq!(held.ok(), Some((LOCAL_LIMIT, 1)));
    }

    #[test]
    fn a_cache_starts_over_at_its_limit_and_never_holds_more() {
        let mut cache = Cache::new();
        insert(&mut cache, 2, "a", None);
        insert(&mut cache, 2, "b", get("^b"));
        assert_eq!(cache.len(), 2, "below the limit nothing is dropped");
        insert(&mut cache, 2, "b", None);
        assert_eq!(cache.len(), 1, "at the limit the cache starts over");
        assert!(
            cache.get("b").is_some_and(Option::is_none),
            "and holds the pattern just added"
        );
        insert(&mut cache, 2, "c", None);
        assert_eq!(cache.len(), 2);
        for i in 0..10 {
            insert(&mut cache, 3, &format!("p{i}"), None);
            assert!(cache.len() <= 3);
        }
    }

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

    #[test]
    fn every_thread_shares_one_compiled_pattern() {
        let here = get("^shared/");
        let there: Vec<_> = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        let tested = (test("^shared/", "shared/a"), test("^shared/", "a"));
                        (tested, get("^shared/"))
                    })
                })
                .collect();
            threads.into_iter().filter_map(|t| t.join().ok()).collect()
        });
        assert_eq!(there.len(), 4);
        for (tested, pointer) in there {
            assert_eq!(tested, (true, false));
            let same = here
                .as_ref()
                .zip(pointer.as_ref())
                .is_some_and(|(a, b)| Arc::ptr_eq(a, b));
            assert!(same, "compiled once, by whichever thread came first");
        }
        assert!(
            get("(").is_none(),
            "an uncompilable pattern is cached as none"
        );
        assert!(groups("(", "x").is_empty());
        assert_eq!(first_match("(", "x"), None);
    }
}
