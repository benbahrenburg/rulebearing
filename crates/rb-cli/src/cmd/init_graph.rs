//! `init`'s proposals that are read from the graph rather than from the folders: .NET layers from
//! the built assemblies' namespaces (or, when no namespace names a layer, the project names), and
//! Python rules from the top-level packages under each import root.
//!
//! - Plan: [Wave 2, Step 15](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#215-step-15-greenfield-init-proof-the-nightly-tables-upstream-offers-second-maintainer-2i)
//!   ("a `.sln` / `.slnx` or `.csproj` selects `rulebearing:dotnet` and proposes rules from layered
//!   namespaces (`Domain`, `Application`, `Infrastructure`, `Web` when present); a `pyproject.toml`
//!   selects `rulebearing:python` and proposes rules from the top-level packages")
//! - Source: [design § Test beds](../../../../docs/artifacts/design.md#test-beds-open-source-repositories-to-validate-against)
//!   item 2 (greenfield `init` must produce a config that passes)
//! - Decision: [ADR-0014](../../../../docs/adr/0014-no-invented-cross-language-edges.md) (each
//!   language's rules are read from its own modules and edges; none joins two languages)
//! - Requirements: [FR-CLI-03](../../../../docs/prd.md#fr-cli-03), [NFR-ADOPT-02](../../../../docs/prd.md#nfr-adopt-02)
//!
//! Every rule proposed here is read from what the first cruise found, so each one's `from` side
//! matches modules (none is vacuous) and none contradicts the code as it stands:
//!
//! - **.NET layers.** A namespace segment `Domain`, `Application`, `Infrastructure` or `Web` marks a
//!   layer, and the segments before it the application it belongs to. Where one application has
//!   two or more layers, each layer but the outermost is forbidden to depend on the layers outside
//!   it, in that order. The findings the code has today are baselined, as any other.
//! - **Python packages.** A folder directly under an import root is a top-level package. A package
//!   that others import today sits below them, so it is forbidden to import them back (that edge
//!   would be a cycle between packages). When no top-level package imports another, they are
//!   fenced from each other.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use rb_model::{GraphDocument, Language, Module};

use crate::cmd::init::{Proposed, quoted, regex_escape};

/// The clean-architecture layers, innermost first.
pub const LAYERS: &[&str] = &["Domain", "Application", "Infrastructure", "Web"];

/// What a layer name was read from, and so the rule key its patterns go under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerSource {
    /// A namespace segment: the rules use `namespace`.
    Namespace,
    /// A project file's name: the rules use `project`.
    Project,
}

/// One application's layers: the name before the layer segment and the layers it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layering {
    /// Where the layer names came from.
    pub source: LayerSource,
    /// The dotted name before the layer segment, empty when the layer segment comes first.
    pub prefix: String,
    /// The layers present, innermost first.
    pub layers: Vec<&'static str>,
}

/// The prefix and the layer of a dotted name: the first segment that is a layer name, and the
/// segments before it.
pub fn layer_of(dotted: &str) -> Option<(String, &'static str)> {
    let parts: Vec<&str> = dotted.split('.').collect();
    parts.iter().enumerate().find_map(|(i, part)| {
        LAYERS
            .iter()
            .find(|layer| **layer == *part)
            .map(|layer| (parts[..i].join("."), *layer))
    })
}

fn project_stem(project: &str) -> &str {
    let name = project.rsplit('/').next().unwrap_or(project);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

fn with_two_layers(source: LayerSource, found: BTreeMap<String, BTreeSet<usize>>) -> Vec<Layering> {
    found
        .into_iter()
        .filter(|(_, layers)| layers.len() >= 2)
        .map(|(prefix, layers)| Layering {
            source,
            prefix,
            layers: layers.into_iter().map(|i| LAYERS[i]).collect(),
        })
        .collect()
}

/// The layered applications among the .NET modules: from their namespaces, else from their
/// project names, sorted by prefix.
pub fn layerings(modules: &[Module]) -> Vec<Layering> {
    let mut by_namespace: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let mut by_project: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let index = |layer: &str| LAYERS.iter().position(|l| *l == layer);
    for module in modules
        .iter()
        .filter(|m| m.language == Some(Language::Dotnet))
    {
        for namespace in module.namespaces.iter().flatten() {
            if let Some((prefix, layer)) = layer_of(namespace) {
                by_namespace.entry(prefix).or_default().extend(index(layer));
            }
        }
        if let Some((prefix, layer)) = module
            .project
            .as_deref()
            .and_then(|p| layer_of(project_stem(p)))
        {
            by_project.entry(prefix).or_default().extend(index(layer));
        }
    }
    let from_namespaces = with_two_layers(LayerSource::Namespace, by_namespace);
    if from_namespaces.is_empty() {
        with_two_layers(LayerSource::Project, by_project)
    } else {
        from_namespaces
    }
}

fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_owned()
}

