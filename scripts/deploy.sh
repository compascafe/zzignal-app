#!/usr/bin/env bash
# ZZignal — manual deploy: pull, build backend + TUI, restart systemd service.
#
# Intended to run on the server (repo checked out at ~/zzignal-app, systemd
# unit named `zzignal-app`). GitHub Actions performs the same steps in CI.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "╔══════════════════════════════════╗"
echo "║   ZZIGNAL — Manual Deploy        ║"
echo "╚══════════════════════════════════╝"

cd "$PROJECT_ROOT"

echo "[1/3] Pull latest..."
git pull

echo "[2/3] Build backend (release)..."
(cd backend_rust && cargo build --release)

echo "[2/3] Build monitor (release)..."
(cd TUI_monitor && cargo build --release)

echo "[3/3] Symlinks + restart..."
ln -sf backend_rust/target/release/polymarket-backend polymarket-backend
ln -sf TUI_monitor/target/release/zzignal-monitor zzignal-monitor
sudo systemctl restart zzignal-app

sleep 5
if curl -sf http://localhost:8080/api/health > /dev/null 2>&1; then
    echo "DEPLOY OK — $(curl -s http://localhost:8080/api/health)"
else
    echo "DEPLOY FAIL — service not healthy"
    sudo journalctl -u zzignal-app --no-pager -n 20
    exit 1
fi
