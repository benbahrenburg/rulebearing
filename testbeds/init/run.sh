#!/usr/bin/env bash
# The init fixtures: `rulebearing init --dry-run` over a test bed, written to
# testbeds/init/<owner>__<name>/rulebearing.yaml so that a change in what init discovers shows in
# review. The TypeScript oracles are the wave 1 fixtures; the greenfield beds semantic-kernel and
# autogen (.NET and Python in folders below the root) are the wave 2 proof, which the nightly
# regenerates and compares (testbeds/greenfield.sh).
#
# Usage: testbeds/init/run.sh [--preset <name>] <checkout> <owner/name> [<checkout> <owner/name> ...]
#
# With --preset, init runs with `--preset <name>` (a framework preset such as nextjs or
# clean-architecture, plan 0003 Step 11) and the fixture is <owner>__<name>/preset-<name>.yaml,
# beside the proposal without it.
#
# A checkout is any clone of the repository at its testbeds/manifest.yaml SHA (the layer 5
# checkouts under $RB_TESTBED_CHECKOUTS/layer5 serve), with its .NET solution built when it has one
# (the manifest's `build` command), and its own configuration left in place:
# --dry-run ignores it, which is the "config moved aside" of the plan. The clock is pinned, and
# the baseline entries are replaced by their count per rule, because the entries are the test
# bed's findings rather than init's discovery, and thousands of them would bury the proposal.
# Plans: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 16;
# docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 15;
# docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 11 (--preset).
# RB_INIT_FIXTURES names another folder to write to (the nightly writes beside the committed ones
# and compares).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"
bin="${RULEBEARING_BIN:-$root/target/release/rulebearing}"
fixtures="${RB_INIT_FIXTURES:-$here}"
[ -x "$bin" ] || (cd "$root" && cargo build --quiet --release -p rb-cli)
preset=""
file=rulebearing.yaml
if [ "${1:-}" = "--preset" ]; then
  preset="${2:?--preset needs a name}"
  file="preset-$preset.yaml"
  shift 2
fi

while [ "$#" -ge 2 ]; do
  checkout="$1" repo="$2"
  shift 2
  mkdir -p "$fixtures/${repo/\//__}"
  out="$fixtures/${repo/\//__}/$file"
  sha="$(git -C "$checkout" rev-parse HEAD)"
  proposal="$(cd "$checkout" && SOURCE_DATE_EPOCH=1790000000 "$bin" init --dry-run --owner testbed ${preset:+--preset "$preset"} 2> "$out.err")" || {
    echo "init: $repo: $(tail -1 "$out.err")" >&2
    rm -f "$out.err"
    continue
  }
  summary="$(tail -1 "$out.err")"
  rm -f "$out.err"
  {
    echo "# testbeds/init: \`rulebearing init --dry-run${preset:+ --preset $preset}\` on $repo at $sha"
    echo "# ($summary). Regenerate with testbeds/init/run.sh."
    printf '%s\n' "$proposal" | python3 -c '
import collections, json, sys
lines = sys.stdin.read().splitlines()
counts = collections.Counter()
for line in lines:
    stripped = line.strip()
    if stripped.startswith("- {\"id\""):
        counts[json.loads(stripped[2:])["rule"]["name"]] += 1
    else:
        print(line)
if counts:
    print("    # " + ", ".join(f"{rule}: {n}" for rule, n in sorted(counts.items())) + " (entries elided)")
'
  } > "$out"
  echo "init: $repo -> ${out#"$root"/}"
done
