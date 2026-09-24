//! Markdown: the JavaScript and TypeScript code fences of a `.md` file, when
//! `extraExtensionsToScan` lists `.md`.
//!
//! - Plan: [Wave 2, Step 9](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#29-step-9-presets---init-presets-vue-svelte-markdown-webpackconfig-collapse-highlight-experimentalstats-2d)
//!   (`md.rs`: "when `extraExtensionsToScan` includes `.md`, extract fenced blocks tagged `js`,
//!   `ts`, `jsx`, `tsx`, `javascript`, `typescript`, with the same offsetting")
//! - Source: [coverage § Extraction and resolution](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution),
//!   row "Markdown code fences"
//! - Requirement: [FR-EXT-TS-04](../../../docs/prd.md#fr-ext-ts-04) ("Markdown code fences MUST be
//!   scanned when `.md` is listed in `extraExtensionsToScan`")
//!
//! dependency-cruiser 18.2.0 never reads a file whose extension is in `extraExtensionsToScan`:
//! such a file is a module with no dependencies. `.md` is the one exception here, because the
//! requirement asks for it; every other listed extension keeps upstream's behaviour, which gate 1
//! layer 1 records ("does not parse files matching extensions in the extraExtensionsToScan
//! array"). A repository that does not list `.md` sees no difference.
//!
//! Fences follow the `CommonMark` specification: an opening line of three or more backticks or tildes indented by at
//! most three spaces, an info string whose first word is the language, and a closing line of the
//! same character at least as long, or the end of the file. Each fence is parsed on its own (two
//! examples may declare the same name), from the file's text with everything outside the fence
//! blanked, so offsets, lines and columns are the file's.

use oxc_span::SourceType;

/// One code fence to parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    /// The source, the same length as the file, with only this fence's content and the file's
    /// newlines kept.
    pub text: String,
    /// The language tag, normalised: `js`, `jsx`, `ts` or `tsx`.
    pub lang: &'static str,
}

impl Fence {
    /// The syntax this fence is parsed with.
    pub fn syntax(&self) -> SourceType {
        syntax(self.lang)
    }
}

/// The syntax for a normalised language tag.
pub fn syntax(lang: &str) -> SourceType {
    match lang {
        "ts" => SourceType::ts(),
        "tsx" => SourceType::tsx(),
        "jsx" => SourceType::mjs().with_jsx(true),
        _ => SourceType::mjs(),
    }
}

/// A fence's language tag, normalised, when it is one this module extracts.
pub fn language(info: &str) -> Option<&'static str> {
    let word = info.split_whitespace().next()?.to_ascii_lowercase();
    match word.as_str() {
        "js" | "javascript" => Some("js"),
        "jsx" => Some("jsx"),
        "ts" | "typescript" => Some("ts"),
        "tsx" => Some("tsx"),
        _ => None,
    }
}

/// An opening or closing fence line: its character, its length and the rest of the line.
fn fence_marker(line: &str) -> Option<(u8, usize, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let marker = *rest.as_bytes().first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let length = rest.len() - rest.trim_start_matches(char::from(marker)).len();
    if length < 3 {
        return None;
    }
    let info = &rest[length..];
    // A backtick fence's info string may not contain a backtick.
    if marker == b'`' && info.contains('`') {
        return None;
    }
    Some((marker, length, info))
}

/// The JavaScript and TypeScript fences of a Markdown source, in order.
pub fn fences(source: &str) -> Vec<Fence> {
    let mut out = Vec::new();
    // (marker, length, language, content start) of the fence being read.
    let mut open: Option<(u8, usize, Option<&'static str>, usize)> = None;
    let mut offset = 0;
    for raw in source.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\n', '\r']);
        let line_start = offset;
        offset += raw.len();
        match open {
            None => {
                if let Some((marker, length, info)) = fence_marker(line) {
                    open = Some((marker, length, language(info), offset));
                }
            }
            Some((marker, length, lang, start)) => {
                let closes = fence_marker(line).is_some_and(|(m, l, info)| {
                    m == marker && l >= length && info.trim().is_empty()
                });
                if closes {
                    if let Some(lang) = lang {
                        out.push(blanked(source, start..line_start, lang));
                    }
                    open = None;
                }
            }
        }
    }
    if let Some((_, _, Some(lang), start)) = open {
        out.push(blanked(source, start..source.len(), lang));
    }
    out
}

