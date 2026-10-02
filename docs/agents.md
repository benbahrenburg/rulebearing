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
- `cruise --from-hook` cruises the repository with the `agent` reporter. When a trustworthy run finds errors it prints `{"decision": "block", "reason": ...}` with the report as the reason, which keeps the turn going with the findings in front of the agent. With no errors, or a run that cannot be trusted, it prints nothing. When the hook input carries `stop_hook_active: true` (the turn was already kept going once), it does nothing, so the agent is never held in a loop. When a `guard --watch` is running ([below](#the-hook-without-the-wait-guard---watch)), it serves the guard's answer instead of cruising. `--affected HEAD` narrows the cruise to the changed files' closure ([cli.md](cli.md#affected-runs)).

## The hook without the wait: `guard --watch`

On a large repository a cruise at the end of every turn is seconds the agent waits for. `guard --watch` does the work as files are saved, so the hook only reads a file ([Wave 3, Step 16](plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof); [design § The agentic engineering hat](artifacts/design.md#the-agentic-engineering-hat-turn-two)):

```sh
rulebearing guard --watch            # in a terminal beside the session; Ctrl-D stops it
```

| Step | What happens |
| --- | --- |
| Start | One extraction, keeping each file's state; the answer `cruise --from-hook` would give is written to `.graph/guard/findings.json` with `writtenAt` and `seenUpTo` (milliseconds since the epoch), `configHash` (the configuration files' hash), and a key of the command line it answers for |
| A source is saved | That file alone is extracted again (the extractor's incremental entry, `.cs` files in source mode) and the answer rewritten; `rechecked` names the file, `latencyMs` the time from its modification to the answer written, `timings` the split |
| A configuration, manifest, tsconfig, solution or project file changes, or a source is added or removed | Everything is read again |
| Nothing changes | The file is rewritten every second, so its age says the guard is alive and has seen every change until then |
| Standard input closes | The guard removes its findings and exits 0 |

The Stop hook (`cruise --from-hook`) serves the guard's answer when the findings are younger than 5 s, name the configuration's current hash and the same command line (every `cruise` flag except those that only shape how the answer is printed: the reporter, the destination, progress, colour, the cache and the mode), and were written by the same build, and once the guard confirms it has seen every change until the hook asked: the hook writes `.graph/guard/request` with the time, and serves the answer when the findings' `seenUpTo` (the start of the scan the answer reflects) passes it. A file saved a moment before the turn ends is therefore in the answer. The guard confirms on its next check, a few milliseconds when nothing changed, or after checking the change again when something did. When no guard confirms within 5 s, or the findings do not answer for this run, the hook cruises as before; the findings `guard` without `--watch` writes are never confirmed, so they are for a script to read, not for the hook. The guard takes the flags the answer depends on (`--config`, the paths, `--affected`, `--affected-depth`, `--liveness`, `--max-findings`, `--sidecar`, `--mode`), so `guard --watch --affected HEAD` pairs with a hook of `cruise --output-type agent --from-hook --affected HEAD`. It reads .NET in source mode unless `--mode compiled` is given, so its answer for a .NET file is approximate and says so. It polls (`--interval`, 25 ms by default) rather than subscribing to file-system events, writes nothing outside `.graph/guard/`, and logs one line per check to stderr. `guard` without `--watch` writes one answer and exits.

A check of one saved file takes under 100 ms on the fixture the integration test uses (`crates/rb-cli/tests/guard.rs`). On the 5,500-module synthetic tree of [perf.md](perf.md) it took about 470 ms; it now takes a p50 of 74 to 81 ms and a p95 of 88 to 98 ms on an idle laptop (about 18 ms extracting, about 55 ms evaluating and reporting), under the 100 ms [NFR-PERF-03](prd.md#nfr-perf-03) asks for. On a 4-vCPU Linux runner the same check took a p95 of 255 ms, so the 100 ms is set for a developer machine ([ADR-0060](adr/0060-the-guards-latency-target-is-set-for-a-developer-machine.md), Proposed). A save that leaves the graph as it was (no import changed) gets the earlier answer again without evaluating, in about 45 ms; one that changes the graph evaluates every rule again. [`testbeds/synth/guard.sh`](../testbeds/synth/guard.sh) measures both kinds, and the nightly `bench` workflow fails when either p95 reaches 100 ms ([perf.md](perf.md)) ([Wave 3, Step 16](plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)).

## Questions before the import is written

| Command | Answers |
| --- | --- |
| `rulebearing can-import <from> <to>` | Would this import be allowed? `yes` (exit 0), or `no` with the rule, its comment and its `fix` (exit 1). It answers from the worktree-aware cache, so it is fast. The target's kind (`npm-dev`, `core`, its licence) comes from the graph; a target the graph has never seen and that is not a file on disk exits 2 rather than guess. `--json` prints the same answer as one object (`verdict`, `from`, `to`, `violations` with each rule's `name`, `severity`, `id`, `comment` and `fix`, and `warnings`), with the violation id the gate would give the edge; [eslint-plugin-rulebearing](../frontends/eslint-plugin-rulebearing/README.md) reads it |
| `rulebearing impact <file> [--depth N] [--json]` | What the file is subject to: the rules that mention it, its dependents to depth N, whether it sits on a cycle and the ratchets its edges count toward; text, or JSON with `--json` |
| `rulebearing explain <rule> [--plain]` | The rule as one English sentence, why it exists, what to do, and the first edges it matched |
| `rulebearing rules --json` | Every rule, its family, severity, `fix`, its lifecycle fields (`since`, `deprecated`, `replacedBy`), and how many modules each side matches |
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
