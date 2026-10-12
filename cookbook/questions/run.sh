#!/usr/bin/env bash
# The cookbook's question pages, held to the binary: each fixture under
# cookbook/questions/<slug>/ is run with the public commands a user runs, and the files the docs
# site renders on the page are compared with what the binary prints, so a page cannot show a rule,
# a conversion or a violation the tool does not produce. Run by the `guards` CI job and by
# site/build.sh before the site is generated.
#
#   rulebearing test --config rulebearing.yaml                         exit 0, no rule without examples
#   (in repo/) rulebearing cruise --config ../rulebearing.yaml -T json summary.violations and
#                                                                      summary.vacuousRules = expected.json,
#                                                                      at least one violation
#   rulebearing config lint --config rulebearing.yaml --graph <that cruise result> \
#       --require-comment-token --strict-compat                        exit 0
#   rulebearing config convert rulebearing.yaml --to dependency-cruiser = converted/.dependency-cruiser.json,
#                                                                      its report = converted/dropped.txt
#   when repo/.importlinter exists, (in repo/):
#     rulebearing import import-linter --from .importlinter            = imported/rulebearing.yaml
#     rulebearing cruise --config ../imported/rulebearing.yaml -T json = imported/expected.json, at least
#                                                                      one violation, and the same
#                                                                      from -> to edges as the recipe
#                                                                      unless the import writes slices
#
# RB_UPDATE_SNAPSHOTS=1 rewrites converted/, expected.json and imported/ from the binary instead;
# the diff is what a reviewer reads, and a pull request that regenerates one says which engine
# change made it.
#
#   cookbook/questions/run.sh <slug>...   run these fixtures
#   cookbook/questions/run.sh --all       run every fixture, one line each
#
# The binary is $RB, or target/release/rulebearing. Needs jq. Exits 1 on the first fixture that
# differs, 2 on a usage error.
# Plan: docs/plans/pending/0004-wave-4-reach.md, Step 5. Requirement: docs/prd.md#fr-reach-01.
# Decisions: docs/adr/0074-the-cookbook-question-pages-are-fixtures-beside-the-guard-catalogue.md,
# docs/adr/0021-agent-surface-cli-first.md (public commands only),
# docs/adr/0024-test-quality-gates.md (a regenerated expectation is explained).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd "$here/../.." && pwd -P)"
rb="${RB:-$root/target/release/rulebearing}"
update="${RB_UPDATE_SNAPSHOTS:-}"

if [ ! -x "$rb" ]; then
    echo "questions: no binary at $rb; run cargo build --release or set RB" >&2
    exit 2
fi

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

# A virtual environment named by the caller's shell would change what the Python extractor reads.
unset VIRTUAL_ENV

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
    [ -f "$3" ] || fail "$1" "${3#"$root/"} is missing; run with RB_UPDATE_SNAPSHOTS=1"
    if ! diff -u "$3" "$2" >&2; then
        fail "$1" "${3#"$root/"} differs from the binary's output (above)"
    fi
}

# findings <cruise result> <out>: the part of a cruise result a page shows.
findings() {
    jq '{violations: .summary.violations, vacuousRules: (.summary.vacuousRules // [])}' "$1" > "$2"
}

# edges <findings>: the sorted from -> to pairs.
edges() {
    jq -c '[.violations[] | [.from, .to]] | unique' "$1"
}

# run_fixture <slug>
run_fixture() {
    local slug="$1"
    local dir="$here/$slug"
    local config="$dir/rulebearing.yaml"
    [ -f "$config" ] || fail "$slug" "no rulebearing.yaml in ${dir#"$root/"}"
    [ -d "$dir/repo" ] || fail "$slug" "no repo/ in ${dir#"$root/"}"
    [ -f "$dir/README.md" ] || fail "$slug" "no README.md in ${dir#"$root/"}"

    if ! "$rb" test --config "$config" > "$scratch/test.txt" 2>&1; then
        cat "$scratch/test.txt" >&2
        fail "$slug" "rulebearing test failed (above)"
    fi
    grep -q '; 0 rule(s) without examples$' "$scratch/test.txt" \
        || fail "$slug" "a rule has no examples: $(tail -n 1 "$scratch/test.txt")"

    # The json reporter exits 0 whatever it finds, as upstream's does; the result is the compare.
    (cd "$dir/repo" && "$rb" cruise --config ../rulebearing.yaml -T json) \
        > "$scratch/result.json" 2> "$scratch/cruise.txt" \
        || { cat "$scratch/cruise.txt" >&2; fail "$slug" "cruise failed (above)"; }
    findings "$scratch/result.json" "$scratch/expected.json" \
        || fail "$slug" "cruise did not print a cruise result"
    same "$slug" "$scratch/expected.json" "$dir/expected.json"
    [ "$(jq '.violations | length' "$scratch/expected.json")" -gt 0 ] \
        || fail "$slug" "the repository holds no failing edge for the page to show"

    if ! "$rb" config lint --config "$config" --graph "$scratch/result.json" \
        --require-comment-token --strict-compat > "$scratch/lint.txt" 2>&1; then
        cat "$scratch/lint.txt" >&2
        fail "$slug" "config lint --require-comment-token --strict-compat failed (above)"
    fi

    "$rb" config convert "$config" --to dependency-cruiser \
        > "$scratch/converted.json" 2> "$scratch/dropped.txt" \
        || fail "$slug" "config convert --to dependency-cruiser failed"
    same "$slug" "$scratch/converted.json" "$dir/converted/.dependency-cruiser.json"
    same "$slug" "$scratch/dropped.txt" "$dir/converted/dropped.txt"

    if [ -f "$dir/repo/.importlinter" ]; then
        (cd "$dir/repo" && "$rb" import import-linter --from .importlinter) \
            > "$scratch/imported.yaml" \
            || fail "$slug" "import import-linter failed"
        same "$slug" "$scratch/imported.yaml" "$dir/imported/rulebearing.yaml"
        # The imported patterns carry nested quantifiers dependency-cruiser refuses; cruise warns
        # about them on stderr, which is not the compare.
        (cd "$dir/repo" && "$rb" cruise --config "$scratch/imported.yaml" -T json) \
            > "$scratch/imported-result.json" 2> /dev/null \
            || fail "$slug" "cruise with the imported contract failed"
        findings "$scratch/imported-result.json" "$scratch/imported-expected.json" \
            || fail "$slug" "cruise with the imported contract did not print a cruise result"
        same "$slug" "$scratch/imported-expected.json" "$dir/imported/expected.json"
        [ "$(jq '.violations | length' "$scratch/imported-expected.json")" -gt 0 ] \
            || fail "$slug" "the imported contract finds no failing edge"
        # A slice finding names slices, not modules, so only edge findings are compared.
        if [ "$(jq '[.violations[] | select(.type == "slice")] | length' \
            "$scratch/imported-expected.json")" -eq 0 ]; then
            [ "$(edges "$scratch/expected.json")" = "$(edges "$scratch/imported-expected.json")" ] \
                || fail "$slug" "the recipe finds $(edges "$scratch/expected.json"), the imported contract $(edges "$scratch/imported-expected.json")"
        fi
    fi
    echo "ok   $slug"
}

if [ "$#" -eq 0 ]; then
    echo "usage: cookbook/questions/run.sh <slug>... | --all" >&2
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
echo "questions: ${#slugs[@]} fixture(s) green"