/// The pattern for `layers` of `layering`, for the key its source names.
fn layer_pattern(layering: &Layering, layers: &[&str]) -> String {
    let alternation = if layers.len() == 1 {
        layers[0].to_owned()
    } else {
        format!("({})", layers.join("|"))
    };
    let prefix = regex_escape(&layering.prefix);
    match layering.source {
        LayerSource::Namespace if prefix.is_empty() => format!("^{alternation}(\\.|$)"),
        LayerSource::Namespace => format!("^{prefix}\\.{alternation}(\\.|$)"),
        LayerSource::Project if prefix.is_empty() => format!("(^|/){alternation}\\.[a-z]+proj$"),
        LayerSource::Project => format!("(^|/){prefix}\\.{alternation}\\.[a-z]+proj$"),
    }
}

/// One rule per layer but the outermost: it never depends on the layers outside it.
pub fn layer_rules(layerings: &[Layering]) -> Vec<Proposed> {
    let mut out = Vec::new();
    for layering in layerings {
        let key = match layering.source {
            LayerSource::Namespace => "namespace",
            LayerSource::Project => "project",
        };
        let suffix = if layerings.len() > 1 && !layering.prefix.is_empty() {
            format!("-in-{}", slug(&layering.prefix))
        } else {
            String::new()
        };
        for (i, layer) in layering.layers.iter().enumerate() {
            let outer = &layering.layers[i + 1..];
            if outer.is_empty() {
                continue;
            }
            let name = format!("{}-not-to-outer-layers{suffix}", slug(layer));
            let listed = outer.join(", ");
            let mut yaml = String::new();
            let _ = writeln!(yaml, "      - name: {name}");
            let _ = writeln!(
                yaml,
                "        comment: {}",
                quoted(&format!(
                    "{layer} is an inner layer: it never depends on {listed}, which depend on it."
                ))
            );
            let _ = writeln!(
                yaml,
                "        fix: {}",
                quoted(&format!(
                    "Declare what {layer} needs as an abstraction in {layer} and implement it in the outer layer; never add a ProjectReference from {layer} to {listed}."
                ))
            );
            let _ = writeln!(yaml, "        severity: error");
            let _ = writeln!(
                yaml,
                "        from: {{ {key}: {} }}",
                quoted(&layer_pattern(layering, &[layer]))
            );
            let _ = writeln!(
                yaml,
                "        to: {{ {key}: {} }}",
                quoted(&layer_pattern(layering, outer))
            );
            out.push(Proposed { name, yaml });
        }
    }
    out
}

/// A top-level Python package: the folder directly under an import root.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Package {
    /// The folder, repository-relative.
    pub folder: String,
    /// The package name.
    pub name: String,
}

fn is_identifier(part: &str) -> bool {
    let mut chars = part.chars();
    chars.next().is_some_and(|c| c == '_' || c.is_alphabetic())
        && chars.all(|c| c == '_' || c.is_alphanumeric())
}

/// The top-level package a module `source` belongs to: the folder directly under the longest of
/// `roots` that holds it. A module directly under its root is no package.
pub fn package_of(source: &str, roots: &[String]) -> Option<Package> {
    let (root, rest) = roots
        .iter()
        .filter_map(|root| {
            if root == "." {
                Some((root, source))
            } else {
                Some((root, source.strip_prefix(root.as_str())?.strip_prefix('/')?))
            }
        })
        .max_by_key(|(root, _)| if *root == "." { 0 } else { root.len() })?;
    let (first, _) = rest.split_once('/')?;
    is_identifier(first).then(|| Package {
        folder: if root == "." {
            first.to_owned()
        } else {
            format!("{root}/{first}")
        },
        name: first.to_owned(),
    })
}

