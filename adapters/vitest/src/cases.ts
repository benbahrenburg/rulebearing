// One test case per rule, read from the JSON of `rulebearing cruise --output-type json`.
//
// Contract: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 1.5 (one test case per
// rule; the adapters' failure message is the junit message text).
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
// Decisions: docs/adr/0007-vacuous-rules-fail-by-default.md (a vacuous rule fails with the liveness
// reason); docs/adr/0010-crate-layout-and-extractor-boundary.md rule 4 (adapters report, they never
// evaluate). Requirement: FR-DIST-03.
//
// This module reads the result the binary wrote and reports it; it never evaluates a rule. It
// follows crates/rb-report/src/catalog.rs line for line (as the Python adapter's cases.py does), so
// a case's `message` is the text the junit reporter writes into the `message` attribute of that
// rule's <failure>, then of each <error>, one per line. test/junit.test.ts proves the two agree on
// the shared fixture in adapters/fixture.

/** How many violations a failure message lists before it says how many more there are. */
export const SHOWN = 5;

/** One rule of the run. */
export interface Rule {
  /**
   * The rule's identity within the run, the test's name: the name, with `#n` for the n-th rule of
   * a name already taken (names repeat: every anonymous rule is `unnamed`). `rules` sets it.
   */
  readonly id: string;
  /** The name violations carry. */
  readonly name: string;
  /**
   * `forbidden`, `allowed`, `required`, `elements`, `slices`, `diagrams`, `ratchets`,
   * `knownViolations` for an expired known violation, or `rules` for one known only from its
   * violations.
   */
  readonly family: string;
  /** The configured severity. */
  readonly severity: string;
  /** The rule's comment. */
  readonly comment?: string | undefined;
  /** The rule's `fix`. */
  readonly fix?: string | undefined;
}

/** One rule's result, as the junit reporter reports it. */
export interface Case {
  readonly rule: Rule;
  /** The failure message (the `fix`, then the first violations), when the rule failed. */
  failure: string | undefined;
  /** `[type, message]` for each reason the rule could not be trusted. */
  readonly errors: [string, string][];
  /** What the case reports without failing: warn, info and known findings. */
  readonly output: string[];
}

type Json = unknown;

function isObject(value: Json): value is Record<string, Json> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function field(value: Json, key: string): { present: boolean; value: Json } {
  if (isObject(value) && Object.hasOwn(value, key)) {
    return { present: true, value: value[key] };
  }
  return { present: false, value: undefined };
}

function string(value: Json, key: string): string | undefined {
  const found = field(value, key).value;
  return typeof found === 'string' ? found : undefined;
}

function list(value: Json, key: string): Json[] {
  const found = field(value, key).value;
  return Array.isArray(found) ? (found as Json[]) : [];
}

function uint(value: Json): number | undefined {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? value : undefined;
}

/**
 * A non-integral float as serde_json writes it. JavaScript and serde_json print the same shortest
 * digits and switch to exponents at the same points, except from 1e-6 up to 1e-5, where JavaScript
 * writes a decimal (`0.0000015`) and serde_json an exponent (`1.5e-6`).
 */
function shortest(value: number): string {
  const magnitude = Math.abs(value);
  return magnitude >= 1e-6 && magnitude < 1e-5 ? value.toExponential() : String(value);
}

/**
 * A value as JavaScript's `${value}` prints it after a round trip through serde_json
 * (crates/rb-report/src/lib.rs `js_number`).
 */
export function jsNumber(value: Json, present = true): string {
  if (!present) {
    return 'undefined';
  }
  if (typeof value === 'string') {
    return value;
  }
  if (typeof value === 'number') {
    return Number.isInteger(value) && Math.abs(value) < 1e21 ? value.toFixed(0) : shortest(value);
  }
  return JSON.stringify(value);
}

/** A field as a string: itself when a string, printed as JavaScript would otherwise. */
export function text(value: Json, key: string): string {
  const found = field(value, key);
  return typeof found.value === 'string' ? found.value : jsNumber(found.value, found.present);
}

function summary(result: Json): Json {
  return field(result, 'summary').value;
}

/** The violations of `summary.violations`, in the result's order. */
export function violations(result: Json): Json[] {
  return list(summary(result), 'violations');
}

/** The entries of a summary list: `vacuousRules`, `ratchets` or `expired`. */
export function summaryList(result: Json, key: string): Json[] {
  return list(summary(result), key);
}

/** A violation's rule name, empty when it has none. */
export function ruleName(violation: Json): string {
  return string(field(violation, 'rule').value, 'name') ?? '';
}

/** A violation's severity: empty without a rule, `undefined` for a rule without one. */
export function severity(violation: Json): string {
  const rule = field(violation, 'rule');
  return rule.present ? text(rule.value, 'severity') : '';
}

function entry(family: string, rule: Json): Rule {
  return {
    id: '',
    name: string(rule, 'name') ?? '',
    family,
    severity: string(rule, 'severity') ?? 'warn',
    comment: string(rule, 'comment'),
    fix: string(rule, 'fix'),
  };
}

