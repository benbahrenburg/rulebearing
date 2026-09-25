This adds an architecture gate that runs the rules in `./.dependency-cruiser.cjs` with Rulebearing 0.1.0. The rules are unchanged, and the gate is green today.

3 current findings are baselined in `rulebearing.yaml` (`options.knownViolations`). Each entry names an owner and expires; the gate fails for any still present after that.

| Rule | Baselined |
| --- | --- |
| `a-not-to-b` | 2 |
| `no-circular` | 1 |

Files:

- `rulebearing.yaml`
- `.githooks/pre-commit`
- `.github/workflows/rulebearing.yml`
- `docs/architecture/rulebearing.md`

To see why a rule exists and what to do when it fires: `rulebearing explain <rule>`.
