# API Reference

Base URL: `http://localhost:8080` (configurable on the TUI side with
`ZZIGNAL_API_URL`).

> ⚠️ There is no authentication. Any client that can reach the port can place
> and cancel orders. Keep it on localhost or a trusted private network.

## REST

### `GET /api/health`

```json
{
  "app":    { "version": "0.3.0", "build_time": "dev", "git_sha": "dev" },
  "system": { "cpu_percent": 3.2, "ram_mb_used": 180, "ram_mb_total": 4096, "ram_pct": 4.4 },
  "status": "LIVE"
}
```

### `GET /api/btc`

```json
{ "price": 104532.15, "open": 104501.00 }
```

`price` is the live Binance mid; `open` is the current 15-minute round's
price-to-beat (0.0 until known).

### `GET /api/hft/latest`

Latest derived microstructure snapshot. Core fields:

```json
{
  "time": "2026-06-07T14:22:31.123-05:00",
  "event": "BOOK_UPDATE",
  "btc_price": 104532.15,
  "mid": 0.63, "spread": 0.02, "imbalance": 1.42,
  "bid_vol": 1520.0, "ask_vol": 1070.0,
  "clob_trade_up": 0.64, "clob_trade_dn": 0.35,
  "clob_trade_up_vol": 320.0, "clob_trade_dn_vol": 210.0,
  "btc_vel": 1.2, "btc_volume_24h": 32100.5,
  "btc_vol": 1.9, "btc_vol_1m": 42.0, "btc_vol_ses": 980.3,
  "spoof": 0, "dump_score": 0, "ask_wall": 0,
  "tick_gap_ms": 102, "secs_left": 421,
  "depth_up_bids": [[0.63, 900.0]], "depth_up_asks": [[0.65, 500.0]],
  "depth_dn_bids": [[0.36, 300.0]], "depth_dn_asks": [[0.38, 450.0]],
  "ofi_up": 12.5, "ofi_dn": -3.0,
  "micro_price_up": 0.6341, "micro_price_dn": 0.3566
}
```

### `GET /api/orders`

```json
[
  { "id": "0x8f...", "outcome": "up", "side": "BUY",
    "price": 0.64, "size_orig": 100.0, "size_matched": 100.0 }
]
```

### `GET /api/sessions`

```json
[
  { "id": 1749312000, "name": "S1749-1430", "status": "recording",
    "scheduled_start": "", "scheduled_end": "",
    "duration_min": 15, "tick_count": 4211, "trade_count": 37 }
]
```

### `GET /api/perf`

Cumulative latency counters per hot-path stage:

```json
{
  "slots": {
    "tick_consumer":     { "calls": 15423, "total_us": 234.5, "avg_us": 0.015 },
    "record_price_sample": { "calls": 15423, "total_us": 421.2, "avg_us": 0.027 },
    "build_binance_tick":  { "calls": 15423, "total_us": 890.2, "avg_us": 0.057 },
    "session_manager_push":{ "calls": 15423, "total_us": 120.1, "avg_us": 0.008 },
    "consumer_iter":       { "calls": 104633, "total_us": 52316.5, "avg_us": 0.500 },
    "broadcast_send":      { "calls": 15423, "total_us": 310.2, "avg_us": 0.020 }
  },
  "sessions": 1
}
```

### `POST /api/orders/limit`

```json
{ "side": "buy", "outcome": "up", "price": 0.64, "size": 100 }
```

Returns `{"ok": true}` immediately; the order result arrives over the
WebSocket as `order_result`. `side` ∈ `buy|sell`, `outcome` ∈ `up|down`.

### `POST /api/orders/market`

```json
{ "side": "sell", "outcome": "down", "amount_usdc": 50 }
```

### `DELETE /api/orders`

Cancels every open order for the active market. Returns `{"ok": true}`.

### `DELETE /api/orders/{id}`

Cancels a single order by id.

### `POST /api/panic`

Emergency: cancels all orders and market-sells both outcomes.

```json
{ "outcome": "up", "amount_up": 100, "amount_down": 0 }
```

All fields optional; omitting `outcome` targets both sides.

### `POST /api/sessions/start`

Starts a 15-minute recording session immediately (stops any active one).

```json
{ "ok": true, "id": 1749312000, "name": "S1749-1430" }
```

## WebSocket — `GET /ws`

On connect the server sends a snapshot, then streams every state change:

```json
{ "type": "snapshot", "status": "LIVE", "btc": 104532.15, "balance": 123.45 }
```

Server → client message types:

| `type` | Shape |
|---|---|
| `status` | `{ "type":"status", "status":"LIVE", "message":"…"? }` |
| `book` | `{ "type":"book", "side":"up", "book":{ "bids":[…], "asks":[…] } }` |
| `trade` | `{ "type":"trade", "side":"down", "price":0.36 }` |
| `balance` | `{ "type":"balance", "balance":123.45 }` |
| `btc_price` | `{ "type":"btc_price", "price":104532.15, "open":104501.0 }` (either field may appear alone) |
| `order_result` | `{ "type":"order_result", "success":true, "message":"…" }` |
| `open_orders` | `{ "type":"open_orders", "orders":[ … ] }` |
| `recent_fills` | `{ "type":"recent_fills", "fills":[ … ] }` |

Client → server commands (JSON text frames):

```json
{ "type": "limit",      "side": "buy",  "outcome": "up", "price": 0.64, "size": 100 }
{ "type": "market",     "side": "sell", "outcome": "down", "amount_usdc": 50 }
{ "type": "cancel",     "order_id": "0x8f…" }
{ "type": "cancel_all" }
```

Unknown message types are ignored. The TUI uses REST for polling and the
WebSocket for pushes.
