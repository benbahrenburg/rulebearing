# Copyright (c) 2026 Ben Bahrenburg. MIT License.
"""One test case per rule, read from the JSON of ``rulebearing cruise --output-type json``.

- Contract: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 1.5 (one test case
  per rule; the adapters' failure message is the ``junit`` message text).
- Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
- Decisions: docs/adr/0007-vacuous-rules-fail-by-default.md (a vacuous rule fails with the
  liveness reason), docs/adr/0010-crate-layout-and-extractor-boundary.md rule 4 (adapters report,
  they never evaluate).
- Requirement: FR-DIST-03 (docs/prd.md).

This module reads the result the binary wrote and reports it; it never evaluates a rule. The
reading follows ``crates/rb-report/src/catalog.rs`` line for line, so a case's :func:`message` is
the text the ``junit`` reporter writes into the ``message`` attribute of that rule's ``<failure>``
(then of each ``<error>``, one per line). ``tests/test_junit_equality.py`` proves the two agree on
the shared fixture in ``adapters/fixture``.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field, replace
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterator

__all__ = ["SHOWN", "Case", "Rule", "cases", "describe", "message", "rule_index", "rules"]

#: How many violations a failure message lists before it says how many more there are.
SHOWN = 5

#: The families of ``summary.ruleSetUsed`` after ``forbidden`` and ``allowed``, in report order.
_LISTED_FAMILIES = ("required", "elements", "slices", "diagrams")


@dataclass(frozen=True)
class Rule:
    """One rule of the run.

    Attributes:
        name: The name violations carry.
        family: ``forbidden``, ``allowed``, ``required``, ``elements``, ``slices``, ``diagrams``,
            ``ratchets``, ``knownViolations`` for an expired known violation, or ``rules`` for one
            known only from its violations.
        severity: The configured severity.
        comment: The rule's comment.
        fix: The rule's ``fix``.
        id: The rule's identity within the run, the test's name: the name, with ``#n`` for the
            n-th rule of a name already taken (names repeat: every anonymous rule is
            ``unnamed``). :func:`rules` sets it.
    """

    name: str
    family: str
    severity: str
    comment: str | None = None
    fix: str | None = None
    id: str = ""


@dataclass
class Case:
    """One rule's result, as the ``junit`` reporter reports it.

    Attributes:
        rule: The rule, or the expired known violation, the case is for.
        failure: The failure message (the ``fix``, then the first violations), or ``None``.
        errors: ``(type, message)`` for each reason the rule could not be trusted: vacuous,
            expired, a ratchet without a budget.
        output: What the case reports without failing: warn, info and known findings.
    """

    rule: Rule
    failure: str | None = None
    errors: list[tuple[str, str]] = field(default_factory=list)
    output: list[str] = field(default_factory=list)

    @property
    def failed(self) -> bool:
        """Whether the case fails: an error-severity violation, or a reason it cannot be trusted."""
        return self.failure is not None or bool(self.errors)


def message(case: Case) -> str:
    """The failure message of a case: the junit failure message, then each error message.

    Args:
        case: The case.

    Returns:
        The lines joined by newlines; empty when the case passes.
    """
    parts = [] if case.failure is None else [case.failure]
    parts.extend(text for _, text in case.errors)
    return "\n".join(parts)


def _uint(value: object) -> int | None:
    """``Value::as_u64``: a non-negative integer, and not a boolean or a float."""
    if isinstance(value, int) and not isinstance(value, bool) and value >= 0:
        return value
    return None


def _or(value: str | None, default: str) -> str:
    """``Option::unwrap_or``: an empty string is a value, not an absence."""
    return default if value is None else value


def js_number(value: object, *, present: bool = True) -> str:
    """A value as JavaScript's ``${value}`` prints it (``crates/rb-report/src/lib.rs``).

    Args:
        value: A value parsed from JSON.
        present: ``False`` when the key was absent, which prints ``undefined``.

    Returns:
        The printed value.
    """
    if not present:
        return "undefined"
    if isinstance(value, str):
        return value
    if isinstance(value, float):
        return _float(value)
    # null, booleans and integers print as JSON does; arrays and objects as serde_json writes them.
    return json.dumps(value, separators=(",", ":"), ensure_ascii=False)


def _float(value: float) -> str:
    """A float as ``js_number`` prints it: integral below 1e21 without a fraction."""
    if value.is_integer() and abs(value) < 1e21:  # noqa: PLR2004 - JavaScript's threshold
        return f"{value:.0f}"
    return _shortest(value)


def _shortest(value: float) -> str:
    """A float as serde_json writes it (the shortest round-trip digits, ryu's layout)."""
    printed = repr(value)
    if "e" not in printed:
        return printed
    mantissa, exponent = printed.split("e")
    power = int(exponent)
    if power == -5:  # noqa: PLR2004 - ryu writes 1e-5 as a decimal, Python in exponent form
        sign = "-" if mantissa.startswith("-") else ""
        return f"{sign}0.0000{mantissa.lstrip('-').replace('.', '')}"
    return f"{mantissa}e{'+' if power > 0 else ''}{power}"


def _get(value: object, key: str) -> tuple[bool, object]:
    if isinstance(value, dict) and key in value:
        return True, value[key]
    return False, None


def text(value: object, key: str) -> str:
    """A field as a string: itself when a string, printed as JavaScript would otherwise.

    Args:
        value: A JSON object (anything else has no fields).
        key: The field.

    Returns:
        The field's text, ``undefined`` when absent.
    """
    present, found = _get(value, key)
    if isinstance(found, str):
        return found
    return js_number(found, present=present)


def _string(value: object, key: str) -> str | None:
    _, found = _get(value, key)
    return found if isinstance(found, str) else None


def _list(value: object, key: str) -> list[object]:
    _, found = _get(value, key)
    return list(found) if isinstance(found, list) else []


def _summary(result: object) -> object:
    return _get(result, "summary")[1]


def violations(result: object) -> list[object]:
    """The violations of ``summary.violations``, in the result's order."""
    return _list(_summary(result), "violations")


def summary_list(result: object, key: str) -> list[object]:
    """The entries of a summary list: ``vacuousRules``, ``ratchets`` or ``expired``."""
    return _list(_summary(result), key)


def rule_name(violation: object) -> str:
    """A violation's rule name, empty when it has none."""
    return _or(_string(_get(violation, "rule")[1], "name"), "")


def severity(violation: object) -> str:
    """A violation's severity: empty without a rule, ``undefined`` for a rule without one."""
    present, rule = _get(violation, "rule")
    return text(rule, "severity") if present else ""


def _entry(family: str, rule: object) -> Rule:
    return Rule(
        name=_or(_string(rule, "name"), ""),
        family=family,
        severity=_or(_string(rule, "severity"), "warn"),
        comment=_string(rule, "comment"),
        fix=_string(rule, "fix"),
    )


def rules(result: object) -> list[Rule]:
    """Every rule of the run, in the order ``catalog::rules`` gives.

    ``forbidden``; the ``allowed`` list as the one rule its violations name (``not-in-allowed``,
    at ``allowedSeverity``, ``warn`` by default); ``required``, then the element, slice and
    diagram rules; a rule known only from its violations (or whose name only rules of another
    family carry), in name order; the ratchets; then any vacuous entry that names none of these.

    Args:
        result: The parsed JSON of a cruise.

    Returns:
        The rules.
    """
    rule_set = _get(_summary(result), "ruleSetUsed")[1]
    out = [_entry("forbidden", rule) for rule in _list(rule_set, "forbidden")]
    allowed = _list(rule_set, "allowed")
    if allowed:
        out.append(
            Rule(
                name="not-in-allowed",
                family="allowed",
                severity=_or(_string(rule_set, "allowedSeverity"), "warn"),
                comment=_string(allowed[0], "comment"),
                fix=_string(allowed[0], "fix"),
            ),
        )
    for key in _LISTED_FAMILIES:
        out.extend(_entry(key, rule) for rule in _list(rule_set, key))
    unlisted: list[Rule] = []
    for violation in violations(result):
        name = rule_name(violation)
        kind = _string(violation, "type")
        if not any(r.name == name and _produces(r.family, kind) for r in [*out, *unlisted]):
            unlisted.append(
                Rule(
                    name=name,
                    family="rules",
                    severity=severity(violation),
                    comment=_string(violation, "comment"),
                    fix=_string(violation, "fix"),
                ),
            )
    out.extend(sorted(unlisted, key=lambda r: r.name.encode()))
    out.extend(
        Rule(name=text(ratchet, "name"), family="ratchets", severity="error")
        for ratchet in summary_list(result, "ratchets")
    )
    for vacuous in summary_list(result, "vacuousRules"):
        name = text(vacuous, "name")
        if not any(r.name == name for r in out):
            out.append(Rule(name=name, family="rules", severity="error"))
    return _identify(out)


def _identify(rules: list[Rule]) -> list[Rule]:
    """``catalog::identify``: the name at its first occurrence, then ``name#n``, past any taken."""
    taken = {r.name for r in rules}
    seen: dict[str, int] = {}
    out: list[Rule] = []
    for rule in rules:
        count = seen.get(rule.name, 0) + 1
        seen[rule.name] = count
        if count == 1:
            out.append(replace(rule, id=rule.name))
            continue
        n = count
        while f"{rule.name}#{n}" in taken:
            n += 1
        taken.add(f"{rule.name}#{n}")
        out.append(replace(rule, id=f"{rule.name}#{n}"))
    return out


def _produces(family: str, kind: str | None) -> bool:
    """``catalog::produces``: whether a rule of ``family`` can produce a violation of ``kind``."""
    if family == "rules":
        return True
    if kind == "element":
        return family in {"elements", "diagrams"}
    if kind == "slice":
        return family == "slices"
    return family in {"forbidden", "allowed", "required", "rules"}


def rule_index(rules: list[Rule], violation: object) -> int | None:
    """The index of the rule a violation belongs to, as ``catalog::rule_index`` picks it.

    Args:
        rules: The rules of :func:`rules`.
        violation: One violation of the result.

    Returns:
        Among the non-ratchet rules of the violation's name: the first whose family can produce
        its ``type`` and whose severity is its own, else the first whose family can produce it,
        else the first; ``None`` when no such rule exists.
    """
    name = rule_name(violation)
    named = [i for i, r in enumerate(rules) if r.name == name and r.family != "ratchets"]
    kind = _string(violation, "type")
    fitting = [i for i in named if _produces(rules[i].family, kind)] or named
    wanted = severity(violation)
    return next(
        (i for i in fitting if rules[i].severity == wanted), fitting[0] if fitting else None
    )


def _first(items: list[object], predicate: str, wanted: str) -> object:
    return next((item for item in items if _string(item, predicate) == wanted), None)


def _or_one(column: int | None) -> int:
    return 1 if column is None else column


def position(result: object, violation: object) -> tuple[int, int] | None:
    """Where a violation sits, when the extractor recorded it.

    Args:
        result: The parsed JSON of a cruise.
        violation: One of its violations.

    Returns:
        ``(line, column)``: the edge's for a dependency, the type's declaration for an element
        violation; ``None`` when unknown.
    """
    source = text(violation, "from")
    target = text(violation, "to")
    if _string(violation, "type") == "element":
        declared = _first(_list(_get(result, "code")[1], "types"), "fullName", target)
        line = _uint(_get(declared, "line")[1])
        if line is None:
            return None
        return line, _or_one(_uint(_get(declared, "column")[1]))
    module = _first(_list(result, "modules"), "source", source)
    if module is None:
        return None
    dependency = _first(_list(module, "dependencies"), "resolved", target)
    line = _uint(_get(dependency, "line")[1])
    column = _uint(_get(dependency, "column")[1])
    if line is None or column is None:
        return None
    return line, column


def describe(result: object, violation: object) -> str:
    """One line per violation: its id, ``from -> to``, the line, and ``[known]`` when baselined.

    Args:
        result: The parsed JSON of a cruise.
        violation: One of its violations.

    Returns:
        The line the failure messages list.
    """
    identifier = _string(violation, "id")
    prefix = "" if identifier is None else f"{identifier} "
    at = position(result, violation)
    where = "" if at is None else f" (line {at[0]}, column {at[1]})"
    known = " [known]" if severity(violation) == "ignore" else ""
    return f"{prefix}{text(violation, 'from')} -> {text(violation, 'to')}{where}{known}"


def _vacuous_message(entry: object) -> str:
    return (
        f"rule `{text(entry, 'name')}` is vacuous: its {text(entry, 'side')} side matched "
        "nothing, so it checks nothing (ADR-0007)"
    )


def _expired_message(entry: object) -> str:
    return (
        f"{text(entry, 'kind')} `{text(entry, 'name')}` expired on {text(entry, 'expires')}; "
        "it no longer applies and the run fails"
    )


def _ratchet_case(case: Case, ratchet: object) -> None:
    present, count_value = _get(ratchet, "count")
    count = js_number(count_value, present=present)
    budget = text(ratchet, "budget")
    present, ceiling_value = _get(ratchet, "ceiling")
    ceiling = js_number(ceiling_value, present=present)
    status = _string(ratchet, "status")
    if status == "exceeded":
        case.failure = (
            f"ratchet `{case.rule.name}`: {count} edges exceed the ceiling of {ceiling} in {budget}"
        )
    elif status == "no-budget":
        case.errors.append(
            (
                "no-budget",
                (
                    f"ratchet `{case.rule.name}`: the budget {budget} cannot be read, so the "
                    f"count {count} is checked against nothing"
                ),
            ),
        )
    else:
        case.output.append(f"{count} edges, within the ceiling of {ceiling} in {budget}")


def _violation_case(case: Case, result: object, catalogue: list[Rule], index: int) -> None:
    name = case.rule.name
    found = [v for v in violations(result) if rule_index(catalogue, v) == index]
    errors = [describe(result, v) for v in found if severity(v) == "error"]
    case.output.extend(
        f"{severity(v)}: {describe(result, v)}" for v in found if severity(v) != "error"
    )
    if not errors:
        return
    own = _string(found[0], "fix")
    fix = case.rule.fix if own is None else own
    lines = [fix if fix is not None else f"{len(errors)} violation(s) of `{name}`"]
    lines.extend(errors[:SHOWN])
    if len(errors) > SHOWN:
        lines.append(f"... and {len(errors) - SHOWN} more")
    case.failure = "\n".join(lines)


def _iter_cases(result: object) -> Iterator[Case]:
    vacuous = summary_list(result, "vacuousRules")
    expired = summary_list(result, "expired")
    ratchets = summary_list(result, "ratchets")
    catalogue = rules(result)
    ratchet_at = 0
    for index, rule in enumerate(catalogue):
        case = Case(rule=rule)
        # Vacuous and expired entries go to the first rule of their name.
        first = next(i for i, r in enumerate(catalogue) if r.name == rule.name) == index
        if rule.family == "ratchets":
            # The ratchet rules are summary.ratchets, in order.
            if ratchet_at < len(ratchets):
                _ratchet_case(case, ratchets[ratchet_at])
            ratchet_at += 1
        else:
            _violation_case(case, result, catalogue, index)
        for entry in (v for v in vacuous if first and text(v, "name") == rule.name):
            if _string(entry, "severity") == "warn":
                case.output.append(f"warning: {_vacuous_message(entry)}")
            else:
                case.errors.append(("vacuous", _vacuous_message(entry)))
        case.errors.extend(
            ("expired", _expired_message(entry))
            for entry in expired
            if first and text(entry, "kind") == "rule" and text(entry, "name") == rule.name
        )
        yield case
    for entry in (e for e in expired if text(e, "kind") != "rule"):
        yield Case(
            rule=Rule(
                name=text(entry, "name"),
                family="knownViolations",
                severity="error",
                id=text(entry, "name"),
            ),
            errors=[("expired", _expired_message(entry))],
        )


def cases(result: object) -> list[Case]:
    """One case per rule of :func:`rules`, then one per expired known violation.

    Args:
        result: The parsed JSON of a cruise.

    Returns:
        The cases, in the order the ``junit`` reporter writes its test cases.
    """
    return list(_iter_cases(result))
