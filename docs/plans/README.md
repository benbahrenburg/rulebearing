# Plans

Plans are the unit of delivery. The process is set by [ADR-0001](../adr/0001-record-architecture-decisions.md): a plan is created under `pending/`, has exactly three sections (an architect section for the architectural review board, a lead-developer section with step-by-step instructions, and a wave-based delivery plan with status tracking, t-shirt sizes and level of effort per sub-wave), and is moved to `implemented/` when every exit criterion in its third section is met.

Every plan links the [PRD](../prd.md) requirements it satisfies, the [ADRs](../adr/README.md) it applies, the [architecture](../architecture.md) sections it builds, and the [design](../artifacts/design.md) sections it derives from.

## Pending

| Plan | Wave | Title | Calendar | Depends on |
| --- | --- | --- | --- | --- |
| [0000](pending/0000-wave-0-spike.md) | 0 | Spike: TypeScript extractor, .NET metadata reader, conformance skeletons, foundation | 4 weeks | none |
| [0001](pending/0001-wave-1-typescript-parity.md) | 1 | TypeScript parity, the native format, the first-run experience | 10 weeks | 0000 |
| [0002](pending/0002-wave-2-dotnet-python-element-rules.md) | 2 | .NET, Python, element rules, migration | 10 weeks | 0001 |
| [0003](pending/0003-wave-3-operations-surface-inner-loop.md) | 3 | Operations, the rest of the surface, the inner loop | 8 weeks | 0002 |
| [0004](pending/0004-wave-4-reach.md) | 4 | Reach, funded on the adoption numbers | 8 weeks | 0003 and [NFR-ADOPT-01](../prd.md#nfr-adopt-01) |
| [0005](pending/0005-guard-catalogue.md) | 1 to 2 | The guard catalogue: quality, convention and lifecycle guards as executable recipes | 5 weeks | 0001 (1A, 1B, 1E); 0002 for sub-wave 5D |

## Implemented

None yet.

## Sizing scale used by every plan

| Size | Part-time effort (~10 h/week) |
| --- | --- |
| XS | up to 1 day |
| S | up to 3 days |
| M | up to 1 week |
| L | up to 2 weeks |
| XL | more than 2 weeks |

## Status vocabulary

`Not started`, `In progress`, `Blocked`, `Done`. A sub-wave is `Done` only when its gating metric (a CI check, a conformance pass rate, a coverage figure, a timing) is green and linked as evidence.
