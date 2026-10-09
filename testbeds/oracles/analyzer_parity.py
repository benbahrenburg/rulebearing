# Copyright (c) 2026 Ben Bahrenburg. MIT licence: see LICENSE.
"""Rulebearing.Analyzer against the gate on one .NET oracle: the same element findings.

Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 21 ("the parity test
that builds each .NET oracle with the analyzer attached and compares the diagnostic set to
`cruise --output-type json` violations for the covered families"). Requirement:
docs/prd.md#fr-dist-04 ("every diagnostic the analyzer reports is also a gate violation, and vice
versa for the rules they cover"). Run by testbeds/oracles/dotnet.sh after the compiled cruise.

The oracle's imported rules are judged twice: as written, and with every rule's `should` negated,
which makes every type a rule selects a finding unless it met the rule, so the comparison covers
selection and evaluation, not only the few findings the rules have today. Each variant is written
as `rulebearing.yaml` with every rule at `warn`, so a finding does not stop the build. The gate's
side is `cruise --graph` over the oracle's compiled graph; the analyzer's is a rebuild of the
oracle's test project with the analyzer and the variant attached through
`CustomAfterMicrosoftCommonTargets`, its RB0002 diagnostics read from each project's SARIF log.
Rules the analyzer leaves to `cruise` are compared on the analyzer's side by their absence only:
the gate's findings for them are not expected of it. A finding is the pair (rule, type full name).
"""

from __future__ import annotations

import argparse
import copy
import json
import shlex
import subprocess
import sys
from pathlib import Path
from typing import Any

import yaml

Finding = tuple[str, str]
VARIANTS = ("as-written", "negated")


def variant(config: dict[str, Any], *, negated: bool) -> dict[str, Any]:
    """The configuration with every element rule at `warn`, its `should` negated when asked."""
    out = copy.deepcopy(config)
    rules = out.get("rules") or {}
    for rule in rules.get("elements") or []:
        if rule.get("severity") == "ignore":
            continue
        rule["severity"] = "warn"
        rule["allowEmpty"] = True
        if negated:
            rule["should"] = {"not": rule["should"]}
    return out


def gate(binary: str, checkout: Path, config: Path, graph: Path) -> set[Finding]:
    """The element findings `cruise` reports for `config` over `graph`."""
    done = subprocess.run(  # noqa: S603 - the binary under test, arguments built here
        [
            binary,
            "cruise",
            "--config",
            str(config),
            "--graph",
            str(graph),
            "-T",
            "json",
            "--no-progress",
        ],
        cwd=checkout,
        capture_output=True,
        text=True,
        check=False,
    )
    if done.returncode >= 2:  # noqa: PLR2004 - exit 2 and 3 are the contract's failures
        message = f"cruise failed ({done.returncode}): {done.stderr.strip()}"
        raise RuntimeError(message)
    result = json.loads(done.stdout)
    return {
        (v["rule"]["name"], v["to"])
        for v in result["summary"]["violations"]
        if v.get("type") == "element"
    }


# An oracle may raise every analyzer diagnostic to an error (`dotnet_analyzer_diagnostic.severity`
# in its .editorconfig), which would stop the build at the first project with a finding. A
# severity set for one id outranks that bulk setting, so the findings stay findings. The file goes
# in as an EditorConfigFiles item: the SDK has already read GlobalAnalyzerConfigFiles by the time
# CustomAfterMicrosoftCommonTargets is imported.
GLOBALCONFIG = """is_global = true
dotnet_diagnostic.RB0001.severity = warning
dotnet_diagnostic.RB0002.severity = warning
dotnet_diagnostic.RB0003.severity = suggestion
"""


def targets(analyzer: Path, config: Path, sarif: Path, globalconfig: Path) -> str:
    """The MSBuild targets that attach the analyzer and the rule file to every project."""
    return f"""<Project>
  <ItemGroup>
    <Analyzer Include="{analyzer / "Rulebearing.Analyzer.dll"}" />
    <Analyzer Include="{analyzer / "YamlDotNet.dll"}" />
    <AdditionalFiles Include="{config}" />
    <EditorConfigFiles Include="{globalconfig}" />
  </ItemGroup>
  <PropertyGroup>
    <ErrorLog>{sarif}/$(MSBuildProjectName)-$(TargetFramework).sarif,version=2.1</ErrorLog>
  </PropertyGroup>
</Project>
"""


