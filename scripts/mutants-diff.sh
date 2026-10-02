#!/usr/bin/env bash
# Mutation testing of the lines a change touches, over the crates docs/adr/0024-test-quality-gates.md
# names. The pull-request job and a step's pre-merge check run this; a push to main runs the whole
# scope (docs/adr/0059-gates-run-by-tier-and-mutants-by-diff.md).
#
# Usage: scripts/mutants-diff.sh <base-ref> [cargo mutants options, e.g. --shard 0/8 --jobs 2]
#
# The diff is the working tree against the merge base with <base-ref>, so uncommitted edits count.
# A file git does not know yet is not in the diff: `git add -N <file>` makes it visible.
# A change to the mutation scope itself (.cargo/mutants.toml) runs the whole scope instead.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

if [[ $# -lt 1 ]]; then
  echo "usage: scripts/mutants-diff.sh <base-ref> [cargo mutants options]" >&2
  exit 2
fi
base="$1"
shift

scope=(--no-shuffle --package rb-model --package rb-rules --package xtask)
merge_base="$(git merge-base "$base" HEAD)"

if ! git diff --quiet "$merge_base" -- .cargo/mutants.toml; then
  echo "mutants-diff: .cargo/mutants.toml changed, so the whole scope runs"
  exec cargo mutants "${scope[@]}" "$@"
fi

mkdir -p target
patch="target/mutants-in-diff.patch"
git diff "$merge_base" -- crates/rb-model crates/rb-rules xtask >"$patch"

if [[ ! -s "$patch" ]]; then
  echo "mutants-diff: no change under rb-model, rb-rules or xtask since $base, so no mutants to run"
  exit 0
fi

echo "mutants-diff: mutating the lines changed since $(git rev-parse --short "$merge_base") ($base)"
exec cargo mutants "${scope[@]}" --in-diff "$patch" "$@"