fn blanked(source: &str, keep: std::ops::Range<usize>, lang: &'static str) -> Fence {
    let bytes: Vec<u8> = source
        .bytes()
        .enumerate()
        .map(|(at, b)| {
            if keep.contains(&at) || b == b'\n' || b == b'\r' {
                b
            } else {
                b' '
            }
        })
        .collect();
    // The kept range starts and ends at line boundaries, and every other byte became an ASCII
    // space, so the result is UTF-8.
    Fence {
        text: String::from_utf8(bytes).unwrap_or_default(),
        lang,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn tagged_fences_are_kept_in_place() {
        let source = "# Tïtle\n\n```ts\nimport a from './a';\n```\n\ntext\n\n~~~~javascript title=\"x\"\nconst b = require('./b');\n~~~~\n\n```python\nimport os\n```\n\n```\nuntagged\n```\n";
        let found = fences(source);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].lang, "ts");
        assert_eq!(found[1].lang, "js");
        let a = source.find("import a").unwrap_or_default();
        assert_eq!(&found[0].text[a..a + 20], "import a from './a';");
        assert!(!found[0].text.contains("require"));
        assert!(found[1].text.contains("const b = require('./b');"));
        for fence in &found {
            assert_eq!(fence.text.len(), source.len());
            assert!(!fence.text.contains("```") && !fence.text.contains("Tïtle"));
        }
    }

    #[test]
    fn closing_needs_the_same_character_and_length() {
        let source = "````tsx\n```\nimport x from 'x';\n~~~~\n````\n```jsx\nimport y from 'y';\n";
        let found = fences(source);
        assert_eq!(found.len(), 2);
        assert!(found[0].text.contains("import x"));
        assert_eq!(found[0].lang, "tsx");
        assert_eq!(found[1].lang, "jsx");
        assert!(
            found[1].text.contains("import y"),
            "an unclosed fence runs to the end"
        );
    }

    #[test]
    fn markers_follow_commonmark() {
        assert_eq!(fence_marker("```ts"), Some((b'`', 3, "ts")));
        assert_eq!(fence_marker("   ~~~"), Some((b'~', 3, "")));
        assert_eq!(
            fence_marker("    ```"),
            None,
            "four spaces is indented code"
        );
        assert_eq!(fence_marker("``"), None);
        assert_eq!(fence_marker("``` a`b"), None);
        assert_eq!(fence_marker("text"), None);
        assert_eq!(fence_marker(""), None);
    }

    #[test]
    fn languages_are_normalised() {
        for (info, lang) in [
            ("js", Some("js")),
            ("JavaScript", Some("js")),
            ("jsx", Some("jsx")),
            ("ts", Some("ts")),
            ("typescript {1,3}", Some("ts")),
            ("tsx", Some("tsx")),
            ("json", None),
            ("", None),
        ] {
            assert_eq!(language(info), lang, "{info}");
        }
        assert_eq!(syntax("ts"), SourceType::ts());
        assert_eq!(syntax("tsx"), SourceType::tsx());
        assert_eq!(syntax("jsx"), SourceType::mjs().with_jsx(true));
        assert_eq!(syntax("js"), SourceType::mjs());
        let fence = Fence {
            text: String::new(),
            lang: "tsx",
        };
        assert_eq!(fence.syntax(), SourceType::tsx());
    }

    proptest! {
        #[test]
        fn every_fence_keeps_length_and_lines(source in "(```ts\n|```\n|~~~js\n|[a-zé ;'()]{0,12}\n){0,12}") {
            let lines = |s: &str| s.match_indices('\n').map(|(i, _)| i).collect::<Vec<_>>();
            for fence in fences(&source) {
                prop_assert_eq!(fence.text.len(), source.len());
                prop_assert_eq!(lines(&fence.text), lines(&source));
            }
        }
    }
}
