# ═══════════════════════════════════════════════
# ZZIGNAL Aliases — pegar en ~/.bashrc o ~/.zshrc
# Uso: source ~/zzignal_aliases.sh
# ═══════════════════════════════════════════════

export ZZ_API="http://localhost:8080"

# ── Trading ──────────────────────────────────
alias zz-live-on='curl -sX POST $ZZ_API/api/odiseo/live -H "Content-Type: application/json" -d "{\"enable\": true}"'
alias zz-live-off='curl -sX POST $ZZ_API/api/odiseo/live -H "Content-Type: application/json" -d "{\"enable\": false}"'
alias zz-balance='curl -s $ZZ_API/api/balance'
alias zz-btc='curl -s $ZZ_API/api/btc'

# ── Odiseo 83 (variant 0) ─────────────────────
alias zz-odi='curl -s $ZZ_API/api/odiseo/status | python3 -m json.tool'
alias zz-odi83-on='curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d "{\"index\": 0, \"enable\": true}"'
alias zz-odi83-off='curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d "{\"index\": 0, \"enable\": false}"'
alias zz-odi83-8='curl -sX POST $ZZ_API/api/odiseo/budget -H "Content-Type: application/json" -d "{\"index\": 0, \"amount\": 8}"'

# ── Houdini 65 (variant 1) ────────────────────
alias zz-houdini='curl -s $ZZ_API/api/odiseo/status | python3 -c "import sys,json;d=json.load(sys.stdin);[print(v) for v in d[\"variants\"] if \"Houdini\" in v[\"name\"]]"'
alias zz-hdn65-on='curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d "{\"index\": 1, \"enable\": true}"'
alias zz-hdn65-off='curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d "{\"index\": 1, \"enable\": false}"'
alias zz-hdn65-8='curl -sX POST $ZZ_API/api/odiseo/budget -H "Content-Type: application/json" -d "{\"index\": 1, \"amount\": 8}"'

# Apagar todas las variantes menos la 0 (Odiseo 83)
zz-odi-only83() {
  curl -sX POST $ZZ_API/api/odiseo/variant \
      -H "Content-Type: application/json" \
      -d '{"index": 1, "enable": false}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/variant \
    -H "Content-Type: application/json" \
    -d '{"index": 0, "enable": true}' > /dev/null
  echo "✅ Solo Odiseo 83 activa"
}

# Apagar todas las variantes menos la 1 (Houdini 65)
zz-hdn-only() {
  curl -sX POST $ZZ_API/api/odiseo/variant \
      -H "Content-Type: application/json" \
      -d '{"index": 0, "enable": false}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/variant \
    -H "Content-Type: application/json" \
    -d '{"index": 1, "enable": true}' > /dev/null
  echo "✅ Solo Houdini 65 activa"
}

# Reinvertir ON/OFF
zz-reinv-on()  { curl -sX POST $ZZ_API/api/odiseo/reinvest -H "Content-Type: application/json" -d '{"enable": true}'; }
zz-reinv-off() { curl -sX POST $ZZ_API/api/odiseo/reinvest -H "Content-Type: application/json" -d '{"enable": false}'; }

# ── EMERGENCIA ─────────────────────────────────
# PANIC: cancela todo + market sell
alias zz-panic='curl -sX POST $ZZ_API/api/panic -H "Content-Type: application/json" -d "{}" && echo "🚨 PANIC ejecutado"'

# PANIC + apagar LIVE
zz-emergency() {
  echo "🚨 EMERGENCIA TOTAL"
  curl -sX POST $ZZ_API/api/panic -H "Content-Type: application/json" -d "{}" > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/live -H "Content-Type: application/json" -d '{"enable": false}' > /dev/null
  for i in 0 1; do
    curl -sX POST $ZZ_API/api/odiseo/variant \
      -H "Content-Type: application/json" \
      -d "{\"index\": $i, \"enable\": false}" > /dev/null
  done
  echo "✅ TODO APAGADO."
}

# ── Monitoreo ──────────────────────────────────
alias zz-log='sudo journalctl -u zzignal-app --no-pager -n 30 | grep -E "ENTER|EXIT|Order result|Odiseo|error"'
alias zz-log-f='sudo journalctl -u zzignal-app -f | grep --line-buffered -E "ENTER|EXIT|Order result|Odiseo|error"'
alias zz-monitor='cd /home/ubuntu/zzignal-app && ./zzignal-monitor'
alias zz-monitor-paper='cd /home/ubuntu/zzignal-app && ./zzignal-monitor --paper'
alias zz-deploy='cd /home/ubuntu/zzignal-app && ./scripts/deploy.sh'
alias zz-orders='curl -s $ZZ_API/api/orders | python3 -m json.tool'
alias zz-fills='curl -s $ZZ_API/api/fills | python3 -m json.tool'

