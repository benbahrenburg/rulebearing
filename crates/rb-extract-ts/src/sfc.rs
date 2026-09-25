//! Single-file components: the `<script>` blocks of `.vue` and `.svelte` files, split out for
//! `oxc` with every byte offset kept.
//!
//! - Plan: [Wave 2, Step 9](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#29-step-9-presets---init-presets-vue-svelte-markdown-webpackconfig-collapse-highlight-experimentalstats-2d)
//!   (`sfc.rs`: "split `.vue` and `.svelte` files to their `<script>` and `<script setup>` /
//!   `<script context="module">` blocks, offset line and column so findings point into the
//!   original file, and hand the text to `oxc` with the `lang` attribute selecting TS or JS")
//! - Source: [coverage § Extraction and resolution](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution),
//!   row "Vue single-file components (`.vue`), Svelte (`.svelte`)": a script-block splitter, then
//!   `oxc`
//! - Decision: [ADR-0012](../../../docs/adr/0012-oxc-for-typescript.md)
//! - Requirement: [FR-EXT-TS-04](../../../docs/prd.md#fr-ext-ts-04)
//! - Specification: dependency-cruiser 18.2.0 `src/extract/transpile/vue-template-wrap.cjs`
//!   (`@vue/compiler-sfc` hands its parsers the `<script>` and `<script setup>` contents) and
//!   `svelte-preprocess.mjs` (the script regex and `lang="ts"`)
//!
//! A block's text is not copied out: everything outside the script bodies is blanked to spaces,
//! newlines kept, so a form's byte offset in the result is its byte offset in the file, and the
//! line and column computed from the original text point into the component. Svelte's instance
//! script and its module script (`<script context="module">` in Svelte 4, `<script module>` in
//! Svelte 5) are both kept, as the compiled component holds both. What dependency-cruiser gets
//! from compiling a Svelte component and this splitter does not see are the compiler's own
//! runtime imports (`svelte/internal/...`, which depend on the installed compiler's version) and
//! `import()` inside template expressions; the coverage tab names the splitter as the approach.

use oxc_span::SourceType;

/// The extensions whose files are single-file components.
pub const EXTENSIONS: &[&str] = &[".vue", ".svelte"];

/// Whether `extension` (as `node:path`'s `extname` gives it) is a single-file component.
pub fn is_component(extension: &str) -> bool {
    EXTENSIONS.contains(&extension)
}

/// The scripts of a component: the source with everything but the script bodies blanked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scripts {
    /// The source, the same length as the file, with only script bodies and newlines kept.
    pub text: String,
    /// The first script's `lang` attribute (`ts`, `tsx`, `jsx`, `js`), when it has one.
    pub lang: Option<String>,
}

/// Splits a `.vue` or `.svelte` source to its script blocks.
pub fn scripts(source: &str) -> Scripts {
    let mut out: Vec<u8> = source
        .bytes()
        .map(|b| if b == b'\n' || b == b'\r' { b } else { b' ' })
        .collect();
    let mut lang = None;
    let mut from = 0;
    while let Some(at) = source[from..].find("<script").map(|i| i + from) {
        let after_name = at + "<script".len();
        let boundary = source[after_name..].chars().next();
        let Some(open_end) = start_tag_end(source, after_name) else {
            break;
        };
        if !matches!(boundary, Some(c) if c == '>' || c.is_whitespace()) {
            from = after_name;
            continue;
        }
        let attributes = &source[after_name..open_end];
        if attributes.trim_end().ends_with('/') {
            // `<script src="..." />` has no body.
            from = open_end + 1;
            continue;
        }
        if lang.is_none() {
            lang = attribute(attributes, "lang");
        }
        let body_start = open_end + 1;
        let body_end = source[body_start..]
            .find("</script>")
            .map_or(source.len(), |i| i + body_start);
        out[body_start..body_end].copy_from_slice(&source.as_bytes()[body_start..body_end]);
        from = body_end;
    }
    // Only single bytes were replaced by spaces and whole script bodies copied back between ASCII
    // delimiters, so the result is UTF-8; a blanked continuation byte is a space too.
    Scripts {
        text: String::from_utf8(out).unwrap_or_default(),
        lang,
    }
}

