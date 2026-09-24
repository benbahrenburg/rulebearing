# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""The cases read from the JSON, against the fixed vectors of rb-report (junit.rs, catalog.rs).

Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
Contract: § 1.5 of the same plan (the adapters' message is the junit message text).
"""

from __future__ import annotations

import pytest
from pytest_rulebearing.cases import (
    SHOWN,
    Case,
    Rule,
    cases,
    describe,
    js_number,
    message,
    rule_name,
    rules,
    severity,
    text,
)

# The vector of crates/rb-report/src/junit.rs, a_rule_per_case_with_failures_errors_and_output.
JUNIT_VECTOR: dict[str, object] = {
    "modules": [
        {"source": "a.ts", "dependencies": [{"resolved": "b.ts", "line": 2, "column": 1}]},
    ],
    "summary": {
        "violations": [
            {
                "type": "dependency",
                "from": "a.ts",
                "to": "b.ts",
                "rule": {"name": "no-b", "severity": "error"},
                "id": "RB-1",
            },
            {
                "type": "dependency",
                "from": "c.ts",
                "to": "b.ts",
                "rule": {"name": "no-b", "severity": "ignore"},
                "id": "RB-2",
            },
            {
                "type": "element",
                "from": "s.cs",
                "to": "S<T>",
                "rule": {"name": "sealed", "severity": "warn"},
                "id": "RB-3",
            },
        ],
        "ruleSetUsed": {
            "forbidden": [
                {"name": "no-b", "severity": "error", "fix": 'Use "the" index & go.'},
                {"name": "dead"},
            ],
            "elements": [{"name": "sealed", "severity": "warn"}],
        },
        "vacuousRules": [{"name": "dead", "side": "from"}],
        "expired": [{"name": "RB-9", "expires": "2026-01-01", "kind": "knownViolation"}],
    },
}


def test_the_junit_vector_case_for_case() -> None:
    found = cases(JUNIT_VECTOR)
    assert [(c.rule.name, c.rule.family) for c in found] == [
        ("no-b", "forbidden"),
        ("dead", "forbidden"),
        ("sealed", "elements"),
        ("RB-9", "knownViolations"),
    ]
    no_b, dead, sealed, expired = found
    assert no_b.failure == 'Use "the" index & go.\nRB-1 a.ts -> b.ts (line 2, column 1)'
    assert no_b.output == ["ignore: RB-2 c.ts -> b.ts [known]"]
    assert dead.errors == [
        (
            "vacuous",
            (
                "rule `dead` is vacuous: its from side matched nothing, so it checks nothing "
                "(ADR-0007)"
            ),
        ),
    ]
    assert dead.failure is None
    assert sealed.failed is False
    assert sealed.output == ["warn: RB-3 s.cs -> S<T>"]
    assert expired.errors == [
        (
            "expired",
            "knownViolation `RB-9` expired on 2026-01-01; it no longer applies and the run fails",
        ),
    ]


def test_message_is_the_failure_then_each_error() -> None:
    case = Case(
        rule=Rule("r", "forbidden", "error"),
        failure="fix\nRB-1 a -> b",
        errors=[
            ("expired", "rule `r` expired on 2026-01-01; it no longer applies and the run fails")
        ],
    )
    assert message(case) == (
        "fix\nRB-1 a -> b\nrule `r` expired on 2026-01-01; it no longer applies and the run fails"
    )
    assert message(Case(rule=Rule("ok", "forbidden", "error"))) == ""


def _violation(
    name: str, index: int, severity: str = "error", **extra: object
) -> dict[str, object]:
    return {
        "type": "dependency",
        "from": f"src/m{index}.ts",
        "to": "src/b.ts",
        "rule": {"name": name, "severity": severity},
        "id": f"RB-{index}",
        **extra,
    }


def test_more_than_five_errors_are_counted_not_listed() -> None:
    result = {
        "summary": {
            "violations": [_violation("many", i) for i in range(1, 9)],
            "ruleSetUsed": {"forbidden": [{"name": "many", "severity": "error"}]},
        },
    }
    (case,) = cases(result)
    lines = (case.failure or "").split("\n")
    assert lines[0] == "8 violation(s) of `many`"
    assert lines[1:6] == [f"RB-{i} src/m{i}.ts -> src/b.ts" for i in range(1, 6)]
    assert lines[6] == "... and 3 more"
    assert len(lines) == SHOWN + 2


def test_a_violation_fix_wins_over_the_rule_fix_even_when_empty() -> None:
    result = {
        "summary": {
            "violations": [_violation("r", 1, fix="")],
            "ruleSetUsed": {"forbidden": [{"name": "r", "severity": "error", "fix": "rule fix"}]},
        },
    }
    assert cases(result)[0].failure == "\nRB-1 src/m1.ts -> src/b.ts"


def test_the_allowed_list_is_one_rule_at_its_severity() -> None:
    result = {
        "summary": {
            "violations": [_violation("not-in-allowed", 1)],
            "ruleSetUsed": {
                "allowed": [{"from": {}, "to": {}, "comment": "c", "fix": "stay inside"}, {}],
                "allowedSeverity": "error",
            },
        },
    }
    (rule,) = rules(result)
    assert rule == Rule("not-in-allowed", "allowed", "error", "c", "stay inside")
    assert cases(result)[0].failure == "stay inside\nRB-1 src/m1.ts -> src/b.ts"
    no_severity: dict[str, object] = {"summary": {"ruleSetUsed": {"allowed": [{}]}}}
    assert rules(no_severity)[0].severity == "warn"


def test_family_order_then_unlisted_by_name_then_ratchets_then_stray_vacuous() -> None:
    result = {
        "summary": {
            "violations": [
                _violation("zeta", 1, comment="z", fix="fz"),
                _violation("alpha", 2, severity="warn"),
                _violation("req", 3),
            ],
            "ruleSetUsed": {
                "diagrams": [{"name": "d"}],
                "slices": [{"name": "s"}],
                "elements": [{"name": "e", "severity": "info"}],
                "required": [{"name": "req"}],
                "forbidden": [{"name": "f"}],
            },
            "ratchets": [{"name": "budget", "status": "within", "count": 1, "ceiling": 2}],
            "vacuousRules": [{"name": "f", "side": "from"}, {"name": "ghost", "side": "to"}],
        },
    }
    assert [(r.name, r.family, r.severity) for r in rules(result)] == [
        ("f", "forbidden", "warn"),
        ("req", "required", "warn"),
        ("e", "elements", "info"),
        ("s", "slices", "warn"),
        ("d", "diagrams", "warn"),
        ("alpha", "rules", "warn"),
        ("zeta", "rules", "error"),
        ("budget", "ratchets", "error"),
        ("ghost", "rules", "error"),
    ]
    zeta = next(r for r in rules(result) if r.name == "zeta")
    assert (zeta.comment, zeta.fix) == ("z", "fz")


def test_ratchets_exceeded_without_budget_and_within() -> None:
    result = {
        "summary": {
            "ratchets": [
                {
                    "name": "up",
                    "status": "exceeded",
                    "count": 12,
                    "ceiling": 10.0,
                    "budget": "b.json",
                },
                {"name": "lost", "status": "no-budget", "count": 3, "budget": "gone.json"},
                {"name": "ok", "status": "within", "count": 1.5, "ceiling": 2, "budget": "b.json"},
                {"name": "unread"},
            ],
        },
    }
    up, lost, ok, unread = cases(result)
    assert up.failure == "ratchet `up`: 12 edges exceed the ceiling of 10 in b.json"
    assert lost.errors == [
        (
            "no-budget",
            (
                "ratchet `lost`: the budget gone.json cannot be read, so the count 3 is checked "
                "against nothing"
            ),
        ),
    ]
    assert ok.output == ["1.5 edges, within the ceiling of 2 in b.json"]
    assert unread.output == ["undefined edges, within the ceiling of undefined in undefined"]


def test_a_warn_vacuous_rule_is_output_and_an_expired_rule_is_an_error() -> None:
    result = {
        "summary": {
            "ruleSetUsed": {"forbidden": [{"name": "stale"}, {"name": "old"}]},
            "vacuousRules": [{"name": "stale", "side": "from", "severity": "warn"}],
            "expired": [{"name": "old", "kind": "rule", "expires": "2026-02-03"}],
        },
    }
    stale, old = cases(result)
    assert stale.failed is False
    assert stale.output == [
        (
            "warning: rule `stale` is vacuous: its from side matched nothing, so it checks "
            "nothing (ADR-0007)"
        ),
    ]
    assert old.errors == [
        ("expired", "rule `old` expired on 2026-02-03; it no longer applies and the run fails"),
    ]
    assert len(cases(result)) == 2  # an expired rule is not also a known-violation case


def test_an_element_violation_sits_at_its_type_declaration() -> None:
    result = {
        "code": {
            "types": [
                {"fullName": "app.Other", "line": 9},
                {"fullName": "app.shapes.shape", "line": 4, "column": 7},
                {"fullName": "app.NoColumn", "line": 2},
                {"fullName": "app.NoLine"},
            ],
        },
    }
    element = {"type": "element", "from": "py/app/shapes.py", "rule": {"severity": "error"}}
    assert describe(result, {**element, "to": "app.shapes.shape"}) == (
        "py/app/shapes.py -> app.shapes.shape (line 4, column 7)"
    )
    assert describe(result, {**element, "to": "app.NoColumn"}).endswith("(line 2, column 1)")
    assert describe(result, {**element, "to": "app.NoLine"}) == "py/app/shapes.py -> app.NoLine"
    assert describe({}, {**element, "to": "x"}) == "py/app/shapes.py -> x"


def test_a_dependency_sits_at_its_first_matching_edge() -> None:
    result = {
        "modules": [
            {"source": "a", "dependencies": [{"resolved": "b", "line": 1}]},
            {"source": "a", "dependencies": [{"resolved": "b", "line": 5, "column": 2}]},
            {"source": "c", "dependencies": [{"resolved": "d", "line": True, "column": 1}]},
            {"source": "e", "dependencies": [{"resolved": "f", "line": 3.0, "column": 1}]},
        ],
    }
    dependency = {"rule": {"severity": "error"}}
    assert describe(result, {**dependency, "from": "a", "to": "b"}) == "a -> b"
    assert describe(result, {**dependency, "from": "c", "to": "d"}) == "c -> d"
    assert describe(result, {**dependency, "from": "e", "to": "f"}) == "e -> f"
    assert describe(result, {**dependency, "from": "x", "to": "b"}) == "x -> b"
    assert describe(result, {"from": "a", "to": "zz"}) == "a -> zz"


def test_missing_fields_print_as_javascript_would() -> None:
    assert describe({}, {}) == "undefined -> undefined"
    assert severity({}) == ""
    assert severity({"rule": None}) == "undefined"
    assert rule_name({"rule": {"name": 3}}) == ""
    assert text({"n": 7}, "n") == "7"
    assert text("not an object", "n") == "undefined"
    assert cases(None) == []
    assert cases({"summary": {"violations": "not a list"}}) == []


@pytest.mark.parametrize(
    ("value", "present", "printed"),
    [
        (None, False, "undefined"),
        (None, True, "null"),
        ("s", True, "s"),
        (True, True, "true"),
        (False, True, "false"),
        (42, True, "42"),
        (-3, True, "-3"),
        (5.0, True, "5"),
        (1.5, True, "1.5"),
        (1e21, True, "1e+21"),
        (1.5e-7, True, "1.5e-7"),
        (1.5e-5, True, "0.000015"),
        (-2e-5, True, "-0.00002"),
        (0.25, True, "0.25"),
        (["py"], True, '["py"]'),
        ({"a": "é"}, True, '{"a":"é"}'),
    ],
)
def test_js_number_prints_values_as_template_literals_do(
    value: object,
    present: bool,  # noqa: FBT001 - a parametrised column
    printed: str,
) -> None:
    assert js_number(value, present=present) == printed


def test_output_is_deterministic() -> None:
    assert cases(JUNIT_VECTOR) == cases(JUNIT_VECTOR)
    assert [message(c) for c in cases(JUNIT_VECTOR)] == [message(c) for c in cases(JUNIT_VECTOR)]
