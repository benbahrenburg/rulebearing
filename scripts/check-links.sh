#!/usr/bin/env bash
# Verifies that every relative link between documents, and from a Rust doc comment, resolves:
# the file exists and, when the link carries an anchor, the target has a heading with that slug.
#
# The check itself lives in xtask/src/doclinks.rs so that it also runs on every compile
# (crates/rb-model/build.rs) and in `cargo xtask lint`. This script is the shell entry point
# kept for habit and for continuous integration.
#
# Decision: docs/adr/0023-documentation-link-and-lint-gates.md
# Rule it enforces: docs/adr/0001-record-architecture-decisions.md
# Requirement: docs/prd.md#nfr-doc-01
set -euo pipefail
cd "$(dirname "$0")/.."
exec cargo run --quiet --package xtask -- check-links
