# ADR-0001: Record architecture decisions, plans and artifacts in `docs/`

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** the user's project brief; [design.md § Rule metadata that says what to do](../artifacts/design.md#rule-metadata-that-says-what-to-do) (rules cite `adr:NNNN`)
- **Constrains:** [architecture.md § Repository layout](../architecture.md#repository-layout); [CLAUDE.md](../../CLAUDE.md)

## Context

Rulebearing is a single-maintainer project whose rules are meant to cite decision records by id. The project itself must therefore keep its decisions, its plans and its source artifacts in a shape that a guard, a reviewer or a coding agent can find without asking.

## Decision

1. **ADRs live under `docs/adr/`**, numbered `NNNN-kebab-title.md`, in this template: Status, Date, Derives from, Constrains, Context, Decision, Consequences, Alternatives considered. The index is [docs/adr/README.md](README.md). An ADR is never edited after acceptance except to change its status; a reversal is a new ADR that supersedes it.
2. **Plans live under `docs/plans/pending/`** while open and are moved, unchanged apart from a status line, to `docs/plans/implemented/` when every exit criterion in their third section is met. Every plan has exactly three sections: an architect section written for an architectural review board, a lead-developer section with step-by-step implementation instructions, and a wave-based delivery plan in which each wave has a status-tracking method, a t-shirt size and an estimated level of effort. The index is [docs/plans/README.md](../plans/README.md).
3. **Artifacts live under `docs/artifacts/`**: the source design document and its two coverage tabs, exported verbatim. They are read-only inputs; a change of intent is a new export plus an ADR, never an edit.
4. **Every code file, plan and ADR carries linked references.** A crate's `lib.rs` module doc links the architecture section and the plan that created it. A plan links the PRD requirements it satisfies, the ADRs it applies and the artifact sections it derives from. An ADR links the artifact section it derives from and the architecture section it constrains.
5. **A rule in this repository's own `rulebearing.yaml` cites an ADR** with `adr:NNNN` in its `comment`, as the design requires of every user.

## Consequences

- A reviewer can trace any line of code to a plan, a plan to a requirement, and a requirement to the design without a conversation.
- Moving a plan is a review event: the pull request that moves it must show the exit criteria met.
- The ADR list grows; that is the intent.

## Alternatives considered

- **Decisions in the README.** Rejected: it does not survive growth, and rules cannot cite a paragraph.
- **A wiki.** Rejected: not versioned with the code, not readable by the tool.
