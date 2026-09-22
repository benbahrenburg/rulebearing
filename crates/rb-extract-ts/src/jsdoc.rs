//! Imports written in JSDoc comments, which dependency-cruiser's tsc extractor reports when
//! `detectJSDocImports` is on.
//!
//! - Plan: [Wave 0, Step 8](../../../docs/plans/pending/0000-wave-0-spike.md#step-8-spike-a-rb-extract-ts-0c)
//!   (`walk.rs`: "JSDoc `@import`/`{import()}` from the comment table")
//! - Source: [coverage § Dependency types and module systems](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#dependency-types-and-module-systems)
//!   (`jsdoc`, `jsdoc-import-tag`, `jsdoc-bracket-import`)
//!
//! Two forms, in the order tsc reports them for one comment: every `@import ... from 'x'` tag,
//! then every `import('x')` inside a tag's `{type expression}`, once per tag.

use rb_model::DependencyType;

use DependencyType as D;

const IMPORT_TAG: &[D] = &[D::TypeOnly, D::Import, D::Jsdoc, D::JsdocImportTag];
const BRACKET_IMPORT: &[D] = &[D::TypeOnly, D::Import, D::Jsdoc, D::JsdocBracketImport];

/// The tags of a JSDoc comment's content: `(name, text up to the next tag)`.
fn tags(content: &str) -> Vec<(&str, &str)> {
    let mut starts = Vec::new();
    let bytes = content.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        let at_boundary =
            index == 0 || matches!(bytes[index - 1], b' ' | b'\t' | b'\n' | b'\r' | b'*');
        if *byte == b'@' && at_boundary {
            starts.push(index);
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            let end = starts.get(i + 1).copied().unwrap_or(content.len());
            let tag = &content[start + 1..end];
            let name_end = tag
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(tag.len());
            (&tag[..name_end], &tag[name_end..])
        })
        .collect()
}

/// A quoted string at the start of `text`, and nothing but whitespace and `*` after it.
fn string_literal(text: &str) -> Option<&str> {
    let text = text.trim();
    let quote = text.chars().next().filter(|q| *q == '\'' || *q == '"')?;
    let rest = &text[1..];
    let end = rest.find(quote)?;
    let trailing = rest[end + 1..].trim_matches(|c: char| c.is_whitespace() || c == '*');
    (!rest[..end].is_empty() && trailing.is_empty()).then(|| &rest[..end])
}

/// `@import clause from 'module'`: the module, when it is a string literal.
fn import_tag(text: &str) -> Option<&str> {
    let from = text.rfind("from")?;
    let before = text[..from].trim();
    if before.is_empty() || before.contains("import(") {
        return None;
    }
    string_literal(&text[from + 4..])
}

/// The `{...}` type expression at the start of a tag's text, braces balanced.
fn type_expression(text: &str) -> Option<&str> {
    let text = text.trim_start();
    if !text.starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    for (index, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[1..index]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Every `import('x')` in a type expression, in order, without repeats.
fn bracket_imports(expression: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut rest = expression;
    let mut offset = 0;
    while let Some(at) = rest.find("import") {
        let absolute = offset + at;
        let preceded_by_identifier = expression[..absolute]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$' || c == '.');
        let after = rest[at + "import".len()..].trim_start();
        if !preceded_by_identifier && let Some(inner) = after.strip_prefix('(') {
            let inner = inner.trim_start();
            if let Some(quote) = inner.chars().next().filter(|q| *q == '\'' || *q == '"')
                && let Some(end) = inner[1..].find(quote)
            {
                let module = &inner[1..=end];
                let closes = inner[end + 2..].trim_start().starts_with(')');
                if closes && !module.is_empty() && !found.iter().any(|m| m == module) {
                    found.push(module.to_owned());
                }
            }
        }
        offset = absolute + "import".len();
        rest = &expression[offset..];
    }
    found
}

/// The imports in one JSDoc comment's content (the text between `/**` and `*/`).
pub fn imports(content: &str) -> Vec<(String, &'static [DependencyType])> {
    let tags = tags(content);
    let mut found: Vec<(String, &'static [DependencyType])> = tags
        .iter()
        .filter(|(name, _)| *name == "import")
        .filter_map(|(_, text)| import_tag(text))
        .map(|module| (module.to_owned(), IMPORT_TAG))
        .collect();
    for (name, text) in &tags {
        if *name == "import" {
            continue;
        }
        if let Some(expression) = type_expression(text) {
            found.extend(
                bracket_imports(expression)
                    .into_iter()
                    .map(|m| (m, BRACKET_IMPORT)),
            );
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modules(content: &str) -> Vec<String> {
        imports(content).into_iter().map(|(m, _)| m).collect()
    }

    #[test]
    fn import_tags_need_a_string_module() {
        assert_eq!(modules(" @import * as fs from 'node:fs' "), ["node:fs"]);
        assert_eq!(
            modules(" @import {thing, thang} from \"./hello.mjs\" "),
            ["./hello.mjs"]
        );
        assert_eq!(
            modules(" @import thing from './hello.mjs' "),
            ["./hello.mjs"]
        );
        for rejected in [
            " @import {thing} from anIdentifier ",
            " @import {thing} from 481 ",
            " @import {thing} from true ",
            " @import {thing} from  ",
            " @import {import('./thing.mjs').thing} ",
        ] {
            assert!(modules(rejected).is_empty(), "{rejected}");
        }
        assert_eq!(imports(" @import a from 'b' ")[0].1, IMPORT_TAG);
    }

    #[test]
    fn bracket_imports_are_found_in_type_expressions_once_per_tag() {
        assert_eq!(modules(" @type {import('./hello.mjs')} "), ["./hello.mjs"]);
        assert_eq!(
            modules(" @type {Map<string, Partial<import(\"./hello.mjs\")>>} "),
            ["./hello.mjs"]
        );
        assert_eq!(
            modules(" @type {import('./a.mjs')|import('./b.mjs')|import('./a.mjs')} "),
            ["./a.mjs", "./b.mjs"]
        );
        assert_eq!(
            modules(
                "\n * @param {import('./hello.mjs')} p a hello\n * @returns {import('./goodbye.mjs').wave} bye\n "
            ),
            ["./hello.mjs", "./goodbye.mjs"]
        );
        assert!(modules(" @type {notAnImport('./hello.mjs').thing} ").is_empty());
        assert!(modules(" @type } ").is_empty());
        assert!(modules(" @type {import(x)} ").is_empty());
        assert!(modules(" just prose, import('./x') ").is_empty());
        assert_eq!(imports(" @type {import('a')} ")[0].1, BRACKET_IMPORT);
    }

    #[test]
    fn import_tags_come_before_bracket_imports() {
        assert_eq!(
            modules(" @type {import('./b')} \n @import a from './a' "),
            ["./a", "./b"]
        );
    }
}
