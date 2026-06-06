#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "╔══════════════════════════════════╗"
echo "║   ZZIGNAL — Deploy Manual       ║"
echo "╚══════════════════════════════════╝"

echo ""
echo "=== 1/3 Pull latest ==="
cd "$PROJECT_ROOT"
git pull

echo ""
echo "=== 2/3 Build Backend ==="
cd "$PROJECT_ROOT/backend_rust"
cargo build --release

echo ""
echo "=== 3/3 Build Monitor ==="
cd "$PROJECT_ROOT/TUI_monitor"
# Touch build.rs to force re-run and embed latest git hash in TUI commit bar
touch build.rs
cargo build --release

echo ""
echo "=== 4/4 Symlink + Restart ==="
cd "$PROJECT_ROOT"
ln -sf backend_rust/target/release/polymarket-backend polymarket-backend
ln -sf TUI_monitor/target/release/zzignal-monitor zzignal-monitor
sudo systemctl restart zzignal-app

sleep 5
if curl -sf http://localhost:8080/api/status > /dev/null 2>&1; then
    echo ""
    echo "✅ DEPLOY OK — $(curl -s http://localhost:8080/api/status)"
else
    echo ""
    echo "❌ DEPLOY FAIL — service not healthy"
    sudo journalctl -u zzignal-app --no-pager -n 15
    exit 1
fi