const LISTED_FAMILIES = ['required', 'elements', 'slices', 'diagrams'] as const;

function byteOrder(a: string, b: string): number {
  const left = Buffer.from(a);
  const right = Buffer.from(b);
  return Buffer.compare(left, right);
}

/**
 * Every rule of the run, in the order `catalog::rules` gives: `forbidden`; the `allowed` list as
 * the one rule its violations name (`not-in-allowed`, at `allowedSeverity`, `warn` by default);
 * `required`, then the element, slice and diagram rules; a rule known only from its violations
 * (or whose name only rules of another family carry), in name order; the ratchets; then any
 * vacuous entry that names none of these.
 */
export function rules(result: Json): Rule[] {
  const ruleSet = field(summary(result), 'ruleSetUsed').value;
  const out: Rule[] = list(ruleSet, 'forbidden').map((rule) => entry('forbidden', rule));
  const allowed = list(ruleSet, 'allowed');
  if (allowed.length > 0) {
    out.push({
      id: '',
      name: 'not-in-allowed',
      family: 'allowed',
      severity: string(ruleSet, 'allowedSeverity') ?? 'warn',
      comment: string(allowed[0], 'comment'),
      fix: string(allowed[0], 'fix'),
    });
  }
  for (const key of LISTED_FAMILIES) {
    out.push(...list(ruleSet, key).map((rule) => entry(key, rule)));
  }
  const unlisted: Rule[] = [];
  for (const violation of violations(result)) {
    const name = ruleName(violation);
    const kind = string(violation, 'type');
    const known = (r: Rule): boolean => r.name === name && produces(r.family, kind);
    if (!out.some(known) && !unlisted.some(known)) {
      unlisted.push({
        id: '',
        name,
        family: 'rules',
        severity: severity(violation),
        comment: string(violation, 'comment'),
        fix: string(violation, 'fix'),
      });
    }
  }
  out.push(...unlisted.sort((a, b) => byteOrder(a.name, b.name)));
  for (const ratchet of summaryList(result, 'ratchets')) {
    out.push({ id: '', name: text(ratchet, 'name'), family: 'ratchets', severity: 'error' });
  }
  for (const vacuous of summaryList(result, 'vacuousRules')) {
    const name = text(vacuous, 'name');
    if (!out.some((r) => r.name === name)) {
      out.push({ id: '', name, family: 'rules', severity: 'error' });
    }
  }
  return identify(out);
}

/** `catalog::identify`: the name at its first occurrence, then `name#n`, past any id taken. */
function identify(found: Rule[]): Rule[] {
  const taken = new Set(found.map((r) => r.name));
  const seen = new Map<string, number>();
  return found.map((rule) => {
    const count = (seen.get(rule.name) ?? 0) + 1;
    seen.set(rule.name, count);
    if (count === 1) {
      return { ...rule, id: rule.name };
    }
    let n = count;
    while (taken.has(`${rule.name}#${String(n)}`)) {
      n += 1;
    }
    const id = `${rule.name}#${String(n)}`;
    taken.add(id);
    return { ...rule, id };
  });
}

/** `catalog::produces`: whether a rule of `family` can produce a violation of `kind`. */
function produces(family: string, kind: string | undefined): boolean {
  if (family === 'rules') {
    return true;
  }
  if (kind === 'element') {
    return family === 'elements' || family === 'diagrams';
  }
  if (kind === 'slice') {
    return family === 'slices';
  }
  return ['forbidden', 'allowed', 'required', 'rules'].includes(family);
}

/**
 * The index of the rule a violation belongs to, as `catalog::rule_index` picks it: among the
 * non-ratchet rules of its name, the first whose family can produce its `type` and whose severity
 * is its own, else the first whose family can produce it, else the first; `undefined` when none.
 */
export function ruleIndex(found: readonly Rule[], violation: Json): number | undefined {
  const name = ruleName(violation);
  const named = found.flatMap((r, i) => (r.name === name && r.family !== 'ratchets' ? [i] : []));
  const kind = string(violation, 'type');
  const producing = named.filter((i) => produces(found[i]?.family ?? '', kind));
  const fitting = producing.length > 0 ? producing : named;
  const wanted = severity(violation);
  return fitting.find((i) => found[i]?.severity === wanted) ?? fitting[0];
}

function first(items: Json[], key: string, wanted: string): Json {
  return items.find((item) => string(item, key) === wanted);
}

/**
 * Where a violation sits, when the extractor recorded it: the edge's line and column for a
 * dependency, the type's declaration for an element violation.
 */
export function position(result: Json, violation: Json): [number, number] | undefined {
  const from = text(violation, 'from');
  const to = text(violation, 'to');
  if (string(violation, 'type') === 'element') {
    const declared = first(list(field(result, 'code').value, 'types'), 'fullName', to);
    const line = uint(field(declared, 'line').value);
    if (line === undefined) {
      return undefined;
    }
    return [line, uint(field(declared, 'column').value) ?? 1];
  }
  const module = first(list(result, 'modules'), 'source', from);
  if (module === undefined) {
    return undefined;
  }
  const dependency = first(list(module, 'dependencies'), 'resolved', to);
  const line = uint(field(dependency, 'line').value);
  const column = uint(field(dependency, 'column').value);
  return line === undefined || column === undefined ? undefined : [line, column];
}

