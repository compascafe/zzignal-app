# ZZignal Setup — Mayo 5, 2026

## Wallets

| Tipo | Dirección |
|---|---|
| **EOA** (gas, firma tx) | `0xc9131fe7ec4dba6e606dc6e4a6b057a37bbdeb13` |
| **Proxy** (saldo USDC) | `0x0000000000000000000000000000000000000000` |

- Polygon RPC: `https://polygon-bor-rpc.publicnode.com`
- PolygonScan EOA: https://polygonscan.com/address/0xc9131fe7ec4dba6e606dc6e4a6b057a37bbdeb13
- PolygonScan Proxy: https://polygonscan.com/address/0x0000000000000000000000000000000000000000

## Server

- IP: `ip-172-26-10-114` (AWS Lightsail, Tokyo)
- SSH: `ubuntu@ip-172-26-10-114`
- Service: `sudo systemctl restart zzignal-app`
- Logs: `sudo journalctl -u zzignal-app -f`
- Binario: `/home/ubuntu/zzignal-app/polymarket-backend`
- `.env`: `/home/ubuntu/zzignal-app/.env`

## Deploy (GitHub Actions)

URL: https://github.com/compascafe/zzignal-app/actions

Secrets requeridos en GitHub:
- `ENV_FILE` — `.env` en base64: `base64 -w0 .env | pbcopy`
- `SERVER_IP_TK`
- `SERVER_SSH_KEY_TK`

Push a `main` dispara deploy automático.

## Comandos clave (en el server)

```bash
# Verificar estado del servicio
curl -s http://localhost:8080/api/status

# Activar USDC (approve on-chain)
curl -X POST http://localhost:8080/api/approve

# Logs del approve
sudo journalctl -u zzignal-app --no-pager -n 50 | grep "Approve USDC"

# Reiniciar backend
sudo systemctl restart zzignal-app
```

## Activar saldo USDC (PASOS)

1. Necesitas ~0.01 POL en la **EOA** `0xc9131fe7ec4dba6e606dc6e4a6b057a37bbdeb13` para gas
2. Opciones para conseguir POL:
   - Faucet: https://faucet.polygon.technology / https://faucet.quicknode.com/polygon
   - Comprar en Binance/Coinbase → Withdraw → red Polygon → EOA
3. Verificar POL: https://polygonscan.com/address/0xc9131fe7ec4dba6e606dc6e4a6b057a37bbdeb13
4. Cuando haya POL, ejecutar: `curl -X POST http://localhost:8080/api/approve`
5. Esperar ~10-15 seg (2 transacciones: ERC-20 approve + ERC-1155 setApprovalForAll)
6. Balance de $7 USDC debería aparecer → Odiseo empieza a tradear con órdenes reales

## Odiseo (paper trading → real)

Odiseo hace paper trading simulado. Las entradas/salidas con pnl=0 son simulación.  
Con balance > 0, el botón "Live" activa órdenes reales vía CLOB.

## Endpoints útiles

- `/api/status` — status LIVE
- `/api/balance` — balance USDC
- `/api/approve` (POST) — approve USDC + CTF on-chain
- `/api/odiseo` — estado Odiseo
- `/api/odiseo/live` (POST) — activar/desactivar live trading
- `/api/btc/provider` (POST) — cambiar proveedor BTC (Binance/Coinbase/Kraken)
- `/ws` — WebSocket (precio BTC, order book, órdenes)

## Errores conocidos y soluciones

| Error | Causa | Solución |
|---|---|---|
| `Invalid symbol 61, offset 44` | `=` padding extra en CLOB_API_SECRET | Arreglado (commit `136d6ca`) |
| `HTTP 451 Unavailable For Legal Reasons` | Binance bloquea IPs de US (instancia vieja `ip-172-26-0-18`) | Usar Coinbase/Kraken si es US; en Tokyo funciona Binance |
| `insufficient funds for gas` | 0 POL en EOA | Enviar POL a EOA |
| `API key disabled, tenant disabled` | RPC `polygon-rpc.com` bloquea IP | Arreglado → `polygon-bor-rpc.publicnode.com` |
| `interval is not defined` (frontend) | Falta prop `interval` | Arreglado (commit `136d6ca`) |