def analyzer_findings(sarif: Path) -> tuple[set[Finding], set[str]]:
    """The RB0002 findings the SARIF logs under `sarif` hold, and the rules RB0003 names."""
    found: set[Finding] = set()
    left: set[str] = set()
    for log in sarif.glob("*.sarif"):
        document = json.loads(log.read_text(encoding="utf-8"))
        for run in document.get("runs", []):
            for result in run.get("results", []):
                properties = result.get("properties", {}).get("customProperties", {})
                if result.get("ruleId") == "RB0002":
                    found.add((properties.get("rule", ""), properties.get("to", "")))
                elif result.get("ruleId") == "RB0003":
                    left.add(properties.get("rule", ""))
    return found, left


def build(checkout: Path, project: str, extra: list[str], targets_file: Path, log: Path) -> None:
    """Rebuilds the test project with the analyzer attached."""
    command = [
        "dotnet",
        "build",
        project,
        "-c",
        "Release",
        "--no-incremental",
        "-p:DebugType=portable",
        "-p:EnableWindowsTargeting=true",
        "-p:TreatWarningsAsErrors=false",
        "-p:CodeAnalysisTreatWarningsAsErrors=false",
        f"-p:CustomAfterMicrosoftCommonTargets={targets_file}",
        *extra,
    ]
    with log.open("w", encoding="utf-8") as out:
        done = subprocess.run(  # noqa: S603 - dotnet with arguments built here
            command, cwd=checkout, stdout=out, stderr=subprocess.STDOUT, check=False
        )
    if done.returncode != 0:
        message = f"the build with the analyzer failed; see {log}"
        raise RuntimeError(message)


def compare(
    gate_side: set[Finding], analyzer_side: set[Finding], left_to_cruise: set[str]
) -> dict[str, Any]:
    """The two sides' findings and where they differ, rules left to cruise set apart."""
    covered = {f for f in gate_side if f[0] not in left_to_cruise}
    only_gate = sorted(covered - analyzer_side)
    only_analyzer = sorted(analyzer_side - covered)
    return {
        "gate": len(covered),
        "analyzer": len(analyzer_side),
        "agreeing": len(covered & analyzer_side),
        "onlyGate": [list(f) for f in only_gate[:50]],
        "onlyAnalyzer": [list(f) for f in only_analyzer[:50]],
        "differences": len(only_gate) + len(only_analyzer),
    }


def main(argv: list[str] | None = None) -> int:
    """Compares both variants on one oracle and writes the result; exit 1 on any difference."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--checkout", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--graph", type=Path, required=True)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--analyzer", type=Path, required=True)
    parser.add_argument("--project", required=True)
    parser.add_argument("--msbuild", default="")
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)

    config = yaml.safe_load(args.config.read_text(encoding="utf-8")) or {}
    result: dict[str, Any] = {"repo": args.repo, "sha": args.sha}
    skipped: set[str] = set()
    differences = 0
    for name in VARIANTS:
        folder = (args.work / name).resolve()
        sarif = folder / "sarif"
        sarif.mkdir(parents=True, exist_ok=True)
        for old in sarif.glob("*.sarif"):
            old.unlink()
        config_file = folder / "rulebearing.yaml"
        config_file.write_text(
            yaml.safe_dump(variant(config, negated=name == "negated"), sort_keys=False),
            encoding="utf-8",
        )
        globalconfig = folder / "rulebearing.globalconfig"
        globalconfig.write_text(GLOBALCONFIG, encoding="utf-8")
        targets_file = folder / "Rulebearing.Analyzer.targets"
        targets_file.write_text(
            targets(args.analyzer.resolve(), config_file, sarif, globalconfig), encoding="utf-8"
        )
        gate_side = gate(args.binary, args.checkout, config_file, args.graph.resolve())
        build(
            args.checkout,
            args.project,
            shlex.split(args.msbuild),
            targets_file,
            folder / "build.log",
        )
        analyzer_side, left = analyzer_findings(sarif)
        skipped |= left
        compared = compare(gate_side, analyzer_side, left)
        differences += compared["differences"]
        result[name] = compared
    result["leftToCruise"] = sorted(skipped)
    result["agrees"] = differences == 0
    args.out.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    for name in VARIANTS:
        c = result[name]
        sys.stdout.write(
            f"analyzer-parity: {args.repo} {name}: gate {c['gate']}, analyzer {c['analyzer']}, "
            f"agreeing {c['agreeing']}, differences {c['differences']}\n"
        )
    return 0 if differences == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
