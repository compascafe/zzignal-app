# Architecture

ZZignal is a single-process Rust application composed of two binaries:

| Binary | Crate | Role |
|---|---|---|
| `polymarket-backend` | `backend_rust/` | Market data, order execution, feature pipeline, REST/WS API |
| `zzignal-monitor` | `TUI_monitor/` | Terminal dashboard (reads the backend API) |

There is **no database**. State lives in memory; time-series data is appended
to per-session CSV files. This keeps the latency path free of I/O and makes
the whole system trivially deployable.

## Process & task model (backend)

```
main thread (tokio::main)
│
├─ auto-session manager (task, 1 Hz)
│    starts/stops 15-min recordings on :00/:15/:30/:45 boundaries
│
├─ OS thread "polymarket-worker" (separate tokio runtime)
│    worker::run
│      └─ run_cycle
│           ├─ CLOB auth (Poly1271 / funder wallet)
│           ├─ Gamma market discovery
│           ├─ spawn btc_stream::run      (Binance aggTrade  → AppMsg::BtcTick)
│           └─ run_live                   (CLOB WS + command loop)
│
├─ binance::run_binance_depth_stream (task)
│    Binance depth20@100ms + ticker → BinanceDepth + PriceRingBuffer + tick_tx
│
├─ HFT tick consumer (task)
│    tick_rx → metrics::build_binance_tick → SessionManager CSV
│
├─ AppMsg consumer (task, behind std::sync::mpsc → tokio bridge)
│    AppMsg → broadcast WS JSON + update AppState (+ spawn pipeline capture)
│
├─ CSV flusher (task, 15 s) and auto-purge (task, 5 min, files > 1 h)
└─ axum server on 0.0.0.0:8080 (REST + /ws)
```

Why a separate OS thread for the worker: the Polymarket SDK's client is
blocking at construction/authentication time, and isolating it guarantees the
UI/API event loop is never stalled by CLOB reconnects.

## Data flow

### Binance depth (hot path)

```
depth20@100ms WS frame
  → parse top-20 levels (stack-free, small Vec)
  → BinanceDepth { bids, asks, price, volume }  ──► AppState (RwLock)
  → BinanceState { timestamp, mid_price }       ──► PriceRingBuffer (lock-free)
  → ticker frame → BtcTick if |Δprice| > $0.15  ──► tick_tx channel
```

`PriceRingBuffer` is a fixed 4,096-slot ring of `BinanceState` (two atomic
fields per slot: timestamp and mid-price bits) written behind a
release-ordered sequence counter. Readers (`get_closest_to`) do a binary
search over the live window and return the sample closest to a target
Exchange timestamp — used for cross-exchange latency and 1-second price
velocity. No mutexes and no `unsafe` on this path.

### Polymarket CLOB (hot path)

```
CLOB WS frame (book / last_trade_price)
  → AppMsg::BookUp | BookDown | LastTradeUp | LastTradeDown
  → consumer: update AppState, broadcast to WS clients
  → tokio::spawn → pipeline::capture_combined
        → metrics::build_book_update / build_trade_record
        → CsvRecord (27 columns)
        → SessionManager::push (pre-format String, 50 MiB BufWriter per session)
        → update latest_hft snapshot (depth, OFI, micro-price…)
```

Formatting happens **outside** the writer mutex; the mutex only guards the
final `write_all`. Flushes occur every 100 rows and on a 15-second timer
(plus on shutdown).

### WebSocket broadcast

`broadcast::channel::<String>(2048)` carries pre-serialized JSON. The consumer
serializes each `AppMsg` once and fans it out; slow clients cannot block the
pipeline (they lag and drop messages).

## Sessions

- A **session** is a 15-minute window aligned to `:00/:15/:30/:45` UTC.
- The auto-session manager starts a recording when a new window begins and
  stops it at the end; the TUI can also start one manually (`s` key).
- Each session writes to `sessions/session_<id>_<name>_hft.csv`.
- Files older than one hour are purged every 5 minutes.
- `tick_drain` briefly pauses the Binance tick consumer around session
  transitions to avoid boundary skew.

