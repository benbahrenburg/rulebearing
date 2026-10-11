// The anchor a heading gets on the docs site: the same slug the documentation link check computes
// (xtask/src/doclinks.rs, `slug`), so every `#anchor` the check accepts in a source page resolves on
// the built page.
//
// Architecture: docs/architecture.md#verification-strategy. Decisions: docs/adr/0023-documentation-link-and-lint-gates.md,
// docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3. Requirements: FR-REACH-01, NFR-DOC-01.

/** A character the link check keeps in a slug: Rust's `char::is_alphanumeric`, `-` and `_`. */
const KEPT = /^[\p{Alphabetic}\p{N}_-]$/u;

/**
 * The slug of a heading's source text: trimmed, lower-cased, spaces to `-`, every character that
 * is not alphanumeric, `-` or `_` dropped. Markup is not stripped first, exactly as the link check
 * reads the raw line, so `` `rb-model` and friends `` is `rb-model-and-friends`.
 */
export function slug(heading: string): string {
  let out = '';
  for (const ch of heading.trim().toLowerCase()) {
    if (KEPT.test(ch)) {
      out += ch;
    } else if (ch === ' ') {
      out += '-';
    }
  }
  return out;
}
