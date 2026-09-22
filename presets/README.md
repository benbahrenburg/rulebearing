# Presets

Bundled rule sets a config `extends` ([design § The native format](../docs/artifacts/design.md#the-native-format), [§ What stays honest across the boundary](../docs/artifacts/design.md#what-stays-honest-across-the-boundary), [FR-CFG-06](../docs/prd.md#fr-cfg-06), [FR-REACH-04](../docs/prd.md#fr-reach-04)). They are embedded in the binary at build time by `rb-config`.

| Preset | Content | Lands in |
| --- | --- | --- |
| `dependency-cruiser/configs/recommended`, `recommended-strict`, `recommended-warn-only` | dependency-cruiser 18.2.0's bundled presets, verbatim | Wave 1 |
| `rulebearing:typescript`, `rulebearing:dotnet`, `rulebearing:python` | per-language default excludes and orphan exclusions | Waves 1 and 2 |
| `rulebearing:recommended` | composes the three per-language presets | Wave 1 |
| `rulebearing:nextjs`, `rulebearing:clean-architecture`, `rulebearing:django`, `rulebearing:fastapi`, `rulebearing:vertical-slices` | framework opinions, off by default, each documented | Wave 3 |
