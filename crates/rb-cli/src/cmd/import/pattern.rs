//! Patterns the importers write: globs and module expressions translated to the regular
//! expressions a `rulebearing.yaml` path condition takes.
//!
//! - Plan: [Wave 2, Step 11](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f)
//!   ("wildcards translated to regex", "the globs converted to regex")
//! - Decision: [ADR-0016](../../../../../docs/adr/0016-linear-time-regex-and-strict-compat.md)
//!   (every pattern must run on the linear-time engine)
//! - Requirement: [FR-CLI-04](../../../../../docs/prd.md#fr-cli-04)
//!
//! | Source | Written as |
//! | --- | --- |
//! | a glob (`src/**/*.ts`, `{a,b}`, `?`, `[ab]`) | an anchored expression, `**/` spanning folders, `*` within one |
//! | a plain path (`./src/server`) | the path and everything under it: `^src/server(/\|$)` |
//! | an import-linter module expression (`pkg.*.models`, `pkg.**`) | the module's file or package folder under its root |

/// Escapes text so a regular expression matches it literally.
pub fn escape(text: &str) -> String {
    text.chars()
        .fold(String::with_capacity(text.len()), |mut out, c| {
            if "\\^$.|?*+()[]{}".contains(c) {
                out.push('\\');
            }
            out.push(c);
            out
        })
}

/// Whether a path is a glob rather than a plain path.
pub fn is_glob(text: &str) -> bool {
    text.contains(['*', '?', '[', '{'])
}

/// A path relative to the repository: `./` and a leading `/` removed, separators forward.
pub fn relative(text: &str) -> String {
    let mut path = text.replace('\\', "/");
    while let Some(rest) = path.strip_prefix("./") {
        path = rest.to_owned();
    }
    path.trim_start_matches('/').to_owned()
}

/// Joins a base folder and a relative path the way `path.join` does, `..` popping a segment.
pub fn join(base: &str, path: &str) -> String {
    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// A glob as an anchored regular expression, as minimatch reads it: `**` spans folders, `*` and
/// `?` stay within one, `{a,b}` is an alternation, `[...]` a class.
pub fn glob(text: &str) -> String {
    let chars: Vec<char> = relative(text).chars().collect();
    // Braces are an alternation only when every one is closed outside a class; otherwise they
    // are literal, as minimatch reads an unbalanced brace.
    translate(&chars, true).unwrap_or_else(|| translate(&chars, false).unwrap_or_default())
}

fn translate(chars: &[char], balanced: bool) -> Option<String> {
    let mut out = String::from("^");
    let mut i = 0;
    let mut braces = 0usize;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '*' if chars.get(i + 1) == Some(&'*') => {
                let at_start = i == 0 || chars[i - 1] == '/';
                if at_start && chars.get(i + 2) == Some(&'/') {
                    out.push_str("(?:.*/)?");
                    i += 3;
                    continue;
                }
                out.push_str(".*");
                i += 2;
                continue;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '[' => {
                let class = chars[i + 1..]
                    .iter()
                    .position(|&x| x == ']')
                    .map(|end| (end, chars[i + 1..i + 1 + end].iter().collect::<String>()));
                if let Some((end, class)) = class {
                    let (negated, body) = class
                        .strip_prefix('!')
                        .map_or((false, class.as_str()), |rest| (true, rest));
                    if !body.is_empty() {
                        out.push('[');
                        if negated {
                            out.push('^');
                        }
                        for c in body.chars() {
                            if matches!(c, '\\' | '[' | '&' | '~' | '^') {
                                out.push('\\');
                            }
                            out.push(c);
                        }
                        out.push(']');
                        i += end + 2;
                        continue;
                    }
                }
                out.push_str("\\[");
            }
            '{' if balanced => {
                braces += 1;
                out.push_str("(?:");
            }
            '}' if braces > 0 => {
                braces -= 1;
                out.push(')');
            }
            ',' if braces > 0 => out.push('|'),
            other => out.push_str(&escape(&other.to_string())),
        }
        i += 1;
    }
    out.push('$');
    (braces == 0).then_some(out)
}

/// A plain path and everything under it: the file itself, or the folder and what it holds.
pub fn path_prefix(text: &str) -> String {
    let path = relative(text);
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return "^".to_owned();
    }
    format!("^{}(/|$)", escape(path))
}

/// A glob or a plain path, whichever `text` is.
pub fn path_or_glob(text: &str) -> String {
    if is_glob(text) {
        glob(text)
    } else {
        path_prefix(text)
    }
}

/// An import-linter module expression as a path pattern: each dotted segment one folder, `*` any
/// one module name, `**` one or more. With `as_packages` (import-linter's default) the module's
/// descendants are included: its package folder, or its `.py` file.
pub fn module_path(root: &str, expression: &str, as_packages: bool) -> String {
    let body = expression
        .split('.')
        .map(|segment| match segment {
            "*" => "[^/]+".to_owned(),
            "**" => "[^/]+(?:/[^/]+)*".to_owned(),
            literal => escape(literal),
        })
        .collect::<Vec<_>>()
        .join("/");
    let prefix = if root.is_empty() || root == "." {
        String::new()
    } else {
        format!("{}/", escape(root.trim_end_matches('/')))
    };
    if as_packages {
        format!("^{prefix}{body}(/|\\.py$)")
    } else {
        format!("^{prefix}{body}(/__init__\\.py|\\.py)$")
    }
}