/// The offset of the `>` that closes a start tag whose attributes begin at `from`: the first one
/// outside a quoted attribute value, so `generic="T extends Record<string, unknown>"` does not
/// end the tag. A quote opens a value only straight after `=` (spaces allowed between), as in
/// HTML; an apostrophe inside an unquoted value or a bare attribute is text.
fn start_tag_end(source: &str, from: usize) -> Option<usize> {
    let mut quote = None;
    let mut after_equals = false;
    for (offset, byte) in source.as_bytes()[from..].iter().enumerate() {
        match (quote, *byte) {
            (Some(open), b) if b == open => quote = None,
            (Some(_), _) => {}
            (None, b'>') => return Some(from + offset),
            (None, b @ (b'"' | b'\'')) if after_equals => {
                quote = Some(b);
                after_equals = false;
            }
            (None, b'=') => after_equals = true,
            (None, b) if b.is_ascii_whitespace() => {}
            (None, _) => after_equals = false,
        }
    }
    None
}

/// The value of `name="..."` (or `'...'`, or unquoted) in a tag's attribute text, where `name`
/// starts an attribute.
pub fn attribute(attributes: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=");
    let mut search = 0;
    let at = loop {
        let found = attributes[search..].find(&needle)? + search;
        let starts_attribute = found == 0
            || attributes[..found]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        if starts_attribute {
            break found + needle.len();
        }
        search = found + needle.len();
    };
    let rest = &attributes[at..];
    let value = match rest.chars().next()? {
        quote @ ('"' | '\'') => rest[1..].split(quote).next()?,
        _ => rest.split(|c: char| c.is_whitespace() || c == '/').next()?,
    };
    Some(value.to_owned())
}

