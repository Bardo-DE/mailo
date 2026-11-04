#!/usr/bin/env bash
# The dependency boundary: scripts/boundary.toml lists, for every workspace crate, the workspace
# crates it may depend on, and this compares the list with `cargo metadata`. A new edge is a
# deliberate edit to the table; an entry that no longer exists is a failure too, so the table
# cannot go stale. The same file holds the few things a crate may never reach (tokio from a pure
# crate, dioxus from the core), followed through every chain of dependencies.
#
# The greps over the sources that are not about dependencies are scripts/check-source.sh.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 2

meta=$(cargo metadata --format-version 1 --no-deps --locked 2>&1) || {
  echo "cargo metadata failed:"
  echo "$meta" | tail -20
  exit 1
}
# Python 3.11+ (tomllib). CI has no uv, only python3.
if command -v uv >/dev/null 2>&1; then py=(uv run --no-project python -I); else py=(python3 -I); fi
printf '%s' "$meta" | "${py[@]}" scripts/boundary.py scripts/boundary.toml
