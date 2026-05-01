# Sesión HFT — 29 Abril 2026

## Commits realizados

```
20df9e4 feat: HFT cross-exchange module — Binance depth stream + VPIN/VBS/micro-price
7eebb71 feat: lock-free PriceRingBuffer (2048→4096 slots) with look-back binary search
bbd82f7 feat: unified HFT CSV pipeline — 20-column format, BOOK_UPDATE|TRADE|BINANCE_TICK
090b532 fix: binance_vol_100ms in ring buffer + volume spike detection (>2σ Welford)
5ceea7e chore: BINANCE_TICK filter $0.10 → $0.15 (Lightsail SSD optimization)
d961487 refactor: single Export button, mimalloc allocator, removed 6 old export formats
4e4fe58 fix: populate hft_snapshots DB table from capture_combined
95b6d30 fix: capture_combined writes even when Binance disconnected (Poly-only fallback)
f97778e fix: in-memory hft_snapshots buffer — export works without PostgreSQL
c9a261f fix: session_id field on CsvRecord for per-session mem_hft filtering
```

## Archivos del módulo HFT

```
backend_rust/src/modules/hft/
├── mod.rs              — declara submodulos
├── types.rs            — BinanceDepth, BinanceState, CsvRecord (20 cols), EventType
├── ring_buffer.rs      — PriceRingBuffer lock-free 4096 slots, búsqueda binaria
├── metrics.rs          — TrackingState, Welford running stats, build_book_update/trade/tick
├── binance_depth.rs    — WebSocket depth20@100ms + ticker, push a ring buffer + tick_tx
└── logger.rs           — CsvLogger con BufWriter, flush cada 60s
```

## CSV unificado — 20 columnas

```
ts_local, ts_exchange, event_type, latencia_ms, binance_price, binance_micro_price,
binance_imbalance, binance_vol_100ms, binance_vol_24h, poly_bid, poly_ask, poly_mid,
poly_spread, poly_bid_vol_all, poly_ask_vol_all, poly_imbalance, trade_side,
trade_price, trade_size, is_informed
```

## Pipeline de datos

```
Binance depth WS ──► PriceRingBuffer.push() ──► ring buffer (4096 slots, lock-free)
Binance ticker    ──► tick_tx ($0.15 filter) ──► BINANCE_TICK CSV rows
Polymarket CLOB   ──► capture_combined() ──► CsvRecord ──┬─► CsvLogger (CSV file)
                                                          ├─► mem_hft (RAM buffer)
                                                          └─► hft_snapshots (DB, opcional)
Export button     ──► GET /api/sessions/{id}/export ──► DB first, mem fallback
```

## DB migrations nuevas

- `011_hft_binance_depth.sql` — tabla `hft_snapshots` + 22 cols en `session_snapshots`
- `012_ring_buffer_lookback.sql` — `binance_lag_ms`, `binance_micro_price_at_t`

## Frontend

- `SessionsPanel.jsx`: 1 solo botón **Export** (quita CSV, Depth, Trades, JSON, Parquet, PqTrades)

## Pendientes / Notas

- Session 35-37 vacías: fueron grabadas antes de los fixes. Session 38+ deben tener datos.
- Si no hay PostgreSQL, los datos se guardan en `mem_hft` (RAM) + `hft_snapshots.csv` (disco).
- El export ahora funciona sin DB gracias al buffer en memoria con filtro por `session_id`.
- `is_informed`: detecta salto >$1.00 O spike de volumen >2σ (Welford online algorithm).
- `binance_vol_100ms`: ventana deslizante de 100ms en TrackingState.
- mimalloc como global allocator para Lightsail 2GB.
