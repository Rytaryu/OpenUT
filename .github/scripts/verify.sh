#!/usr/bin/env bash
# The checks every PR must pass (docs/project/roles/README.md, section 6).
# CI runs exactly this script; run it before opening a PR:
#
#   bash .github/scripts/verify.sh
#
# The Rust toolchain is pinned in rust-toolchain.toml (rustup installs it on
# first use). The tests of the hand-off scripts need Node.js 22 or later, bash
# and jq; they are skipped when `node` is not installed (CI always runs them,
# on Linux).
set -euo pipefail
cd "$(dirname "$0")/../.."

step() { echo "##verify-step $1"; }

step "cargo fmt --check"
cargo fmt --all --check
step "cargo clippy"
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
step "cargo test"
cargo test --workspace --all-features --locked

if [[ "${OPENUT_SKIP_SCRIPT_TESTS:-}" == "1" ]]; then
  echo "skipped the tests of .github/scripts (OPENUT_SKIP_SCRIPT_TESTS=1; CI runs them in their own job)"
elif command -v node >/dev/null 2>&1; then
  step "node --test (.github/scripts)"
  node --test .github/scripts/*.test.mjs
else
  echo "node is not installed: skipped the tests of .github/scripts"
fi
echo "##verify-ok"
