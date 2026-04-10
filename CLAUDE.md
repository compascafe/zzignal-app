# Polymarket BTC 15-min Dashboard — Guía para LLMs

Proyecto Rust de trading en tiempo real sobre Polymarket (mercados de predicción BTC 15-min).
GUI nativa con egui/eframe. Arquitectura worker asíncrono + UI thread.

---

## Stack tecnológico

| Componente | Crate | Versión |
|---|---|---|
| GUI | `eframe` + `egui` | 0.29 |
| Plots | `egui_plot` | 0.29 |
| Backend gráfico | **wgpu** (Metal en macOS) | — |
| Async runtime | `tokio` | 1 |
| Polymarket SDK | `polymarket-client-sdk` | 0.4 |
| Ethereum signer | `alloy` | =1.6.3 (pinado) |
| WebSocket | `tokio-tungstenite` | 0.29 |
| Serialización | `serde` + `serde_json` | 1 |
| HTTP client | `reqwest` | 0.13 (rustls, sin native-tls) |
| Tiempo | `chrono` | 0.4 |
| Credenciales | `dotenvy` + `secrecy` | — |
| Errores | `anyhow` | 1 |
| Logging | `tracing` + `tracing-subscriber` | 0.1/0.3 |

**MSRV: Rust 1.88** — impuesto por `polymarket-client-sdk`. No actualizar `alloy` más allá de `=1.6.3` sin subir el compilador a 1.91+.

### CRÍTICO: backend gráfico

```toml
eframe = { version = "0.29", default-features = false, features = [
    "default_fonts",
    "wgpu",   # NO "glow" — glow crashea en macOS con múltiples viewports
] }
```

El backend `glow` (OpenGL) crashea en macOS Monterey+ con múltiples ventanas OS separadas. `wgpu` usa Metal y funciona correctamente.

---

## Estructura de archivos

```
dashboard_poly/
├── Cargo.toml
├── CLAUDE.md                  ← este archivo
├── .env                       ← credenciales (no commitear)
├── src/
│   ├── main.rs                ← UI: eframe App, viewports, renderizado (~1530 líneas)
│   ├── worker.rs              ← lógica asíncrona: WS, REST, órdenes (~1190 líneas)
│   ├── credentials.rs         ← carga y gestión de credenciales desde .env
│   └── setup.rs               ← binario auxiliar para configuración inicial
```

---

## Variables de entorno requeridas (`.env`)

```env
POLYMARKET_PRIVATE_KEY=0x...   # Clave privada Ethereum (hex, con o sin 0x)
CLOB_API_KEY=...               # UUID — API key L2 del CLOB
CLOB_API_SECRET=...            # API secret L2
CLOB_API_PASSPHRASE=...        # Passphrase L2
```

La dirección de wallet se **deriva** de la clave privada mediante alloy. Nunca se almacena/loguea en texto plano.

---

## Arquitectura general

```
main() ─── hilo OS ──► TradingApp::update() [60 fps, egui]
                │
                │ mpsc::channel<AppMsg>   (worker → UI)
                │ tokio_mpsc::unbounded<CmdMsg>  (UI → worker)
                │
                └── hilo OS ──► tokio runtime ──► worker::run()
                                                        │
                                                        ├── run_cycle()
                                                        │     ├── auth CLOB
                                                        │     ├── discover_btc_market()
                                                        │     ├── fetch initial snapshots
                                                        │     ├── spawn: run_btc_price_stream() [Binance WS]
                                                        │     ├── spawn: run_candle_stream()    [Binance REST+WS]
                                                        │     └── run_live()  [CLOB WS + cmd loop]
                                                        │           └── select! { WS msg | CmdMsg | 5s timer }
                                                        │
                                                        └── on error: reconnect con backoff exponencial
```

### Mensajes worker → UI (`AppMsg`)

