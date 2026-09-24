# ArchUnitNET test fixtures

The assemblies ArchUnitNET's own tests load, built once and committed so conformance gate 2 and the `rb-extract-dotnet` tests read the same bytes on every machine: `TestAssembly` (slices, PlantUML, the reader's tests) and the ten purpose-built assemblies under upstream's `TestAssemblies/`, against which the element tests' snapshots were recorded ([plan 0002, Step 7](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#27-step-7-gate-2-porting-to-completion-2c)) ([design § Conformance gate 2](../../../docs/artifacts/design.md#conformance-gate-2-archunitnets-test-assemblies-validate-the-element-rules), [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md)).

| Item | Value |
| --- | --- |
| Upstream | [TNG/ArchUnitNET](https://github.com/TNG/ArchUnitNET), tag `0.13.4` (commit `1ab5943d761d48f86b42f45ef047130dc9aff1c6`), projects `TestAssembly/TestAssembly.csproj` and `TestAssemblies/{Attribute,Class,DuplicateFullName,OtherDuplicateFullName,MethodDependency,MethodMember,PropertyMember,Type,TypeDependency,Visibility}Assembly` |
| Target framework | `net10.0` |
| .NET SDK used | 10.0.100 |
| Build | `--configuration Debug -p:DebugType=portable -p:Deterministic=true -p:ContinuousIntegrationBuild=true`, so source paths are mapped to `/_/` and a rebuild with the same SDK gives the same bytes; Debug because upstream's CI runs the tests with `dotnet test -c Debug` |
| `TestAssembly.dll` SHA-256 | `9b2cc7f17d18c1b7f26abd5ed0b416d1fae7c1093daae5b5c65ab51c4a26ca72` |
| `TestAssembly.pdb` SHA-256 | `b407a5d66fd997575f30b5aeb4d6086984cdb0b17a2b0ec8a7c094a83a424e64` |
| `AttributeAssembly.dll` SHA-256 | `d065bc536064018a92bce73d8322ec8f0602023060adb85bef450b50614de57b` |
| `AttributeAssembly.pdb` SHA-256 | `3f66e44e256b70c53bc7b294d1989512d2892a127bbc84ad7e56d0a6581e97a5` |
| `ClassAssembly.dll` SHA-256 | `122ac100e3c03e29f4d2dd7bd734f7cf412bcaf5512e22859d5e373aa28fae00` |
| `ClassAssembly.pdb` SHA-256 | `dbbc5f7823ef3f4842e793ee69931a9df4a56fcf7bc4d3adb25ce2cfa46ff5a4` |
| `DuplicateFullNameAssembly.dll` SHA-256 | `412b5c40fdb4fd806f5d682d745c97845483d793cb68c827e0ed6cbe2dd55e0a` |
| `DuplicateFullNameAssembly.pdb` SHA-256 | `1e4642515b7d410d7bc39c10bf45cde92c70ae2fbc8bc5bd5b47b2ef99ef831b` |
| `OtherDuplicateFullNameAssembly.dll` SHA-256 | `310de68cadcbc5f1d8d6777ebc10f4b7ff5049f7ee768113ccc2ee95aea98ddf` |
| `OtherDuplicateFullNameAssembly.pdb` SHA-256 | `4dff1a2d038003dcb161cc8b572e8fd298e9297090ca6180ebfd9b3ad93a8f7c` |
| `MethodDependencyAssembly.dll` SHA-256 | `559da71c5f2ad88c652eabd181d6fa2205fe25beb7ad9b732eaf5d3c4313457a` |
| `MethodDependencyAssembly.pdb` SHA-256 | `4a0e360e9b21c2d35bef97d0fa47cbc9bc63836aff494949f04a77578facb970` |
| `MethodMemberAssembly.dll` SHA-256 | `1bfd56c44db89b7fdc2044c88b4f0577b9efa7359e664f66ab237c03d886c8fd` |
| `MethodMemberAssembly.pdb` SHA-256 | `0a12767788211057ca4e8f5d5cb84c2d78f144127edcbdd0b05ef3813e868619` |
| `PropertyMemberAssembly.dll` SHA-256 | `6fe95424dc6dfd5f1d8dc0348e1912fae005906c85c0d361e7c2438cf4d61b1d` |
| `PropertyMemberAssembly.pdb` SHA-256 | `b4fe65747a9e208240788a4f6153805837a1082bbecdabd43bf7d57645b8c4cf` |
| `TypeAssembly.dll` SHA-256 | `dc98822b319d3a0fcf5da45e30fe64d8732493682de58af667729b30587bf41b` |
| `TypeAssembly.pdb` SHA-256 | `7041b0fb169fbd22432f6df9a30b1e5cec8bede9ea70c701b8bbc3adef48d33d` |
| `TypeDependencyAssembly.dll` SHA-256 | `662ecd38120a4d926c6c90904aa1758f8280bd444bf6da5753810d71585078a9` |
| `TypeDependencyAssembly.pdb` SHA-256 | `02cf5d7658fa05f99a34f1857f2c9065da7a0705827f8921fdac8d4973bcb6ac` |
| `VisibilityAssembly.dll` SHA-256 | `1fd00c6d8b912263e213c006ff60cbf84d0817d0d2157e41e5f75f6874972de1` |
| `VisibilityAssembly.pdb` SHA-256 | `f519de92b490266545bdd6faf70b8ed96d78477dee52fb9e7147df93e07ac127` |
| Licence | Apache-2.0; [LICENSE](LICENSE) and [NOTICE](NOTICE) are ArchUnitNET's, copied beside the binaries ([ADR-0019](../../../docs/adr/0019-mit-licence.md)) |

Only the built binaries and the notice are committed; the C# source is fetched at the tag by [`../scripts/build-test-assembly.sh`](../scripts/build-test-assembly.sh), which rebuilds every file and rewrites `SHA256SUMS`. `scripts/gate2-check.sh` verifies the hashes on every pull request.
