# ArchUnitNET `TestAssembly` fixture

The assembly ArchUnitNET's own tests load, built once and committed so conformance gate 2 and the `rb-extract-dotnet` tests read the same bytes on every machine ([design § Conformance gate 2](../../../docs/artifacts/design.md#conformance-gate-2-archunitnets-test-assemblies-validate-the-element-rules), [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md)).

| Item | Value |
| --- | --- |
| Upstream | [TNG/ArchUnitNET](https://github.com/TNG/ArchUnitNET), tag `0.13.4` (commit `1ab5943d761d48f86b42f45ef047130dc9aff1c6`), project `TestAssembly/TestAssembly.csproj` |
| Target framework | `net10.0` |
| .NET SDK used | 10.0.100 |
| Build | `--configuration Release -p:DebugType=portable -p:Deterministic=true -p:ContinuousIntegrationBuild=true`, so source paths are mapped to `/_/` and a rebuild with the same SDK gives the same bytes |
| `TestAssembly.dll` SHA-256 | `0599caa2f771aad2358d84ea120549eb19f6d4c705de1cf52e8e847af4a34600` |
| `TestAssembly.pdb` SHA-256 | `079cce5a3fb13c1a5c3732d8fa41684b612090ebb7c31a3f5474c495e225a4c2` (portable PDB, signature `BSJB`) |
| Licence | Apache-2.0; [LICENSE](LICENSE) and [NOTICE](NOTICE) are ArchUnitNET's, copied beside the binaries ([ADR-0019](../../../docs/adr/0019-mit-licence.md)) |

Only the built binaries and the notice are committed; the C# source is fetched at the tag by [`../scripts/build-test-assembly.sh`](../scripts/build-test-assembly.sh), which rebuilds these files and rewrites `SHA256SUMS`. `scripts/gate2-check.sh` verifies the hashes on every pull request.
