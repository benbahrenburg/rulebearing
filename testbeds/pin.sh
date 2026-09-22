#!/usr/bin/env bash
# Re-pins every row of manifest.yaml to the head of its repository's default branch.
# Plan: docs/plans/pending/0000-wave-0-spike.md, Step 7 item 1. Commit the diff by pull request;
# the nightly table then compares against the new SHAs.
# Usage: testbeds/pin.sh [repo ...]   (default: every row)
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
repos=("$@")
if [ "$#" -eq 0 ]; then
  while IFS= read -r repo; do repos+=("$repo"); done < <(python3 -c 'import yaml; print("\n".join(r["repo"] for r in yaml.safe_load(open("manifest.yaml"))["rows"]))')
fi
for repo in "${repos[@]}"; do
  sha="$(git ls-remote "https://github.com/$repo.git" HEAD | cut -f1)"
  if [ -z "$sha" ]; then
    echo "pin: $repo: could not resolve HEAD (private, renamed or gone); left unchanged" >&2
    continue
  fi
  python3 - "$repo" "$sha" <<'PY'
import re, sys
repo, sha = sys.argv[1], sys.argv[2]
text = open("manifest.yaml").read()
pattern = re.compile(r"(repo: " + re.escape(repo) + r", sha: )[0-9a-f]{40}")
text, count = pattern.subn(lambda m: m.group(1) + sha, text)
if count != 1:
    sys.exit(f"pin: {repo} appears {count} times in manifest.yaml")
open("manifest.yaml", "w").write(text)
PY
  echo "pin: $repo $sha"
done
