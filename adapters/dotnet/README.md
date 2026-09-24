# Rulebearing.TestAdapter

Runs Rulebearing inside a .NET test run: one test per architecture rule, failing with the rule's `fix` text and its first violations. The adapter never evaluates a rule; it runs the `rulebearing` binary (`rulebearing cruise --output-type json`), or reads a graph or a saved result, and reports what the binary found. Source, documentation and releases: https://github.com/benbahrenburg/rulebearing

```sh
dotnet tool install -g Rulebearing                 # the binary; or set RULEBEARING_BINARY
dotnet add package Rulebearing.TestAdapter.xUnit   # or .xUnitV3, .NUnit, .MSTestV2, .MSTestV4, .TUnit
```

```csharp
public class ArchitectureRules
{
    [Theory]
    [RulebearingRules("rulebearing.yaml")]
    public void Holds(RuleResult rule) => rule.Assert();
}
```

## Packages

| Package | Framework | Attribute | Test method | `rule.Assert()` throws |
| --- | --- | --- | --- | --- |
| `Rulebearing.TestAdapter` | none: the core | `RulebearingRun.Load(options)` | any | `RuleFailedException` |
| `Rulebearing.TestAdapter.xUnit` | xUnit v2 (2.9) | `[RulebearingRules("rulebearing.yaml")]` | `[Theory]` | `Xunit.Sdk.FailException` |
| `Rulebearing.TestAdapter.xUnitV3` | xUnit v3 (3.x) | `[RulebearingRules("rulebearing.yaml")]` | `[Theory]` | `Xunit.Sdk.FailException` |
| `Rulebearing.TestAdapter.NUnit` | NUnit 4 | `[RulebearingTestCaseSource("rulebearing.yaml")]` | none needed | `NUnit.Framework.AssertionException` |
| `Rulebearing.TestAdapter.MSTestV2` | MSTest 2.x and 3.x | `[RulebearingDataSource("rulebearing.yaml")]` | `[DataTestMethod]` or `[TestMethod]` | `AssertFailedException` |
| `Rulebearing.TestAdapter.MSTestV4` | MSTest 4.x | `[RulebearingDataSource("rulebearing.yaml")]` | `[TestMethod]` | `AssertFailedException` |
| `Rulebearing.TestAdapter.TUnit` | TUnit 1.x | `[RulebearingDataSource("rulebearing.yaml")]` | `[Test]` | `TUnit.Assertions.Exceptions.AssertionException` |

Each test case is named after its rule. The packages target `net8.0`, so a test project on .NET 8 or later can use them; they mirror ArchUnitNET's six adapter packages ([ArchUnitNET coverage § Test framework adapters](../../docs/artifacts/archunitnet-0.13.4-coverage.md#test-framework-adapters)).

## What a test reports

A rule's message is the text the `junit` reporter writes for the same rule, byte for byte ([plan 0002 § 1.5](../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#15-interfaces-and-contracts-this-wave-freezes)): the rule's `fix` (or `N violation(s) of <rule>` when it has none), then the first five error-severity violations with their stable id ([ADR-0015](../../docs/adr/0015-stable-violation-id.md)), `from -> to` and line, then `... and N more`.

| Result of the rule | `RuleResult.Outcome` | `rule.Assert()` |
| --- | --- | --- |
| no error-severity violation | `Passed` | returns; warn, info and known findings are in `Output` |
| error-severity violations | `Failed` | throws with the failure message |
| vacuous under strict liveness, expired, a ratchet without a budget | `Error` | throws with the reason, for a vacuous rule the liveness reason of [ADR-0007](../../docs/adr/0007-vacuous-rules-fail-by-default.md) |
| a ratchet over its ceiling | `Failed` | throws with the count, the ceiling and the budget |

An expired known violation is a test case of its own. A run the binary cannot trust for another reason (zero modules, an unsupported file) or an invalid configuration ([ADR-0008](../../docs/adr/0008-exit-code-contract.md) exits 2 and 3) is a `RulebearingException` naming the command, the binary's message and the fix; it is never a pass.

## Options

Every attribute takes the configuration path and these named properties, which map to `RulebearingOptions`:

| Property | Meaning |
| --- | --- |
| `Graph` | a graph document to evaluate instead of extracting (`--graph`) |
| `Result` | a result saved from `rulebearing cruise -T json`; the binary is not run |
| `Binary` | the binary; by default `RULEBEARING_BINARY`, then `rulebearing` on `PATH` |
| `WorkingDirectory` | where to run, when it is not the configuration's directory |
| `Arguments` | more `cruise` arguments, such as the directories to cruise or `--liveness warn` |

A relative path is looked up from the test assembly's directory upwards, then from the current directory upwards, so `"rulebearing.yaml"` finds the file at the repository root from any test project below it. The binary runs once per distinct set of options in a test run, in the configuration's directory.

## Developing these packages

The core is under `Rulebearing.TestAdapter/`, each framework package beside it, and the tests under `tests/`: the core's own tests, including the proof that every message equals `rulebearing cruise -T junit` output for the same inputs (`tests/Rulebearing.TestAdapter.Tests/JunitEqualityTests.cs`), and one fixture project per framework over the fixture repository `tests/fixture/`, whose failing rule is asserted rather than left to fail the suite. Each package is held to its own 70% line floor ([ADR-0018](../../docs/adr/0018-test-coverage-threshold.md)); the framework versions are pinned once in `Directory.Build.props`, and the package version is the workspace version in `Cargo.toml` ([eng/Rulebearing.Packages.props](../../eng/Rulebearing.Packages.props), [docs/release.md](../../docs/release.md)).

```sh
adapters/dotnet/test.sh                               # every project, with the coverage floor
dotnet pack adapters/dotnet/Rulebearing.TestAdapter.xUnit -c Release -o out -p:Version=<version>
```

Plan: [Wave 2, Step 14 (2H)](../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#214-step-14-test-adapters-and-wrappers-2h). Requirement: [FR-DIST-03](../../docs/prd.md#fr-dist-03).