/** One line per violation: its id, `from -> to`, the line, and `[known]` when baselined. */
export function describe(result: Json, violation: Json): string {
  const id = string(violation, 'id');
  const at = position(result, violation);
  const where = at === undefined ? '' : ` (line ${String(at[0])}, column ${String(at[1])})`;
  const known = severity(violation) === 'ignore' ? ' [known]' : '';
  return `${id === undefined ? '' : `${id} `}${text(violation, 'from')} -> ${text(violation, 'to')}${where}${known}`;
}

function vacuousMessage(item: Json): string {
  return `rule \`${text(item, 'name')}\` is vacuous: its ${text(item, 'side')} side matched nothing, so it checks nothing (ADR-0007)`;
}

function expiredMessage(item: Json): string {
  return `${text(item, 'kind')} \`${text(item, 'name')}\` expired on ${text(item, 'expires')}; it no longer applies and the run fails`;
}

function ratchetCase(found: Case, ratchet: Json): void {
  const count = field(ratchet, 'count');
  const ceiling = field(ratchet, 'ceiling');
  const counted = jsNumber(count.value, count.present);
  const limit = jsNumber(ceiling.value, ceiling.present);
  const budget = text(ratchet, 'budget');
  const status = string(ratchet, 'status');
  if (status === 'exceeded') {
    found.failure = `ratchet \`${found.rule.name}\`: ${counted} edges exceed the ceiling of ${limit} in ${budget}`;
  } else if (status === 'no-budget') {
    found.errors.push([
      'no-budget',
      `ratchet \`${found.rule.name}\`: the budget ${budget} cannot be read, so the count ${counted} is checked against nothing`,
    ]);
  } else {
    found.output.push(`${counted} edges, within the ceiling of ${limit} in ${budget}`);
  }
}

function violationCase(found: Case, result: Json, catalogue: readonly Rule[], index: number): void {
  const name = found.rule.name;
  const matching = violations(result).filter((v) => ruleIndex(catalogue, v) === index);
  const errors = matching.filter((v) => severity(v) === 'error').map((v) => describe(result, v));
  for (const v of matching.filter((item) => severity(item) !== 'error')) {
    found.output.push(`${severity(v)}: ${describe(result, v)}`);
  }
  if (errors.length === 0) {
    return;
  }
  const fix = string(matching[0], 'fix') ?? found.rule.fix;
  const lines = [fix ?? `${String(errors.length)} violation(s) of \`${name}\``];
  lines.push(...errors.slice(0, SHOWN));
  if (errors.length > SHOWN) {
    lines.push(`... and ${String(errors.length - SHOWN)} more`);
  }
  found.failure = lines.join('\n');
}

/** One case per rule of `rules`, then one per expired known violation, in junit's order. */
export function cases(result: Json): Case[] {
  const vacuous = summaryList(result, 'vacuousRules');
  const expired = summaryList(result, 'expired');
  const ratchets = summaryList(result, 'ratchets');
  const out: Case[] = [];
  const catalogue = rules(result);
  let ratchetAt = 0;
  catalogue.forEach((rule, index) => {
    const found: Case = { rule, failure: undefined, errors: [], output: [] };
    // Vacuous and expired entries go to the first rule of their name.
    const first = catalogue.findIndex((r) => r.name === rule.name) === index;
    if (rule.family === 'ratchets') {
      // The ratchet rules are summary.ratchets, in order.
      const ratchet = ratchets[ratchetAt];
      if (ratchet !== undefined) {
        ratchetCase(found, ratchet);
      }
      ratchetAt += 1;
    } else {
      violationCase(found, result, catalogue, index);
    }
    for (const item of vacuous.filter((v) => first && text(v, 'name') === rule.name)) {
      if (string(item, 'severity') === 'warn') {
        found.output.push(`warning: ${vacuousMessage(item)}`);
      } else {
        found.errors.push(['vacuous', vacuousMessage(item)]);
      }
    }
    for (const item of expired) {
      if (first && text(item, 'kind') === 'rule' && text(item, 'name') === rule.name) {
        found.errors.push(['expired', expiredMessage(item)]);
      }
    }
    out.push(found);
  });
  for (const item of expired.filter((e) => text(e, 'kind') !== 'rule')) {
    out.push({
      rule: {
        id: text(item, 'name'),
        name: text(item, 'name'),
        family: 'knownViolations',
        severity: 'error',
      },
      failure: undefined,
      errors: [['expired', expiredMessage(item)]],
      output: [],
    });
  }
  return out;
}

/** Whether a case fails: an error-severity violation, or a reason it cannot be trusted. */
export function failed(found: Case): boolean {
  return found.failure !== undefined || found.errors.length > 0;
}

/** The failure message of a case: the junit failure message, then each error message. */
export function message(found: Case): string {
  return [
    ...(found.failure === undefined ? [] : [found.failure]),
    ...found.errors.map((e) => e[1]),
  ].join('\n');
}
