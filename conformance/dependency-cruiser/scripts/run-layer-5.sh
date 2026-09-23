#!/usr/bin/env bash
# Conformance gate 1, layer 5: the oracle zero-diff and the mutation branch.
#
#   --repo <owner/name>  clone the oracle at its testbeds/manifest.yaml SHA, run dependency-cruiser
#                        and `rulebearing cruise` with the repository's own configuration and
#                        roots, and diff the two cruise results (harness/zero-diff.mjs). A
#                        difference not recorded in conformance/divergences.md fails the run.
#   --all                the three oracles the plan names, one after the other
#   --mutations          apply mutations/mutations.patch to a fresh checkout of dependency-cruiser's
#                        repository at its manifest SHA and assert that both tools report exactly
#                        the twelve violations in mutations/expected.json, and agree otherwise
#
# Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 18 and sub-wave 1G.
# Requirements: docs/prd.md#nfr-conf-01, docs/prd.md#nfr-conf-03.
# Decision: docs/adr/0009-conformance-suites-as-specification.md.
#
# Checkouts, like testbeds/run.sh's, live outside this repository (tsconfig, package.json and
# node_modules lookups walk upward, and must not find this tree) under $RB_TESTBED_CHECKOUTS
# (default: rulebearing-testbeds under the temp directory), in a layer5/ folder of their own.
# Nothing from a checkout is ever committed. Results go to target/conformance/layer5/<slug>/.
# The binary is $RULEBEARING_BIN, else target/release/rulebearing, built when absent.
# Needs git, Node 22 or later, python3 with PyYAML, and network access.
#
# The roots and set-up, per oracle, and why (the repository's own invocation of depcruise):
#
#   sverweij/dependency-cruiser  roots `src bin test configs types tools`, from its package.json
#       script `depcruise`; config .dependency-cruiser.mjs. `npm ci` first, since npm edges and
#       the `npm*` dependency types need node_modules. The incumbent is the checkout's own
#       bin/dependency-cruiser.mjs, as `npm run depcruise` runs it: at the manifest SHA the
#       repository is dependency-cruiser 18.4.0, whose configuration sets `options.baseline`,
#       which the pinned 18.2.0 rejects as an unknown key. 18.3 and 18.4 changed the baseline
#       and swc only (their release notes), neither of which this cruise uses.
#   langfuse/langfuse            roots `src` in web/, from web/scripts/structure/stats.mjs, the
#       one place the repository calls dependency-cruiser (`cruise(["src"], ...)` with this
#       config's options); config web/.dependency-cruiser.js. `pnpm install --filter web...`
#       first: the tsconfig the config names extends @repo/typescript-config, a workspace
#       package that is only reachable through node_modules.
#   microsoft/FluidFramework     roots `src/` in packages/dds/tree, from that package's script
#       `depcruise`; config packages/dds/tree/.dependency-cruiser.cjs. A blobless, sparse clone
#       of packages/dds/tree alone: the config's `includeOnly: "^src/"` drops every edge that
#       leaves the package, so neither the rest of the monorepo nor an install can change the
#       result, and both tools read the identical tree.
#
# `--ignore-known`, which two of those scripts pass, is left out for both tools: Rulebearing's
# baseline modes arrive in wave 2, and without it every violation is compared, which is the
# stricter test. `--no-liveness` is passed to Rulebearing only: liveness is its addition
# (docs/adr/0007-vacuous-rules-fail-by-default.md) and dependency-cruiser has no equivalent.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
gate="$(cd "$here/.." && pwd -P)"
root="$(cd "$gate/../.." && pwd -P)"
pin="$(tr -d '[:space:]' < "$gate/PIN")"
# The TypeScript the incumbent loads for tsc parsing and for transpiling before acorn: the
# version 18.2.0's own lockfile carries, which layer 1 was recorded with.
typescript_version="6.0.3"
checkouts="${RB_TESTBED_CHECKOUTS:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/rulebearing-testbeds}/layer5"
out_root="$root/target/conformance/layer5"
oracles=(sverweij/dependency-cruiser langfuse/langfuse microsoft/FluidFramework)

usage() {
  echo "usage: $0 --repo <owner/name> | --all | --mutations" >&2
  exit 2
}

mode=""
repo=""
while [ $# -gt 0 ]; do
  case "$1" in
    --repo) [ $# -ge 2 ] || usage; mode=repo; repo="$2"; shift 2 ;;
    --all) mode=all; shift ;;
    --mutations) mode=mutations; shift ;;
    *) usage ;;
  esac
done
[ -n "$mode" ] || usage

field() { # repo, field
  python3 -c '
import sys, yaml
rows = yaml.safe_load(open(sys.argv[1]))["rows"]
match = [r for r in rows if r["repo"] == sys.argv[2]]
if not match:
    sys.exit(f"layer5: {sys.argv[2]} is not in the manifest")
print(match[0].get(sys.argv[3], ""))
' "$root/testbeds/manifest.yaml" "$1" "$2"
}

rulebearing="${RULEBEARING_BIN:-$root/target/release/rulebearing}"
if [ -z "${RULEBEARING_BIN:-}" ] && [ ! -x "$rulebearing" ]; then
  (cd "$root" && cargo build --quiet --release -p rb-cli)
fi

# dependency-cruiser at the pin, with the TypeScript it parses with, in a tool folder of its own.
pinned_incumbent() {
  local tools="$checkouts/.tools/dependency-cruiser-$pin"
  if [ ! -f "$tools/node_modules/dependency-cruiser/bin/dependency-cruise.mjs" ]; then
    mkdir -p "$tools"
    npm install --silent --no-audit --no-fund --prefix "$tools" \
      "dependency-cruiser@$pin" "typescript@$typescript_version" >&2
  fi
  echo "$tools/node_modules/dependency-cruiser/bin/dependency-cruise.mjs"
}

