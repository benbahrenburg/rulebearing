# Discovery fixtures

Solution layouts for `rb-extract-dotnet`'s discovery tests ([plan 0002, Step 1](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#21-step-1-net-discovery-and-the-loader-options-2a)). Only the project files are committed; `tests/discover.rs` copies each layout to a temporary folder and puts the extraction fixture's built `Sample.dll` and `Sample.Core.dll` where a build would, so no .NET SDK is needed to run the tests.

| Layout | Exercises |
| --- | --- |
| `classic/` | a `.sln` with a solution folder, `Directory.Build.props`, central package versions in `Directory.Packages.props`, a project reference, a test project |
| `xml/` | a `.slnx`, a multi-targeting project and `targetFramework`, an unbuilt .NET Framework project, `excludeProjects` |
| `arcade/` | the Arcade SDK layout dotnet/aspnetcore builds with: `global.json` naming `Microsoft.DotNet.Arcade.Sdk`, a `Directory.Build.props` importing it, assemblies under `artifacts/bin/<Project>/<Configuration>/<TFM>/` |
| `artifacts/` | the .NET 8 `UseArtifactsOutput` layout: assemblies under `artifacts/bin/<Project>/<pivot>/`, a multi-targeting project's pivot naming its framework |
