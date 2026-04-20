#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=== Building Polymarket Backend ==="

cd "$SCRIPT_DIR/backend_rust"
cargo build --release

echo ""
echo "=== Build complete ==="
echo "Run: ./backend_rust/target/release/polymarket-backend"
