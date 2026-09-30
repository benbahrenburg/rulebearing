# .NET without a build: `--mode source`

Compiled mode reads the built assemblies and their portable PDBs, and it is the gate: the compiler has resolved every reference, so its edges are exact ([ADR-0011](adr/0011-read-dotnet-assemblies-not-source.md)). On a large solution a build takes minutes, and an agent will not build to check one import ([design § Where it would be ignored](artifacts/design.md#where-it-would-be-ignored)). `--mode source` reads the `.cs` files instead, with `tree-sitter-c-sharp`, and resolves names the way the compiler looks them up, without compiling anything. It is the inner loop's answer, never the gate ([Wave 3, Step 14](plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof); [FR-EXT-DN-04](prd.md#fr-ext-dn-04)).

```sh
rulebearing cruise --mode source -T err
```

or, in a native configuration:

```yaml
languages:
  dotnet:
    mode: source      # compiled (the default) | source
```

`--mode` wins over `languages.dotnet.mode`.

## What is read

| Step | What happens |
| --- | --- |
| Projects | Discovered as compiled mode discovers them: the solution (`languages.dotnet.solution`, else the only `.sln` or `.slnx`), else every project file; `excludeProjects` applied. Nothing needs to have been built |
| Files | Every `.cs` file under a discovered project's folder, except build output (`bin/`, `obj/`, `artifacts/`) and generated code (`*.g.cs`, `*.g.i.cs`, `*.Designer.cs`). A file belongs to the project whose folder is nearest above it; a file whose nearest project is outside the solution or excluded is not read. With no project file anywhere, every `.cs` file is read |
| Parse | Each file with `tree-sitter-c-sharp`, in parallel: the types it declares, its `using` directives by the namespace declaration they are written in, and every name written where a type can stand |
| Resolution | Each name looked up as C# orders the lookup: the nested types of the enclosing types; then, from the innermost namespace out, the types each namespace declares, then the aliases and the namespaces its directives import; the compilation unit's directives and the project's `global using` directives last. Generic arity must match |
| Projection | One module per file that declares a type or holds top-level statements, with an edge per target file and dependency kind, as compiled mode projects them |

A partial type lands in one file, as a compiled build attributes it: the part that declares a constructor when exactly one does, else the file named after the type, else the first by path. What a build compiles into the type rather than into a method (a field's type, a signature, an attribute, a base type, an initializer) is attributed to that file, and a part with code has an edge to it; what is written in a method body stays with the file that writes it. A namespace a `using` directive imports that no file declares is an external module named by the namespace: `package` when a package reference of the project provides it, `framework` for `System.*` and `Microsoft.*`, else `undetermined`, since without a build nothing says whether it resolves.

## What is marked

| Field | Value |
| --- | --- |
| every .NET dependency | `approximate: true` |
| every .NET file module | `attribution: source` |
| `summary.inspected.dotnet` | `mode: source`, `assemblies: 0` |
| the `agent` report | opens with `approximate`, a sentence saying the findings are namespace-level and compiled mode is the gate; each finding on an approximate edge carries `approximate: true` |

All of them are additive and `--strict-schema` removes them ([ADR-0004](adr/0004-graph-document-is-cruise-result-superset.md)). Source mode has no code layer, so element rules have nothing to evaluate there; compiled mode evaluates them.

## What it cannot know

A name is resolved without a compiler, so what only a compiler decides is not known: overload resolution, members inherited from a base class (a call to one lands in the class that uses it, not the one that declares it), and the type behind `var`. An extension method call (`services.AddClock()`) is resolved as C# looks extension methods up, from the innermost namespace out through the static classes each level declares and imports, with C# 14 `extension(T x) { ... }` blocks included; without the receiver's type every candidate at the first level that has one counts. A constant or an enum member read is no edge, since the compiler writes its value where it is read. A local variable or a property that shares its name with a type in scope reads as that type. Every edge is therefore `approximate`, and how close it is to a compiled graph is measured rather than assumed:

| Graph | Source edges | Compiled edges | Agreeing | Precision | Recall |
| --- | --- | --- | --- | --- | --- |
| `crates/rb-extract-dotnet/tests/fixtures/sample` | 5 | 5 | 5 | 100% | 100% |
| `crates/rb-extract-dotnet/tests/fixtures/toplevel` | 2 | 2 | 2 | 100% | 100% |
| ardalis/RiverBooks | 97 | 119 | 97 | 100% | 82% |
| NeVeSpl/NetArchTest.eNhancedEdition | 512 | 609 | 488 | 95.3% | 80% |
| evolutionary-architecture/evolutionary-architecture-by-example | 129 | 140 | 128 | 99.2% | 91% |
| onebeyond/monaco | 263 | 342 | 258 | 98.1% | 75% |
| phongnguyend/Practical.CleanArchitecture | 888 | 1,066 | 884 | 99.6% | 83% |

The comparison covers edges between two files both modes know, each oracle at its pinned SHA with the solution the oracle row names, the compiled side built in Release as the nightly builds it. `cargo test -p rb-extract-dotnet --features source-mode --test source_mode` asserts at least 90% precision and recall on the two built fixtures and writes `target/source-mode-precision.json`; the nightly runs [`testbeds/oracles/precision.py`](../testbeds/oracles/precision.py) on every .NET oracle and joins the results into `source-mode-precision.json` ([Wave 3, Step 14](plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)). Measuring it also found two edges compiled mode missed, now fixed: the body of an `async` or iterator method of a release build, and top-level statements.

## Never the gate

A run read in source mode never passes or fails a gate. `cruise` with a reporter that gates, `fmt --exit-code` on a saved source-mode result and `diff --exit-code` with a source-mode side print `warning: approximate-mode-not-a-gate: ...` and exit 2, whether the count was zero or not; `attest` refuses to sign such a graph at all. `cruise --from-hook` answers as it always does, because the Stop hook is the inner loop source mode exists for. `--allow-approximate-gate` gives the count back for a local script ([cli.md](cli.md#net-without-a-build---mode-source); [FR-EXT-DN-04](prd.md#fr-ext-dn-04)).

## The cache

Under `--cache`, a changed `.cs` file is the only one parsed again; every other file's parse is taken from the entry, and the resolution, which depends on every file, is run over all of them, so an incremental run equals a full one. A `.cs` file added or deleted, or a project file changed, reads everything again. A build since the entry was written changes nothing, since no assembly is read ([cli.md](cli.md#the-cache)).
