#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "╔══════════════════════════════════╗"
echo "║   ZZIGNAL — Deploy Manual       ║"
echo "╚══════════════════════════════════╝"

echo ""
echo "=== 1/3 Pull latest ==="
cd "$SCRIPT_DIR"
git pull

echo ""
echo "=== 2/3 Build Backend ==="
cd "$SCRIPT_DIR/backend_rust"
cargo build --release

cd "$SCRIPT_DIR"

echo ""
echo "=== 3/3 Symlink + Restart ==="
ln -sf backend_rust/target/release/polymarket-backend polymarket-backend
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