/// The top-level packages and, for each, the packages it imports directly.
pub fn package_graph(modules: &[Module], roots: &[String]) -> BTreeMap<Package, BTreeSet<Package>> {
    let python: BTreeMap<&str, Package> = modules
        .iter()
        .filter(|m| m.language == Some(Language::Python))
        .filter_map(|m| Some((m.source.as_str(), package_of(&m.source, roots)?)))
        .collect();
    let mut graph: BTreeMap<Package, BTreeSet<Package>> = python
        .values()
        .map(|p| (p.clone(), BTreeSet::new()))
        .collect();
    for module in modules {
        let Some(from) = python.get(module.source.as_str()) else {
            continue;
        };
        for dependency in &module.dependencies {
            if let Some(to) = python.get(dependency.resolved.as_str())
                && to != from
            {
                graph.entry(from.clone()).or_default().insert(to.clone());
            }
        }
    }
    graph
}

fn reach(graph: &BTreeMap<Package, BTreeSet<Package>>, start: &Package) -> BTreeSet<Package> {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<&Package> = graph.get(start).into_iter().flatten().collect();
    while let Some(next) = stack.pop() {
        if seen.insert(next.clone()) {
            stack.extend(graph.get(next).into_iter().flatten());
        }
    }
    seen
}

fn alternation(packages: &[&Package]) -> String {
    let folders: Vec<String> = packages.iter().map(|p| regex_escape(&p.folder)).collect();
    if folders.len() == 1 {
        folders.concat()
    } else {
        format!("({})", folders.join("|"))
    }
}

/// The rules the top-level packages call for: each package imported by others never imports them
/// back; when none imports another, they are fenced from each other.
pub fn package_rules(graph: &BTreeMap<Package, BTreeSet<Package>>) -> Vec<Proposed> {
    let reaches: BTreeMap<&Package, BTreeSet<Package>> =
        graph.keys().map(|p| (p, reach(graph, p))).collect();
    let mut names: BTreeMap<&str, usize> = BTreeMap::new();
    for package in graph.keys() {
        *names.entry(package.name.as_str()).or_default() += 1;
    }
    let mut out = Vec::new();
    for (package, reached) in &reaches {
        let dependents: Vec<&Package> = reaches
            .iter()
            .filter(|(other, theirs)| {
                *other != package && theirs.contains(package) && !reached.contains(*other)
            })
            .map(|(other, _)| *other)
            .collect();
        if dependents.is_empty() {
            continue;
        }
        let label = if names.get(package.name.as_str()) > Some(&1) {
            slug(&package.folder)
        } else {
            package.name.clone()
        };
        let name = format!("{label}-not-to-its-dependents");
        let listed: Vec<&str> = dependents.iter().map(|p| p.name.as_str()).collect();
        let mut yaml = String::new();
        let _ = writeln!(yaml, "      - name: {name}");
        let _ = writeln!(
            yaml,
            "        comment: {}",
            quoted(&format!(
                "{} import {}, so it sits below them: {} importing one of them back would make a cycle between top-level packages.",
                listed.join(", "),
                package.name,
                package.name
            ))
        );
        let _ = writeln!(
            yaml,
            "        fix: {}",
            quoted(&format!(
                "Move what {} needs from them down into {} (or a package below it), or have the caller pass it in.",
                package.name, package.name
            ))
        );
        let _ = writeln!(yaml, "        severity: error");
        let _ = writeln!(
            yaml,
            "        from: {{ path: {} }}",
            quoted(&format!("^{}/", regex_escape(&package.folder)))
        );
        let _ = writeln!(
            yaml,
            "        to: {{ path: {} }}",
            quoted(&format!("^{}/", alternation(&dependents)))
        );
        out.push(Proposed { name, yaml });
    }
    let unconnected = graph.values().all(BTreeSet::is_empty);
    if graph.len() >= 2 && unconnected {
        let all: Vec<&Package> = graph.keys().collect();
        let folders = alternation(&all);
        let folders = folders
            .strip_prefix('(')
            .and_then(|f| f.strip_suffix(')'))
            .unwrap_or(&folders);
        let mut yaml = String::new();
        let _ = writeln!(yaml, "      - name: top-level-packages-are-independent");
        let _ = writeln!(
            yaml,
            "        comment: {}",
            quoted(
                "No top-level package imports another today; each is released and changed on its own."
            )
        );
        let _ = writeln!(
            yaml,
            "        fix: {}",
            quoted(
                "Move what both packages need into a package of its own that both import, or pass it in from the caller."
            )
        );
        let _ = writeln!(yaml, "        severity: error");
        let _ = writeln!(
            yaml,
            "        from: {{ path: {} }}",
            quoted(&format!("^({folders})/"))
        );
        let _ = writeln!(
            yaml,
            "        to: {{ path: {}, pathNot: {} }}",
            quoted(&format!("^({folders})/")),
            quoted("^$1/")
        );
        out.push(Proposed {
            name: "top-level-packages-are-independent".into(),
            yaml,
        });
    }
    out
}

