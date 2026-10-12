# Usage counts: the payload, the schema and the retention

Usage counts tell the project which commands are run and which rules fire, so the next cookbook page and the next fix go where they are used ([design § The developer relations hat](artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo); [FR-REACH-03](prd.md#fr-reach-03)). They are off unless a repository turns them on, and **nothing is sent in this release**: there is no endpoint yet, so the binary counts on the machine, prints what it counted, and makes no network connection in any configuration ([ADR-0075](adr/0075-usage-counts-are-counted-and-printed-but-not-sent.md); [architecture § Security posture](architecture.md#security-posture); [NFR-SEC-01](prd.md#nfr-sec-01)).

## Turning counting on

Counting is off by default. A repository turns it on in its own native configuration file:

```yaml
telemetry:
  enabled: true      # count, per calendar month, under .graph/usage/
  ciAllowed: false   # the default: a CI run counts nothing
```

| Key | Default | What it does |
| --- | --- | --- |
| `telemetry.enabled` | `false` | Each command that reads this configuration is counted, and each rule that reports a violation in a `cruise` |
| `telemetry.ciAllowed` | `false` | Without it, a run where `CI` is set (to anything but empty, `false` or `0`) counts nothing and reports nothing |

Only the repository's own file can set `telemetry`: a file named in `extends` that sets it is refused with exit 3, so a shared preset cannot turn counting on for the repositories that use it. A `.dependency-cruiser.*` file may carry the key, with the warning every Rulebearing addition gets, and `config convert --to dependency-cruiser` drops it and says so.

The run that starts counting in a repository says so on stderr, with where the counts are and how to print them.

## What is counted

| Counter | Adds 1 when |
| --- | --- |
| `commands.<name>` | a top-level subcommand (`cruise`, `can-import`, `config`, and the rest) runs and reads a configuration that turns counting on. `serve` and `guard --watch`, which run until their input closes, are not counted |
| `rules.<key>` | a rule reports at least one violation in a `cruise`; a run adds 1 however many violations the rule has |

A rule's key is its name when the rule is one of the public library's: the rules in the presets the binary carries (`rulebearing:recommended`, `rulebearing:typescript`, `rulebearing:dotnet`, `rulebearing:python`, the framework presets and `dependency-cruiser/configs/*`). Any other rule is keyed `sha256:` followed by the hex SHA-256 of the repository's salt and then its name. The salt is 32 random bytes written once to `.graph/usage/salt` at the repository root, so a count is comparable with the same repository's next month and with nothing else.

There is no path, no module name, no repository name, no user, no host and no time finer than the month.

## Seeing the counts

```sh
rulebearing --usage-counts print
```

prints this month's payload to stdout: exactly the bytes a send would carry. It writes nothing. With no counts it prints the empty payload, and says on stderr how counting is turned on.

```json
{
  "tool": "rulebearing",
  "version": "0.4.0",
  "period": "2026-11",
  "commands": {
    "can-import": 3891,
    "cruise": 412
  },
  "rules": {
    "no-circular": 3,
    "sha256:9f2c4e0b8a6d1f3e5c7b9a0d2e4f6a8c0b1d3e5f7a9c2b4d6e8f0a1c3e5b7d9f": 14
  }
}
```

`print` is the only value `--usage-counts` takes in this release; there is no `send`.

## The monthly report

The first counted run in a new calendar month prints the latest closed month's payload on stderr (stdout belongs to the reporter), followed by the statement that sending is not available in this release and nothing was sent. It then deletes the closed months' files and records the month in `.graph/usage/state.json`, so the report appears at most once a month however many runs there are. A CI run reports nothing unless `telemetry.ciAllowed: true`.

## Retention

| Where | What is kept | For how long |
| --- | --- | --- |
| Remotely | Nothing: nothing is sent in this release | not applicable |
| `.graph/usage/<YYYY-MM>.json` | the payload of that month, already hashed | the current month, and a closed month until the next month's first counted run prints it; then it is deleted |
| `.graph/usage/salt` | the repository's salt | until the folder is deleted; a new salt makes later counts incomparable with earlier ones |
| `.graph/usage/state.json` | the month of the last monthly report | as the salt |

A send, when one is built, is a new decision record that names the endpoint, its domain and what the endpoint keeps; until then this table is the whole of the retention.

## The payload schema

The payload is valid against this schema, committed as `schema/usage-counts-v1.json`. A test holds the page and the file equal and validates every payload the tests produce against it.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Rulebearing usage counts",
  "description": "One calendar month of one repository's usage counts: what `rulebearing --usage-counts print` shows. No path, no repository name and no user; a rule outside the public library appears only as a salted hash. Sending is not available in this release (ADR-0075).",
  "type": "object",
  "additionalProperties": false,
  "required": ["tool", "version", "period", "commands", "rules"],
  "properties": {
    "tool": {
      "description": "Always `rulebearing`.",
      "const": "rulebearing"
    },
    "version": {
      "description": "The version of the binary that last counted into this month.",
      "type": "string",
      "pattern": "^[0-9]+\\.[0-9]+\\.[0-9]+([-+][0-9A-Za-z.+-]+)?$"
    },
    "period": {
      "description": "The calendar month counted, `YYYY-MM`.",
      "type": "string",
      "pattern": "^[0-9]{4}-(0[1-9]|1[0-2])$"
    },
    "commands": {
      "description": "Runs per top-level subcommand that read a configuration turning counting on.",
      "type": "object",
      "propertyNames": { "pattern": "^[a-z][a-z-]*$" },
      "additionalProperties": { "type": "integer", "minimum": 1 }
    },
    "rules": {
      "description": "Per rule, the `cruise` runs in which it reported at least one violation. A rule of the public library is keyed by its name; any other by `sha256:` and the hex SHA-256 of the repository's salt followed by its name.",
      "type": "object",
      "propertyNames": { "pattern": "^(sha256:[0-9a-f]{64}|[a-z0-9][a-z0-9-]*)$" },
      "additionalProperties": { "type": "integer", "minimum": 1 }
    }
  }
}
```
