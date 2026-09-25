# Test-runner adapters

One test case per rule in the host test runner, reading the JSON and putting the rule's `fix` text in the failure message ([design § Hooks, test runners, an MCP server, an LSP](../docs/artifacts/design.md#hooks-test-runners-an-mcp-server-an-lsp); [ArchUnitNET coverage § Test framework adapters](../docs/artifacts/archunitnet-0.13.4-coverage.md#test-framework-adapters); [FR-DIST-03](../docs/prd.md#fr-dist-03)). Each adapter has its own 70% coverage floor ([ADR-0018](../docs/adr/0018-test-coverage-threshold.md)). All land in [Wave 2, sub-wave 2H](../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md).

| Directory | Package | Host runners |
| --- | --- | --- |
| [dotnet/](dotnet/) | `Rulebearing.TestAdapter` | xUnit v2 and v3, NUnit, MSTest v2 and v4, TUnit: a `[Theory]` / `[TestCaseSource]` data source that runs the binary and yields one test per rule |
| [python/](python/) | `pytest-rulebearing` | pytest |
| [vitest/](vitest/) | `rulebearing/vitest` (shipped inside the npm wrapper) | vitest |
