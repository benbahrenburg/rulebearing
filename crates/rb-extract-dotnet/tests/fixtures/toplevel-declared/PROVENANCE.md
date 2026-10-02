# Top-level statements fixture, `Program` declared in another file

A small C# program written for `rb-extract-dotnet`'s extraction tests ([plan 0003, Step 14](../../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)). `Program.cs` holds top-level statements and declares no type; `Program.Declared.cs` declares `public partial class Program { }`, as the ASP.NET SDK's generator does under `obj/` for every web project. The statements' edges are `Program.cs`'s, so that file must be a module. Found on microsoft/semantic-kernel's samples, where the nightly's init fixture gained eleven orphan assemblies.

| Item | Value |
| --- | --- |
| Sources | [`src/`](src/) (`TopLevelDeclared`) |
| Build | [`build.sh`](build.sh): `--configuration Release -p:DebugType=portable -p:Deterministic=true -p:ContinuousIntegrationBuild=true` |
| .NET SDK used | 10.0.100 |
| `built/TopLevelDeclared.dll` SHA-256 | `c2ced30a2ad1e91aeb722d78a2ca391220b87559337081326ce2f45680078632` |
| `built/TopLevelDeclared.pdb` SHA-256 | `335eed6d9e09fbf45405cabb2b450a1c48c7f77a84bce096b47f2a005593f8bf` |

Change a source file, run `build.sh`, and update the hashes here; `crates/rb-extract-dotnet/tests/extract.rs` asserts the modules and edges.
