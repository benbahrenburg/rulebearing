# Rulebearing for coding agents

A rule file is the best interface a coding agent has to an architecture: it is text, it is in the repository, it names the fence and the reason, and CI runs it. What this page covers is the part that makes an agent able to act on it inside its own loop, not only fail CI afterwards ([design § Rules an agent can implement and follow](artifacts/design.md#rules-an-agent-can-implement-and-follow), [ADR-0021](adr/0021-agent-surface-cli-first.md)). Everything here is a subcommand of the one binary; the MCP and LSP servers are wave 3.

## The hooks

```sh
rulebearing hooks install --claude-code
```

merges three hooks into `.claude/settings.json`, keeping every other setting and hook, and adding nothing on a second run:

| Event | Runs | So that |
| --- | --- | --- |
| `SessionStart` | `rulebearing summary --format agent` | The session starts with the open violations by rule, each with its `fix`, the ratchets' headroom and any vacuous rule |
| `PreToolUse` on `Edit` and `Write` | `rulebearing impact --from-hook` | Before a file changes, the agent sees the rules that mention it, its dependents, whether it is on a cycle and the ratchets its edges count toward |
| `Stop` | `rulebearing cruise --output-type agent --from-hook` | Before the turn ends, the findings, cheapest fix first |

Both `--from-hook` commands answer in Claude Code's hook protocol and exit 0, so a hook never fails a turn by accident:

- `impact --from-hook` reads the file from the hook's JSON on stdin (`tool_input.file_path`) and returns its report as `hookSpecificOutput.additionalContext`, the one place a `PreToolUse` hook's output reaches the agent. It never blocks the edit. When it cannot answer (no configuration, nothing extracted yet) it says why on stderr and the edit goes ahead.
- `cruise --from-hook` cruises the repository with the `agent` reporter. When a trustworthy run finds errors it prints `{"decision": "block", "reason": ...}` with the report as the reason, which keeps the turn going with the findings in front of the agent. With no errors, or a run that cannot be trusted, it prints nothing. When the hook input carries `stop_hook_active: true` (the turn was already kept going once), it does nothing, so the agent is never held in a loop. Wave 3 narrows the cruise to the changed files' closure with `--affected`.

## Questions before the import is written

| Command | Answers |
| --- | --- |
| `rulebearing can-import <from> <to>` | Would this import be allowed? `yes` (exit 0), or `no` with the rule, its comment and its `fix` (exit 1). It answers from the worktree-aware cache, so it is fast. The target's kind (`npm-dev`, `core`, its licence) comes from the graph; a target the graph has never seen and that is not a file on disk exits 2 rather than guess |
| `rulebearing impact <file> [--depth N] [--json]` | What the file is subject to: the rules that mention it, its dependents to depth N, whether it sits on a cycle and the ratchets its edges count toward; text, or JSON with `--json` |
| `rulebearing explain <rule> [--plain]` | The rule as one English sentence, why it exists, what to do, and the first edges it matched |
| `rulebearing rules --json` | Every rule, its family, severity, `fix`, and how many modules each side matches |
| `rulebearing count --from <regex> --to <regex>` | How many direct edges match, against a budget with `--budget` |

The query commands (`can-import`, `impact`, `propose`, `place`) read `--graph FILE` when given; otherwise the worktree-aware cache under `.graph/cache/<key>/`, keyed by the worktree root, `HEAD` and the configuration's hash ([plan 0002, Step 13](plans/pending/0002-wave-2-dotnet-python-element-rules.md#213-step-13-worktree-aware-cache-and-the-eslint-plugin-2g)), so two worktrees of one repository never read each other's graph. A miss extracts the paths given and writes the cache; `--no-cache` neither reads nor writes it. The key follows commits, not uncommitted edits: pass `--no-cache` to see an edit before committing it.

## The `agent` reporter

`cruise --output-type agent` (and `fmt -T agent`) writes JSON grouped by rule, with each rule's `fix` and decision token, each violation's `id`, `line` and `column`, and a cost: how many edges must move and how many modules depend on the target. Cheapest fixes come first, and `--max-findings N` keeps the answer inside a context window while `count` keeps the total ([reporters.md](reporters.md#agent)).

## Rules an agent writes, held to the same bar

An agent that adds a rule can make it wrong in ways that read green. Five checks catch that before merge ([design § Rules an agent writes](artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar)):

| Guard | Catches |
| --- | --- |
| Liveness, on by default | A rule whose `from` matches nothing: exit 2 under a `rulebearing.yaml`, a warning under a dependency-cruiser file, listed in `summary.vacuousRules` either way ([ADR-0007](adr/0007-vacuous-rules-fail-by-default.md), [ADR-0032](adr/0032-liveness-follows-the-configuration-format.md)) |
| `rulebearing test` | A rule that does not flag its own `forbidden` examples, or flags its `allowed` ones |
| `rulebearing config lint` | Rules that can never match, shadowed rules, an `allowed` list that admits everything, missing or empty `fix` text |
| `--require-comment-token` | A rule with no `adr:NNNN` or `plan:<slug>` in its comment |
| Ratchets | A budget that would rise: `count --write` refuses, and an exceeded ceiling fails `cruise` |

## Proving what ran

```sh
rulebearing attest --config rulebearing.yaml src          # writes .graph/attest.json
rulebearing attest --verify --config rulebearing.yaml src # exit 1, naming the hash that differs
```

The receipt holds SHA-256 hashes of the configuration files, of every extracted source file (or the `--graph` document) and of the violations, with the commit and the time. `--verify` recomputes them, so a claim that "the gate passed at this commit" can be checked rather than trusted. This repository's CI runs both after its self-check.

## Starting in a repository

| Situation | Command |
| --- | --- |
| No rules yet | `rulebearing init` proposes a `rulebearing.yaml` from what the repository holds (apps and packages, feature folders, framework entry points), baselines what fails today, and writes it only when a cruise with it exits 0 |
| Already on dependency-cruiser | `rulebearing adopt` keeps the configuration as it is, writes a `rulebearing.yaml` that extends it with a baseline (each entry with an owner and an expiry), adds the CI step (installing dependencies first when the folder has a lockfile) and the pre-commit hook at the repository root, running in the configuration's folder when it is below the root, and `docs/architecture/rulebearing.md` beside the configuration, and opens one pull request that is green. A repository whose hooks lefthook, pre-commit or simple-git-hooks manage gets no hook; the pull request says what to add |
