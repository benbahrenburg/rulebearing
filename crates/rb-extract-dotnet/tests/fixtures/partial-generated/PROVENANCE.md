# Partial type split with a source generator's output

A small C# library written for `rb-extract-dotnet`'s extraction tests ([ADR-0061](../../../../../docs/adr/0061-a-type-is-attributed-to-a-file-its-developer-wrote.md)). `src/Shapes/Square.cs` declares `internal sealed partial class Square : Shape` and its members. [`src/Generated/Square.g.cs`](src/Generated/Square.g.cs) holds what a singleton source generator writes: the constructor, the instance, and `SquareFactory`, a type no written file declares. [`build.sh`](build.sh) compiles that file from `src/obj/Generator/`, where a generator's output lies, so the PDB records that path. The case was found on DrJohnMelville/Pdf, whose `[StaticSingleton]` generator put the base class of `ComputeOwnerPasswordV3` in a file under `obj/`.

| Item | Value |
| --- | --- |
| Sources | [`src/`](src/) (`PartialGenerated`) |
| Build | [`build.sh`](build.sh): `--configuration Release -p:DebugType=portable -p:Deterministic=true -p:ContinuousIntegrationBuild=true` |
| .NET SDK used | 10.0.401 |
| `built/PartialGenerated.dll` SHA-256 | `d14b663c88687621c782aadd01d62f7fde2f025f933105b6e7ae368069bec70d` |
| `built/PartialGenerated.pdb` SHA-256 | `5a0a052bdd185f7fe5668de8eed9471a51593d7d81cf7e3b8db2ba0a7705a952` |

Change a source file, run `build.sh`, and update the hashes here; `crates/rb-extract-dotnet/tests/extract.rs` (`a_partial_type_is_the_file_its_developer_wrote`) asserts the attribution.
