#!/usr/bin/env bash
# Run the workspace topology tool against this repo; arguments pass through.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec cargo run --release --quiet --manifest-path "$root/tools/topology/Cargo.toml" -- \
  --workspace "$root" --config "$root/tools/topology/topology.toml" \
  --out "$root/tools/topology/out" "$@"
