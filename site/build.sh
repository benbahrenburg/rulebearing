#!/usr/bin/env bash
# Builds the docs site into target/site (or the folder given): rb-wasm compiled for
# wasm32-unknown-unknown and bound for the browser with wasm-bindgen, as the `wasm` CI job builds
# it; the generator and the page scripts compiled; then every tracked document rendered, the
# released schemas added and the link check run over the result. A broken link fails the build.
#
#   site/build.sh [out-dir]          # RB_SCHEMA_REF=vX.Y.Z picks the schemas' release tag;
#                                    # by default the highest vX.Y.Z tag in the checkout
#
# Needs: the wasm32-unknown-unknown target, wasm-bindgen 0.2.128 (the version the workspace pins),
# Node 22 and `npm ci` at the repository root.
# Decision: docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md.
# Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3. Requirements: FR-REACH-01, NFR-DOC-01.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="${1:-$root/target/site}"
wasm_out="$root/target/site-wasm"

cargo build --manifest-path "$root/Cargo.toml" --release -p rb-wasm --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir "$wasm_out" \
  "$root/target/wasm32-unknown-unknown/release/rb_wasm.wasm"

npm --prefix "$root/site" run build

args=(build --repo "$root" --out "$out" --wasm "$wasm_out" --web "$root/site/dist/web")
if [[ -n "${RB_SCHEMA_REF:-}" ]]; then
  args+=(--schema-ref "$RB_SCHEMA_REF")
fi
node "$root/site/dist/node/main.js" "${args[@]}"
