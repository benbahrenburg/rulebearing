# Top-level statements fixture

A small C# program written for `rb-extract-dotnet`'s extraction tests ([plan 0003, Step 14](../../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)): top-level statements in `Program.cs`, which the compiler puts in `Program.<Main>$` and its async state machine, creating a type and calling an extension method of the same assembly. It is this repository's own code, under its MIT licence.

| Item | Value |
| --- | --- |
| Sources | [`src/`](src/) (`TopLevel`) |
| Build | [`build.sh`](build.sh): `--configuration Release -p:DebugType=portable -p:Deterministic=true -p:ContinuousIntegrationBuild=true` |
| .NET SDK used | 10.0.100 |
| `built/TopLevel.dll` SHA-256 | `41ea525f28be4f3d0793d84545548d82c1e54a6bbfaa247d54f8a236446dff4f` |
| `built/TopLevel.pdb` SHA-256 | `7feee479a4f0d78c6b4cd80781d6e9dbbab2a1ee402b7833ab1ea7810c4ac446` |

Change a source file, run `build.sh`, and update the hashes here; `crates/rb-extract-dotnet/tests/extract.rs` asserts the edges, and `tests/source_mode.rs` compares source mode with them.
