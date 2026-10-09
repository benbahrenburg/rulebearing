# Provenance

Copied verbatim from dependency-cruiser 18.2.0 (MIT, [LICENSE](LICENSE)), the version
conformance gate 1 pins ([conformance/dependency-cruiser/PIN](../../../../conformance/dependency-cruiser/PIN)),
so `rb-node`'s tests run dependency-cruiser's own cases for the four configuration extractors
([plan 0003, Step 22](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#26-steps-for-sub-wave-3f-the-roslyn-analyzer-and-rb-node)).

| Folder | Upstream path |
| --- | --- |
| `depcruise/` | `test/config-utl/extract-depcruise-config/__mocks__/` |
| `typescript/` | `test/config-utl/__mocks__/typescriptconfig/` |
| `babel/` | `test/config-utl/__mocks__/babelconfig/` |
| `babel-js/` | `test/config-utl/__mocks__/babelconfig-js/` |
| `webpack/` | `test/config-utl/__mocks__/webpackconfig/` |

The expectations are the upstream specs' own (`test/config-utl/*.spec.mjs`), ported to vitest in
[`../extract.test.ts`](../extract.test.ts).
