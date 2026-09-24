// Why a rule could not be checked, as the junit reporter's <error> elements carry it
// (crates/rb-report/src/catalog.rs; docs/adr/0007-vacuous-rules-fail-by-default.md).

namespace Rulebearing.TestAdapter;

/// <summary>Why a rule could not be checked.</summary>
/// <param name="Kind"><c>vacuous</c>, <c>expired</c> or <c>no-budget</c>.</param>
/// <param name="Message">The message the <c>junit</c> reporter writes for it.</param>
public sealed record RuleError(string Kind, string Message);
