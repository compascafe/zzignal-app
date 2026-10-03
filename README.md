# ZZignal

**Low-latency BTC 15-minute up/down trading engine for [Polymarket](https://polymarket.com) — Rust backend + terminal dashboard.**

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.91%2B-orange.svg)](https://www.rust-lang.org)

ZZignal discovers the live BTC 15-minute market on Polymarket, streams the
Polymarket CLOB order book and Binance market data side by side, builds a
microstructure feature pipeline, records every session to CSV, and lets you
trade it from a Bloomberg-style terminal UI — all in Rust, with per-operation
latency instrumentation.

> ⚠️ **Disclaimer** — This software places **real orders with real money** on
> Polymarket. It is provided for educational and research purposes, with no
> guarantee of profit and no warranty of any kind. Markets involve risk;
> you can lose your funds. Only run it with capital you can afford to lose.
> This is not financial advice. See [`LICENSE`](LICENSE).

---

## Architecture

```
                        ┌─────────────────────────────────────────────┐
                        │              Rust backend                   │
  Binance               │  ┌───────────────┐   ┌───────────────────┐  │
  ├─ depth20@100ms ─────┼─►│ binance.rs    │──►│ PriceRingBuffer   │  │
  └─ aggTrade ──────────┼─►│ btc_stream.rs │──►│ (lock-free, 4k)   │  │
                        │  └───────────────┘   └─────────┬─────────┘  │
  Polymarket            │  ┌───────────────┐             ▼            │
  ├─ Gamma API ─────────┼─►│ worker.rs     │   ┌───────────────────┐  │
  └─ CLOB WS ───────────┼─►│ CLOB + orders │──►│ pipeline.rs       │  │
                        │  └───────┬───────┘   │ metrics / OFI /   │  │
                        │          │           │ micro-price / CSV │  │
                        │          ▼           └─────────┬─────────┘  │
                        │  ┌─────────────────────────────▼─────────┐  │
                        │  │ AppState (RwLock) + broadcast (2048)  │  │
                        │  └───────┬───────────────────────┬───────┘  │
                        │          │ REST + WS /ws         │          │
                        └──────────┼───────────────────────┼──────────┘
                                   ▼                       ▼
                          ┌─────────────────┐    sessions/*.csv
                          │ zzignal-monitor │
                          │ (ratatui TUI)   │
                          └─────────────────┘
```

- **`backend_rust/`** — Axum + tokio server on `0.0.0.0:8080`: market
  discovery, CLOB authentication (proxy/funder wallet, `SignatureType::Poly1271`),
  order placement, Binance streams, the microstructure pipeline, CSV session
  recording, REST API and WebSocket.
- **`TUI_monitor/`** — `ratatui` terminal dashboard: live books, BTC price and
  price-to-beat, derived microstructure cards (OFI, micro-price, spread health,
  BTC lead-lag, liquidity, tick health) and real-money order entry.

## Engineering highlights

- **Lock-free price ring buffer** — `PriceRingBuffer` (4,096 slots) stores
  Binance mid-price snapshots in two independent atomics per slot (no torn
  reads, **no `unsafe`**); lookups are a binary search by exchange timestamp
  with **zero locks in the hot path**.
- **`mimalloc` global allocator** for stable low-latency allocation.
- **Latency instrumentation built in** — every hot-path stage is timed with
  atomic accumulators and exposed at `GET /api/perf` (avg/aggregate µs).
- **Copy-minimizing tick pipeline** — snapshots are cloned once, heavy work is
  offloaded with `tokio::spawn`, and the broadcast channel (capacity 2048) is
  reserved for WebSocket clients.
- **Single-allocation CSV rows** — records are formatted with `write!` into a
  pre-allocated `String` (27 columns), buffered in 50 MiB `BufWriter`s and
  flushed every 100 rows / 15 s.
- **15-minute session lifecycle** — sessions auto-start/stop exactly on
  `:00/:15/:30/:45`, and stale CSVs (> 1 h) are purged automatically.
- **Mid-price methodology** — BTC display and price-to-beat use Binance
  top-of-book mid `(bid+ask)/2`, the closest cheap proxy to Chainlink/Pyth
  settlement prices; the Polymarket-provided price-to-beat is used at startup.
- **Thin-LTO release profile** (`opt-level = 3`, `codegen-units = 1`,
  `panic = "abort"`, stripped) and `rustls` everywhere — no OpenSSL.
- Clean module layout: `models/` (state & data types), `controllers/`
  (worker + HTTP/WS API), `services/` (streams, metrics, pipeline, sessions,
  perf).

## Repository layout

```
.
├── backend_rust/            # Trading backend (Axum, tokio)
│   ├── src/
│   │   ├── main.rs          # Entry point: session manager + tick consumer
│   │   ├── controllers/
│   │   │   ├── worker.rs    # CLOB auth, market discovery, orders, WS
│   │   │   └── api.rs       # REST endpoints + /ws
│   │   ├── models/          # AppState, credentials, HFT types, ring buffer
│   │   └── services/
│   │       ├── binance.rs   # Binance depth stream → BinanceDepth
│   │       ├── btc_stream.rs# Binance aggTrade → BtcTick
│   │       ├── metrics.rs   # Feature builders + TrackingState
│   │       ├── pipeline.rs  # CSV capture + latest-HFT state
│   │       ├── session.rs   # Per-session CSV writers
│   │       └── perf.rs      # Hot-path latency counters
│   └── Cargo.toml
├── TUI_monitor/             # Terminal dashboard (ratatui)
│   └── src/{main,ui,api,commands}.rs
├── scripts/deploy.sh        # Manual build + systemd restart
├── .github/workflows/       # CI (fmt/clippy/test/audit) + optional SSH deploy
├── .env.example             # Credential template
└── LICENSE
```

## Quick start

**Requirements:** Rust **1.91+** (MSRV imposed by the Polymarket SDK),
Linux or macOS. No database required.

### 1. Credentials

```bash
cp .env.example .env
$EDITOR .env
```

| Variable | Description |
|---|---|
| `POLYMARKET_PRIVATE_KEY` | Ethereum private key (hex, `0x` optional) used to sign orders. The EOA address is derived from it at startup. |
| `CLOB_API_KEY` / `CLOB_API_SECRET` / `CLOB_API_PASSPHRASE` | L2 CLOB API credentials from [polymarket.com → API keys](https://polymarket.com/settings?tab=api-keys). |
| `POLYMARKET_FUNDER_ADDRESS` | Your Polymarket **proxy/deposit wallet** address — the on-chain account that holds your pUSD balance and funds orders. Public information. |

`.env` is loaded automatically from the working directory or any parent
(`dotenvy`). It is gitignored — never commit it.

### 2. Build & run

```bash
# Backend (terminal 1)
cd backend_rust
cargo run --release          # serves http://0.0.0.0:8080

# Terminal dashboard (terminal 2)
cd TUI_monitor
cargo run --release
```

The dashboard polls `http://localhost:8080` by default. To monitor a remote
backend without changing code:

```bash
ZZIGNAL_API_URL=http://10.0.0.5:8080 ZZIGNAL_WS_URL=ws://10.0.0.5:8080/ws \
  cargo run --release
```

## TUI usage

| Key | Action |
|---|---|
| `/` | Command mode |
| `s` | Manually start a 15-minute recording session |
| `↑` / `↓` | Command history |
| `Esc` / `q` | Quit |

| Command | Action |
|---|---|
| `/b up <usd>` / `/b down <usd>` | Market **buy** for `$X` |
| `/s up <usd>` / `/s down <usd>` | Market **sell** for `$X` |
| `/l up <price> <size>` / `/l down <price> <size>` | Limit buy |
| `/c` | Cancel all orders |
| `/panic` | **Emergency:** cancel all + market-sell both outcomes |
| `/man` | Command help |

The dashboard renders six microstructure cards (S13–S18): order-flow
imbalance, micro-price deviation, spread health, BTC lead-lag, liquidity
pressure and tick health.

## HTTP API

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/health` | Version/build, CPU/RAM, connection status |
| `GET` | `/api/btc` | `{"price": …, "open": …}` (live mid / price-to-beat) |
| `GET` | `/api/hft/latest` | Latest derived microstructure state |
| `GET` | `/api/orders` | Open orders |
| `GET` | `/api/sessions` | Active recording sessions |
| `GET` | `/api/perf` | Per-operation latency counters |
| `POST` | `/api/orders/limit` | `{"side":"buy","outcome":"up","price":0.65,"size":100}` |
| `POST` | `/api/orders/market` | `{"side":"sell","outcome":"down","amount_usdc":50}` |
| `DELETE` | `/api/orders` | Cancel all orders |
| `DELETE` | `/api/orders/{id}` | Cancel one order |
| `POST` | `/api/panic` | Cancel all + market-sell |
| `POST` | `/api/sessions/start` | Start a 15-min recording session |
| `WS` | `/ws` | Snapshot + live broadcast + commands |

Full request/response reference and the WebSocket protocol live in
[`docs/API.md`](docs/API.md).

## Recorded data

While a session is recording, the backend appends one row per book update,
trade and Binance tick to `session_<id>_<name>_hft.csv` inside a `sessions/`
directory created under the backend's working directory (27 columns):

```
time, ts_exchange, event, latencia_ms,
binance_price, binance_imbalance, binance_vol_24h, btc_vol, btc_vel,
bid, ask, mid, spread, bid_vol, ask_vol, imbalance,
spoof, tick_gap_ms, ask_wall, dump_score, secs_left,
clob_trade_up, clob_trade_dn, clob_trade_up_vol, clob_trade_dn_vol,
clob_trade_count_up, clob_trade_count_dn
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md#csv-schema) for a column-by-column
description. Files older than one hour are purged automatically.

## Performance monitoring

```bash
curl -s localhost:8080/api/perf | jq
```

Returns cumulative calls / total µs / average µs per hot-path stage
(`tick_consumer`, `build_binance_tick`, `consumer_iter`, `broadcast_send`, …).

## Deployment

Any process supervisor works; a `systemd` unit on the server is the classic
setup:

```ini
[Unit]
Description=ZZignal backend
After=network-online.target

[Service]
User=ubuntu
WorkingDirectory=/home/ubuntu/zzignal-app
EnvironmentFile=/home/ubuntu/zzignal-app/.env
ExecStart=/home/ubuntu/zzignal-app/polymarket-backend
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

`scripts/deploy.sh` builds both binaries and restarts the service. The
included GitHub Actions workflow (`.github/workflows/deploy.yml`) does the
same over SSH; it is guarded to run only on the upstream repository and can
be deleted if you deploy differently. Required secrets: `SERVER_IP_TK`,
`SERVER_SSH_KEY_TK`, `ENV_FILE` (base64 of the production `.env`).

> 🔒 The API has **no authentication** and permissive CORS by design: it is a
> localhost tool. Do not expose port 8080 to the public internet — anyone who
> can reach it can trade your account.

## Development

```bash
cd backend_rust && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
cd TUI_monitor  && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
```

Commit style: short imperative subject lines (see the git log). PRs welcome —
see [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`SECURITY.md`](SECURITY.md).

## License

[MIT](LICENSE) © 2026 David Tello
