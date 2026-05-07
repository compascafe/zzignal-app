//! Odiseo Entry Filters — Capa de filtros pre-compra modular
//!
//! Antes de que Odiseo 83 (o cualquier variante) ejecute una compra al cruzar
//! el umbral de 0.83, TODOS los filtros activos deben dar Pass.
//!
//! Arquitectura:
//!   Odiseo signal (px >= 0.83)
//!        │
//!        ▼
//!   ┌─────────────────────┐
//!   │  FilterChain::check │  ← CADA FILTRO SECUENCIAL
//!   │  ├─ Filter 1: Pass  │     Si alguno devuelve Block → NO SE COMPRA
//!   │  ├─ Filter 2: Pass  │     El motivo se loguea: [Odiseo] BLOCKED by <FilterName>: <reason>
//!   │  └─ Filter N: Pass  │
//!   └─────────┬───────────┘
//!             │ All Pass
//!             ▼
//!       EXECUTE ENTRY
//!
//! Cómo agregar un filtro nuevo:
//!   1. Define una struct con sus parámetros
//!   2. Implementa `OdiseoEntryFilter` para esa struct
//!   3. Agrega la instancia en `FilterChain::default_chain()`
//!   4. Listo — se puede habilitar/deshabilitar por nombre en caliente

use std::collections::HashSet;
use std::sync::Mutex;
use tracing::info;

// ══════════════════════════════════════════════════════════════════════════════
// FilterContext — Todos los datos disponibles al momento de decidir entrada
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct FilterContext {
    // ── Trigger ──
    pub px:                f64,  // precio que disparó la señal (clob_trade_up o _dn)
    pub is_up:             bool, // true=UP, false=DOWN
    pub seconds_left:      i32,  // segundos hasta cierre de sesión
    pub budget:            f64,  // presupuesto disponible para esta variante

    // ── BTC Market ──
    pub binance_price:     f64,  // precio BTC en USD
    pub btc_vel:           f64,  // velocidad (USD/s)
    pub btc_acel:          f64,  // aceleración (USD/s²)

    // ── Polymarket Order Book ──
    pub spread:            f64,  // ask - bid
    pub mid:               f64,  // (bid+ask)/2
    pub bid_vol:           f64,  // volumen total bid side
    pub ask_vol:           f64,  // volumen total ask side
    pub imbalance:         f64,  // bid_vol / (bid_vol + ask_vol)

    // ── Risk Signals ──
    pub dump_score:        u8,   // 0=safe 1=warning 2=critical 3=dead
    pub reversal_score:    u8,   // 0=safe 1=alert 2=danger 3=exit
    pub tick_gap_ms:       i64,  // ms desde último tick
    pub spoof:             u8,   // 1 = posible spoof detectado
    pub ask_wall:          u8,   // 1 = ask_vol > 3x bid_vol

    // ── CLOB Last Trade ──
    pub trade_up:          Option<f64>, // último precio trade UP
    pub trade_dn:          Option<f64>, // último precio trade DOWN
}

// ══════════════════════════════════════════════════════════════════════════════
// FilterResult — Pass = comprar, Block = NO comprar (con motivo logueable)
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub enum FilterResult {
    Pass,
    Block { reason: String },
}

impl FilterResult {
    pub fn block(reason: impl Into<String>) -> Self { FilterResult::Block { reason: reason.into() } }
    pub fn is_pass(&self) -> bool { matches!(self, FilterResult::Pass) }
}

// ══════════════════════════════════════════════════════════════════════════════
// OdiseoEntryFilter trait — Interfaz que todo filtro debe implementar
// ══════════════════════════════════════════════════════════════════════════════

pub trait OdiseoEntryFilter: Send + Sync {
    /// Nombre único del filtro (para logueo y enable/disable)
    fn name(&self) -> &'static str;

    /// Chequea si la entrada debe proceder. Pass = OK, Block = NO comprar.
    fn check(&self, ctx: &FilterContext) -> FilterResult;
}

// ══════════════════════════════════════════════════════════════════════════════
// FilterChain — Cadena de filtros con enable/disable por nombre
// ══════════════════════════════════════════════════════════════════════════════

