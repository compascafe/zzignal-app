#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=== Building ZZIGNAL Backend ==="
cd "$SCRIPT_DIR/backend_rust"
cargo build --release

echo ""
echo "=== Building ZZIGNAL Monitor (TUI) ==="
cd "$SCRIPT_DIR/monitor"
cargo build --release

echo ""
echo "=== Build complete ==="
echo "Backend:  ./backend_rust/target/release/polymarket-backend"
echo "Monitor:  ./monitor/target/release/zzignal-monitor"
