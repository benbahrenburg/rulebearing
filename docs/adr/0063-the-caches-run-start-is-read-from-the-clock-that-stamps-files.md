# ADR-0063: The cache's run start is also read from the clock that stamps files

- **Status:** Proposed (2026-10-09)
- **Date:** 2026-10-09
- **Derives from:** [plan 0003, Steps 1 and 2](../plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict) (the cache: an input modified during a run is recorded unsettled and read again by the next)
- **Supersedes:** nothing. It replaces how the start that rule compares with is read.
- **Constrains:** `crates/rb-cli/src/cache/changes.rs` (`Start`, `record`, `after_start`), `crates/rb-cli/src/cache/mod.rs`
- **Implemented by:** [plan 0003, 3G](../plans/pending/0003-wave-3-operations-surface-inner-loop.md#wave-3g-the-rule-library-the-scale-table-adoption-action-5), the follow-up from 3A on a coarse file clock
- **Requirements:** [FR-CLI-05](../prd.md#fr-cli-05)

## Context

A cached run records each input's digest and stamp after its extraction. An input whose modification time is after the run started is recorded as unsettled, so the next run reads it again rather than trusting a digest taken before the edit. The start was read from the system clock, to the nanosecond.

Windows stamps a file with a clock that ticks about every 16 ms. A file edited a few milliseconds after the run started can therefore carry a time a few milliseconds before the start. It is recorded settled, with the digest of the content the run read before the edit. The next run sees the stamp unchanged and serves the stale extraction.

Pull request 53's Windows job found this. The unit test was then made to edit 50 ms after the start, so that it would not depend on the tick. The plan recorded that the fix needed a design: a slack on the comparison closes the gap, but it also marks every file written within the slack before a run as unsettled. Five cache tests that write and then run at once would then miss.

## Decision

**The start is read twice: by the system clock, and by the clock that stamps files.** When a run starts, before it looks at anything, it writes a probe file of its own in the system's temporary folder, reads the file's modification time and removes it (`Start::now`).

An input counts as modified during the run when any of these holds:
- its time is after the system clock's start, as before;
- its time is at or after the probe's time;
- its time is a whole second within two seconds before the start, as before, for file systems that keep whole seconds.

Same clock, same tick: an edit after the start can never carry a time earlier than a file written at the start. An edit stamped in the start's own tick shares the probe's time and is read again, since it cannot be told apart from one made just before the start.

**If the probe cannot be written, the system clock alone decides, as before.**

## Consequences

- On Windows, a file written in the same tick as a run's start, at most about 16 ms before it, is read again by the next run. It is never trusted on a stamp that could be stale. A file written earlier than that is settled, as before. On Linux and macOS, whose files carry nanosecond times, the probe's time is effectively the start.
- The unit test edits at once, with no wait, so the Windows job checks the case this decision closes.
- Each cached run writes and removes one small file in the temporary folder.
- The probe measures the system's clock as file times use it. A file system that keeps whole seconds is still covered by the whole-second rule, wherever the probe is written.

## Alternatives considered

| Option | Why not |
| --- | --- |
| A slack: count anything within 16 ms (or a second) before the start as unsettled | Over-reads every file written in the slack window, and fails the cache tests that write and run at once. The probe narrows the window to the one tick that cannot be told apart. |
| Probe inside the cache folder | Adds a file to every user's cache folder that a listing would show (the ESLint plugin's test counts that folder's entries). The temporary folder carries the same clock. |
| Re-hash every input at record time and compare with the run's read | Doubles the hashing of every cached run to close a window of milliseconds. |
