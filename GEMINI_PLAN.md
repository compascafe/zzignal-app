# GEMINI — Plan de Implementación

## Sintaxis
| Comando | Significado |
|---|---|
| `/5g70` | Gemini $5, target 0.70, trigger 0.65, sin exit |
| `/8g72` | Gemini $8, target 0.72, trigger 0.67 |
| `/7g70e82` | Gemini $7, target 0.70, trigger 0.65, exit @0.82 |

- **Offset fijo**: trigger = target − 0.05
- **Primer tick que toque gana**: UP o DOWN, el que cruce primero el trigger
- **Trigger**: WS hft_state (primario, ~50ms) + poll 500ms (fallback)

## State Machine
```
IDLE
 │ /5g70 activa Gemini
 ▼
WAITING  [GEMINI UP/DN @0.65→0.70 $5]
 │ tick UP ≤ 0.65 o tick DN ≤ 0.65
 ▼
TRIGGERED → lanza BUY UP @0.70 ($5, ~7 shares) o BUY DOWN @0.70
 │ llena el BUY
 ▼
ACTIVE → si tiene exit, coloca SELL @0.82 (máximo de shares)
 │ llena el exit
 ▼
DONE → P&L registrado
```

`/c`  = cancela Gemini pendiente + órdenes si ya lanzadas
`/co` = market sell posición + cancela todo

## Resumen de Cambios

### `monitor/src/main.rs`
- 7 campos nuevos en `State`: `gemini_active`, `gemini_budget`, `gemini_target`, `gemini_trigger`, `gemini_exit`, `gemini_outcome`, `gemini_triggered`
- `check_gemini_trigger()`: si `clob_trade_up ≤ trigger` → dispara UP; si `clob_trade_dn ≤ trigger` → dispara DN. Llamado desde `apply_hft_state()` → cubre WS `hft_state` + poll HFT 500ms
- `trigger_gemini_buy()` se ejecuta tras trigger en el main loop

### `monitor/src/commands.rs`
- `Parsed::Gemini { budget, target, exit }` — nuevo comando
- Parser `try_parse_gemini`: `<budget>g<cents>e<cents>`
- `exec_gemini()`: activa state machine
- `trigger_gemini_buy()`: lanza BUY UP/DOWN al target, con exit automático
- `/c` y `/co` extendidos para cancelar/liquidar Gemini
- REGISTRY actualizado con 2 entradas GEMINI
- 23 tests (3 gemini_simple/gemini_with_exit/gemini_invalid + gemini_50_cases con 50 casos)

### `monitor/src/ui.rs`
- Grid lines eliminadas de `draw_book_side`
- S1 → indicador GEMINI (WAITING/TRIGGERED/ACTIVE/EXITING)
- Footer: línea `/5g70 /5g70e80` en Magenta
- `pos_h: 1 → 3`
- Escala logarítmica: `ln(size+1) / ln(max+1)` en barras (draw_book_side + draw_dom_side)
- Orderbook: `Min(18)` → `Length(17)` fixed, no expande
- Layout dashboard: market_info 3→4, price_cards 3→4, indicators 2→3, manual 2→3
- `half = (avail-1)/2` cap 9, entra en altura fija

## Commits
| Hash | Descripción |
|---|---|
| `ea487df` | Gemini trigger trading: state machine, parser, executor, WS+HFT hooks |
| `1fdffd2` | gemini_50_cases test: 43 valid + 7 invalid edge cases |
| `5a01e52` | orderbook: logarithmic bar scale + dynamic levels + grid removed |
| `44fc82b` | dashboard layout: fixed orderbook + more space for top cards |

## Compilar & Test
```bash
cd monitor
cargo check          # verificación rápida
cargo test           # 23/23 tests
cargo build --release
```
