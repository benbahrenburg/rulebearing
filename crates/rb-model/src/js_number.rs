//! Serialises floating-point metrics the way JavaScript's `JSON.stringify` does.
//!
//! - Contract: [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//!   (the JSON is dependency-cruiser's, byte for byte after key ordering)
//! - Plan: [Wave 0, Step 3](../../../docs/plans/pending/0000-wave-0-spike.md#step-3-rb-model-graph-document-schema-violation-id-0a)
//!
//! JavaScript has one number type, so an instability of one prints as `1`; serde prints an `f64`
//! one as `1.0`. The difference is invisible to a JSON reader and visible to a byte comparison, and
//! the conformance gate byte-compares, so integral values are written as integers. Non-integral
//! values use the shortest round-trip form, which is what both runtimes print. Integral values past
//! 2^53 keep serde's exponent form, which JavaScript would print differently; no metric the
//! document carries (a coupling ratio, a count of modules) comes near that size.

use serde::Serializer;

/// The largest integer JavaScript represents exactly.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Serialises one metric.
///
/// # Errors
/// Whatever the serializer returns.
#[allow(clippy::trivially_copy_pass_by_ref)] // serde's `serialize_with` passes a reference
pub fn plain<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
    if value.is_finite() && value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER {
        #[allow(clippy::cast_possible_truncation)] // bounded by MAX_SAFE_INTEGER above
        serializer.serialize_i64(*value as i64)
    } else {
        serializer.serialize_f64(*value)
    }
}

/// Serialises an optional metric; callers pair it with `skip_serializing_if = "Option::is_none"`.
///
/// # Errors
/// Whatever the serializer returns.
#[allow(clippy::ref_option)] // serde's `serialize_with` passes a reference to the field
pub fn option<S: Serializer>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error> {
    match value {
        Some(number) => plain(number, serializer),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: f64) -> String {
        let mut out = Vec::new();
        let mut serializer = serde_json::Serializer::new(&mut out);
        let _ = plain(&value, &mut serializer);
        String::from_utf8(out).unwrap_or_default()
    }

    #[test]
    fn integers_print_without_a_fraction() {
        assert_eq!(text(1.0), "1");
        assert_eq!(text(0.0), "0");
        assert_eq!(text(-3.0), "-3");
        assert_eq!(text(MAX_SAFE_INTEGER), "9007199254740991");
    }

    #[test]
    fn fractions_and_huge_values_keep_the_float_form() {
        assert_eq!(text(0.5), "0.5");
        assert_eq!(text(0.333_333_333_333_333_3), "0.3333333333333333");
        assert_eq!(text(MAX_SAFE_INTEGER * 4.0), "3.6028797018963964e+16");
    }

    #[test]
    fn none_serialises_as_null() {
        let mut out = Vec::new();
        let mut serializer = serde_json::Serializer::new(&mut out);
        let _ = option(&None, &mut serializer);
        assert_eq!(out, b"null");
        let mut out = Vec::new();
        let mut serializer = serde_json::Serializer::new(&mut out);
        let _ = option(&Some(2.0), &mut serializer);
        assert_eq!(out, b"2");
    }
}
