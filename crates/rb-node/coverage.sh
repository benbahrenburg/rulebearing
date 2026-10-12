#!/usr/bin/env bash
# rb-node's Rust line coverage, measured through its JavaScript tests: the addon is built
# instrumented under `cargo llvm-cov show-env`, Node loads it while vitest runs __tests__/, and
# llvm-cov reports the crate. No Rust test binary can link the N-API symbols, which only a Node
# process has (Cargo.toml, [lib]). Prints llvm-cov's JSON summary, as `cargo llvm-cov --json` does.
# Called by scripts/coverage-per-crate.sh (docs/adr/0018-test-coverage-threshold.md).
# Plan: docs/plans/implemented/0003-wave-3-operations-surface-inner-loop.md, Step 22.
#
# The measurement does not depend on how the test process ends. An instrumented library normally
# writes its profile from an exit handler, and vitest stops a forked worker with SIGTERM and then
# SIGKILL 500 ms later: a worker killed before the write lost its share of the coverage, and on a
# busy runner rb-node read 51% or 60% instead of 91% with the same code. So:
#   1. All test files run in one forked worker (--maxWorkers=1 --no-isolate), so exactly one
#      process loads the addon.
#   2. That process profiles in LLVM's continuous mode (`%c` in LLVM_PROFILE_FILE): the counters
#      are mapped onto the profile file and are on disk as they change, so a worker that is killed
#      keeps every count. Continuous mode needs the counter section page-aligned on macOS (linker
#      flags) and relocated at run time on Linux (an LLVM flag); see `continuous_flags` below.
#   3. The run fails with a named error unless exactly one profile holds rb-node's counters and
#      they are not all zero, so a lost profile can never pass as a low figure.
#
# Usage: crates/rb-node/coverage.sh   (needs Node 22, the root's `npm ci`, and cargo-llvm-cov)
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"
cd "$root"

# The rustc flags continuous mode needs on this platform.
continuous_flags() {
  case "$(uname -s)" in
    Darwin)
      local section flags=""
      for section in __llvm_prf_cnts __llvm_prf_bits __llvm_prf_data; do
        flags+=" -Clink-arg=-Wl,-sectalign,__DATA,$section,0x4000"
      done
      printf '%s' "${flags# }"
      ;;
    Linux) printf '%s' "-Cllvm-args=-runtime-counter-relocation" ;;
    *)
      echo "rb-node coverage: error: continuous-mode profiling is not set up for $(uname -s); measure on Linux or macOS" >&2
      return 2
      ;;
  esac
}

# A target directory of its own: the instrumentation reaches rustc through a wrapper cargo does not
# fingerprint, so instrumented builds in target/debug would be taken for plain ones afterwards (an
# instrumented binary writes default.profraw wherever it runs).
export CARGO_TARGET_DIR="$root/target/rb-node-coverage"
flags="$(continuous_flags)"
eval "$(cargo llvm-cov show-env --sh 2>/dev/null)"
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }$flags"
export LLVM_PROFILE_FILE="${LLVM_PROFILE_FILE%.profraw}%c.profraw"
# The tests build the addon and the binary with this environment, so both are instrumented.
unset RULEBEARING_ADDON RULEBEARING_BINARY
cargo llvm-cov clean --workspace >&2
(cd "$here" && "$root/node_modules/.bin/vitest" run --maxWorkers=1 --no-isolate --no-file-parallelism) >&2

# Which profiles hold rb-node's counters: the addon's, written by the one worker that loaded it.
# The binary the tests run and the build scripts write profiles too, without rb-node's functions.
profdata="${LLVM_PROFDATA:-$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/llvm-profdata}"
profiles=("$CARGO_TARGET_DIR"/*.profraw)
addon=0
counted=0
for profile in "${profiles[@]}"; do
  [ -e "$profile" ] || continue
  # `7rb_node` is the crate's segment in every mangled rb-node symbol.
  calls="$("$profdata" show --function=7rb_node "$profile" | awk '/Function count:/ { shown++; sum += $3 } END { if (shown) print sum + 0 }')"
  if [ -n "$calls" ]; then
    addon=$((addon + 1))
    if [ "$calls" -gt 0 ]; then counted=$((counted + 1)); fi
  fi
done
echo "rb-node coverage: ${#profiles[@]} profiles, $addon with rb-node's counters" >&2
if [ "$addon" -ne 1 ] || [ "$counted" -ne 1 ]; then
  echo "rb-node coverage: error: expected exactly one profile with rb-node's counters, all from the single vitest worker; found $addon, $counted of them with any count. A profile was lost or split, so the figure would not be the tests' coverage (crates/rb-node/coverage.sh header)" >&2
  exit 1
fi
cargo llvm-cov report --package rb-node --summary-only >&2
cargo llvm-cov report --package rb-node --summary-only --json