## CSV schema

27 columns, one row per event:

| # | Column | Event | Meaning |
|---|---|---|---|
| 1 | `time` | * | Local timestamp (UTC-5) with ms |
| 2 | `ts_exchange` | * | Exchange event timestamp (ms) |
| 3 | `event` | * | `BOOK_UPDATE` \| `TRADE` \| `BINANCE_TICK` |
| 4 | `latencia_ms` | BOOK | Binance→Polymarket cross-exchange latency |
| 5 | `binance_price` | * | BTC mid price from Binance top of book |
| 6 | `binance_imbalance` | * | (bid−ask)/(bid+ask) over top-20 depth |
| 7 | `binance_vol_24h` | * | BTC 24 h rolling volume |
| 8 | `btc_vol` | * | Latest aggTrade volume |
| 9 | `btc_vel` | * | BTC price velocity (USD/s, 1 s window) |
| 10 | `bid` | BOOK/TRADE | Polymarket best bid |
| 11 | `ask` | BOOK/TRADE | Polymarket best ask |
| 12 | `mid` | BOOK/TRADE | Polymarket mid (one-sided fallback) |
| 13 | `spread` | BOOK/TRADE | Polymarket best ask − best bid |
| 14 | `bid_vol` | BOOK/TRADE | Total bid size (all levels) |
| 15 | `ask_vol` | BOOK/TRADE | Total ask size (all levels) |
| 16 | `imbalance` | BOOK/TRADE | bid_vol / ask_vol |
| 17 | `spoof` | BOOK/TRADE | 1 = ask volume dropped >50 % vs. previous tick, no trade |
| 18 | `tick_gap_ms` | BOOK/TRADE | ms since previous captured record |
| 19 | `ask_wall` | BOOK/TRADE | 1 = ask_vol > 3× bid_vol (3+ consecutive ticks) |
| 20 | `dump_score` | BOOK/TRADE | 0 safe · 1 gap > 500 ms · 2 ask wall · 3 dead book / gap > 2 s |
| 21 | `secs_left` | BOOK/TRADE | Seconds left in the 15-min round |
| 22 | `clob_trade_up` | BOOK/TRADE | Average price of the last N UP trades ≥ `trade_min_vol` |
| 23 | `clob_trade_dn` | BOOK/TRADE | Average price of the last N DOWN trades |
| 24–25 | `clob_trade_*_vol` | BOOK/TRADE | Volume in the UP/DOWN trade windows |
| 26–27 | `clob_trade_count_*` | BOOK/TRADE | Number of trades in each window |

## Derived microstructure (TUI cards)

Computed per book update in `pipeline.rs` and served via `/api/hft/latest`:

- **S13 OFI** — order-flow imbalance from best bid/ask size deltas (UP/DN).
- **S14 μP** — micro-price `(bid·ask_size + ask·bid_size)/(bid_size+ask_size)`
  vs. mid, per outcome.
- **S15 SPR** — spread health.
- **S16 LEAD** — BTC velocity vs. Poly mid (lead-lag).
- **S17 LIQ** — bid/ask depth ratio.
- **S18 TICK** — tick gap, dump score, spoof flag.

## BTC price methodology

- Displayed BTC price = Binance top-of-book mid `(bid+ask)/2`, the closest
  cheap proxy to Chainlink/Pyth settlement values (aggTrade prints can lead).
- Price-to-beat (`btc_open`): Gamma API `groupItemThreshold` at startup,
  then Binance mid at every 15-minute boundary. The countdown in the TUI is
  computed locally from the system clock (zero pipeline delay).

## Credentials & safety

- `POLYMARKET_PRIVATE_KEY` is used to build an `alloy` signer; the EOA address
  is derived at startup and only a truncated form is logged.
- Orders are funded by the proxy/deposit wallet configured in
  `POLYMARKET_FUNDER_ADDRESS` with `SignatureType::Poly1271`.
- No key material is ever written to disk by the application.
- The API is unauthenticated and CORS-permissive: bind it to localhost or a
  private network only.