```rust
pub enum AppMsg {
    Status(ConnStatus),
    BookUp(BookSnapshot),
    BookDown(BookSnapshot),
    LastTradeUp(f64),
    LastTradeDown(f64),
    Balance(f64),
    BtcOpen(f64),      // precio BTC al inicio del período 15-min
    BtcPrice(f64),     // precio BTC en tiempo real (Binance aggTrade)
    OrderResult(String), // feedback de orden colocada/cancelada
    OpenOrders(Vec<OpenOrder>),
    RecentFills(Vec<RecentFill>),
    Candles(Vec<Candle>),       // batch inicial
    CandleUpdate(Candle),       // update de la última vela
}
```

### Comandos UI → worker (`CmdMsg`)

```rust
pub enum CmdMsg {
    PlaceLimitOrder  { side: OrderSide, outcome: Outcome, price: f64, size: f64 },
    PlaceMarketOrder { side: OrderSide, outcome: Outcome, amount_usdc: f64 },
    ScalpBuy { outcome: Outcome, price: f64, size: f64, target_price: f64 },
    CancelOrder  { order_id: String },
    CancelMarket,
}
```

---

## Tipos de datos clave

```rust
pub enum OrderSide { Buy, Sell }
pub enum Outcome   { Up, Down }

pub struct MarketInfo {
    pub title:          String,
    pub token_id_up:    String,          // token address del outcome UP
    pub token_id_down:  Option<String>,  // token address del outcome DOWN
    pub outcome_up:     String,          // label: "Up" / "Yes"
    pub outcome_down:   String,          // label: "Down" / "No"
    pub end_date:       DateTime<Utc>,
    pub active:         bool,
    pub price_to_beat:  Option<f64>,     // BTC price al inicio del 15-min
}

pub struct OpenOrder {
    pub id:           String,
    pub outcome:      String,
    pub side:         OrderSide,
    pub price:        f64,
    pub size_orig:    f64,
    pub size_matched: f64,
}

pub struct RecentFill {
    pub outcome: String,
    pub side:    OrderSide,
    pub price:   f64,
    pub size:    f64,
    pub time:    String,    // "14:32:07" — hora del match
    pub session: String,    // "14:30"    — inicio de la sesión 15-min
}

pub struct Candle {
    pub open_time: i64,  // Unix ms
    pub open: f64, pub high: f64, pub low: f64, pub close: f64, pub volume: f64,
}

pub struct PriceLevel { pub price: f64, pub size: f64 }
pub struct BookSnapshot { pub bids: Vec<PriceLevel>, pub asks: Vec<PriceLevel> }

pub enum CandleInterval {
    OneSecond, OneMinute, FiveMinutes, FifteenMinutes, OneHour
}
```

---

## Estado de la UI

### `TradingApp` (struct principal)

```rust
struct TradingApp {
    rx:              mpsc::Receiver<AppMsg>,
    cmd_tx:          tokio_mpsc::UnboundedSender<CmdMsg>,
    creds:           Option<Arc<ClobCredentials>>,
    conn_status:     ConnStatus,
    market:          Option<MarketInfo>,
    book_up:         Option<BookSnapshot>,
    book_down:       Option<BookSnapshot>,
    last_update:     Option<Instant>,
    balance:         Option<f64>,
    last_trade_up:   Option<f64>,
    last_trade_down: Option<f64>,
    btc_price:       Option<f64>,
    btc_open:        Option<f64>,
    open_orders:     Vec<OpenOrder>,
    recent_fills:    Vec<RecentFill>,
    candles:         Vec<Candle>,
    current_interval: CandleInterval,
    interval_arc:    Arc<Mutex<CandleInterval>>,   // compartido con worker
    chart_st:        Arc<Mutex<ChartState>>,        // estado compartido con viewport
    trading_st:      Arc<Mutex<TradingState>>,      // estado compartido con viewport
    chart_open:      bool,
    orders_open:     bool,
    trading_open:    bool,
    frame_count:     u32,  // para warmup del atlas de fuentes
}
```