/// Every rule the graph calls for: the .NET layers, then the Python packages.
pub fn graph_rules(document: &GraphDocument) -> Vec<Proposed> {
    let mut out = layer_rules(&layerings(&document.modules));
    let roots: Vec<String> = document
        .summary
        .inspected
        .as_ref()
        .and_then(|i| i.get(&Language::Python))
        .and_then(|r| r.roots.clone())
        .unwrap_or_default();
    if !roots.is_empty() {
        out.extend(package_rules(&package_graph(&document.modules, &roots)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dotnet(source: &str, namespaces: &[&str], project: &str) -> Module {
        Module {
            language: Some(Language::Dotnet),
            namespaces: Some(namespaces.iter().map(|n| (*n).to_owned()).collect()),
            project: Some(project.to_owned()),
            ..Module::new(source.to_owned())
        }
    }

    fn python(source: &str, imports: &[&str]) -> Module {
        let mut module = Module {
            language: Some(Language::Python),
            ..Module::new(source.to_owned())
        };
        module.dependencies = imports
            .iter()
            .map(|to| rb_model::Dependency::new(*to, *to, rb_model::ModuleSystem::Py))
            .collect();
        module
    }

    #[test]
    fn a_layer_is_the_first_segment_that_names_one() {
        let cases = [
            ("App.Domain.Entities", Some(("App", "Domain"))),
            ("App.Web.Infrastructure", Some(("App", "Web"))),
            ("Domain", Some(("", "Domain"))),
            ("A.B.Application", Some(("A.B", "Application"))),
            ("App.Domains", None),
            ("App.domain", None),
            ("", None),
        ];
        for (dotted, expected) in cases {
            assert_eq!(
                layer_of(dotted),
                expected.map(|(p, l)| (p.to_owned(), l)),
                "{dotted}"
            );
        }
        assert_eq!(
            project_stem("src/App.Domain/App.Domain.csproj"),
            "App.Domain"
        );
        assert_eq!(project_stem("Web.csproj"), "Web");
        assert_eq!(slug("Clean.Architecture_X"), "clean-architecture-x");
    }

    #[test]
    fn namespaces_name_the_layers_and_projects_are_the_fallback() {
        let modules = [
            dotnet(
                "src/Domain/A.cs",
                &["Clean.Domain.Entities"],
                "src/Domain/Domain.csproj",
            ),
            dotnet(
                "src/Web/B.cs",
                &["Clean.Web", "Clean.Web.Endpoints"],
                "src/Web/Web.csproj",
            ),
            dotnet(
                "src/Infrastructure/C.cs",
                &["Clean.Infrastructure.Data"],
                "src/Infrastructure/Infrastructure.csproj",
            ),
            dotnet("tools/D.cs", &["Other.Domain"], "tools/Tools.csproj"),
            python("py/x.py", &[]),
        ];
        assert_eq!(
            layerings(&modules),
            [Layering {
                source: LayerSource::Namespace,
                prefix: "Clean".into(),
                layers: vec!["Domain", "Infrastructure", "Web"],
            }]
        );
        let names: Vec<String> = layer_rules(&layerings(&modules))
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(
            names,
            [
                "domain-not-to-outer-layers",
                "infrastructure-not-to-outer-layers"
            ]
        );
        // With no layered namespace, the project names are read.
        let bare = [
            dotnet("a/A.cs", &["Shop"], "src/Shop.Domain/Shop.Domain.csproj"),
            dotnet("b/B.cs", &["Shop"], "src/Shop.Web/Shop.Web.csproj"),
        ];
        let found = layerings(&bare);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].source, LayerSource::Project);
        let rules = layer_rules(&found);
        assert_eq!(rules.len(), 1);
        assert!(
            rules[0]
                .yaml
                .contains(r#"from: { project: "(^|/)Shop\\.Domain\\.[a-z]+proj$" }"#),
            "{}",
            rules[0].yaml
        );
        // One layer alone is no layering.
        assert!(layerings(&bare[..1]).is_empty());
    }

    #[test]
    fn layer_patterns_match_the_layer_and_below_only() -> Result<(), regex::Error> {
        let layering = Layering {
            source: LayerSource::Namespace,
            prefix: "My.App".into(),
            layers: vec!["Domain", "Application", "Web"],
        };
        let inner = regex::Regex::new(&layer_pattern(&layering, &["Domain"]))?;
        let outer = regex::Regex::new(&layer_pattern(&layering, &["Application", "Web"]))?;
        for (namespace, is_inner, is_outer) in [
            ("My.App.Domain", true, false),
            ("My.App.Domain.Entities", true, false),
            ("My.App.DomainEvents", false, false),
            ("My.App.Web.Endpoints", false, true),
            ("My.App.Application", false, true),
            ("MyXApp.Web", false, false),
        ] {
            assert_eq!(inner.is_match(namespace), is_inner, "{namespace}");
            assert_eq!(outer.is_match(namespace), is_outer, "{namespace}");
        }
        let two = [
            layering.clone(),
            Layering {
                prefix: String::new(),
                ..layering
            },
        ];
        let names: Vec<String> = layer_rules(&two).into_iter().map(|r| r.name).collect();
        assert_eq!(
            names,
            [
                "domain-not-to-outer-layers-in-my-app",
                "application-not-to-outer-layers-in-my-app",
                "domain-not-to-outer-layers",
                "application-not-to-outer-layers"
            ]
        );
        Ok(())
    }

    #[test]
    fn packages_sit_under_the_longest_root() {
        let roots = vec![".".to_owned(), "python/src".to_owned()];
        assert_eq!(
            package_of("python/src/pkg/core.py", &roots),
            Some(Package {
                folder: "python/src/pkg".into(),
                name: "pkg".into()
            })
        );
        assert_eq!(
            package_of("tests/test_x.py", &roots).map(|p| p.folder),
            Some("tests".into())
        );
        assert_eq!(
            package_of("setup.py", &roots),
            None,
            "a module, not a package"
        );
        assert_eq!(
            package_of("python/src/x.py", &roots),
            None,
            "directly under its root"
        );
        assert_eq!(package_of("my-scripts/a.py", &roots), None);
        assert_eq!(package_of("other/pkg/a.py", &["python".to_owned()]), None);
    }

    #[test]
    fn a_package_others_import_never_imports_them_back() {
        let roots = vec![
            "libs/core/src".to_owned(),
            "libs/chat/src".to_owned(),
            ".".to_owned(),
        ];
        let modules = [
            python("libs/core/src/core/a.py", &[]),
            python("libs/core/src/core/b.py", &["libs/core/src/core/a.py"]),
            python("libs/chat/src/chat/c.py", &["libs/core/src/core/a.py"]),
            python("tests/test_c.py", &["libs/chat/src/chat/c.py"]),
            python("tests/helpers/h.py", &["tests/test_c.py"]),
            python("helpers/h.py", &["tests/test_c.py"]),
        ];
        let graph = package_graph(&modules, &roots);
        assert_eq!(graph.len(), 4);
        let rules = package_rules(&graph);
        let names: Vec<&str> = rules.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "chat-not-to-its-dependents",
                "core-not-to-its-dependents",
                "tests-not-to-its-dependents"
            ]
        );
        let core = &rules[1].yaml;
        assert!(
            core.contains(r#"from: { path: "^libs/core/src/core/" }"#),
            "{core}"
        );
        assert!(
            core.contains(r#"to: { path: "^(helpers|libs/chat/src/chat|tests)/" }"#),
            "{core}"
        );
        // A cycle between two packages proposes nothing for either.
        let cycle = [python("a/x.py", &["b/y.py"]), python("b/y.py", &["a/x.py"])];
        assert!(package_rules(&package_graph(&cycle, &[".".to_owned()])).is_empty());
    }

    #[test]
    fn unconnected_packages_are_fenced() {
        let modules = [python("a/x.py", &[]), python("b/y.py", &["json"])];
        let rules = package_rules(&package_graph(&modules, &[".".to_owned()]));
        assert_eq!(rules.len(), 1);
        assert!(
            rules[0].yaml.contains(r#"from: { path: "^(a|b)/" }"#),
            "{}",
            rules[0].yaml
        );
        assert!(package_rules(&package_graph(&modules[..1], &[".".to_owned()])).is_empty());
        let document = GraphDocument {
            modules: modules.to_vec(),
            ..GraphDocument::default()
        };
        assert!(graph_rules(&document).is_empty(), "no receipt, no roots");
    }
}
