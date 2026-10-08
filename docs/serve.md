# serve: MCP tools and LSP diagnostics

`rulebearing serve --mcp` gives an agent the query commands as Model Context Protocol tools, and `rulebearing serve --lsp` gives an editor the gate's findings as diagnostics. Both are the same binary, both speak JSON-RPC over standard input and output, and both answer from one warm graph ([design § Hooks, test runners, an MCP server, an LSP](artifacts/design.md#hooks-test-runners-an-mcp-server-an-lsp); [FR-CLI-06](prd.md#fr-cli-06)). Neither has a rule file of its own or reads anything the CLI does not: each is a thin loop over the commands ([ADR-0021](adr/0021-agent-surface-cli-first.md)).

## The warm graph

A server answers from the cruise result it holds in memory: the newer of `.graph/cruise.json` (what `rulebearing cruise -T json -f .graph/cruise.json` writes and the query commands read) and `.graph/guard/graph.json` (what a running `guard --watch` writes after a save changes the graph; [agents.md](agents.md#the-hook-without-the-wait-guard---watch)). Before each tool call, and before diagnostics are published, the server checks the two files' modification time and size and reads the newer one again only when it moved. With neither file it answers as the commands do: a fresh extraction or the cache. Start `guard --watch` beside a server and every save reaches it within a check.

## `serve --mcp`

Register it for Claude Code with the hooks:

```sh
rulebearing hooks install --claude-code --mcp   # adds "rulebearing" to .mcp.json
```

That merges this entry into `.mcp.json`, keeping any other servers, and keeps an entry already named `rulebearing` as it is:

```json
{ "mcpServers": { "rulebearing": { "command": "rulebearing", "args": ["serve", "--mcp"] } } }
```

The eight tools, in the design's order. Each runs the command beside it, in the server's process, over the warm graph, and its text is that command's standard output byte for byte, with `structuredContent` holding the same JSON:

| Tool | Command | Arguments |
| --- | --- | --- |
| `rules` | `rules --json` | `unused`, `releases`, `graph` |
| `explain` | `explain <rule> --json` | `rule` (required), `plain`, `graph` |
| `can_import` | `can-import <from> <to> --json` | `from`, `to` (required), `graph` |
| `place` | `place --json` | `language` (required), `imports`, `importedBy`, `name`, `graph` |
| `impact` | `impact <file> --json` | `file` (required), `depth`, `graph` |
| `count` | `count --from --to --json` | `from`, `to` (required), `budget`, `graph` |
| `query` | `query --from --to --json` | `from`, `to` (required), `graph` |
| `diff` | `diff -T json` | `old` and `new` (two saved results), or `base` and `paths` |

So a tool answers exactly what `rulebearing <command> --json --graph .graph/cruise.json` prints, and `crates/rb-cli/tests/serve_mcp.rs` asserts that for every tool. The server's `--config` is passed to each command that reads one. Exit 1 is an answer, as [ADR-0008](adr/0008-exit-code-contract.md) defines it: a forbidden import, or a count over its budget, comes back as the JSON with `isError: false`. A command that could not answer (exit 2 or 3) comes back with its standard error and `isError: true`. The tools only read: `count` has no `--write`.

The server answers `initialize` (protocol versions 2025-06-18, 2025-03-26, 2024-11-05 and 2024-10-07; the client's when it is one of them, else the newest), `ping`, `tools/list` and `tools/call`, one message per line, and ignores notifications. Any other method is `-32601`.

## `serve --lsp`

An editor starts it as a language server over standard input and output:

```sh
rulebearing serve --lsp [--config rulebearing.yaml]
```

| Message | What the server does |
| --- | --- |
| `textDocument/didOpen` | Publishes the file's diagnostics |
| `textDocument/didSave` | Re-checks: `cruise --cache -T json` over the paths the warm graph was cruised from (`optionsUsed.args`, else `.`), so the cache reads again only what changed; the result becomes the warm graph and every open file's diagnostics are published again |
| `textDocument/didChange` | Nothing: the gate reads files, so an unsaved edit changes no finding |
| `textDocument/didClose` | Clears the file's diagnostics |
| `textDocument/codeAction` | One `quickfix` per diagnostic of this server, titled with the rule's `fix` (its comment when it has none), with the command `rulebearing.explain` |
| `workspace/executeCommand` `rulebearing.explain` | Runs `explain <rule>`, shows its text with `window/showMessage` and returns it |
| `shutdown`, `exit` | Exits 0 after `shutdown` then `exit`, 1 for `exit` alone |

Each diagnostic is one violation of the open file, as `cruise` evaluates the warm graph with the current configuration ([Wave 3 plan § 1.5](plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)):

| Field | Value |
| --- | --- |
| `code` | The stable violation id, `RB-` and eight hex characters ([ADR-0015](adr/0015-stable-violation-id.md)) |
| `source` | `rulebearing` |
| `message` | The rule's name and its `fix`, `domain-not-to-web: Move the shared type into src/domain` |
| `severity` | Error, Warning or Information from the rule's; `ignore` is not reported |
| `range` | The line of the offending import when the extractor recorded it, else the file's first line |

The quick fix does not edit code. The design promises the `fix` as the title, and an automated edit would be a guess at what the rule's author meant.

## What the servers never do

They open no socket: both speak over standard input and output only, so [NFR-SEC-01](prd.md#nfr-sec-01) stays true of the binary, and `crates/rb-cli/tests/serve_lsp.rs` checks each running server's open sockets (`/proc` on Linux, `lsof` elsewhere). They log to standard error only, stop at the end of their input, and write nothing but the cache under `.graph/` (the LSP's re-check). `--config -` is refused, since standard input carries the protocol.