/// The syntax a component's scripts are parsed with: `lang` decides; without one, TypeScript
/// when `typescript` (tsc parses a Vue script as TypeScript), JavaScript otherwise.
pub fn syntax(lang: Option<&str>, typescript: bool) -> SourceType {
    match lang {
        Some("tsx") => SourceType::tsx(),
        Some("ts" | "typescript") => SourceType::ts(),
        Some("jsx") => SourceType::mjs().with_jsx(true),
        _ if typescript => SourceType::ts(),
        _ => SourceType::mjs(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn vue_keeps_script_and_script_setup_in_place() {
        let source = "<template>\n  <p>é</p>\n</template>\n<script lang=\"ts\">\nimport a from './a'\n</script>\n<script setup>\nimport b from './b'\n</script>\n<style>x{}</style>\n";
        let split = scripts(source);
        assert_eq!(split.text.len(), source.len());
        assert_eq!(split.lang.as_deref(), Some("ts"));
        let a = source.find("import a").unwrap_or_default();
        let b = source.find("import b").unwrap_or_default();
        assert_eq!(&split.text[a..a + 19], "import a from './a'");
        assert_eq!(&split.text[b..b + 19], "import b from './b'");
        assert!(!split.text.contains("template") && !split.text.contains("style"));
        assert_eq!(
            split.text.matches('\n').count(),
            source.matches('\n').count()
        );
    }

    #[test]
    fn svelte_keeps_the_module_and_instance_scripts() {
        let source = "<script context=\"module\" lang=\"ts\">\n  import type { T } from './t';\n  import m from './m';\n</script>\n<script module>\nimport n from './n'\n</script>\n<script>\n  import i from './i';\n</script>\n<h1>{#await import('./later') then x}{x}{/await}</h1>\n";
        let split = scripts(source);
        assert_eq!(split.lang.as_deref(), Some("ts"));
        for kept in [
            "import m from './m'",
            "import n from './n'",
            "import i from './i'",
        ] {
            assert!(split.text.contains(kept), "{kept}");
        }
        assert!(!split.text.contains("later"));
    }

    #[test]
    fn tags_that_are_not_scripts_and_self_closing_scripts_are_skipped() {
        let split = scripts(
            "<scripts>import x from 'x'</scripts><script src=\"./a.js\" /><noscript>y</noscript>",
        );
        assert!(split.text.trim().is_empty(), "{:?}", split.text);
        assert_eq!(split.lang, None);
        let unclosed = scripts("<script>import z from 'z'");
        assert!(unclosed.text.contains("import z from 'z'"));
        let untermianted_tag = scripts("<script lang=\"ts\"");
        assert!(untermianted_tag.text.trim().is_empty());
    }

    /// A `>` inside a quoted attribute value (Vue's `generic`) does not end the start tag.
    #[test]
    fn a_quoted_greater_than_does_not_end_the_start_tag() {
        let source = "<script setup lang=\"ts\" generic=\"T extends Record<string, unknown>\">\nimport a from './a'\nimport b from './b'\n</script>\n";
        let split = scripts(source);
        assert_eq!(split.lang.as_deref(), Some("ts"));
        let body = &split.text[source.find('\n').unwrap_or_default()..];
        assert_eq!(body.trim(), "import a from './a'\nimport b from './b'");
        assert!(!split.text.contains("unknown"));
        let single =
            scripts("<script generic='T extends A<B>' lang='ts'>import c from 'c'</script>");
        assert_eq!(single.text.trim(), "import c from 'c'");
        assert_eq!(single.lang.as_deref(), Some("ts"));
    }

    #[test]
    fn start_tags_end_outside_quoted_values_only() {
        let end = |tag: &str| start_tag_end(tag, 0);
        assert_eq!(end(" a=\"x>y\">"), Some(8));
        assert_eq!(end(" a = 'x>y' >"), Some(11));
        assert_eq!(end(" a=\"it's\">"), Some(9));
        // A quote that does not follow `=` is text, not the start of a value.
        assert_eq!(end(" don't>"), Some(6));
        assert_eq!(end(" a=b'c>"), Some(6));
        assert_eq!(end(" a=\"unterminated>"), None);
        assert_eq!(end(" setup"), None);
    }

    #[test]
    fn attributes_read_every_quoting() {
        assert_eq!(attribute(" lang=\"ts\"", "lang").as_deref(), Some("ts"));
        assert_eq!(attribute(" lang='tsx'", "lang").as_deref(), Some("tsx"));
        assert_eq!(attribute(" setup lang=js", "lang").as_deref(), Some("js"));
        assert_eq!(attribute(" lang=ts/", "lang").as_deref(), Some("ts"));
        assert_eq!(attribute(" xlang=\"ts\"", "lang"), None);
        assert_eq!(
            attribute(" xlang=\"ts\" lang=\"jsx\"", "lang").as_deref(),
            Some("jsx")
        );
        assert_eq!(attribute(" setup", "lang"), None);
        assert_eq!(attribute(" lang=", "lang"), None);
    }

    #[test]
    fn syntax_follows_lang() {
        assert_eq!(syntax(Some("ts"), false), SourceType::ts());
        assert_eq!(syntax(Some("typescript"), false), SourceType::ts());
        assert_eq!(syntax(Some("tsx"), false), SourceType::tsx());
        assert_eq!(syntax(Some("jsx"), true), SourceType::mjs().with_jsx(true));
        assert_eq!(syntax(Some("js"), false), SourceType::mjs());
        assert_eq!(syntax(None, true), SourceType::ts());
        assert_eq!(syntax(None, false), SourceType::mjs());
        assert!(is_component(".vue") && is_component(".svelte") && !is_component(".ts"));
    }

    proptest! {
        #[test]
        fn blanking_keeps_length_and_lines(source in "[<>a-z \\n\"=/é]{0,200}") {
            let split = scripts(&source);
            prop_assert_eq!(split.text.len(), source.len());
            let lines = |s: &str| s.match_indices('\n').map(|(i, _)| i).collect::<Vec<_>>();
            prop_assert_eq!(lines(&split.text), lines(&source));
        }
    }
}
