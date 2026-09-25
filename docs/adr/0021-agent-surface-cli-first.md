# ADR-0021: The command line with the `agent` reporter is the primary agent surface; MCP and LSP are additive

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § Rules an agent can implement and follow](../artifacts/design.md#rules-an-agent-can-implement-and-follow), [§ Where it would be ignored](../artifacts/design.md#where-it-would-be-ignored) (MCP over CLI), [§ Two front-ends that will matter more than the MCP server](../artifacts/design.md#two-front-ends-that-will-matter-more-than-the-mcp-server), [§ Where each lands](../artifacts/design.md#where-each-lands), [§ How to know, rather than believe](../artifacts/design.md#how-to-know-rather-than-believe)
- **Constrains:** [architecture.md § Agent surface](../architecture.md#agent-surface)
- **Implemented by:** [Wave 1 plan](../plans/pending/0001-wave-1-typescript-parity.md), [Wave 2 plan](../plans/pending/0002-wave-2-dotnet-python-element-rules.md), [Wave 3 plan](../plans/pending/0003-wave-3-operations-surface-inner-loop.md)

## Context

Agents in Claude Code reach for `Bash` first and use an MCP server when it is the only door. They live in compiler and linter output. A separate ritual beside `tsc`, `eslint`, `vitest` and `dotnet build` is skipped.

## Decision

1. **Wave 1:** the `agent` reporter (token-budgeted, fix-cost ordered, with member references and decision links), `fix`, `examples`, `expires`, line and column, stable ids, receipts, `test`, `explain` and `explain --plain`, `can-import`, `config lint`, `hooks install --claude-code` (SessionStart brief, PreToolUse `impact`, Stop affected cruise), `attest`, and `--require-comment-token`.
2. **Wave 2:** `docs` (`agents-md`, `contributing`, `skill`), `propose`, `impact`, `place`, `test --generate`, `decisions`, the test adapters, `sarif` and `junit`, and `eslint-plugin-rulebearing`.
3. **Wave 3:** `serve --mcp`, `serve --lsp`, the Roslyn analyzer, `guard --watch`, `--mode source` for .NET. All are thin loops over the same commands and the same cached graph; none has its own rule file.
4. Adoption is measured from wave 1 by the six signals in the design (first-run pass rate above 90%, one-turn median to green, authoring guardrails catching rules, Stop-hook p95 under 2 s, `fix` on above 80% of rules, zero merged budget raises). If the first two do not move, the agent surface is cut back to the `agent` reporter and the hook.

## Consequences

- Nothing in the agent surface may disagree with the gate, because it reads the same config and graph.
- The Stop hook's two-second budget is a performance requirement on `--affected` and the cache, not a nice-to-have.
