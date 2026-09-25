// The message of a finding: the rule's name, then the text the `junit` reporter gives the same edge.
//
// Contract: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 1.5 (the junit failure
// message: the `fix`, then each violation as `<id> <from> -> <to> (line L, column C)`), rendered by
// crates/rb-report/src/catalog.rs. Decision: docs/adr/0015-stable-violation-id.md.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 13. Requirement: FR-DIST-04.
//
// The front-ends cannot disagree with the gate, so the words are the gate's: an agent that has
// seen the junit failure in CI sees the same text in its editor, with the rule's name in front.

import type { Finding } from './cli.js';

/** Where the import sits: 1-based line and column, as the extractor records an edge. */
export interface Position {
  readonly line: number;
  readonly column: number;
}

/** The `fix`, else the sentence junit prints for a rule without one. */
export function fixText(finding: Finding): string {
  return finding.fix ?? `1 violation(s) of \`${finding.name}\``;
}

/** One violation as junit lists it: `<id> <from> -> <to> (line L, column C)`. */
export function describe(finding: Finding, from: string, to: string, at: Position): string {
  return `${finding.id} ${from} -> ${to} (line ${String(at.line)}, column ${String(at.column)})`;
}

/** `<rule>: <fix>` then the violation's line, as the gate's junit failure message words them. */
export function message(finding: Finding, from: string, to: string, at: Position): string {
  return `${finding.name}: ${fixText(finding)}\n${describe(finding, from, to, at)}`;
}
