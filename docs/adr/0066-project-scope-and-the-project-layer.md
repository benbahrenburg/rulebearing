# ADR-0066: Project scope and the project layer

- **Status:** Accepted (2026-10-10, by the owner: "build it")
- **Date:** 2026-10-10
- **Derives from:** [FR-RULE-08](../prd.md#fr-rule-08) ("`moreUnstable` and `metrics` for modules, folders and .NET projects"); the dependency-cruiser coverage tab's `metrics` row ("Parity+ (module, folder, and for .NET project instability)", [coverage § Options](../artifacts/dependency-cruiser-18.2.0-coverage.md#options)); [FR-CLI-07](../prd.md#fr-cli-07) (`snapshot`: "instability per folder or project")
- **Constrains:** `crates/rb-model` (`GraphDocument::projects`, `ViolationType::Project`), `crates/rb-config` (`Scope::Project`), `crates/rb-rules` (`projects.rs`, `Matcher::Project`, liveness), `crates/rb-report` (`metrics`), `crates/rb-cli` (`snapshot`)
- **Builds on:** [ADR-0004](0004-graph-document-is-cruise-result-superset.md) (additions are additive), [ADR-0014](0014-no-invented-cross-language-edges.md)

## Context

dependency-cruiser computes instability for modules and for folders. A folder record (`folders[]`) carries `moduleCount`, the folders it depends on and that depend on it, afferent and efferent couplings, and `instability`. A `scope: folder` rule, such as `to.moreUnstable` or `to.circular`, then compares folders.

The design promises the same for .NET projects, and the PRD asks for `moreUnstable` and `metrics` over projects. Nothing computed them. The .NET extractor already records each module's project (`Module::project`, the `.csproj` that owns the file), and the Python extractor records the package, so the facts were in the graph and only the aggregation was missing. A project's folder is not its project: a solution's projects often share folders, and a project's files can sit outside its folder.

## Decision

**The project layer mirrors the folder layer.**
- When metrics are on (`--metrics`, or any `moreUnstable`, `scope: folder` or `scope: project` rule) and at least one module has a project, the document carries `projects[]`. It is an additive top-level field, absent otherwise.
- Its records have the folder record's shape and use the same code. A module's dependent or dependency counts as a coupling when it belongs to another project, or to no project (a package, a core module), and it is named by that project or by its own `source`.
- The unowned names become sinks with `moduleCount` -1, as the folder layer's packages do. Instability is efferent over total, and each project dependency carries its target's instability.

**`scope: project` compares projects** as `scope: folder` compares folders:
- `from.path`, `from.pathNot`, `to.path` and `to.pathNot` match project names;
- `to.moreUnstable` and `to.circular` read the project records;
- liveness counts a project rule's `from` against the modules' projects;
- a violation that is neither a cycle nor an instability has the new type `project`.

It is a native-configuration addition: in a `.dependency-cruiser.*` file it is exit 3, naming the fix (move the rule, or use `scope: folder`). Cross-language keys and `graph` are refused on it, as on a folder rule.

**The `metrics` reporter lists projects** beside folders and modules, with type `project`, when `projects[]` is present. `reporterOptions.metrics.hideProjects` hides them, as `hideFolders` hides folders.

**`snapshot` records instability per project** beside instability per folder, when the run has projects.

## Consequences

- The coverage tab's `metrics` row and FR-RULE-08 are met for .NET projects, and for Python packages, with no new extraction: the layer is derived from facts the extractors already write.
- dependency-cruiser's output is unchanged. TypeScript modules carry no project, so a TypeScript-only run has no `projects[]`, no project rows and no new violation type. Only a native configuration can ask for project scope.
- `--strict-schema` removes `projects[]`, and `project` violations with it, as it removes the other additions.
- A project dependency on a package is a sink, not a project. A rule that should ignore packages says so with `to.pathNot`, as a folder rule does with `node_modules`.

## Alternatives considered

| Option | Why not |
| --- | --- |
| Record that a project's instability is its folder's | Wrong when projects share a folder or a project spans folders, which solutions often do. The PRD asks for projects. |
| Count only project-to-project edges, leaving packages out | Martin's efferent coupling counts every dependency the component owns. The folder layer counts packages, and the two layers should read the same. |
| A separate record type for projects | Every reporter, the anonymiser and the metrics table already read folder records. The same shape keeps them working unchanged. |
