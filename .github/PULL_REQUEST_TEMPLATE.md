<!--
The definition of done lives in CLAUDE.md. This template is that checklist, so a reviewer, human
or agent, can see it was followed rather than assumed.
-->

## What and why

<!-- One paragraph. What changes, and which decision or requirement it serves. -->

## Traceability

- Plan and sub-wave:
- Requirement IDs (`docs/prd.md`):
- ADRs applied:

## Checks

- [ ] `cargo lint` clean: doc links, rustfmt, clippy, eslint, prettier, ruff, mypy, dotnet format
- [ ] `cargo test --workspace --all-features` and the doc tests pass
- [ ] Coverage at or above 70% for every crate touched
- [ ] Mutation testing reports no new surviving mutants in `rb-model`, `rb-rules` or `xtask`
- [ ] Conformance ratchets did not grow (`conformance/excluded.json`, the unported count)
- [ ] New public items carry doc comments with links; new crates carry the linked header
- [ ] No `unsafe`, no `unwrap`/`expect`/`panic` outside tests, no network, no code execution outside the sandbox
- [ ] A decision made here has an ADR; a plan whose exit criteria are all met has been moved

## Deliberate exceptions

<!-- A widened pattern, a baselined violation, a skipped check: name it here with the reason and
     an expiry. "None" is the usual answer. -->
