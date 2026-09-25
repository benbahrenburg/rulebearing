# Adoption

The six signals of [NFR-ADOPT-01](prd.md#nfr-adopt-01), measured monthly from wave 1 on the repositories where agent-authored pull requests are visible ([design § How to know, rather than believe](artifacts/design.md#how-to-know-rather-than-believe)). [`scripts/adoption-signals.sh`](../scripts/adoption-signals.sh) computes them for one repository from a checkout and the GitHub API, read-only. Before a repository switches, the gate is whatever boundary check it already runs, so the first row is the pre-switch baseline ([plan 0001, Step 21](plans/pending/0001-wave-1-typescript-parity.md#step-21-the-upstream-offer-and-the-adoption-baseline-1g)).

| Signal | Target after two months |
| --- | --- |
| Agent-authored pull requests whose first gate run passes | above 90% |
| Median pushes from a failing gate run to green (GitHub's nearest record of agent turns) | one |
| Rules added by agents that fail `test` or liveness before merge | any number, as long as it is caught |
| p95 of the Stop hook with `--affected` | under 2 seconds |
| Rules carrying `fix` | above 80% |
| Merged budget raises | zero |

## Baseline, 2026-09-23

| Signal | benbahrenburg/rulebearing (since 2026-09-01, gate `self-check`) | benbahrenburg/ai-sdk-otel-logger (since 2026-07-25, no boundary gate) |
| --- | --- | --- |
| First gate run passed | no agent-authored pull requests detected (0 of 11 merged) | no gate; 0 of 4 merged detected as agent-authored |
| Pushes from failing to green | none failed | no gate |
| Agent rule changes caught | 0 | 0 |
| Stop hook p95 with `--affected` | not measurable before wave 3 | not measurable before wave 3 |
| Rules carrying `fix` | 100% of 5 | no Rulebearing rules yet |
| Merged budget raises | 0 (no ratchets) | 0 (no ratchets) |

What the baseline can and cannot see:

- **Agent authorship** is detected from the pull request's author login (Copilot, Devin, Codex, Claude, Cursor, Jules) or a `Co-Authored-By` trailer naming an agent. Work an agent writes and a person commits under their own name, without a trailer, is invisible to it. This repository's commits are of that kind, so its first signal reads zero agent-authored pull requests. Until the maintainer's repositories mark agent work (a trailer or a label), the first three signals measure nothing there. That gap is the first thing to close before the two-month comparison means anything.
- **The private monorepo**, where dependency-cruiser runs today and the design's evidence comes from, is the baseline that matters most. Only the maintainer can run the script there, with `--gate` naming its current dependency-cruiser job.
- **The Stop hook's p95** needs `--affected`, which is wave 3.

## The upstream offer

[NFR-ADOPT-02](prd.md#nfr-adopt-02) and the wave 1 exit criterion ask for the drop-in to be offered upstream to at least one oracle repository, as an issue first, with the zero-diff result attached, and withdrawn without argument if declined. The evidence it would attach: layer 5 on dependency-cruiser's own repository, langfuse and FluidFramework at their pinned commits, zero differences each, with exit codes equal ([conformance/divergences.md](../conformance/divergences.md) is empty). Opening that issue is the maintainer's decision and has not been made.

| Date | Repository | Issue | Outcome |
| --- | --- | --- | --- |
| | | | |
