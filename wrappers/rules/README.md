# rulebearing-rules

Rulebearing's framework presets as a versioned library, published with every Rulebearing release at the release version to npm (`rulebearing-rules`), NuGet (`Rulebearing.Rules`) and PyPI (`rulebearing-rules`). Install it from whichever registry your repository already uses, then extend a preset by the same name everywhere. Source, documentation and releases: https://github.com/benbahrenburg/rulebearing

```sh
npm install --save-dev rulebearing-rules          # node_modules/rulebearing-rules/
dotnet add package Rulebearing.Rules              # the NuGet global packages folder, after restore
pip install rulebearing-rules                     # site-packages/rulebearing_rules/
```

```yaml
extends: [rulebearing:typescript, rulebearing:recommended, rulebearing-rules/nextjs]
```

Rulebearing looks for the package in `node_modules/`, then the NuGet global packages folder, then a Python environment's `site-packages` ([docs/config.md](../../docs/config.md)). The binary bundles the same files as `rulebearing:<preset>`; the package lets a repository pin the rules' version through its package manager.

| Preset | What it is |
| --- | --- |
| `nextjs` | The boundaries a Next.js application's folders imply, for the App Router and the Pages Router |
| `clean-architecture` | Clean architecture's dependency rule, Domain <- Application <- Infrastructure and Presentation, read from paths so it holds for a .NET solution and for a source tree |
| `django` | The layering a Django project's file names imply: models innermost, views reached through URL configurations |
| `fastapi` | The layering most FastAPI services are written in: routers call services, which hold the logic |
| `vertical-slices` | A feature never imports another feature, and the shared kernel never imports a feature back |
| `recommended` | Rulebearing's `rulebearing:recommended`: the three language presets and the rules every language shares |

**Each framework preset is an opinion, not a default.** Nothing extends one unless a configuration names it ([design § The developer relations hat](../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo): "off by default, each a documented opinion"). Every rule's `comment` carries a decision token, and every rule's `examples` pass `rulebearing test`, which this repository's CI runs on each preset. What each preset forbids, and what it leaves out on purpose, is in [presets/frameworks/README.md](../../presets/frameworks/README.md).

MIT licence.

## How the packages are built

The presets live once, in [`presets/frameworks/`](../../presets/frameworks/README.md) and [`presets/rulebearing/recommended.yaml`](../../presets/rulebearing/recommended.yaml); this folder holds the three package definitions and no copy of them ([ADR-0071](../../docs/adr/0071-the-rule-library-ships-from-this-repository.md), [FR-REACH-04](../../docs/prd.md#fr-reach-04), [plan 0003 Step 23](../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#27-steps-for-sub-wave-3g-the-rule-library-the-scale-table-adoption)). [`pack.sh`](pack.sh) packs them at one version; the committed metadata carries none, and nothing in the source tree is edited.

| Registry | Definition | Version | Layout installed |
| --- | --- | --- | --- |
| npm | [`package.json`](package.json), staged with the presets, `README.md` and `LICENSE` in a temporary folder | `npm pkg set version=` on the staged copy | `node_modules/rulebearing-rules/<preset>.yaml` |
| NuGet | [`Rulebearing.Rules.csproj`](Rulebearing.Rules.csproj), a content package with no assembly | `-p:Version`, else the workspace version ([eng/Rulebearing.Packages.props](../../eng/Rulebearing.Packages.props)) | `<global packages>/rulebearing.rules/<version>/rulebearing-rules/<preset>.yaml` |
| PyPI | [`pyproject.toml`](pyproject.toml) and the hatchling hook [`hatch_rules.py`](hatch_rules.py), a wheel and a source distribution | `RULEBEARING_VERSION`, else the workspace version; a pre-release as PEP 440 | `site-packages/rulebearing_rules/<preset>.yaml` |

```sh
wrappers/rules/pack.sh <version> <out-dir> [npm] [nuget] [pypi]   # none named: all three
```

The release workflow calls it from its existing packing jobs, install-checks each package on macOS, Linux and Windows by resolving `extends: rulebearing-rules/nextjs` from it, and publishes it beside the other packages ([docs/release.md](../../docs/release.md#the-rule-library-from-wave-3)). `pytest wrappers/rules --cov=wrappers/rules` tests the hook and packs all three packages, then resolves the preset from each installed layout with the binary built from this checkout.