# A fresh checkout of one commit, shallow; with paths, blobless and sparse over those paths.
checkout() { # owner/name, sha, directory, sparse paths...
  local name="$1" sha="$2" dir="$3"
  shift 3
  rm -rf "$dir"
  mkdir -p "$dir"
  git -C "$dir" init --quiet
  git -C "$dir" remote add origin "https://github.com/$name.git"
  if [ $# -gt 0 ]; then
    git -C "$dir" sparse-checkout set --no-cone "$@"
    git -C "$dir" fetch --quiet --depth 1 --filter=blob:none origin "$sha"
  else
    git -C "$dir" fetch --quiet --depth 1 origin "$sha"
  fi
  git -C "$dir" -c advice.detachedHead=false checkout --quiet FETCH_HEAD
}

# Runs both tools in a directory; writes incumbent.json and rulebearing.json under an out folder.
cruise_both() { # out, directory, config, incumbent script, roots...
  local out="$1" dir="$2" config="$3" incumbent="$4"
  shift 4
  mkdir -p "$out"
  local started status incumbent_status
  started="$(date +%s)"
  status=0
  (cd "$dir" && node "$incumbent" --config "$config" --output-type json --no-progress "$@") \
    > "$out/incumbent.json" 2> "$out/incumbent.err" || status=$?
  incumbent_status="$status"
  echo "layer5: dependency-cruiser exit $status in $(( $(date +%s) - started ))s"
  started="$(date +%s)"
  status=0
  (cd "$dir" && "$rulebearing" cruise --config "$config" --output-type json --no-liveness "$@") \
    > "$out/rulebearing.json" 2> "$out/rulebearing.err" || status=$?
  echo "layer5: rulebearing exit $status in $(( $(date +%s) - started ))s"
  # The json reporter does not gate in either tool, so both exit 0 on a trustworthy run
  # (docs/adr/0030-the-reporter-decides-the-error-count-exit.md); a different code is a difference.
  if [ "$status" != "$incumbent_status" ]; then
    echo "layer5: exit codes differ: dependency-cruiser $incumbent_status, rulebearing $status" >&2
    head -c 2000 "$out/rulebearing.err" >&2
    return 1
  fi
  local tool
  for tool in incumbent rulebearing; do
    if ! node -e 'JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"))' "$out/$tool.json" 2> /dev/null; then
      echo "layer5: $tool produced no cruise result:" >&2
      head -c 2000 "$out/$tool.err" >&2
      return 1
    fi
  done
}

zero_diff() { # out, repo, extra arguments...
  local out="$1" name="$2"
  shift 2
  node "$gate/harness/zero-diff.mjs" --repo "$name" \
    --incumbent "$out/incumbent.json" --rulebearing "$out/rulebearing.json" \
    --divergences "$root/conformance/divergences.md" "$@"
}

run_oracle() { # owner/name
  local name="$1" sha config dir out incumbent
  sha="$(field "$name" sha)"
  config="$(field "$name" config)"
  dir="$checkouts/${name//\//__}"
  out="$out_root/${name//\//__}"
  echo "layer5: $name at $sha"
  local config_dir roots=()
  config_dir="$(dirname "$config")"
  case "$name" in
    sverweij/dependency-cruiser)
      checkout "$name" "$sha" "$dir"
      (cd "$dir" && npm ci --silent --no-audit --no-fund --ignore-scripts)
      incumbent="$dir/bin/dependency-cruiser.mjs"
      roots=(src bin test configs types tools)
      ;;
    langfuse/langfuse)
      checkout "$name" "$sha" "$dir"
      local pnpm
      pnpm="$(node -p 'require(process.argv[1]).packageManager' "$dir/package.json")"
      (cd "$dir" && npx --yes "$pnpm" install --silent --frozen-lockfile --ignore-scripts --filter 'web...')
      incumbent="$(pinned_incumbent)"
      roots=(src)
      ;;
    microsoft/FluidFramework)
      checkout "$name" "$sha" "$dir" "/$config_dir/"
      incumbent="$(pinned_incumbent)"
      roots=(src/)
      ;;
    *)
      echo "layer5: no roots recorded for $name; add it to this script with its reason" >&2
      return 2
      ;;
  esac
  cruise_both "$out" "$dir/$config_dir" "$(basename "$config")" "$incumbent" "${roots[@]}"
  zero_diff "$out" "$name"
}

run_mutations() {
  local name=sverweij/dependency-cruiser sha dir out
  sha="$(field "$name" sha)"
  dir="$checkouts/mutations"
  out="$out_root/mutations"
  echo "layer5: mutation branch over $name at $sha"
  checkout "$name" "$sha" "$dir"
  git -C "$dir" apply --whitespace=nowarn "$gate/mutations/mutations.patch"
  (cd "$dir" && npm ci --silent --no-audit --no-fund --ignore-scripts)
  # The pinned incumbent: the mutation config uses nothing newer than 18.2.0.
  cruise_both "$out" "$dir" .dependency-cruiser.mutations.cjs "$(pinned_incumbent)" src bin
  zero_diff "$out" "$name" --expect "$gate/mutations/expected.json"
}

case "$mode" in
  repo) run_oracle "$repo" ;;
  all)
    failed=0
    for name in "${oracles[@]}"; do
      run_oracle "$name" || failed=1
    done
    exit "$failed"
    ;;
  mutations) run_mutations ;;
esac
