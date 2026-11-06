#!/usr/bin/env bash
# Run every check CI runs. Stops at the first failure.
set -euo pipefail
export MSYS_NO_PATHCONV=1
cd "$(dirname "$0")/.."

python scripts/release_notes.py
(cd app && npx tsc --noEmit && npx vite build)
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release -p pavlov-mod-studio
python tests/upload_flow.py
python tests/manage_flow.py
echo "All checks passed"