### `ChartState` y `TradingState`

```rust
struct ChartState {
    tab:           ChartTab,         // MACD | RSI | VFI | Depth
    settings:      IndicatorSettings,
    settings_open: bool,
    interval:      CandleInterval,   // seleccionado en el viewport del chart
}

struct TradingState {
    outcome:          Outcome,       // UP o DOWN seleccionado
    amount:           String,        // input de monto en USDC/shares
    price:            String,        // input de precio (0.01–0.99)
    feedback:         String,        // mensaje de último resultado de orden
    fills_show_all:   bool,          // toggle historial completo vs 10 últimos
    scalp_profit_pct: f64,           // target ganancia en % (default 5.0)
    scalp_profit_pts: f64,           // target ganancia en puntos (default 0.05)
    scalp_use_pct:    bool,          // true = modo %, false = modo puntos
}
```

---

## Layout de ventanas (multi-viewport)

El dashboard usa **3 ventanas OS separadas** via `egui::Context::show_viewport_immediate`:

| Viewport | ID hash | Tamaño default | Contenido |
|---|---|---|---|
| Chart | `"vp_chart"` | 980 × 620 | Velas BTC, indicadores, order book depth |
| Orders | `"vp_orders"` | 480 × 760 | Order book dual, posiciones, historial |
| Trading | `"vp_trading"` | 700 × 300 | Selector outcome, inputs, scalp, panic |
| Control | ventana principal | 520 × 210 | Status bar, wallet, BTC price, toggles |

**Multi-monitor**: Cada viewport es una ventana OS real. El usuario puede arrastrar Chart a pantalla 1 y Orders+Trading a pantalla 2.

### Fix del crash de textura (IMPORTANTE)

egui 0.29 en macOS/wgpu tiene un bug: cuando el atlas de fuentes crece en runtime (glifos nuevos), las partial texture updates pueden ir fuera de bounds → panic `"Partial texture update outside bounds of Managed(0)"`.

**Solución implementada** en `update()`:

```rust
// Frame 1: pre-cargar todos los glifos usados en la app
if self.frame_count == 1 {
    ctx.fonts(|fonts| {
        for &size in &[9.0, 9.5, ..., 18.0_f32] {
            let _ = fonts.layout_no_wrap(warmup_text, FontId::proportional(size), WHITE);
            let _ = fonts.layout_no_wrap(warmup_text, FontId::monospace(size), WHITE);
        }
    });
}
// Frames 1-2: no renderizar sub-viewports (dejar que el GPU commitee el atlas)
if self.frame_count < 3 {
    egui::CentralPanel::default().show(ctx, |ui| { draw_control_bar(ui, self); });
    return;
}
// Frame 3+: renderizar todos los viewports normalmente
```

**Por esto NO usar emoji** en los labels de botones ni títulos de ventanas — los emoji cargan fuentes adicionales en runtime que no se pueden pre-calentar fácilmente.

---

## Worker: fuentes de datos

### Mercado BTC 15-min (Polymarket)

- Descubierto via **Gamma API** (`gamma::Client`): busca el evento activo con slug `btc-15` o similar
- El mercado tiene DOS tokens: `token_id_up` (BTC sube) y `token_id_down` (BTC baja)
- `price_to_beat` = `groupItemThreshold` del mercado en Gamma (precio BTC al inicio del período)
- Fallback si no hay `groupItemThreshold`: Pyth Network REST

### Order book en tiempo real

- **WebSocket CLOB**: `wss://ws-subscriptions-clob.polymarket.com/ws/market`
- Suscripción: `{"assets_ids": [token_up, token_down], "type": "market"}`
- Mensajes `"book"` → actualiza `BookUp`/`BookDown`
- Mensajes `"last_trade_price"` → actualiza `LastTradeUp`/`LastTradeDown`
- Reconexión automática al desconectarse

### BTC/USD precio

