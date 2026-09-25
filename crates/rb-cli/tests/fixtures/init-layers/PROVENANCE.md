# init-layers fixture

A four-project clean-architecture solution written for `rulebearing init`'s .NET detector ([plan 0002, Step 15](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#215-step-15-greenfield-init-proof-the-nightly-tables-upstream-offers-second-maintainer-2i)): `Shop.Domain`, `Shop.Application`, `Shop.Infrastructure` and `Shop.Web`, whose namespaces name the layers. `Shop.Application` uses `Shop.Infrastructure`'s `OrderStore` directly, the one finding `init` baselines. It is this repository's own code, under its MIT licence.

| Item | Value |
| --- | --- |
| Sources | [`Shop.slnx`](Shop.slnx), [`src/`](src/) |
| Build | [`build.sh`](build.sh): `--configuration Release -p:DebugType=portable -p:Deterministic=true -p:ContinuousIntegrationBuild=true`, the four assemblies and PDBs copied to `built/` |
| .NET SDK used | 10.0.100 |
| `built/Shop.Domain.dll` SHA-256 | `5ba5fa9643ca3411c6b16c0b1050334afdd75ace88d12c9645504f5e6392ecc1` |
| `built/Shop.Domain.pdb` SHA-256 | `de5fb5032a54b534475340a4a84c299a21e876730bb2f610d542d7f60f0a28fb` |
| `built/Shop.Application.dll` SHA-256 | `96ed316e3821d707274e35fb072c6bacc1419b32b0aa7bf9bbe0e8798ad72d52` |
| `built/Shop.Application.pdb` SHA-256 | `2c63598e59641f7339c4a9552868e9f949ebca17b50478f4bf5bb2cf2145a7fb` |
| `built/Shop.Infrastructure.dll` SHA-256 | `0673b043828b744a443292c9393bc38a36bfe5d16995fc0d021e3389d05c941e` |
| `built/Shop.Infrastructure.pdb` SHA-256 | `55d28148a028ed7aee9ed9ef589ee0b76649a72c4d6a59553f97a954ddbea942` |
| `built/Shop.Web.dll` SHA-256 | `91a41068f876419ce0de0428210ffa2defc3d1f0cdeb284cd6654946d9f03bf4` |
| `built/Shop.Web.pdb` SHA-256 | `66bd3250c5205e357cb2ccad3c579eeb711b700605f2559cf12f365e2baf44b7` |

The test (`crates/rb-cli/tests/init_detectors.rs`) copies the tree to a temporary folder and each assembly to its project's `bin/Debug/net10.0/`, where a `dotnet build` would have put it. Change a source file, run `build.sh`, and update the hashes here.