pub struct FilterChain {
    filters:  Vec<Box<dyn OdiseoEntryFilter>>,
    disabled: Mutex<HashSet<String>>,
}

impl FilterChain {
    /// Crea la cadena con todos los filtros disponibles (algunos vienen disabled por defecto)
    pub fn default_chain() -> Self {
        let mut chain = Self {
            filters:  Vec::new(),
            disabled: Mutex::new(HashSet::new()),
        };

        // ─── FILTROS (orden = prioridad) ──────────────────────────────
        // Cada filtro se agrega con .add(). Si querés que arranque
        // deshabilitado, llamá .disable(name) después de agregarlo.

        // F1: Mercado congelado (sin ticks = datos fantasmas)
        chain.add(Box::new(FrozenMarketFilter { max_gap_ms: 3000 }));

        // F2: Spread máximo (spread > X = ilíquido, no entrar)
        chain.add(Box::new(SpreadHealthFilter { max_spread: 0.05 }));

        // F3: Flash dump protection (dump_score crítico = no comprar)
        chain.add(Box::new(FlashDumpFilter { max_dump: 2 }));

        // F4: Volumen mínimo en el book (sin liquidez = slippage mortal)
        chain.add(Box::new(MinVolumeFilter { min_bid_vol: 500.0, min_ask_vol: 500.0 }));

        // F5: Confirmación de tendencia BTC (no comprar contra la tendencia)
        chain.add(Box::new(BtcTrendConfirmFilter));

        // F6: Reversal score (riesgo de reversión alto = no entrar)
        chain.add(Box::new(ReversalRiskFilter { max_reversal: 2 }));

        // F7: Spoof / manipulación (posible orden falsa en el book)
        chain.add(Box::new(SpoofProtectionFilter));

        // F8: Ask wall (one-sided market = probable dump inminente)
        chain.add(Box::new(AskWallFilter));

        // ─── ALL FILTERS OFF by default (RAW mode) ────────────────
        // Enable individual filters via POST /api/odiseo/filters
        chain.disable_all();

        chain
    }

    pub fn add(&mut self, filter: Box<dyn OdiseoEntryFilter>) {
        self.filters.push(filter);
    }

    pub fn enable(&self, name: &str) {
        self.disabled.lock().unwrap().remove(name);
        info!("[OdiseoFilter] ✅ ENABLED  {}", name);
    }

    pub fn disable(&self, name: &str) {
        self.disabled.lock().unwrap().insert(name.to_string());
        info!("[OdiseoFilter] ❌ DISABLED {}", name);
    }

    pub fn disable_all(&self) {
        let mut d = self.disabled.lock().unwrap();
        for f in &self.filters {
            d.insert(f.name().to_string());
        }
        info!("[OdiseoFilter] ALL DISABLED — Odiseo running RAW (no filters)");
    }

    pub fn enable_all(&self) {
        self.disabled.lock().unwrap().clear();
        info!("[OdiseoFilter] ALL ENABLED");
    }

    /// Returns u8 bitmask: bit 0=frozen, 1=spread, 2=dump, 3=volume, 4=trend, 5=reversal, 6=spoof, 7=wall
    pub fn enabled_mask(&self) -> u8 {
        let d = self.disabled.lock().unwrap();
        let mut mask = 0u8;
        for (i, f) in self.filters.iter().enumerate() {
            if !d.contains(f.name()) {
                mask |= 1 << i;
            }
        }
        mask
    }

    pub fn is_enabled(&self, name: &str) -> bool {
        !self.disabled.lock().unwrap().contains(name)
    }

    pub fn list_filters(&self) -> Vec<(String, bool)> {
        self.filters.iter().map(|f| {
            let name = f.name().to_string();
            let enabled = !self.disabled.lock().unwrap().contains(&name);
            (name, enabled)
        }).collect()
    }

