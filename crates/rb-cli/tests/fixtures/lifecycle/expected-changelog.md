# Architecture changelog: 1.0.0 to 1.1.0

From commit `1111111111111111111111111111111111111111` to commit `2222222222222222222222222222222222222222`.

## Counts

| Count | 1.0.0 | 1.1.0 | Change |
| --- | ---: | ---: | ---: |
| Modules | 8 | 7 | -1 |
| Dependencies | 8 | 6 | -2 |
| Error violations | 0 | 1 | +1 |
| Warn violations | 1 | 2 | +1 |
| Info violations | 0 | 0 | 0 |

## New edges across boundaries

| From | To | Boundary |
| --- | --- | --- |
| `src/domain/model.ts` | `src/ui/format.ts` | `app-layers` layer 2 to layer 1 |
| `src/features/a/x.ts` | `src/features/b/y.ts` | `features` slice `a` to slice `b` |

## Retired rules

- `no-legacy-http`: deprecated in 1.1.0, replaced by `ui-through-services`
- `old-rule`: removed

## Ratchets that fell

| Ratchet | 1.0.0 | 1.1.0 |
| --- | ---: | ---: |
| `ui-to-db` | 2 | 1 |
