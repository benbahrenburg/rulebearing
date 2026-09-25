#!/usr/bin/env bash
# Fetches the dependency-cruiser configuration of every oracle row in testbeds/manifest.yaml, at
# the row's pinned SHA, into target/oracle-configs/<owner>__<repo>/<path>, for
# crates/rb-config/tests/oracle_configs.rs. Nothing fetched is committed (testbeds/manifest.yaml:
# "every repository is a read-only test bed"). For dependency-cruiser itself the bundled configs/
# folder its configuration extends is fetched too.
# Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 1 ("every manifest config loads").
# Usage: scripts/fetch-oracle-configs.sh
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
out=target/oracle-configs
mkdir -p "$out"
python3 - "$out" <<'PY'
import os, re, subprocess, sys
out = sys.argv[1]
rows = [line for line in open("testbeds/manifest.yaml") if "tool: dependency-cruiser" in line]
extra = {
    "sverweij/dependency-cruiser": [
        "configs/recommended.cjs", "configs/recommended-strict.cjs",
        "configs/rules/no-circular.cjs", "configs/rules/no-deprecated-core.cjs",
        "configs/rules/no-duplicate-dependency-types.cjs", "configs/rules/no-non-package-json.cjs",
        "configs/rules/no-orphans.cjs", "configs/rules/not-to-deprecated.cjs",
        "configs/rules/not-to-unresolvable.cjs", "package.json",
    ],
}
for line in rows:
    repo = re.search(r"repo: ([^,]+)", line).group(1)
    sha = re.search(r"sha: ([0-9a-f]+)", line).group(1)
    config = re.search(r'config: "?([^",}]+)"?', line).group(1).strip()
    base = os.path.join(out, repo.replace("/", "__"))
    os.makedirs(os.path.join(base, ".git"), exist_ok=True)
    for path in [config] + extra.get(repo, []):
        target = os.path.join(base, path)
        os.makedirs(os.path.dirname(target), exist_ok=True)
        url = f"https://raw.githubusercontent.com/{repo}/{sha}/{path}"
        result = subprocess.run(["curl", "-sfL", "-o", target, url])
        print(f"fetch-oracle-configs: {repo} {path} {'ok' if result.returncode == 0 else 'FAILED'}")
        if result.returncode != 0:
            sys.exit(1)
PY
