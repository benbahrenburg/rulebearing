# ADR-0065: No unsolicited offers to other projects

- **Status:** Accepted (2026-10-09), the owner's decision
- **Date:** 2026-10-09
- **Derives from:** [design § Adoption order](../artifacts/design.md#adoption-order) and [§ Open questions](../artifacts/design.md#open-questions) (upstream etiquette), which this decision departs from
- **Supersedes:** nothing. It changes [NFR-ADOPT-02](../prd.md#nfr-adopt-02).
- **Constrains:** [docs/prd.md](../prd.md), [docs/adoption.md](../adoption.md), [CONTRIBUTING.md](../../CONTRIBUTING.md), [plan 0002, Step 15](../plans/pending/0002-wave-2-dotnet-python-element-rules.md#215-step-15-greenfield-init-proof-the-nightly-tables-second-maintainer-2i), [plan 0004, Step 14](../plans/pending/0004-wave-4-reach.md#25-steps-for-sub-wave-4e-usage-counts-and-the-adoption-review)
- **Requirements:** [NFR-ADOPT-02](../prd.md#nfr-adopt-02)

## Context

The design's adoption order offered Rulebearing to the projects used as test beds:
- the TypeScript oracles as a drop-in, dependency-cruiser's maintainer first (wave 1);
- evolutionary-architecture-by-example and RiverBooks through `import archunit` (wave 2);
- kedro and sqlfluff through `import import-linter` (wave 2);
- the greenfield mixed-language repositories through `init` and `propose` (wave 3).

Each offer was an issue on the other project with the zero-diff or agreement result attached, followed by a pull request adding Rulebearing beside the incumbent. The second-maintainer invitation was to go in the same issues.

None was made. The owner deferred the wave 2 offers on 2026-09-24, and withdrew the wave 3 greenfield offers on 2026-10-09. The test beds remain what they are for: pinned repositories that prove the tool agrees with the incumbents.

## Decision

**Rulebearing makes no unsolicited offer to another project.** No issue or pull request is opened on another project's repository to propose adopting Rulebearing, and no maintainer is approached about it. That covers the oracle and greenfield test beds and the second-maintainer invitation.

The evidence stays public in this repository: the zero-diff results, the agreement tables, the scale table and the `init` fixtures. A project that wants to adopt the tool finds them there.

Contributions this repository makes elsewhere are limited to ordinary bug reports and fixes against tools it depends on, made on their own merits.

## Consequences

- [NFR-ADOPT-02](../prd.md#nfr-adopt-02) keeps its adoption order as the order in which Rulebearing is proven against each ecosystem's test beds. It drops the offers, and the wave 1 acceptance clause that asked for one.
- Plan 0002 drops its four oracle-offer rows and the step that opened them. Plan 0004's closing review no longer decides whether to make them.
- The second-maintainer criterion stays published in [CONTRIBUTING.md](../../CONTRIBUTING.md). Nobody is invited through another project's issue tracker.
- NFR-ADOPT-02's wave 4 acceptance clause and plan 0004's exit criterion still ask for "one greenfield maintainer accepting a proposed rule set". Without an offer, that can only happen if a maintainer adopts the tool unprompted. The clause is left for the owner to restate or drop with plan 0004.
- The design document is a dated export and is not edited ([docs/artifacts/README.md](../artifacts/README.md)). Where it describes offers, this ADR governs.

## Alternatives considered

| Option | Why not |
| --- | --- |
| Keep the offers deferred to plan 0004's review | Leaves a decision open that the owner has made: this is not how the project wants to do business. |
| Offer only where a project's contributing guide invites tools like this | Still an unsolicited approach, and it reopens a judgement call for every repository. |
