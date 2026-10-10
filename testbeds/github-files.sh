#!/usr/bin/env bash
# Sourced by every test-bed script (testbeds/lib.sh, testbeds/run.sh), never run.
#
# A test bed's build and tests are another project's code. On a GitHub runner they inherit the
# step's file commands ($GITHUB_ENV, $GITHUB_PATH, $GITHUB_OUTPUT, $GITHUB_STATE,
# $GITHUB_STEP_SUMMARY), so they could change the environment and outputs of the job's later
# steps. Umbraco-CMS's build did: Nerdbank.GitVersioning writes its version variables to
# $GITHUB_ENV when it sees GitHub Actions, the runner refused a line of it ("Invalid format
# '31-rc+e81538b'"), and the init job failed after its proof had passed (nightly run 38021235710).
# Each file command is pointed at a throwaway file for the whole script. No test-bed script
# writes one itself; the workflow's own steps keep theirs.
#
# Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 25 (the greenfield
# rows). Decision: docs/adr/0025-ci-and-supply-chain-hardening.md (least privilege).
if [ -n "${GITHUB_ACTIONS:-}" ]; then
  rb_github_files="$(mktemp -d)"
  export GITHUB_ENV="$rb_github_files/env" GITHUB_PATH="$rb_github_files/path" \
    GITHUB_OUTPUT="$rb_github_files/output" GITHUB_STATE="$rb_github_files/state" \
    GITHUB_STEP_SUMMARY="$rb_github_files/summary"
  touch "$GITHUB_ENV" "$GITHUB_PATH" "$GITHUB_OUTPUT" "$GITHUB_STATE" "$GITHUB_STEP_SUMMARY"
fi