- **Binance WebSocket**: `wss://stream.binance.com:9443/ws/btcusdt@aggTrade`
- Campo `"p"` del JSON → `AppMsg::BtcPrice`
- Corre en `tokio::spawn` independiente con backoff exponencial

### Velas BTC/USDT

- **Binance Kline WebSocket**: `wss://stream.binance.com:9443/ws/btcusdt@kline_{interval}`
- Fetch inicial REST: `https://api.binance.com/api/v3/klines`
- Intervalo seleccionable: 1s / 1m / 5m / 15m / 1h
- Cambio de intervalo: UI escribe en `Arc<Mutex<CandleInterval>>`, worker detecta cambio y reconecta

### Balance + órdenes

- Refrescados cada 5 segundos vía timer en el `select!` loop
- `client.balance_allowance()` → `AppMsg::Balance`
- `client.orders()` + `client.trades()` → `AppMsg::OpenOrders` + `AppMsg::RecentFills`
- La sesión 15-min de cada fill se calcula: `(match_time.timestamp() / 900) * 900`

---

## Autenticación Polymarket

El SDK soporta dos modos: EOA (Externally Owned Account) y Proxy Wallet.

```
1. Gamma API: PublicProfileRequest(address=EOA) → obtener proxy_wallet
2. Si proxy_wallet existe:
   Client::new(...).authentication_builder(&signer)
       .credentials(l2_creds)
       .funder(proxy_wallet)
       .signature_type(SignatureType::Proxy)
       .authenticate().await
3. Si no hay proxy_wallet: flujo normal EOA
```

**Por qué importa**: el saldo USDC real está bajo el proxy wallet, no bajo el EOA. Sin funder correcto, `balance_allowance()` devuelve 0.

---

## Colocación de órdenes

### Limit order

```rust
// En handle_limit_order() en worker.rs
let order = client.order_builder()
    .token_id(token_id)
    .side(side)
    .price(price)      // Decimal
    .size(size)        // shares, no USDC
    .order_type(OrderType::Gtc)
    .build()?;
let signed = client.sign_order(order, &signer).await?;
let resp = client.post_order(&signed, None).await?;
```

### Market order

```rust
// Implementado como limit order agresivo (taker) vía Amount::Usdc
let order = client.order_builder()
    .token_id(token_id)
    .side(side)
    .amount(Amount::Usdc(amount_usdc))
    .order_type(OrderType::Market)
    .build()?;
```

### Scalp mode (`ScalpBuy`)

Flujo automático en `handle_scalp_buy()`:

```
1. Colocar BUY limit → obtener order_id
2. Poll client.orders() cada 500ms (max 5min)
   - Si la orden desaparece de open orders → fill completo
   - Si size_matched >= size_orig * 0.995 → fill parcial aceptado
3. Colocar SELL limit a target_price por filled_size shares
```

El campo correcto para tamaño original en `OpenOrderResponse` es **`original_size`** (no `size`).

---

## Indicadores técnicos (calculados en UI)

Todos calculados sobre el slice de `Vec<Candle>` en cada frame:

| Indicador | Parámetros | Función |
|---|---|---|
| Bollinger Bands | `bb_period`, `bb_std` | `bollinger_bands()` |
| MACD | `macd_fast`, `macd_slow`, `macd_signal` | `draw_macd()` |
| RSI | `rsi_period`, `rsi_ob`, `rsi_os` | `rsi_series()` + `draw_rsi()` |
| VFI (Katsanos) | `vfi_period`, `vfi_coeff`, `vfi_vcoeff`, `vfi_smooth` | `draw_vfi()` |
| EMA | — | `ema()` — usada internamente por MACD y VFI |

`IndicatorSettings` tiene valores default razonables para scalping en 15-min.

---

## Funciones principales de UI (`main.rs`)

