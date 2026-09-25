#!/usr/bin/env bash
# Shared by testbeds/oracles/python.sh and dotnet.sh: a manifest row's fields and its clone.
#
# Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 11 (the oracle
# harness). Requirement: docs/prd.md#nfr-conf-03. Sourced, not run.

# oracle_field <manifest> <owner/repo> <field>: the field's value, a list joined with commas, empty
# when the row has no such field; exits 2 when the row is not in the manifest.
oracle_field() {
  python3 -c '
import sys, yaml
rows = [r for r in yaml.safe_load(open(sys.argv[1]))["rows"] if r["repo"] == sys.argv[2]]
if not rows:
    sys.exit(2)
value = rows[0].get(sys.argv[3], "")
print(",".join(value) if isinstance(value, list) else value)
' "$1" "$2" "$3"
}

# oracle_clone <owner/repo> <sha> <checkout> <log>: the one commit, shallow, read-only, outside
# this repository (MSBuild, eslint and tsconfig search upward for their configuration). A checkout
# already at the SHA is kept, so a local rerun does not clone or build again.
oracle_clone() {
  local repo="$1" sha="$2" checkout="$3" log="$4"
  [ "$(git -C "$checkout" rev-parse HEAD 2>/dev/null)" = "$sha" ] && return 0
  rm -rf "$checkout" && mkdir -p "$checkout"
  { git -C "$checkout" init --quiet &&
    git -C "$checkout" remote add origin "https://github.com/$repo.git" &&
    git -C "$checkout" fetch --quiet --depth 1 origin "$sha" &&
    git -C "$checkout" -c advice.detachedHead=false checkout --quiet FETCH_HEAD; } > "$log" 2>&1
}

# oracle_error <file> <repo> <sha> <tool> <detail>: a result file for a row that could not be
# compared, so the table shows why.
oracle_error() {
  python3 - "$@" <<'PY'
import json, sys
path, repo, sha, tool, detail = sys.argv[1:6]
document = {"repo": repo, "sha": sha, "tool": tool, "status": "error", "detail": detail, "agrees": False}
open(path, "w").write(json.dumps(document, indent=2) + "\n")
print(f"oracle: {repo}: error: {detail}", file=sys.stderr)
PY
}
