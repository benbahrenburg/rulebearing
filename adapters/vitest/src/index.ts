// `rulebearing/vitest`: Rulebearing's architecture rules as vitest tests.
//
// Architecture: docs/architecture.md#distribution (`rulebearing/vitest` ships inside the npm
// package). Decision: docs/adr/0020-single-name-across-registries.md (a subpath of `rulebearing`,
// since the @rulebearing scope is not held). Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md
// § 2.14, Step 14 (2H). Coverage: docs/artifacts/archunitnet-0.13.4-coverage.md § Test framework
// adapters. Requirement: FR-DIST-03 (docs/prd.md).

export { defineArchitectureTests, ArchitectureRuleError, ruleMeta, ruleBody } from './define.js';
export type { ArchitectureTestOptions, Registrar, RuleMeta } from './define.js';
export { RulebearingReporter, summarise } from './reporter.js';
export type { RuleOutcome, RulebearingReporterOptions } from './reporter.js';
export { cases, message, failed, rules, SHOWN } from './cases.js';
export type { Case, Rule } from './cases.js';
export { cruise, CruiseError, BINARY_OVERRIDE } from './run.js';
export type { RunOptions } from './run.js';