```rust
fn setup_style(ctx: &egui::Context)
// Configura fuentes y estilo de egui. Llamada UNA VEZ en la inicialización.
// NO usar ctx.fonts() aquí — no disponible antes del primer run().

fn draw_control_bar(ui: &mut Ui, app: &mut TradingApp)
// Barra de control principal: wallet, balance, precio BTC, countdown mercado,
// botones de toggle para abrir/cerrar los 3 viewports.

fn draw_chart_viewport(ui, candles, book_up, book_down, cs: &mut ChartState)
// Velas BTC/USDT + Bollinger, indicador seleccionado (MACD/RSI/VFI/Depth),
// barras de volumen, selector de intervalo.

fn draw_trading_viewport(ui, has_creds, cmd_tx, trading_st, market)
// Banner UP/DOWN prominente + botones de cambio
// Inputs monto/precio → botones LMT y MKT
// Panel SCALP MODE: toggle %/pts, slider, preview precio objetivo, botón SCALP BUY
// Botón PANIC rojo de ancho completo (CancelMarket)

fn draw_position_panel(ui, pos_up, pos_dn, last_trade_up, last_trade_down,
                       open_orders, recent_fills, trading_st, cmd_tx)
// Tarjetas visuales de posición (frame verde oscuro=UP, naranja=DOWN)
// con shares + valor estimado + botones SELL LMT / MKT SELL alineados a la derecha
// Tabla de órdenes abiertas con fondo colorido por side (BUY=verde, SELL=rojo)
// Historial de fills con columnas: outcome, side, precio, tamaño, hora, sesión

fn draw_book_dual(ui, book_up, book_down, market, last_trade_up, last_trade_down)
// Order book dual en dos columnas: UP (izq) y DOWN (der)
// Colorizado: asks=rojo, bids=verde, MAX_LEVELS=12 niveles por side

fn position_from_fills(fills: &[RecentFill]) -> (f64, f64)
// Calcula posición neta UP y DOWN sumando/restando fills
// BUY += size, SELL -= size. Derivado del historial, no del CLOB directamente.
```

---

## Convenciones y patrones

### Pattern para viewports (multi-OS-window)

```rust
let close_flag = Arc::new(AtomicBool::new(false));
if self.X_open {
    let flag = Arc::clone(&close_flag);
    let data = self.data.clone();  // clonar datos necesarios ANTES del closure
    let state = Arc::clone(&self.X_st);
    ctx.show_viewport_immediate(
        ViewportId::from_hash_of("vp_X"),
        ViewportBuilder::default().with_title("Titulo sin emoji").with_inner_size([w, h]),
        move |vp_ctx, _class| {
            if vp_ctx.input(|i| i.viewport().close_requested()) {
                flag.store(true, Ordering::Relaxed);
            }
            vp_ctx.request_repaint_after(Duration::from_millis(16));
            egui::CentralPanel::default().show(vp_ctx, |ui| {
                if let Ok(mut st) = state.lock() {
                    draw_X_viewport(ui, &data, &mut st, ...);
                }
            });
        },
    );
}
if close_flag.load(Ordering::Relaxed) { self.X_open = false; }
```

### Colores estándar del proyecto

```rust
// Status
Color32::from_rgb(0, 210, 100)   // verde LIVE / BUY / UP / profit
Color32::from_rgb(220, 100, 20)  // naranja DOWN
Color32::from_rgb(240, 70, 70)   // rojo SELL / error / baja
Color32::YELLOW                  // advertencia
Color32::GRAY                    // labels secundarios

// Backgrounds de posición
Color32::from_rgb(0, 45, 18)     // fondo tarjeta UP (verde muy oscuro)
Color32::from_rgb(50, 20, 0)     // fondo tarjeta DOWN (naranja muy oscuro)

// Backgrounds de órdenes
Color32::from_rgb(0, 35, 12)     // fila BUY
Color32::from_rgb(45, 8, 8)      // fila SELL

// SCALP panel
Color32::from_rgb(18, 28, 45)    // fondo panel scalp (azul muy oscuro)
Color32::from_rgb(255, 215, 50)  // label "SCALP MODE"

// Panic button
Color32::from_rgb(185, 0, 0)     // rojo intenso
```

