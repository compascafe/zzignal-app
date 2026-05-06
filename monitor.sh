#!/bin/bash
# ═══════════════════════════════════════════════════════════════════════
# ZZIGNAL MONITOR — Seguimiento de trading en tiempo real
# Uso: ./monitor.sh
# Muestra: ENTER/EXIT, órdenes, balance, errores, estado LIVE
# ═══════════════════════════════════════════════════════════════════════

API="http://localhost:8080"
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
MAGENTA='\033[0;35m'
NC='\033[0m' # No Color
BOLD='\033[1m'

clear
echo -e "${BOLD}╔══════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║          ZZIGNAL MONITOR — Trading en vivo              ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════════════════════╝${NC}"
echo ""

# ── Estado LIVE ────────────────────────────────────────────
check_live() {
    local live=$(curl -s "$API/api/odiseo/status" 2>/dev/null | python3 -c "import sys,json; d=json.load(sys.stdin); print('LIVE' if d.get('live_mode') else 'PAPER')" 2>/dev/null)
    echo "${live:-?}"
}

# ── Balance ─────────────────────────────────────────────────
check_balance() {
    local bal=$(curl -s "$API/api/balance" 2>/dev/null | python3 -c "import sys,json; print(f\"\${json.load(sys.stdin)['balance']:.2f}\")" 2>/dev/null)
    echo "${bal:-?.??}"
}

# ── Variantes activas ──────────────────────────────────────
check_active() {
    curl -s "$API/api/odiseo/status" 2>/dev/null | python3 -c "
import sys,json
d=json.load(sys.stdin)
active = [v['name'] for v in d.get('variants',[]) if v.get('enabled')]
print(len(active))
" 2>/dev/null
}

# ── Órdenes abiertas ───────────────────────────────────────
check_orders() {
    curl -s "$API/api/orders" 2>/dev/null | python3 -c "
import sys,json
orders=json.load(sys.stdin)
print(len(orders))
" 2>/dev/null
}

# ── Últimos eventos Odiseo ─────────────────────────────────
last_tick=0
show_events() {
    # Buscar nuevos eventos desde el último tick
    local events=$(sudo journalctl -u zzignal-app --no-pager -n 100 --since "1 min ago" 2>/dev/null | grep -E "\[Odiseo\]|Order result|error.*order|PANIC|PlaceLimit|Funder" | tail -20)
    
    if [ -n "$events" ]; then
        while IFS= read -r line; do
            # Colorear según tipo
            if echo "$line" | grep -q "ENTER"; then
                echo -e "  ${GREEN}▶ ENTER${NC} $(echo "$line" | grep -oP 'odiseo\S+.*' | head -c 80)"
            elif echo "$line" | grep -q "EXIT r=1"; then
                echo -e "  ${CYAN}✓ TP${NC}   $(echo "$line" | grep -oP 'odiseo\S+.*' | head -c 80)"
            elif echo "$line" | grep -q "EXIT r=[234]"; then
                echo -e "  ${RED}✗ SL${NC}   $(echo "$line" | grep -oP 'odiseo\S+.*' | head -c 80)"
            elif echo "$line" | grep -q "SETTLED"; then
                echo -e "  ${MAGENTA}◼ SETTLED${NC} $(echo "$line" | grep -oP 'odiseo\S+.*' | head -c 80)"
            elif echo "$line" | grep -q "Order result: ✓"; then
                echo -e "  ${GREEN}✅ OK${NC}     $(echo "$line" | grep -oP 'Order result:.*' | head -c 70)"
            elif echo "$line" | grep -q "Order result: ✗"; then
                echo -e "  ${RED}❌ ERROR${NC}  $(echo "$line" | grep -oP 'Order result:.*' | head -c 70)"
            elif echo "$line" | grep -qi "error"; then
                echo -e "  ${RED}⚠ ERROR${NC}  $(echo "$line" | head -c 80)"
            fi
        done <<< "$events"
    fi
}

# ── Bucle principal ────────────────────────────────────────
echo -e "Presiona ${BOLD}Ctrl+C${NC} para salir"
echo ""

while true; do
    LIVE=$(check_live)
    BAL=$(check_balance)
    ACTIVE=$(check_active)
    ORDERS=$(check_orders)
    
    # Color del estado LIVE
    if [ "$LIVE" = "LIVE" ]; then
        LIVE_COLOR="${RED}${BOLD}${LIVE}${NC}"
    else
        LIVE_COLOR="${CYAN}${LIVE}${NC}"
    fi
    
    # Color del balance
    if (( $(echo "$BAL > 0" | bc -l 2>/dev/null) )); then
        BAL_COLOR="${GREEN}\$${BAL}${NC}"
    else
        BAL_COLOR="${RED}\$${BAL}${NC}"
    fi
    
    # Barra de estado
    TIMESTAMP=$(date '+%H:%M:%S')
    echo -e "${BOLD}━━━ ${TIMESTAMP} ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "  Estado: ${LIVE_COLOR}  |  Balance: ${BAL_COLOR}  |  Active: ${ACTIVE} variantes  |  Órdenes: ${ORDERS}"
    
    show_events
    
    sleep 5
done
