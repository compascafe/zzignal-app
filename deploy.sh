#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "╔══════════════════════════════════╗"
echo "║   ZZIGNAL — Deploy Manual       ║"
echo "╚══════════════════════════════════╝"

echo ""
echo "=== 1/4 Pull latest ==="
cd "$SCRIPT_DIR"
git pull

echo ""
echo "=== 2/4 Build Backend ==="
cd "$SCRIPT_DIR/backend_rust"
cargo build --release

echo ""
echo "=== 3/4 Build Monitor (TUI) ==="
cd "$SCRIPT_DIR/monitor"
cargo build --release

cd "$SCRIPT_DIR"

echo ""
echo "=== 4/4 Symlink + Restart ==="
ln -sf backend_rust/target/release/polymarket-backend polymarket-backend
ln -sf monitor/target/release/zzignal-monitor zzignal-monitor
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