### Rounding en egui 0.29

```rust
// CORRECTO:
egui::Frame::none().rounding(4.0).show(...)
// MAL — no existe en 0.29:
egui::Frame::none().corner_radius(4.0).show(...)
```

---

## Errores conocidos y soluciones

### `"Partial texture update outside bounds of Managed(0)"`

- **Causa**: atlas de fuentes egui crece en runtime con múltiples viewports en macOS/wgpu
- **Solución**: pre-calentar atlas en `update()` frame 1 con `ctx.fonts(|f| f.layout_no_wrap(...))`
  + skipear sub-viewports los primeros 2 frames (ver `frame_count` en `TradingApp`)
- **Prevención**: NO usar emoji en labels que se renderizen en viewports secundarios

### `"No fonts available until first call to Context::run()"`

- `ctx.fonts()` NO está disponible en `setup_style()` (llamado desde `cc.egui_ctx` antes del event loop)
- Mover cualquier pre-carga de fuentes al inicio de `update()`

### Campo `size` vs `original_size` en `OpenOrderResponse`

- SDK `polymarket-client-sdk 0.4`: el campo correcto es **`original_size`** (tipo `Decimal`)
- `size_matched` también es `Decimal` → `.to_string().parse::<f64>()`

### Proxy wallet vs EOA en Polymarket

- Si el usuario tiene proxy wallet, el saldo USDC está bajo el proxy, no el EOA
- Sin `SignatureType::Proxy` + `funder(proxy_wallet)`, las órdenes pueden fallar o `balance_allowance()` devuelve 0

---

## Comandos útiles

```bash
# Compilar (dev)
cargo build

# Correr el dashboard
cargo run

# Correr el setup inicial de credenciales
cargo run --bin setup

# Compilar release (binario optimizado ~1/3 del tamaño)
cargo build --release

# Ver logs (ajustar filtro)
RUST_LOG=polymarket_dashboard=debug cargo run

# Ver backtrace en panic
RUST_BACKTRACE=1 cargo run
```

---

## Estado actual del proyecto (Abril 2026)

### Funcionando

- [x] Autenticación CLOB con soporte EOA + Proxy wallet
- [x] Descubrimiento automático del mercado BTC 15-min activo via Gamma API
- [x] Order book en tiempo real (WebSocket CLOB)
- [x] Precio BTC en tiempo real (Binance aggTrade WS)
- [x] Velas OHLCV BTC/USDT con cambio de intervalo en caliente (Binance)
- [x] Indicadores: MACD, RSI, VFI Katsanos, Bollinger Bands
- [x] Colocación de órdenes: limit y market
- [x] **Scalp mode**: buy a precio X → auto-sell al fill a precio X+Y
- [x] Cancelación de órdenes individuales y de todo el mercado
- [x] **Tarjetas visuales de posición** con botones SELL LMT / MKT SELL inline
- [x] **Selector UP/DOWN** prominente con banner de color
- [x] Historial de fills con hora (`HH:MM:SS`) y sesión 15-min (`HH:MM`)
- [x] Botón PANIC (CancelMarket) a ancho completo en rojo
- [x] Multi-ventana OS real (ThinkorSwim-style) soporta 2 monitores
- [x] Fix del crash de textura en macOS/wgpu

### Pendiente / mejoras posibles

- [ ] Posición calculada desde CLOB directamente (actualmente se deriva de fills locales)
- [ ] PnL en tiempo real por posición (necesita precio de entrada almacenado)
- [ ] Alertas sonoras en fills
- [ ] Persistencia de configuración entre sesiones (egui Memory ya guarda posición de ventanas)
- [ ] Soporte para múltiples mercados simultáneos
- [ ] Tests de integración del worker