/// An external module expression (a distribution or the standard library) as the dotted name the
/// Python extractor resolves it to, with its submodules.
pub fn external_module(expression: &str) -> String {
    let body = expression
        .split('.')
        .map(|segment| match segment {
            "*" => "[^.]+".to_owned(),
            "**" => "[^.]+(?:\\.[^.]+)*".to_owned(),
            literal => escape(literal),
        })
        .collect::<Vec<_>>()
        .join("\\.");
    format!("^{body}(\\.|$)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn matches(pattern: &str, text: &str) -> bool {
        regex::Regex::new(pattern).is_ok_and(|re| re.is_match(text))
    }

    #[test]
    fn globs_follow_minimatch() {
        let table = [
            ("src/**/*.ts", "src/a/b/c.ts", true),
            ("src/**/*.ts", "src/c.ts", true),
            ("src/**/*.ts", "src/c.tsx", false),
            ("./src/*.js", "src/a.js", true),
            ("./src/*.js", "src/a/b.js", false),
            ("src/{client,server}/**", "src/server/x/y.js", true),
            ("src/{client,server}/**", "src/shared/x.js", false),
            ("src/file?.js", "src/file1.js", true),
            ("src/[ab].js", "src/a.js", true),
            ("src/[!ab].js", "src/a.js", false),
            ("src/[!ab].js", "src/c.js", true),
            ("**/*.test.ts", "a/b.test.ts", true),
            ("lib/a+b.js", "lib/a+b.js", true),
        ];
        for (pattern, path, expected) in table {
            let re = glob(pattern);
            assert_eq!(
                matches(&re, path),
                expected,
                "{pattern} ({re}) against {path}"
            );
        }
        assert_eq!(glob("src/**/*.ts"), "^src/(?:.*/)?[^/]*\\.ts$");
    }

    #[test]
    fn plain_paths_cover_the_folder_and_the_file() {
        let re = path_prefix("./src/server/");
        assert_eq!(re, "^src/server(/|$)");
        assert!(matches(&re, "src/server/a.js"));
        assert!(matches(&re, "src/server"));
        assert!(!matches(&re, "src/serverless/a.js"));
        assert_eq!(path_prefix("./"), "^");
        assert_eq!(path_or_glob("src/*.js"), glob("src/*.js"));
        assert_eq!(path_or_glob("src"), "^src(/|$)");
        assert!(is_glob("a/{b,c}") && !is_glob("a/b"));
    }

    #[test]
    fn joins_resolve_dot_segments() {
        assert_eq!(join("src/server", "./shared"), "src/server/shared");
        assert_eq!(join("src/server", "../client"), "src/client");
        assert_eq!(join("", "a"), "a");
        assert_eq!(relative(".\\a\\b"), "a/b");
    }

    #[test]
    fn module_expressions_become_package_or_file_paths() {
        let re = module_path("src", "importlinter.ui", true);
        assert_eq!(re, "^src/importlinter/ui(/|\\.py$)");
        assert!(matches(&re, "src/importlinter/ui/app.py"));
        assert!(matches(&re, "src/importlinter/ui.py"));
        assert!(!matches(&re, "src/importlinter/uix.py"));
        let star = module_path("", "pkg.*.models", true);
        assert!(matches(&star, "pkg/a/models.py"));
        assert!(!matches(&star, "pkg/a/b/models.py"));
        let stars = module_path(".", "pkg.**.models", true);
        assert!(matches(&stars, "pkg/a/b/models/x.py"));
        let module = module_path("src", "pkg.a", false);
        assert!(matches(&module, "src/pkg/a.py"));
        assert!(matches(&module, "src/pkg/a/__init__.py"));
        assert!(!matches(&module, "src/pkg/a/b.py"));
        let external = external_module("django.db");
        assert!(matches(&external, "django.db.models"));
        assert!(matches(&external, "django.db"));
        assert!(!matches(&external, "django.dbx"));
        assert!(matches(&external_module("a.*"), "a.b"));
        assert!(matches(&external_module("a.**"), "a.b.c"));
    }

    proptest! {
        #[test]
        fn an_escaped_literal_matches_only_itself(text in "\\PC{0,16}") {
            let re = regex::Regex::new(&format!("^{}$", escape(&text)))
                .map_err(|e| TestCaseError::fail(e.to_string()))?;
            prop_assert!(re.is_match(&text));
            let longer = format!("{text}x");
            prop_assert!(!re.is_match(&longer));
        }

        #[test]
        fn every_glob_compiles(text in "[a-z/*?.{},\\[\\]!]{0,16}") {
            prop_assert!(regex::Regex::new(&glob(&text)).is_ok(), "{}", glob(&text));
        }
    }
}
