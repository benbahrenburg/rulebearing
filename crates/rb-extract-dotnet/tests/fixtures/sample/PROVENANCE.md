# Extraction fixture

A small C# project written for `rb-extract-dotnet`'s extraction tests ([plan 0002, Step 3](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#23-step-3-attribution-edge-projection-the-net-code-layer-and-defaults-2a)): partial types, `async` and iterator methods, a lambda, a record, `init` setters, generics, nesting, attributes with positional, named and `typeof` arguments, and a literal `Type.GetType` load. It is this repository's own code, under its MIT licence.

| Item | Value |
| --- | --- |
| Sources | [`src/`](src/) (`Sample`), [`core/`](core/) (`Sample.Core`, which `Sample` references) |
| Build | [`build.sh`](build.sh): `--configuration Release -p:DebugType=portable -p:Deterministic=true -p:ContinuousIntegrationBuild=true`, so documents are written `/_/crates/rb-extract-dotnet/tests/fixtures/sample/src/...` and a rebuild with the same SDK gives the same bytes |
| .NET SDK used | 10.0.100 |
| `built/Sample.dll` SHA-256 | `53d55069c0ce296f9a62c3c3f6d4603891911170b0119d20922038558cec510e` |
| `built/Sample.pdb` SHA-256 | `49b8e9c53c8de275864890a4875536e855821e06d20dc7bf3b59b1dbc4ef3938` |
| `built/Sample.Core.dll` SHA-256 | `72f29c4d2d4b7ba322d3bd59b8ecfce1cf226a9e0616d3089a907e72481f3e93` |
| `built/Sample.Core.pdb` SHA-256 | `93aec04190a43b251ae78e047df714f459dead356c9a0877c7950e35b70de639` |

Change a source file, run `build.sh`, update the hashes here, and regenerate the expectation with `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-extract-dotnet --test extract`; the diff of `sample.expected.json` is the review.
