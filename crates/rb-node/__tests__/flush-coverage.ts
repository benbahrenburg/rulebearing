// Under `cargo llvm-cov` (CARGO_LLVM_COV set, scripts/coverage-per-crate.sh): vitest stops each
// forked worker with SIGTERM, and a process a signal ends never runs its exit handlers, so the
// instrumented addon would write no profile. Exiting on SIGTERM runs them.
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 22.
const FLAG = Symbol.for('rb-node.flush-coverage');
const scope = globalThis as Record<symbol, unknown>;

if (process.env.CARGO_LLVM_COV !== undefined && scope[FLAG] === undefined) {
  scope[FLAG] = true;
  process.once('SIGTERM', () => {
    process.exit(0);
  });
}
