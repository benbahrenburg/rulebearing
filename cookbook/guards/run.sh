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
#   rulebearing cruise --config rulebearing.yaml --graph graph.json -T json     summary.violations,
#                                                                                 summary.vacuousRules and, when
#                                                                                 present, summary.ratchets and
#                                                                                 summary.expired = expected.json
#   rulebearing cruise --config rulebearing.yaml --graph graph.json \
#       --require-comment-token -T err                                          exit = exit-code, when the fixture has one
#
# Each command runs in the fixture's folder, so a relative budget file resolves as it does for a
# user at the root of a repository.
#
# A negative case, negative/<case>/, holds the files that differ from the fixture. The runner
# copies the fixture to a temporary folder, overlays the case and runs one command there:
#
#   command           the arguments, one per line (default: cruise --config rulebearing.yaml
#                     --graph graph.json --require-comment-token -T err)
#   exit-code         the exit code the command must give (required)
#   remove            files to delete from the copy, one per line
#   names             text the command's output must contain, one per line, so a case cannot pass
#                     by failing for another reason
#   expected.json     as above, from a cruise of the overlaid copy with -T json
#
# A negative case is a refusal, so the command must leave every file in the copy byte-identical.
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
# Plan: docs/plans/pending/0005-guard-catalogue.md, Steps 1 and 4. Requirements:
# docs/prd.md#fr-rule-01, docs/prd.md#fr-rule-06, docs/prd.md#fr-rule-09, docs/prd.md#fr-cfg-07.
# Decisions: docs/adr/0008-exit-code-contract.md (the exit codes),
# docs/adr/0021-agent-surface-cli-first.md (public commands only),
# docs/adr/0024-test-quality-gates.md (a regenerated expectation is explained).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"
rb="${RB:-$root/target/release/rulebearing}"
# Each command runs in a fixture's folder, so a relative $RB is taken from here first.
case "$rb" in
    /*) ;;
    */*) rb="$PWD/$rb" ;;
esac
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

# summarise <slug> <cruise result> <output>: the part of a cruise result a fixture commits to.
summarise() {
    jq '{violations: .summary.violations, vacuousRules: (.summary.vacuousRules // [])}
        + (if .summary.ratchets then {ratchets: .summary.ratchets} else {} end)
        + (if .summary.expired then {expired: .summary.expired} else {} end)' \
        "$2" > "$3" || fail "$1" "cruise did not print a cruise result"
}

# run_negative <slug> <fixture dir> <case dir>
run_negative() {
    local slug="$1" dir="$2" case="$3"
    local label="${case#"$root/"}"
    local work="$scratch/case" before="$scratch/before"
    local line expected code
    local args=(cruise --config rulebearing.yaml --graph graph.json --require-comment-token -T err)

    [ -f "$case/exit-code" ] || fail "$slug" "no exit-code in $label"
    expected="$(tr -d '[:space:]' < "$case/exit-code")"
    if [ -f "$case/command" ]; then
        args=()
        while IFS= read -r line || [ -n "$line" ]; do
            args+=("$line")
        done < "$case/command"
    fi

    rm -rf "$work" "$before"
    mkdir -p "$work"
    cp -R "$dir/." "$work/"
    rm -rf "$work/negative" "$work/expected.json" "$work/exit-code"
    cp -R "$case/." "$work/"
    rm -f "$work/command" "$work/exit-code" "$work/remove" "$work/names" "$work/expected.json"
    if [ -f "$case/remove" ]; then
        while IFS= read -r line || [ -n "$line" ]; do
            [ -e "$work/$line" ] || fail "$slug" "$label removes $line, which is not there"
            rm -f "$work/$line"
        done < "$case/remove"
    fi
    cp -R "$work" "$before"

    code=0
    (cd "$work" && "$rb" "${args[@]}") > "$scratch/case.txt" 2>&1 || code=$?
    if [ "$code" != "$expected" ]; then
        cat "$scratch/case.txt" >&2
        fail "$slug" "$label exited $code, expected $expected (output above)"
    fi
    if [ -f "$case/names" ]; then
        while IFS= read -r line || [ -n "$line" ]; do
            if ! grep -qF -- "$line" "$scratch/case.txt"; then
                cat "$scratch/case.txt" >&2
                fail "$slug" "$label does not name \"$line\" (output above)"
            fi
        done < "$case/names"
    fi
    diff -r "$before" "$work" >&2 || fail "$slug" "$label changed a file (above)"
    if [ -f "$case/expected.json" ]; then
        # The json reporter's exit code is not the result here: the exit-code file asserted that.
        (cd "$work" && "$rb" cruise --config rulebearing.yaml --graph graph.json -T json) \
            > "$scratch/case.json" 2> /dev/null || true
        summarise "$slug" "$scratch/case.json" "$scratch/case-expected.json"
        same "$slug" "$scratch/case-expected.json" "$case/expected.json"
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

    cd "$dir"
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
    summarise "$slug" "$scratch/result.json" "$scratch/expected.json"
    same "$slug" "$scratch/expected.json" "$dir/expected.json"
    if [ -f "$dir/exit-code" ]; then
        local want got=0
        want="$(tr -d '[:space:]' < "$dir/exit-code")"
        "$rb" cruise --config "$config" --graph "$graph" --require-comment-token -T err \
            > "$scratch/gate.txt" 2>&1 || got=$?
        if [ "$got" != "$want" ]; then
            cat "$scratch/gate.txt" >&2
            fail "$slug" "the gate exited $got, expected $want (output above)"
        fi
    fi

    if [ -d "$dir/negative" ]; then
        local case
        for case in "$dir"/negative/*/; do
            run_negative "$slug" "$dir" "${case%/}"
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
