# NetArchTest test fixtures

The assemblies NetArchTest's own unit tests load, built once and committed so the NetArchTest half of conformance gate 2 reads the same bytes on every machine: `NetArchTest.TestStructure` (every predicate, condition and dependency-search test) and the two `CrossAssemblyTest` projects (`Inherit` across an assembly boundary) ([plan 0002, Step 7](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#27-step-7-gate-2-porting-to-completion-2c)) ([design § Conformance gate 2](../../../docs/artifacts/design.md#conformance-gate-2-archunitnets-test-assemblies-validate-the-element-rules), [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md)).

| Item | Value |
| --- | --- |
| Upstream | [BenMorris/NetArchTest](https://github.com/BenMorris/NetArchTest), tag `v1.3.2` (commit `23fd270a24281727438deac84956bbba6873218a`), projects `test/NetArchTest.TestStructure`, `test/NetArchTest.CrossAssemblyTest.A` and `test/NetArchTest.CrossAssemblyTest.B` |
| Target frameworks | upstream's own, unchanged: `netstandard2.1` (`NetArchTest.TestStructure`) and `netstandard2.0` (the two `CrossAssemblyTest` projects); the .NET 10 SDK builds both, so no retargeting |
| .NET SDK used | 10.0.100 |
| Build | `--configuration Debug -p:DebugType=portable -p:Deterministic=true -p:ContinuousIntegrationBuild=true`, so source paths are mapped to `/_/` and a rebuild with the same SDK gives the same bytes; Debug because upstream builds and tests without a configuration, which is Debug |
| `NetArchTest.TestStructure.dll` SHA-256 | `acff51080a7d64633bb91cdbe03f61c76aabe00a60aca61cfdd7186981fc9859` |
| `NetArchTest.TestStructure.pdb` SHA-256 | `37753573e3cecbeef7e7e7ec74d1648934fd9ee940a6df06e6184701fc7ad1f7` |
| `NetArchTest.CrossAssemblyTest.A.dll` SHA-256 | `616cd2af53c3a04386ab67b3aad2d4c3d0c1f2c338d7e467053b9632611f97e0` |
| `NetArchTest.CrossAssemblyTest.A.pdb` SHA-256 | `08a1d68bd9a940397e1b2322f6714200d03b9596f75dced2366c168908d4395b` |
| `NetArchTest.CrossAssemblyTest.B.dll` SHA-256 | `49537985d3a697bcd52be0493c6686796225c271d48a0415b3818d36ba400f01` |
| `NetArchTest.CrossAssemblyTest.B.pdb` SHA-256 | `4d2d414e7019eab2827749a471cc72693876b2bd7b54f84fdafbae76b536c392` |
| Licence | MIT; [LICENSE](LICENSE) is NetArchTest's, copied beside the binaries ([ADR-0019](../../../docs/adr/0019-mit-licence.md)) |

Only the built binaries and the licence are committed; the C# source is fetched at the tag by [`../scripts/build-test-assemblies.sh`](../scripts/build-test-assemblies.sh), which rebuilds every file and rewrites `SHA256SUMS`. `scripts/gate2-netarchtest-check.sh` verifies the hashes on every pull request.
