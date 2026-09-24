# ArchUnitNET test fixtures

The assemblies ArchUnitNET's own tests load, built once and committed so conformance gate 2 and the `rb-extract-dotnet` tests read the same bytes on every machine: `TestAssembly` (slices, PlantUML, the reader's tests) and the ten purpose-built assemblies under upstream's `TestAssemblies/`, against which the element tests' snapshots were recorded ([plan 0002, Step 7](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#27-step-7-gate-2-porting-to-completion-2c)) ([design § Conformance gate 2](../../../docs/artifacts/design.md#conformance-gate-2-archunitnets-test-assemblies-validate-the-element-rules), [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md)).

| Item | Value |
| --- | --- |
| Upstream | [TNG/ArchUnitNET](https://github.com/TNG/ArchUnitNET), tag `0.13.4` (commit `1ab5943d761d48f86b42f45ef047130dc9aff1c6`), projects `TestAssembly/TestAssembly.csproj` and `TestAssemblies/{Attribute,Class,DuplicateFullName,OtherDuplicateFullName,MethodDependency,MethodMember,PropertyMember,Type,TypeDependency,Visibility}Assembly` |
| Target framework | `net10.0` |
| .NET SDK used | 10.0.100 |
| Build | `--configuration Release -p:DebugType=portable -p:Deterministic=true -p:ContinuousIntegrationBuild=true`, so source paths are mapped to `/_/` and a rebuild with the same SDK gives the same bytes |
| `TestAssembly.dll` SHA-256 | `0599caa2f771aad2358d84ea120549eb19f6d4c705de1cf52e8e847af4a34600` |
| `TestAssembly.pdb` SHA-256 | `079cce5a3fb13c1a5c3732d8fa41684b612090ebb7c31a3f5474c495e225a4c2` |
| `AttributeAssembly.dll` SHA-256 | `64b9bdcb109985033b4ccd7f89b69776482569b67db84a68768113b0246fc451` |
| `AttributeAssembly.pdb` SHA-256 | `52b134f0055783c658040f205e3a2bc6d910bf3970adfdfcc94e4d18e784dcec` |
| `ClassAssembly.dll` SHA-256 | `fbb7e5dbca1db786b78698603e5f280708628a5b4f81b8b337cc6a5ffed1c4c9` |
| `ClassAssembly.pdb` SHA-256 | `adc15226e004e236c24e4a9cf40dac16cfa09f8de1ea58f01d1032d8e1f51395` |
| `DuplicateFullNameAssembly.dll` SHA-256 | `f70b0748895adb63a46a17ec77570fb273987194328015c17e2088486d3ade35` |
| `DuplicateFullNameAssembly.pdb` SHA-256 | `aea6ba0c13a8863a29545568fbf7f58974059ec8bb04d52f0c0b93ce72dc307a` |
| `OtherDuplicateFullNameAssembly.dll` SHA-256 | `3008d9e1422a3cffba76daa9de1500424c9007df322bf2efbb3bbe3a4e3436a1` |
| `OtherDuplicateFullNameAssembly.pdb` SHA-256 | `558a9c1db93dd86c4172ce465b297fe6c3c1c5c71bee1c0fe9c8a5995541b155` |
| `MethodDependencyAssembly.dll` SHA-256 | `59cccdc040b86208e7ab8c3a2a058c4f5ed540208efce01f695560ed522adddd` |
| `MethodDependencyAssembly.pdb` SHA-256 | `2bddfa03cf73b3c8db7839ee7580b7aad2676d383d3344331ca3c55676cef63a` |
| `MethodMemberAssembly.dll` SHA-256 | `2db061138552dec7f8b0f0651b322a6128490ef2e13b75eefa93d6f89f702957` |
| `MethodMemberAssembly.pdb` SHA-256 | `6aa61ee5cbc2fae80c61a19bc4ba8125e3f05aff07d88711bc05f07ff040054a` |
| `PropertyMemberAssembly.dll` SHA-256 | `fb4741e1564158f7869a97ebdada8759ef8d5bd208b749befb0a8d95dc29c69e` |
| `PropertyMemberAssembly.pdb` SHA-256 | `b667635fa6ad59705c4b2b21dd0864acecacf5f4e282a68d9eedfb5e356a521e` |
| `TypeAssembly.dll` SHA-256 | `ce83a93093f6998b2acfae01cea90aff823284d4b0d811e36b8b8f81f0811ca0` |
| `TypeAssembly.pdb` SHA-256 | `0bbd0fd10d85efee336243b1dca782324a9a9a987f33d55a091fb93fa61d7c59` |
| `TypeDependencyAssembly.dll` SHA-256 | `82cdb5bcd8dde679dd4ec2a0425732e85c204b66dffe64399340aa42ff91b1b8` |
| `TypeDependencyAssembly.pdb` SHA-256 | `86ff2388018073ebbb48b76760eb40f4f65684f6f0ef5f841008ecb2299744ae` |
| `VisibilityAssembly.dll` SHA-256 | `6462a1e28c29c413115f1501c8921efeee29ab2e45b1db571d43c46f46393b13` |
| `VisibilityAssembly.pdb` SHA-256 | `5dc81fbc55bab5c31e7476b39cf47001d840d6aeb3e47c2eb2f26c5c138d82ff` |
| Licence | Apache-2.0; [LICENSE](LICENSE) and [NOTICE](NOTICE) are ArchUnitNET's, copied beside the binaries ([ADR-0019](../../../docs/adr/0019-mit-licence.md)) |

Only the built binaries and the notice are committed; the C# source is fetched at the tag by [`../scripts/build-test-assembly.sh`](../scripts/build-test-assembly.sh), which rebuilds every file and rewrites `SHA256SUMS`. `scripts/gate2-check.sh` verifies the hashes on every pull request.
