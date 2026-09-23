#!/usr/bin/env bash
# The six adoption signals of NFR-ADOPT-01, for one repository, as one Markdown table row per
# signal. Read-only: it reads a local checkout and the GitHub API through `gh`, and writes nothing
# but stdout.
#
#   1. share of agent-authored merged pull requests whose first run of the gate check passed
#   2. median pushes from a failing gate run to a passing one on those pull requests (the nearest
#      thing to "agent turns" GitHub records)
#   3. agent-authored pull requests that changed the rules and had a failing gate run before merge
#   4. p95 of the Stop hook with --affected: not measurable before wave 3, which adds --affected
#   5. share of rules carrying `fix` (`rulebearing rules --json` in the checkout)
#   6. merged commits that raised a ratchet budget's ceiling (the budget files' git history)
#
# Before a repository switches, the gate is whatever boundary check it already runs (for example
# a dependency-cruiser job); pass its check-run name with --gate. That is the pre-switch baseline
# the design asks for.
#
# Usage: scripts/adoption-signals.sh <checkout> --repo <owner/name> [--gate <check name>]
#          [--since YYYY-MM-DD] [--agents <regex>] [--graph <graph document>]
# --graph is for a repository whose rules run over a saved graph (this one's run over
# scripts/cargo-graph.sh's crate graph).
# Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 21. Requirement: docs/prd.md#nfr-adopt-01.
# Signals: docs/artifacts/design.md#how-to-know-rather-than-believe. Results: docs/adoption.md.
set -euo pipefail

checkout="" repo="" gate="" since="" graph="" agents='copilot|devin|codex|claude|cursor|jules'
while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo) repo="$2"; shift 2 ;;
    --gate) gate="$2"; shift 2 ;;
    --since) since="$2"; shift 2 ;;
    --agents) agents="$2"; shift 2 ;;
    --graph) graph="$2"; shift 2 ;;
    -*) echo "adoption-signals: unknown option $1" >&2; exit 2 ;;
    *) checkout="$1"; shift ;;
  esac
done
if [ -z "$checkout" ] || [ -z "$repo" ]; then
  echo "usage: scripts/adoption-signals.sh <checkout> --repo <owner/name> [--gate <check>] [--since YYYY-MM-DD] [--agents <regex>]" >&2
  exit 2
fi
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
bin="${RULEBEARING_BIN:-$root/target/release/rulebearing}"
[ -x "$bin" ] || (cd "$root" && cargo build --quiet --release -p rb-cli)
since="${since:-$(date -u -d '60 days ago' +%F 2> /dev/null || date -u -v-60d +%F)}"

graph_args=()
[ -n "$graph" ] && graph_args=(--graph "$graph")
rules="$(cd "$checkout" && "$bin" rules --json ${graph_args[@]+"${graph_args[@]}"} 2> /dev/null || echo '{}')"
budgets="$(cd "$checkout" && "$bin" config expand rulebearing.yaml --json 2> /dev/null || echo '{}')"
# Merged pull requests in the window, 40 at a time: with commits and files in one query, a larger
# page exceeds GitHub's GraphQL node limit. A failed query stops the run rather than reporting zero.
pulls="[]"
cursor="$since"
while :; do
  page="$(gh pr list --repo "$repo" --state merged --limit 40 --search "merged:>=$cursor sort:updated-asc" \
    --json number,author,body,files,commits,mergedAt)" || { echo "adoption-signals: gh pr list failed for $repo" >&2; exit 2; }
  pulls="$(PULLS="$pulls" PAGE="$page" python3 -c 'import json, os
seen = {p["number"]: p for p in json.loads(os.environ["PULLS"])}
seen.update({p["number"]: p for p in json.loads(os.environ["PAGE"])})
print(json.dumps(list(seen.values())))')"
  count="$(PAGE="$page" python3 -c 'import json, os; print(len(json.loads(os.environ["PAGE"])))')"
  [ "$count" -lt 40 ] && break
  next="$(PAGE="$page" python3 -c 'import json, os; print(max(p["mergedAt"] for p in json.loads(os.environ["PAGE"]))[:10])')"
  [ "$next" = "$cursor" ] && break
  cursor="$next"
