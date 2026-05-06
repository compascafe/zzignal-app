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

# ── Odiseo ────────────────────────────────────
alias zz-odi='curl -s $ZZ_API/api/odiseo/status | python3 -m json.tool'
alias zz-odi85-on='curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d "{\"index\": 0, \"enable\": true}"'
alias zz-odi85-off='curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d "{\"index\": 0, \"enable\": false}"'
alias zz-odi85-7='curl -sX POST $ZZ_API/api/odiseo/budget -H "Content-Type: application/json" -d "{\"index\": 0, \"amount\": 7}"'

# Apagar todas las variantes menos la 0
zz-odi-only85() {
  for i in 1 2 3 4 5 6 7 8 9 10 11; do
    curl -sX POST $ZZ_API/api/odiseo/variant \
      -H "Content-Type: application/json" \
      -d "{\"index\": $i, \"enable\": false}" > /dev/null
  done
  curl -sX POST $ZZ_API/api/odiseo/variant \
    -H "Content-Type: application/json" \
    -d '{"index": 0, "enable": true}' > /dev/null
  echo "✅ Solo Odiseo 85 activa"
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
  for i in 0 1 2 3 4 5 6 7 8 9 10 11; do
    curl -sX POST $ZZ_API/api/odiseo/variant \
      -H "Content-Type: application/json" \
      -d "{\"index\": $i, \"enable\": false}" > /dev/null
  done
  echo "✅ TODO APAGADO. Balance: $(curl -s $ZZ_API/api/balance | python3 -c 'import sys,json;print(json.load(sys.stdin)["balance"])')"
}

# ── Monitoreo ──────────────────────────────────
alias zz-log='sudo journalctl -u zzignal-app --no-pager -n 30 | grep -E "ENTER|EXIT|Order result|Odiseo|error"'
alias zz-log-f='sudo journalctl -u zzignal-app -f | grep --line-buffered -E "ENTER|EXIT|Order result|Odiseo|error"'
alias zz-monitor='cd /home/ubuntu/zzignal-app && ./zzignal-monitor'
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
echo "  zz-resume     zz-monitor    zz-emergency   zz-panic"
echo "  zz-live-on    zz-live-off   zz-odi85-on    zz-odi85-off"
echo "  zz-reinv-on   zz-reinv-off  zz-odi-only85  zz-odi85-7"
echo "  zz-balance    zz-btc        zz-log         zz-log-f"
echo "  zz-orders     zz-fills      zz-restart     zz-status"
echo "  zz-go         <- activa todo listo para tradear"

# ── One-click setup ────────────────────────────
zz-go() {
  local amt=${1:-7}
  echo "⚡ Activando Odiseo 83 LIVE con \$${amt}..."
  curl -sX POST $ZZ_API/api/odiseo/live -H "Content-Type: application/json" -d '{"enable": true}' > /dev/null
  # Apagar todas menos 0
  for i in 1 2 3 4 5 6 7 8 9 10 11; do
    curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d "{\"index\": $i, \"enable\": false}" > /dev/null
  done
  curl -sX POST $ZZ_API/api/odiseo/variant -H "Content-Type: application/json" -d '{"index": 0, "enable": true}' > /dev/null
  curl -sX POST $ZZ_API/api/odiseo/budget -H "Content-Type: application/json" -d "{\"index\": 0, \"amount\": $amt}" > /dev/null
  echo "✅ Odiseo 83 LIVE \$${amt} | Balance: $(curl -s $ZZ_API/api/balance | python3 -c 'import sys,json;print(json.load(sys.stdin)["balance"])')"
}
