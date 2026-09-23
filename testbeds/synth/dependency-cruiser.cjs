// The configuration the synthetic benchmark cruises with: what a monorepo already on
// dependency-cruiser typically has. bench.sh copies it into the generated tree as
// .dependency-cruiser.cjs. Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 20.
module.exports = {
  forbidden: [
    {
      name: "no-circular",
      severity: "warn",
      comment: "A module on a cycle cannot be changed on its own.",
      from: {},
      to: { circular: true, dependencyTypesNot: ["type-only"] },
    },
    {
      name: "not-to-unresolvable",
      severity: "error",
      from: {},
      to: { couldNotResolve: true },
    },
    {
      name: "no-app-to-app",
      severity: "error",
      comment: "An app never imports another app.",
      from: { path: "^apps/([^/]+)/" },
      to: { path: "^apps/([^/]+)/", pathNot: "^apps/$1/" },
    },
    {
      name: "packages-not-to-apps",
      severity: "error",
      from: { path: "^packages/" },
      to: { path: "^apps/" },
    },
  ],
  options: {
    doNotFollow: { path: "node_modules" },
    tsPreCompilationDeps: true,
    tsConfig: { fileName: "tsconfig.json" },
  },
};
