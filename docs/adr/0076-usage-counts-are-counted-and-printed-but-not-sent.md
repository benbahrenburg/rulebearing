# ADR-0076: Usage counts are counted and printed on the machine, and not sent, until there is an endpoint

- **Status:** Proposed (2026-10-11)
- **Date:** 2026-10-11
- **Derives from:** [plan 0004, Step 13](../plans/pending/0004-wave-4-reach.md#25-steps-for-sub-wave-4e-usage-counts-and-the-adoption-review) (a counter under the cache folder, the payload printed, `usage send` or the monthly send, the salted hashes), its [§ 1.5](../plans/pending/0004-wave-4-reach.md#15-architecture-of-what-this-wave-builds) (the payload), [§ 1.7](../plans/pending/0004-wave-4-reach.md#17-decisions-applied-and-decisions-this-wave-must-make) ("Usage counts transport and endpoint") and [§ 1.11](../plans/pending/0004-wave-4-reach.md#111-compliance-and-licence-review) (the published schema and retention); [ADR-0021](0021-agent-surface-cli-first.md) (adoption is measured, and the review reads the counts)
- **Amends:** plan 0004 Step 13 for this release. No send path is built: there is no `usage send`, no `--usage-counts send`, no monthly send and no HTTP client. The Cargo feature `usage-counts` is not added. `rulebearing usage print` is spelled `rulebearing --usage-counts print`, as § 1.7 has it.
- **Constrains:** `crates/rb-cli/src/usage/`, `crates/rb-cli/src/cli.rs`, `crates/rb-config` (`telemetry`), `schema/config-v1.json`, `schema/usage-counts-v1.json`, [docs/usage-counts.md](../usage-counts.md), [architecture § Security posture](../architecture.md#security-posture)
- **Requirements:** [FR-REACH-03](../prd.md#fr-reach-03), [NFR-SEC-01](../prd.md#nfr-sec-01)

## Context

Plan 0004 asks for opt-in, anonymous usage counts: per command and per firing rule, off by default, printed before any send, sent at most once a calendar month by HTTPS POST to "a single documented endpoint under the project's domain", never from CI unless `telemetry.ciAllowed: true`, with the payload schema and the endpoint's retention published on the site.

Two facts decide what can be built now:

- **There is no endpoint and no project domain.** The site is served from GitHub Pages (ADR-0072), which takes no POST. A send path written today would post to an address that does not exist, or to one picked without the owner's decision on hosting, retention and who holds the data.
- **The repository forbids a stand-in.** A send function that does nothing, or a `send` value that is accepted and ignored, is a stub by the working agreement ([CLAUDE.md § Things not to do](../../CLAUDE.md#things-not-to-do)). A feature that cannot be delivered in full is left out and recorded.

Everything except the send does not depend on the endpoint: the opt-in, the counting, the salted hashes, the payload and its schema, the cadence, the CI guard, and the printing.

## Decision

**Build the client up to the send, and leave the send out.** The binary makes no network connection in any configuration; the only change [NFR-SEC-01](../prd.md#nfr-sec-01) sees is a file under `.graph/usage/`.

| Part | What this release does |
| --- | --- |
| Opt-in | `telemetry: { enabled: true }` in the repository's own native configuration file. Off when absent. A file named in `extends` that sets `telemetry` is refused (exit 3), so a shared preset cannot turn counting on for the repositories that use it. A dependency-cruiser file may carry it, with the usual "Rulebearing addition" warning; `config convert --to dependency-cruiser` drops it and says so. |
| What is counted | Each command that reads a configuration turning counting on adds 1 to `commands.<name>`, the top-level subcommand. Each rule with at least one violation in a `cruise` adds 1 to `rules.<key>`: the number of runs in which it fired, not the number of violations. `serve` and `guard --watch`, which run until their input closes, are not counted. |
| Rule keys | A rule whose name is a rule of the public library (every rule in the presets the binary carries, `rulebearing:*` and `dependency-cruiser/configs/*`) is sent by name. Any other name is `sha256:` and the hex SHA-256 of the repository's salt followed by the name. The salt is 32 bytes from the operating system's randomness (the keys `std` seeds `RandomState` with, mixed through SHA-256 with the clock and the process id), written once to `.graph/usage/salt` at the repository root. Counts are comparable within a repository and meaningless across repositories. |
| Storage | `.graph/usage/<YYYY-MM>.json` is the payload of that calendar month, already hashed: no rule name outside the library, no path, no repository name and no user is ever written. `state.json` records the month of the last monthly report. |
| `--usage-counts print` | Prints this month's payload to stdout, exactly the bytes a send would carry, and writes nothing. With no counts it prints the empty payload and says on stderr how counting is turned on. Its only value is `print`; `send` is not offered, so asking for it is a usage error naming the values there are. |
| Monthly cadence | The first counted run in a new calendar month prints the latest closed month's payload on stderr (stdout belongs to the reporter), followed by the statement that sending is not available in this release and nothing was sent, then deletes the closed months' files and records the month in `state.json`. It happens at most once per calendar month, whatever the number of runs. |
| CI guard | When `CI` is set to anything but empty, `false` or `0`, nothing is counted and nothing is reported unless `telemetry.ciAllowed: true`. |
| First use | The run that creates the salt says on stderr that counting is on, where the counts are, how to print them, and that nothing is sent in this release. |
| Errors | A counts file that cannot be read or written is a `warning:` line naming the file; it never changes the exit code. `--usage-counts print` over an unreadable file exits 2 with the file named. |

**Why the opt-in is offered rather than withheld.** The alternative was to leave `telemetry` out of the schema until a send exists. That would leave nothing for the owner, or an early adopter, to inspect before a send is ever built: the payload, the hashing and the cadence could not be reviewed on real repositories, and the schema and retention page would describe a feature nobody can run. Offering the opt-in is honest because it does what it says: it counts, on the machine, and it prints. Every message that mentions the opt-in, the schema description and the docs page state that sending is not available, so no reader can take `enabled: true` for consent to a send that happens.

**The schema and retention page** is [docs/usage-counts.md](../usage-counts.md), published by the site generator with every other guide. It carries the payload's JSON schema (committed as `schema/usage-counts-v1.json`, which a test holds equal to the page) and the retention: nothing is held remotely because nothing is sent; locally, the current month and at most the previous one are kept, and a closed month is deleted once it has been printed. The schema is not added to the site's served schemas, because those are read from the latest release tag (ADR-0072), which does not carry it yet.

**Building the send later is a new ADR**, which names the endpoint and its domain, the retention the endpoint applies (plan 0004 § 1.11 proposes aggregates only and no raw payload kept beyond 30 days), the HTTP client and its feature flag (§ 1.9), and the printed-before-sending test against a mock endpoint (Step 13). It supersedes this one.

## Consequences

- `rulebearing --help` gains `--usage-counts <ACTION>`, and its usage line shows that the top level takes the option or a subcommand. The help snapshot changes for that reason only.
- The binary links no network call. A test reads the built binary's undefined symbols and fails if `connect`, `socket`, `getaddrinfo` or `sendto` appears; another fails if any crate's source names `std::net`, `TcpStream` or `UdpSocket` outside the tests, or if `Cargo.lock` carries an HTTP client.
- The plan 0004 4E row "off by default; printed before sending; CI guard; salted hashes" is met in part: off by default, printed, CI guard and salted hashes are tested; "before sending" has no sending to precede. Its status table records the send as not built, with this ADR.
- The closing adoption review (Step 14) has no remote aggregates to read; a repository that opts in can paste its printed payload.
- `.graph/usage/` sits under `.graph/` at the repository root, beside the cache and the snapshots; a repository that ignores `.graph/` ignores it too.

## Alternatives considered

| Option | Why not |
| --- | --- |
| Build the send against a placeholder URL behind a disabled flag | A stub by the working agreement, and a network path in the binary with no endpoint to justify it. |
| Accept `telemetry.enabled` and `--usage-counts send` and ignore the send | An option accepted and ignored, which the working agreement forbids, and a reader would believe something was sent. |
| Refuse `telemetry.enabled: true` with exit 3 until a send exists | Honest, but leaves nothing to count, so `print` would only ever show an empty payload and the payload, hashing and cadence could not be reviewed on a real repository. |
| Leave the whole feature out | Loses the reviewable client, the schema and the retention page, all of which the endpoint decision needs. |
| Keep the counter in clear text and hash at send time | Rule names would sit on disk in a file meant for sending; hashing at count time means the file on disk is exactly the payload and `print` shows nothing that is not in it. |
| A new random-number dependency for the salt | `std` already seeds `RandomState` from the operating system; 128 bits of it, mixed with the clock and process id through SHA-256, suffices for a salt whose purpose is to make counts incomparable across repositories. |
