# Front-ends that report where agents already look

Both read the same `rulebearing.yaml` and the same cached graph as the gate, so neither can disagree with it ([design § Two front-ends that will matter more than the MCP server](../docs/artifacts/design.md#two-front-ends-that-will-matter-more-than-the-mcp-server); [ADR-0021](../docs/adr/0021-agent-surface-cli-first.md); [FR-DIST-04](../docs/prd.md#fr-dist-04)).

| Directory | What | Lands in |
| --- | --- | --- |
| [eslint-plugin-rulebearing/](eslint-plugin-rulebearing/README.md) | one rule, `rulebearing/boundaries`, that asks the cached graph `can-import` for each import statement and reports inline, in the words of the gate's `junit` message | [Wave 2, 2G](../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#213-step-13-worktree-aware-cache-and-the-eslint-plugin-2g) |
| [Rulebearing.Analyzer/](Rulebearing.Analyzer/) | a Roslyn analyzer that evaluates dependency and element rules on the semantic model at compile time and reports `RB0001`-style diagnostics with the `fix` as the message | [Wave 3, 3F](../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md) |