    /// Ejecuta todos los filtros activos en orden. Si alguno bloquea, devuelve
    /// el motivo. Si todos pasan, devuelve Pass.
    pub fn check(&self, ctx: &FilterContext, variant: &str, session_id: i32) -> FilterResult {
        for filter in &self.filters {
            let name = filter.name();
            if !self.is_enabled(name) {
                continue;
            }
            match filter.check(ctx) {
                FilterResult::Pass => continue,
                FilterResult::Block { reason } => {
                    info!(
                        "[Odiseo] #{} {} BLOCKED by {}: {} (px={:.4} spread={:.4} dump={} rev={} gap={}ms)",
                        session_id, variant, name, reason,
                        ctx.px, ctx.spread, ctx.dump_score, ctx.reversal_score, ctx.tick_gap_ms
                    );
                    return FilterResult::Block { reason: format!("[{}] {}", name, reason) };
                }
            }
        }
        FilterResult::Pass
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// IMPLEMENTACIONES DE FILTROS
// ══════════════════════════════════════════════════════════════════════════════
//
// ─── REGLA: cada filtro es una struct independiente ───
// Agregá nuevos filtros abajo siguiendo el mismo patrón.
// Después instancialos en FilterChain::default_chain().
//
// Parámetros:
//   Cada filtro recibe sus thresholds en el constructor.
//   Para cambiar thresholds en caliente sin recompilar,
//   usá AtomicF64/AtomicU8 en vez de campos normales.
//
// ══════════════════════════════════════════════════════════════════════════════

// ─── F1: FrozenMarketFilter ───────────────────────────────────────────────
/// Bloquea entrada si el mercado está congelado (gap entre ticks > umbral).
/// Un gap grande significa que no hay datos frescos → los precios pueden ser stale.
pub struct FrozenMarketFilter { pub max_gap_ms: i64 }
impl OdiseoEntryFilter for FrozenMarketFilter {
    fn name(&self) -> &'static str { "frozen_market" }
    fn check(&self, ctx: &FilterContext) -> FilterResult {
        if ctx.tick_gap_ms > self.max_gap_ms {
            return FilterResult::block(format!(
                "market frozen ({}ms gap > {}ms max)", ctx.tick_gap_ms, self.max_gap_ms
            ));
        }
        FilterResult::Pass
    }
}

// ─── F2: SpreadHealthFilter ───────────────────────────────────────────────
/// Bloquea entrada si el spread es muy grande (mercado ilíquido).
/// En Polymarket BTC 15-min, spreads > 5% son comunes y mortales para scalping.
/// Valor recomendado: 0.01-0.05 (1%-5%)
pub struct SpreadHealthFilter { pub max_spread: f64 }
impl OdiseoEntryFilter for SpreadHealthFilter {
    fn name(&self) -> &'static str { "spread_health" }
    fn check(&self, ctx: &FilterContext) -> FilterResult {
        if ctx.spread > self.max_spread {
            return FilterResult::block(format!(
                "spread too wide ({:.4} > {:.4})", ctx.spread, self.max_spread
            ));
        }
        FilterResult::Pass
    }
}

// ─── F3: FlashDumpFilter ──────────────────────────────────────────────────
/// Bloquea entrada si el dump_score es muy alto (riesgo de flash crash).
/// dump_score: 0=safe, 1=warning, 2=critical, 3=dead (bid=0)
pub struct FlashDumpFilter { pub max_dump: u8 }
impl OdiseoEntryFilter for FlashDumpFilter {
    fn name(&self) -> &'static str { "flash_dump" }
    fn check(&self, ctx: &FilterContext) -> FilterResult {
        if ctx.dump_score >= self.max_dump {
            return FilterResult::block(format!(
                "flash dump risk (dump_score={} >= {})", ctx.dump_score, self.max_dump
            ));
        }
        FilterResult::Pass
    }
}

// ─── F4: MinVolumeFilter ──────────────────────────────────────────────────
/// Bloquea entrada si no hay suficiente volumen en el order book.
/// Sin liquidez, cualquier orden mueve el precio → slippage extremo.
pub struct MinVolumeFilter { pub min_bid_vol: f64, pub min_ask_vol: f64 }
impl OdiseoEntryFilter for MinVolumeFilter {
    fn name(&self) -> &'static str { "min_volume" }
    fn check(&self, ctx: &FilterContext) -> FilterResult {
        if ctx.bid_vol < self.min_bid_vol || ctx.ask_vol < self.min_ask_vol {
            return FilterResult::block(format!(
                "low volume (bid_vol={:.0} need={:.0}, ask_vol={:.0} need={:.0})",
                ctx.bid_vol, self.min_bid_vol, ctx.ask_vol, self.min_ask_vol
            ));
        }
        FilterResult::Pass
    }
}

// ─── F5: BtcTrendConfirmFilter ────────────────────────────────────────────
/// Bloquea entrada si la dirección del trade (UP/DOWN) va CONTRA la tendencia BTC.
/// Reglas:
///   UP   → BTC debe estar subiendo o lateral (vel >= vel_min)
///   DOWN → BTC debe estar bajando o lateral (vel <= -vel_min)
/// Si BTC está flat (|vel| < vel_min), permite la entrada en ambas direcciones.
pub struct BtcTrendConfirmFilter;
impl OdiseoEntryFilter for BtcTrendConfirmFilter {
    fn name(&self) -> &'static str { "btc_trend_confirm" }
    fn check(&self, ctx: &FilterContext) -> FilterResult {
        let vel = ctx.btc_vel;
        let vel_min = 0.5; // USD/s mínimo para considerar tendencia

        if ctx.is_up && vel < -vel_min {
            return FilterResult::block(format!(
                "BTC falling (vel={:.2}) while trying UP entry", vel
            ));
        }
        if !ctx.is_up && vel > vel_min {
            return FilterResult::block(format!(
                "BTC rising (vel={:.2}) while trying DOWN entry", vel
            ));
        }
        FilterResult::Pass
    }
}

// ─── F6: ReversalRiskFilter ───────────────────────────────────────────────
/// Bloquea entrada si el riesgo de reversión de tendencia es alto.
/// reversal_score: 0=safe, 1=alert, 2=danger, 3=exit
pub struct ReversalRiskFilter { pub max_reversal: u8 }
impl OdiseoEntryFilter for ReversalRiskFilter {
    fn name(&self) -> &'static str { "reversal_risk" }
    fn check(&self, ctx: &FilterContext) -> FilterResult {
        if ctx.reversal_score >= self.max_reversal {
            return FilterResult::block(format!(
                "reversal risk (score={} >= {})", ctx.reversal_score, self.max_reversal
            ));
        }
        FilterResult::Pass
    }
}

// ─── F7: SpoofProtectionFilter ────────────────────────────────────────────
/// Bloquea entrada si se detectó spoofing (órdenes falsas para manipular el book).
pub struct SpoofProtectionFilter;
impl OdiseoEntryFilter for SpoofProtectionFilter {
    fn name(&self) -> &'static str { "spoof_protection" }
    fn check(&self, ctx: &FilterContext) -> FilterResult {
        if ctx.spoof == 1 {
            return FilterResult::block("spoof detected".to_string());
        }
        FilterResult::Pass
    }
}

// ─── F8: AskWallFilter ────────────────────────────────────────────────────
/// Bloquea entrada si hay un ask wall (mercado one-sided → probable dump).
/// ask_wall = 1 cuando ask_vol > 3x bid_vol
pub struct AskWallFilter;
impl OdiseoEntryFilter for AskWallFilter {
    fn name(&self) -> &'static str { "ask_wall" }
    fn check(&self, ctx: &FilterContext) -> FilterResult {
        if ctx.ask_wall == 1 {
            return FilterResult::block("ask wall detected (ask_vol > 3x bid_vol)".to_string());
        }
        FilterResult::Pass
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// PLANTILLA para filtros nuevos (copiá y pegá abajo):
// ══════════════════════════════════════════════════════════════════════════════
//
// pub struct MyNewFilter { pub threshold: f64 }
// impl OdiseoEntryFilter for MyNewFilter {
//     fn name(&self) -> &'static str { "my_new_filter" }
//     fn check(&self, ctx: &FilterContext) -> FilterResult {
//         if <condición_de_bloqueo> {
//             return FilterResult::block("<razón legible>");
//         }
//         FilterResult::Pass
//     }
// }
//
// Luego en FilterChain::default_chain():
//     chain.add(Box::new(MyNewFilter { threshold: 0.5 }));
//
// ══════════════════════════════════════════════════════════════════════════════
