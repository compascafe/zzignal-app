# Manual: Filtros de Liquidez para Houdini 65

## Problema

Houdini 65 entra a precios extremos (0.60–0.70). A esos niveles el orderbook de
Polymarket es muy fino: poca profundidad, spreads grandes, slippage alto.
Necesitamos verificar que haya **liquidez real** antes de entrar.

---

## Datos de liquidez disponibles

El backend ya calcula estos campos en cada tick (`CsvRecord` en `types.rs`:

| Campo | Fórmula | Interpretacion |
|---|---|---|
| `price_impact` | `|trade_up - trade_dn| / (vol_up + vol_dn)` | Amihud: cuanto mueve el precio cada dolar tradeado |
| `depth_concentration` | `max(bid_vol, ask_vol) / total_depth` | >0.7 = mercado one-sided, facil de manipular |
| `bid_vol` | `poly_bid_vol_all` | Volumen total en el bid |
| `ask_vol` | `poly_ask_vol_all` | Volumen total en el ask |
| `spread` | `ask - bid` | Spread actual del orderbook |

---

## Paso 1: Agregar `price_impact` y `depth_concentration` al `FilterContext`

**Archivo:** `backend_rust/src/modules/hft/odiseo_filters.rs`

En el struct `FilterContext`, agregar dos campos nuevos (linea ~60, antes de `trade_up`):

```rust
// ── Liquidity Metrics ──
pub price_impact:        f64,  // Amihud illiquidity ratio
pub depth_concentration: f64,  // max(bid_vol,ask_vol)/total → one-sided risk
```

**Archivo:** `backend_rust/src/main.rs`

En la construccion del `FilterContext` (linea ~721), agregar:

```rust
let filter_ctx = FilterContext {
    // ... campos existentes ...
    trade_up: lt_up,
    trade_dn: lt_down,
    // ── NUEVOS ──
    price_impact: rec.price_impact,
    depth_concentration: rec.depth_concentration,
};
```

---

## Paso 2: Crear F13 — LiquidityDepthFilter

**Archivo:** `backend_rust/src/modules/hft/odiseo_filters.rs`

Agregar al final del archivo (antes de la plantilla):

```rust
// ─── F13: LiquidityDepthFilter ─────────────────────────────────────────────
/// Bloquea entrada si no hay suficiente liquidez en AMBOS lados del book.
/// Para H65 (entry ~0.65, size ~$20/0.65 = ~30 shares):
///   - min_total_vol = 2000 (suficiente para absorber 30 shares sin slippage)
///   - max_price_impact = 0.001 (0.1% — si cada $1 mueve mas de 0.001,
///     una orden de $20 moveria 2% = suicida para scalping)
/// Estos thresholds son mas estrictos que F4 (MinVolumeFilter, min=500 por lado)
/// porque H65 opera en precios extremos donde la liquidez es inherentemente menor.
pub struct LiquidityDepthFilter {
    pub min_total_vol:   f64,  // bid_vol + ask_vol minimo
    pub max_price_impact: f64, // Amihud ratio maximo
}
impl OdiseoEntryFilter for LiquidityDepthFilter {
    fn name(&self) -> &'static str { "liquidity_depth" }
    fn check(&self, ctx: &FilterContext) -> FilterResult {
        let total = ctx.bid_vol + ctx.ask_vol;

        if total < self.min_total_vol {
            return FilterResult::block(format!(
                "low total depth (total={:.0} < {:.0})", total, self.min_total_vol
            ));
        }

        if ctx.price_impact > self.max_price_impact && ctx.price_impact > 0.0 {
            return FilterResult::block(format!(
                "high price impact ({:.6} > {:.6} — thin book)", ctx.price_impact, self.max_price_impact
            ));
        }

        FilterResult::Pass
    }
}
```

---

## Paso 3: Crear F14 — DepthBalanceFilter

```rust
// ─── F14: DepthBalanceFilter ───────────────────────────────────────────────
/// Bloquea entrada si el orderbook esta muy desbalanceado (one-sided).
/// depth_concentration > 0.7 significa que un lado tiene >70% del volumen total
/// → mercado facil de manipular, probable dump/pump falso.
/// Para H65, esto es critico: si entras UP pero el 80% del volumen esta en ask,
/// probablemente es un dump en progreso y tu entrada queda atrapada.
pub struct DepthBalanceFilter { pub max_concentration: f64 }
impl OdiseoEntryFilter for DepthBalanceFilter {
    fn name(&self) -> &'static str { "depth_balance" }
    fn check(&self, ctx: &FilterContext) -> FilterResult {
        if ctx.depth_concentration > self.max_concentration {
            let dominant = if ctx.bid_vol > ctx.ask_vol { "bid" } else { "ask" };
            return FilterResult::block(format!(
                "one-sided depth (concentration={:.2} > {:.2}, {} heavy)",
                ctx.depth_concentration, self.max_concentration, dominant
            ));
        }
        FilterResult::Pass
    }
}
```

---

## Paso 4: Registrar los filtros en `default_chain()`

En `FilterChain::default_chain()`, agregar despues de F12:

```rust
// F13: Liquidity depth (total volume + Amihud impact for H65 thin books)
chain.add(Box::new(LiquidityDepthFilter { min_total_vol: 2000.0, max_price_impact: 0.001 }));

// F14: Depth balance (block one-sided orderbooks)
chain.add(Box::new(DepthBalanceFilter { max_concentration: 0.75 }));
```

---

## Paso 5: Compilar y usar

```bash
cd backend_rust && cargo build --release
```

En el server, copiar el binario y reiniciar. Luego:

```bash
# Activar SOLO los filtros de liquidez
./zz-go-h liquidity_depth depth_balance

# Activar set completo recomendado para H65:
./zz-go-h frozen_market spread_health flash_dump min_volume \
          btc_trend_confirm spoof_protection ask_wall mid_price_sanity \
          imbalance_sanity session_age liquidity_depth depth_balance

# Verificar
./zz-go-h
```

---

## Resumen de los 14 filtros

| # | Nombre | Que bloquea | Threshold |
|---|---|---|---|
| F1 | `frozen_market` | Sin ticks por >3s | max_gap=3000ms |
| F2 | `spread_health` | Spread >5% | max=0.05 |
| F3 | `flash_dump` | dump_score >=2 | max=1 |
| F4 | `min_volume` | bid/ask vol <500 | min=500 |
| F5 | `btc_trend_confirm` | Entry contra BTC | vel_min=0.5 |
| F6 | `reversal_risk` | Placeholder | — |
| F7 | `spoof_protection` | Spoof detectado | — |
| F8 | `ask_wall` | ask_vol > 3x bid_vol | — |
| F9 | `mid_price_sanity` | Mid en 0.50 stale | dead_zone=0.01 |
| F10 | `imbalance_sanity` | Imbalance corrupto | max=10 |
| F11 | `session_age` | Bordes de sesion | 30s-840s |
| F12 | `reentry_cooldown` | Re-entrada <30s | cooldown=30s |
| **F13** | **`liquidity_depth`** | **Profundidad total <2000 o price_impact >0.001** | **vol=2000, impact=0.001** |
| **F14** | **`depth_balance`** | **Book one-sided (concentracion >75%)** | **max=0.75** |

---

## Como ajustar thresholds sin recompilar

Los thresholds son constantes en el codigo. Si necesitas cambiarlos en caliente:

1. Agrega un `std::sync::atomic::AtomicU64` o `AtomicF64` como campo del filtro
2. Inicializa con `AtomicF64::new(valor)`
3. En `check()`, lee con `.load(Ordering::Relaxed)`
4. Expone un endpoint REST para modificarlo

Ejemplo para `LiquidityDepthFilter` con thresholds dinamicos:

```rust
use std::sync::atomic::{AtomicU64, Ordering};

pub struct LiquidityDepthFilter {
    pub min_total_vol:   AtomicU64,
    pub max_price_impact: AtomicU64, // stored as f64 bits
}
impl LiquidityDepthFilter {
    pub fn new(min_vol: f64, max_impact: f64) -> Self {
        Self {
            min_total_vol: AtomicU64::new(min_vol as u64),
            max_price_impact: AtomicU64::new(max_impact.to_bits()),
        }
    }
    pub fn set_thresholds(&self, min_vol: f64, max_impact: f64) {
        self.min_total_vol.store(min_vol as u64, Ordering::Relaxed);
        self.max_price_impact.store(max_impact.to_bits(), Ordering::Relaxed);
    }
}
impl OdiseoEntryFilter for LiquidityDepthFilter {
    fn name(&self) -> &'static str { "liquidity_depth" }
    fn check(&self, ctx: &FilterContext) -> FilterResult {
        let min_vol = self.min_total_vol.load(Ordering::Relaxed) as f64;
        let max_impact = f64::from_bits(self.max_price_impact.load(Ordering::Relaxed));
        // ... same logic ...
    }
}
```

---

## Flujo de decision

```
H65 signal: price >= 0.65  (ej: clob_trade_up = 0.6620)
    │
    ▼
┌──────────────────────────────────────────────────────────┐
│ F13 liquidity_depth: ¿total_vol >= 2000?                 │
│   ├─ NO  → BLOCK "low total depth"                       │
│   └─ SI  → sigue                                         │
│                                                          │
│ F13 liquidity_depth: ¿price_impact <= 0.001?             │
│   ├─ NO  → BLOCK "high price impact — thin book"         │
│   └─ SI  → sigue                                         │
│                                                          │
│ F14 depth_balance: ¿concentration <= 0.75?               │
│   ├─ NO  → BLOCK "one-sided depth — manipulable"         │
│   └─ SI  → sigue                                         │
│                                                          │
│ F2 spread_health: ¿spread <= 5%?                         │
│   ├─ NO  → BLOCK "spread too wide"                       │
│   └─ SI  → sigue                                         │
│                                                          │
│ F4 min_volume: ¿bid_vol >= 500 && ask_vol >= 500?        │
│   ├─ NO  → BLOCK "low volume"                            │
│   └─ SI  → sigue                                         │
└──────────────────────────────────────────────────────────┘
    │ All pass
    ▼
EXECUTE ENTRY
```

---

## Notas

- Los filtros de liquidez (F13, F14) son **complementarios** a F2 (spread) y F4 (min_volume).
  F2 y F4 verifican condiciones minimas absolutas. F13 y F14 verifican condiciones
  relativas al tamaño del trade que vas a ejecutar.

- Para H65 con budget=$20 y entry~0.66, el tamaño es ~30 shares.
  Con `min_total_vol=2000`, hay 60x mas volumen que tu orden → slippage insignificante.

- Si el budget se sube a $100 (size~150 shares), considera subir `min_total_vol` a 5000.

- `price_impact > 0.001` significa que cada $1 tradeado mueve el precio >0.001.
  Una orden de $20 moveria el precio 0.02 (2%) → demasiado para scalping.
