# ADR-0083: `chrono` stays while it is unmaintained and not vulnerable, and its advisory is ignored by id

- **Status:** Accepted (2026-11-22, by the owner)
- **Date:** 2026-11-22
- **Derives from:** [ADR-0025](0025-ci-and-supply-chain-hardening.md) (`cargo deny check licenses advisories bans sources` is a required check), [ADR-0079](0079-yaml-is-parsed-with-serde-saphyr.md) (the last unmaintained-crate decision, which replaced the crate)
- **Constrains:** `deny.toml` (`[advisories] ignore`), `Cargo.toml` (`[workspace.dependencies] chrono`)
- **Requirements:** [NFR-SEC-01](../prd.md#nfr-sec-01)

## Context

On 2026-11-22 the RustSec database published [RUSTSEC-2026-0543](https://rustsec.org/advisories/RUSTSEC-2026-0543): `chrono` is unmaintained, every version, with `jiff` and `time` named as replacements. The advisory is informational. It records that nobody maintains the crate; it names no vulnerability, and the crate's own announcement ([chronotope/chrono#1768](https://github.com/chronotope/chrono/issues/1768)) names none either.

The `deny` job fetches the database on every run, so from that day every pull request and every nightly fails on `advisories`, including branches whose last green run was hours earlier ([run 45019581473](https://github.com/benbahrenburg/rulebearing/actions/runs/45019581473) on the nightly's coverage-tables branch).

`chrono` is a direct dependency of `rb-config`, `rb-rules`, `rb-cli` and `rb-wasm`, with `default-features = false` and the `clock`, `std` and `serde` features. What the workspace takes from it:

| Use | Where |
| --- | --- |
| `NaiveDate`: the `expires` date of a known violation or an exception, the `today` an evaluation compares it with, the clock a test pins | `rb-config` (the configuration model, `elements`), `rb-rules` (`evaluate`, `known`), every command context in `rb-cli`, the playground in `rb-wasm` |
| `Days`: the expiry `adopt` writes, a fixed count of days ahead | `rb-cli` `adopt` |
| `DateTime::from_timestamp` and `Utc::now`: today's date from `SOURCE_DATE_EPOCH` or the system clock | `rb-cli` `context` |

Each is civil-date arithmetic over ISO-8601 strings. None reaches a time zone database, a parser of free-form text, or any code path the advisory's alternatives would change in behaviour.

[ADR-0079](0079-yaml-is-parsed-with-serde-saphyr.md) replaced `serde_yaml` when `unsafe-libyaml` was reported unmaintained. That case was different in two ways: the unmaintained code was `unsafe` and parsed untrusted input, and the replacement was contained in one module. Here the crate is safe Rust, the input is a date the repository's own configuration carries, and the type is in the public shape of `rb-config` and in forty-odd call sites.

## Decision

- `chrono` stays. The workspace pin and the feature set do not change.
- `deny.toml` ignores RUSTSEC-2026-0543 by id, with a `reason` that names this ADR. Nothing else is ignored; `[advisories] ignore` holds this one entry.
- The entry is removed, and the dependency with it, when either of these happens, whichever is first:
  - a vulnerability advisory is published against `chrono`, in which case the replacement is the next change on `main`, as [ADR-0079](0079-yaml-is-parsed-with-serde-saphyr.md) was;
  - a plan step changes the date model of `rb-config` for its own reasons, in which case the step moves the dates to `jiff` (`jiff::civil::Date`, the same ISO-8601 text, no time-zone data compiled in) rather than touching `chrono` again.
- The ADR that records the replacement supersedes this one.

## Consequences

- The `deny` job is green again on every branch without a change to the code, and the ratchet on ignored advisories stays at one, with the reason beside it.
- A reader of `deny.toml` finds the decision and its exit condition from the entry.
- The project carries a dependency nobody maintains. The exposure is a crate of safe Rust doing calendar arithmetic; a future vulnerability in it is the first trigger above.

## Alternatives considered

- **Replace `chrono` with `jiff` now.** Rejected for this change: it is a refactor across four crates and the public type of the configuration model, with no behavioural need behind it today, and it would ride in a pull request whose purpose is a dependency batch. It is the recorded end state, under the triggers above.
- **Replace it with `time`.** Rejected: `time`'s `Date` has the same reach as `jiff`'s and a less direct serde story for the ISO-8601 forms the configuration uses; if the move happens, `jiff` is the target.
- **Turn off the `unmaintained` check for the workspace.** Rejected: it would hide the next unmaintained crate as well. An ignore by id names exactly one.
- **Leave `deny` red until the migration lands.** Rejected: a required check that fails for every branch stops unrelated work, and a red check nobody reads stops being a check.