done

RULES="$rules" BUDGETS="$budgets" PULLS="$pulls" REPO="$repo" GATE="$gate" AGENTS="$agents" \
  SINCE="$since" CHECKOUT="$checkout" python3 - <<'PY'
import json, os, re, statistics, subprocess

repo, gate, since = os.environ["REPO"], os.environ["GATE"], os.environ["SINCE"]
agent = re.compile(os.environ["AGENTS"], re.IGNORECASE)
pulls = json.loads(os.environ["PULLS"] or "[]")
rules = json.loads(os.environ["RULES"] or "{}").get("rules", [])
config = json.loads(os.environ["BUDGETS"] or "{}")

def authored_by_agent(pull):
    login = (pull.get("author") or {}).get("login", "")
    trailers = " ".join(c.get("messageBody", "") for c in pull.get("commits", []))
    return bool(agent.search(login)) or bool(re.search(r"Co-Authored-By: .*(Claude|Copilot|Codex)", trailers + (pull.get("body") or ""), re.I))

def gate_runs(pull):
    """The gate's conclusions on each pushed commit, oldest first."""
    out = []
    for commit in pull.get("commits", []):
        sha = commit.get("oid")
        if not sha or not gate:
            continue
        result = subprocess.run(
            ["gh", "api", f"repos/{repo}/commits/{sha}/check-runs", "--jq",
             f'[.check_runs[] | select(.name == "{gate}") | .conclusion] | first'],
            capture_output=True, text=True, check=False)
        conclusion = result.stdout.strip()
        if conclusion and conclusion != "null":
            out.append(conclusion)
    return out

agent_pulls = [p for p in pulls if authored_by_agent(p)]
first_pass, to_green, rule_catches = [], [], 0
for pull in agent_pulls:
    runs = gate_runs(pull)
    if runs:
        first_pass.append(runs[0] == "success")
    if "failure" in runs:
        failed = runs.index("failure")
        green = next((i for i in range(failed, len(runs)) if runs[i] == "success"), None)
        if green is not None:
            to_green.append(green - failed)
        touched = [f.get("path", "") for f in pull.get("files", [])]
        if any(re.search(r"(^|/)(rulebearing\.ya?ml|\.dependency-cruiser\.[cm]?js(on)?)$", t) for t in touched):
            rule_catches += 1

with_fix = sum(1 for r in rules if r.get("fix"))
raised = 0
for ratchet in (config.get("rules") or {}).get("ratchets", []):
    budget = ratchet.get("budget")
    log = subprocess.run(["git", "-C", os.environ["CHECKOUT"], "log", f"--since={since}", "-p", "--format=%H", "--", budget],
                         capture_output=True, text=True, check=False).stdout
    for removed, added in re.findall(r'^-\s*"ceiling":\s*(\d+).*?^\+\s*"ceiling":\s*(\d+)', log, re.M | re.S):
        raised += int(added) > int(removed)

def share(values):
    return f"{100 * sum(values) / len(values):.0f}% of {len(values)}" if values else "no gate runs"

print(f"| Signal | {repo}, merged since {since} |")
print("| --- | --- |")
print(f"| Agent-authored PRs whose first gate run passed | {share(first_pass) if gate else 'no gate check named'} ({len(agent_pulls)} agent-authored of {len(pulls)} merged) |")
print(f"| Median pushes from a failing gate run to green | {statistics.median(to_green) if to_green else 'none failed'} |")
print(f"| Agent rule changes caught by the gate before merge | {rule_catches} |")
print("| p95 of the Stop hook with --affected | not measurable before wave 3 (--affected) |")
print(f"| Rules carrying fix | {f'{100 * with_fix / len(rules):.0f}% of {len(rules)}' if rules else 'no Rulebearing rules yet'} |")
print(f"| Merged budget raises | {raised} |")
PY
