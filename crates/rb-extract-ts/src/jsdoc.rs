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

/// A quoted string at the start of `text`. What follows it (a `;`, import attributes, prose) is
/// the tag's comment to tsc's JSDoc parser, which reads the module specifier and stops.
fn string_literal(text: &str) -> Option<&str> {
    let text = text.trim();
    let quote = text.chars().next().filter(|q| *q == '\'' || *q == '"')?;
    let rest = &text[1..];
    let end = rest.find(quote)?;
    (!rest[..end].is_empty()).then(|| &rest[..end])
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
    for expression in tag_type_expressions(&tags) {
        found.extend(
            bracket_imports(expression)
                .into_iter()
                .map(|m| (m, BRACKET_IMPORT)),
        );
    }
    found
}

/// The type expression of every tag tsc gives a `JSDocTypeExpression`, in order. Upstream reads
/// `import('x')` only there (`tag.typeExpression.kind === JSDocTypeExpression`), so a tag with
/// no type expression slot (`@template`, `@see`, an unknown tag) contributes nothing, and neither
/// does a `@typedef` whose `@property` children tsc folds into a type literal, nor a `@callback`
/// or `@overload` whose `@param` and `@returns` children become a signature.
fn tag_type_expressions<'c>(tags: &[(&str, &'c str)]) -> Vec<&'c str> {
    let mut expressions = Vec::new();
    let mut index = 0;
    while let Some((name, text)) = tags.get(index) {
        index += 1;
        match *name {
            "param" | "arg" | "argument" | "property" | "prop" => {
                expressions.extend(type_expression(text).or_else(|| name_first_type(text)));
            }
            "returns" | "return" | "throws" | "exception" => {
                expressions.extend(type_expression(text));
            }
            "type" | "this" | "enum" | "satisfies" => {
                expressions.extend(type_expression(text).or_else(|| braceless_type(text)));
            }
            "typedef" => {
                let own = type_expression(text);
                let children = count_children(&tags[index..], &["property", "prop", "type"]);
                if children == 0 {
                    expressions.extend(own);
                    continue;
                }
                let child_type = tags[index..index + children]
                    .iter()
                    .find(|(child, _)| *child == "type")
                    .and_then(|(_, text)| type_expression(text).or_else(|| braceless_type(text)));
                index += children;
                if own.is_some_and(|e| !is_object_type(e)) {
                    expressions.extend(own);
                } else if let Some(child) = child_type.filter(|e| !is_object_type(e)) {
                    expressions.push(child);
                }
            }
            "callback" | "overload" => {
                index += count_children(&tags[index..], &["param", "arg", "argument", "template"]);
                if tags
                    .get(index)
                    .is_some_and(|(child, _)| matches!(*child, "returns" | "return"))
                {
                    index += 1;
                }
            }
            _ => {}
        }
    }
    expressions
}

/// How many of the leading `tags` are named in `children`.
fn count_children(tags: &[(&str, &str)], children: &[&str]) -> usize {
    tags.iter()
        .take_while(|(name, _)| children.contains(name))
        .count()
}

/// tsc's `isObjectOrObjectArrayTypeReference`: `Object`, `object`, and arrays of them.
fn is_object_type(expression: &str) -> bool {
    let mut expression = expression.trim();
    while let Some(element) = expression.strip_suffix("[]") {
        expression = element.trim_end();
    }
    matches!(expression, "Object" | "object")
}

/// `@param name {type}`: tsc tries the type again after the name when it did not come first.
fn name_first_type(text: &str) -> Option<&str> {
    let text = text.trim_start();
    let name_end = if text.starts_with('[') {
        text.find(']').map_or(text.len(), |at| at + 1)
    } else {
        text.find(char::is_whitespace).unwrap_or(text.len())
    };
    type_expression(&text[name_end..])
}

/// A type written without braces (`@type import('x').T`), which tsc accepts for `@type`,
/// `@this`, `@enum` and `@satisfies`: the text up to the first white space outside brackets.
fn braceless_type(text: &str) -> Option<&str> {
    let text = text.trim_start_matches(|c: char| c.is_whitespace() || c == '*');
    let mut depth = 0usize;
    let mut end = text.len();
    for (at, c) in text.char_indices() {
        match c {
            '(' | '<' | '[' | '{' => depth += 1,
            ')' | '>' | ']' | '}' => depth = depth.saturating_sub(1),
            c if c.is_whitespace() && depth == 0 => {
                end = at;
                break;
            }
            _ => {}
        }
    }
    (end > 0).then(|| &text[..end])
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
        // What follows the specifier is the tag's comment: a semicolon, several tags in a row.
        assert_eq!(
            modules(
                "\n * @import { A, B } from \"../x.mjs\";\n * @import { C } from \"./y.mjs\"; and prose\n "
            ),
            ["../x.mjs", "./y.mjs"]
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
    fn only_tags_with_a_type_expression_slot_count() {
        // @type, @this, @enum and @satisfies may omit the braces.
        assert_eq!(
            modules(" @type import('watskeburt').changeType "),
            ["watskeburt"]
        );
        assert_eq!(modules(" @this import('./a').B more "), ["./a"]);
        assert_eq!(modules(" @satisfies {import('./s')} "), ["./s"]);
        assert_eq!(modules(" @throws {import('./e').E} "), ["./e"]);
        // The others need them.
        assert!(modules(" @returns import('./r').R ").is_empty());
        // @param with the name first.
        assert_eq!(modules(" @param thing {import('./p').P} "), ["./p"]);
        assert_eq!(modules(" @param [thing=1] {import('./q').Q} "), ["./q"]);
        // No type expression slot: @template's constraint, unknown tags.
        assert!(modules(" @template {import('./t').T} T ").is_empty());
        assert!(modules(" @default {import('./d')} ").is_empty());
    }

    #[test]
    fn typedef_children_fold_into_a_type_literal() {
        // Object with @property children: a type literal, not a type expression.
        assert!(
            modules(
                "\n * @typedef {Object} Shape\n * @property {import('./a').A} a\n * @prop {import('./b').B} b\n "
            )
            .is_empty()
        );
        assert!(modules("\n * @typedef Shape\n * @property {import('./a').A} a\n ").is_empty());
        // A typedef with a real type keeps it; its @property tags are children all the same.
        assert_eq!(
            modules("\n * @typedef {import('./x').X} Shape\n * @property {import('./a').A} a\n "),
            ["./x"]
        );
        // A child @type that is not Object becomes the typedef's type.
        assert_eq!(
            modules(
                "\n * @typedef Shape\n * @type {import('./t').T}\n * @property {import('./a').A} a\n "
            ),
            ["./t"]
        );
        // Without children, the typedef's own expression, and later tags as usual.
        assert_eq!(
            modules("\n * @typedef {import('./x').X} Shape\n * @returns {import('./r')} r\n "),
            ["./x", "./r"]
        );
        assert!(is_object_type(" Object[] []") && !is_object_type("Objects"));
    }

    #[test]
    fn callback_and_overload_signatures_are_not_type_expressions() {
        assert_eq!(
            modules(
                "\n * @callback Done\n * @param {import('./a').A} a\n * @returns {import('./r').R}\n * @type {import('./t').T}\n "
            ),
            ["./t"]
        );
        assert!(modules(" @overload\n * @param {import('./a').A} a ").is_empty());
    }

    #[test]
    fn import_tags_come_before_bracket_imports() {
        assert_eq!(
            modules(" @type {import('./b')} \n @import a from './a' "),
            ["./a", "./b"]
        );
    }
}
