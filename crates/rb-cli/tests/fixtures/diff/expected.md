## Architecture diff

1 new violation, 1 resolved; 1 edge added, 1 removed; 1 ratchet changed.

### New violations

| Id | Rule | Severity | From | To | Fix |
| --- | --- | --- | --- | --- | --- |
| `RB-915449e6` | `routes-not-to-web` | error | `src/routes/b.ts:1:1` | `src/web/view.ts` | Return data from the route and render it in src/web |

### Resolved violations

| Id | Rule | Severity | From | To | Fix |
| --- | --- | --- | --- | --- | --- |
| `RB-e37cce41` | `routes-not-to-db` | error | `src/routes/b.ts:1:1` | `src/db/store.ts` | Call the store through src/services instead of importing src/db |

### Ratchets

| Ratchet | Before | After |
| --- | --- | --- |
| `routes-via-service` | 2 | 1 |

### Added edges

| From | To |
| --- | --- |
| `src/routes/b.ts:1:1` | `src/web/view.ts` |

### Removed edges

| From | To |
| --- | --- |
| `src/routes/b.ts:1:1` | `src/db/store.ts` |