# ── Status ─────────────────────────────────────
alias zz-status='curl -s $ZZ_API/api/status'
alias zz-health='curl -s $ZZ_API/api/health'
alias zz-restart='sudo systemctl restart zzignal-app && sleep 15 && curl -s $ZZ_API/api/status'

# ── Quick resume ───────────────────────────────
zz-resume() {
  echo "📊 ZZIGNAL Status"
  echo "══════════════════════════════════"
  echo -n "Backend: "; zz-status
  echo -n "Balance: "; zz-balance
  echo -n "BTC:     "; zz-btc
  echo ""
  zz-log
}

echo "✅ ZZIGNAL aliases loaded. Comandos:"
echo "  zz-resume     zz-monitor    zz-monitor-paper   zz-emergency   zz-panic"
echo "  zz-live-on    zz-live-off   zz-odi83-on    zz-odi83-off"
echo "  zz-reinv-on   zz-reinv-off  zz-odi-only83  zz-odi83-8"
echo "  zz-balance    zz-btc        zz-log         zz-log-f"
echo "  zz-orders     zz-fills      zz-restart     zz-status"
echo "  zz-hdn65-on   zz-hdn65-off  zz-hdn-only    zz-hdn65-8"
echo "  zz-go-o N     ← Odiseo 83 LIVE \$N"
echo "  zz-go-h N     ← Houdini 65 LIVE \$N"
echo "  zz-go-o-paper ← Odiseo 83 PAPER \$20"
echo "  zz-go-h-paper ← Houdini 65 PAPER \$20"

# ── One-click: Odiseo 83 LIVE ──────────────────
zz-go-o() {
  local amt=${1:-20}
  echo "⚡ Activando Odiseo 83 LIVE con \$${amt}..."
  curl -sX POST $ZZ_API/api/odiseo/live -H "Content-Type: application/json" -d '{"enable": true}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d '{"index": 1, "enable": false}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d '{"index": 0, "enable": true}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/budget -H "Content-Type: application/json" -d "{\"index\": 0, \"amount\": $amt}" > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/reinvest -H "Content-Type: application/json" -d '{"enable": true}' > /dev/null
  local bal=$(curl -s $ZZ_API/api/balance | python3 -c 'import sys,json;print(json.load(sys.stdin)["balance"])' 2>/dev/null || echo "?")
  echo "✅ Odiseo 83 LIVE \$${amt} | Balance: \$${bal}"
}

# ── One-click: Houdini 65 LIVE ─────────────────
zz-go-h() {
  local amt=${1:-20}
  echo "⚡ Activando Houdini 65 LIVE con \$${amt}..."
  curl -sX POST $ZZ_API/api/odiseo/live -H "Content-Type: application/json" -d '{"enable": true}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d '{"index": 0, "enable": false}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d '{"index": 1, "enable": true}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/budget -H "Content-Type: application/json" -d "{\"index\": 1, \"amount\": $amt}" > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/reinvest -H "Content-Type: application/json" -d '{"enable": true}' > /dev/null
  local bal=$(curl -s $ZZ_API/api/balance | python3 -c 'import sys,json;print(json.load(sys.stdin)["balance"])' 2>/dev/null || echo "?")
  echo "✅ Houdini 65 LIVE \$${amt} | Balance: \$${bal}"
}

# ── One-click: Odiseo 83 PAPER ─────────────────
zz-go-o-paper() {
  echo "📝 Activando Odiseo 83 PAPER \$20..."
  curl -sX POST $ZZ_API/api/odiseo/live -H "Content-Type: application/json" -d '{"enable": false}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d '{"index": 1, "enable": false}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d '{"index": 0, "enable": true}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/budget -H "Content-Type: application/json" -d '{"index": 0, "amount": 20}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/reinvest -H "Content-Type: application/json" -d '{"enable": true}' > /dev/null
  echo "✅ Odiseo 83 PAPER \$20 | Monitorear: zz-monitor"
}

# ── One-click: Houdini 65 PAPER ────────────────
zz-go-h-paper() {
  echo "📝 Activando Houdini 65 PAPER \$20..."
  curl -sX POST $ZZ_API/api/odiseo/live -H "Content-Type: application/json" -d '{"enable": false}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d '{"index": 0, "enable": false}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d '{"index": 1, "enable": true}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/budget -H "Content-Type: application/json" -d '{"index": 1, "amount": 20}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/reinvest -H "Content-Type: application/json" -d '{"enable": true}' > /dev/null
  echo "✅ Houdini 65 PAPER \$20 | Monitorear: zz-monitor"
}
