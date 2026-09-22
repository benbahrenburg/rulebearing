# ADR-0014: No cross-language edges are invented

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § What stays honest across the boundary](../artifacts/design.md#what-stays-honest-across-the-boundary), [§ The architect's hat](../artifacts/design.md#the-architects-hat-across-repos-and-across-time) (declared edges)
- **Constrains:** [architecture.md § The graph document](../architecture.md#the-graph-document), [§ The rule engine](../architecture.md#the-rule-engine)
- **Implemented by:** [Wave 2 plan](../plans/pending/0002-wave-2-dotnet-python-element-rules.md); declared edges in [Wave 4](../plans/pending/0004-wave-4-reach.md)

## Context

A C# service that calls a Python process over HTTP, or a TypeScript app that calls a .NET API, has no import edge to it. Guessing one would make the graph, and every rule over it, untrustworthy.

## Decision

- Extractors record only edges the language's own import or reference mechanism produces. The tool never infers an edge from a URL, a service name or a filename.
- Rules may still span languages, because paths are the shared namespace; such a rule matches nothing until an edge exists, and the liveness default ([ADR-0007](0007-vacuous-rules-fail-by-default.md)) says so.
- A predicate a language cannot answer is a validation error (exit 3), not a silent false: `areSealed` on a Python class fails validation; `haveAnyAttributes` on it reads decorators. The per-language predicate table lives in `rb-rules` and is part of the published schema descriptions.
- Wave 4 adds **declared** cross-service edges (`edges.yaml`, or read from OpenAPI clients), marked `declared` on the edge so a reader can tell them from detected ones.

## Consequences

- The `dependencyKind` and `dependencyTypes` vocabularies are per language, and the coverage tabs record which apply where.
- A mixed-language oracle (dify, OpenMetadata) is one run and one graph with no fabricated edges between halves.

## Alternatives considered

- **Heuristic cross-language edges.** Rejected: precision is the product.
