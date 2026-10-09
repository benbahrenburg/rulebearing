#!/usr/bin/env bash
# The guard catalogue's runner: the `guards` CI job. Each fixture under cookbook/guards/<slug>/ is
# run with the public commands a user runs, and nothing else:
#
#   rulebearing test --config rulebearing.yaml                                  exit 0
#   rulebearing config lint --config rulebearing.yaml --graph graph.json \
#       --require-comment-token --strict-compat                                 exit 0
#   rulebearing config expand rulebearing.yaml                                  = expanded.yaml
#   rulebearing config convert rulebearing.yaml --to dependency-cruiser         = converted/.dependency-cruiser.json,
#                                                                                 its report = converted/dropped.txt
#   rulebearing cruise --config rulebearing.yaml --graph graph.json -T json     summary.violations and
#                                                                                 summary.vacuousRules = expected.json
#   each negative/<case>/: rulebearing cruise --config <case>/rulebearing.yaml \
#       --graph <case>/graph.json or graph.json --require-comment-token         exit = <case>/exit-code
#
# expanded.yaml and converted/ are compared when the fixture has them. RB_UPDATE_SNAPSHOTS=1
# rewrites expanded.yaml, converted/ and expected.json from the binary instead; the diff is what a
# reviewer reads, and a pull request that regenerates one says which engine change made it.
#
#   cookbook/guards/run.sh <slug>...   run these fixtures
#   cookbook/guards/run.sh --all       run every fixture, one line each
#
# The binary is $RB, or target/release/rulebearing. Needs jq. Exits 1 on the first fixture that
# differs, 2 on a usage error.
# Plan: docs/plans/pending/0005-guard-catalogue.md, Step 1. Requirements: docs/prd.md#fr-rule-01,
# docs/prd.md#fr-cfg-07. Decisions: docs/adr/0021-agent-surface-cli-first.md (public commands
# only), docs/adr/0024-test-quality-gates.md (a regenerated expectation is explained).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"
rb="${RB:-$root/target/release/rulebearing}"
update="${RB_UPDATE_SNAPSHOTS:-}"

if [ ! -x "$rb" ]; then
    echo "guards: no binary at $rb; run cargo build --release or set RB" >&2
    exit 2
fi

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

# fail <slug> <what>: report the first difference and stop.
fail() {
    echo "FAIL $1: $2" >&2
    exit 1
}

# same <slug> <produced> <committed>: the produced file equals the committed one, or replaces it
# under RB_UPDATE_SNAPSHOTS.
same() {
    if [ -n "$update" ]; then
        mkdir -p "$(dirname "$3")"
        cp "$2" "$3"
        return
    fi
    if ! diff -u "$3" "$2" >&2; then
        fail "$1" "${3#"$root/"} differs from the binary's output (above)"
    fi
}

# run_fixture <slug>
run_fixture() {
    local slug="$1"
    local dir="$here/$slug"
    local config="$dir/rulebearing.yaml"
    local graph="$dir/graph.json"
    [ -f "$config" ] || fail "$slug" "no rulebearing.yaml in ${dir#"$root/"}"
    [ -f "$graph" ] || fail "$slug" "no graph.json in ${dir#"$root/"}"
    [ -f "$dir/README.md" ] || fail "$slug" "no README.md in ${dir#"$root/"}"

    if ! "$rb" test --config "$config" > "$scratch/test.txt" 2>&1; then
        cat "$scratch/test.txt" >&2
        fail "$slug" "rulebearing test failed (above)"
    fi
    if ! "$rb" config lint --config "$config" --graph "$graph" --require-comment-token \
        --strict-compat > "$scratch/lint.txt" 2>&1; then
        cat "$scratch/lint.txt" >&2
        fail "$slug" "config lint --require-comment-token --strict-compat failed (above)"
    fi
    if [ -f "$dir/expanded.yaml" ] || [ -n "$update" ]; then
        "$rb" config expand "$config" > "$scratch/expanded.yaml" \
            || fail "$slug" "config expand failed"
        same "$slug" "$scratch/expanded.yaml" "$dir/expanded.yaml"
    fi
    if [ -d "$dir/converted" ] || [ -n "$update" ]; then
        "$rb" config convert "$config" --to dependency-cruiser \
            > "$scratch/converted.json" 2> "$scratch/dropped.txt" \
            || fail "$slug" "config convert --to dependency-cruiser failed"
        same "$slug" "$scratch/converted.json" "$dir/converted/.dependency-cruiser.json"
        same "$slug" "$scratch/dropped.txt" "$dir/converted/dropped.txt"
    fi
    # The json reporter exits 0 whatever it finds, as upstream's does; the result is the compare.
    "$rb" cruise --config "$config" --graph "$graph" -T json > "$scratch/result.json" \
        || fail "$slug" "cruise --graph failed"
    jq '{violations: .summary.violations, vacuousRules: (.summary.vacuousRules // [])}' \
        "$scratch/result.json" > "$scratch/expected.json" \
        || fail "$slug" "cruise did not print a cruise result"
    same "$slug" "$scratch/expected.json" "$dir/expected.json"

    if [ -d "$dir/negative" ]; then
        local case code expected
        for case in "$dir"/negative/*/; do
            case="${case%/}"
            [ -f "$case/exit-code" ] || fail "$slug" "no exit-code in ${case#"$root/"}"
            expected="$(tr -d '[:space:]' < "$case/exit-code")"
            local case_graph="$graph"
            [ -f "$case/graph.json" ] && case_graph="$case/graph.json"
            code=0
            "$rb" cruise --config "$case/rulebearing.yaml" --graph "$case_graph" \
                --require-comment-token -T err > /dev/null 2>&1 || code=$?
            [ "$code" = "$expected" ] \
                || fail "$slug" "${case#"$root/"} exited $code, expected $expected"
        done
    fi
    echo "ok   $slug"
}

if [ "$#" -eq 0 ]; then
    echo "usage: cookbook/guards/run.sh <slug>... | --all" >&2
    exit 2
fi
slugs=()
if [ "$1" = "--all" ]; then
    for dir in "$here"/*/; do
        slugs+=("$(basename "$dir")")
    done
else
    slugs=("$@")
fi
for slug in "${slugs[@]}"; do
    run_fixture "$slug"
done
echo "guards: ${#slugs[@]} fixture(s) green"
