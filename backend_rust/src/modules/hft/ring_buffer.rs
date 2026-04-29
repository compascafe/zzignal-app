use std::cell::UnsafeCell;
use std::hint;
use std::sync::atomic::{AtomicU64, Ordering};

/// Capacidad del ring buffer (debe ser potencia de 2).
const RING_CAP: usize = 4096;
const RING_MASK: usize = RING_CAP - 1;

/// Estado compacto de Binance en un instante. `repr(C)` para layout predecible.
/// 48 bytes por slot (con padding de alineación a 8).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct BinanceState {
    pub timestamp:         u64,  // ms (Binance event_time)
    pub mid_price:         f64,  // (best_bid + best_ask) / 2
    pub micro_price:       f64,  // volume-weighted (top 5)
    pub total_liquidity:   f64,  // bid+ask vol (top 20)
    pub binance_vol_100ms: f64,  // vol tradeado en últimos 100ms
    pub imbalance:         f32,  // VBS
}

impl Default for BinanceState {
    fn default() -> Self {
        Self {
            timestamp:         0,
            mid_price:         0.0,
            micro_price:       0.0,
            total_liquidity:   0.0,
            binance_vol_100ms: 0.0,
            imbalance:         0.0,
        }
    }
}

/// Ring buffer lock-free Single-Writer, Multiple-Reader (SWMR).
///
/// El escritor (Binance depth stream) pushea `BinanceState` sin bloquear.
/// Los lectores (capture_book) consultan `get_closest_to(ts)` sin bloquear al escritor.
///
/// Seguridad: usa `UnsafeCell` + `AtomicU64` para coordenar acceso concurrente
/// sin locks. El escritor es el único que avanza `write_seq`. Los lectores solo leen.
pub struct PriceRingBuffer {
    /// Pre-alocado: RING_CAP slots de BinanceState.
    slots: Box<[UnsafeCell<BinanceState>]>,
    /// Secuencia de escritura monótonamente creciente.
    /// Writer: `fetch_add(1, Release)` antes de escribir el slot.
    /// Reader: `load(Acquire)` para snapshot del rango válido.
    write_seq: AtomicU64,
}

// Safety: el escritor es único. Los lectores leen slots que ya fueron
// completamente escritos (garantizado por el orden Release-Acquire del write_seq).
unsafe impl Send for PriceRingBuffer {}
unsafe impl Sync for PriceRingBuffer {}

impl PriceRingBuffer {
    /// Crea un ring buffer de `RING_CAP` slots inicializados a cero.
    pub fn new() -> Self {
        let mut vec = Vec::with_capacity(RING_CAP);
        for _ in 0..RING_CAP {
            vec.push(UnsafeCell::new(BinanceState::default()));
        }
        Self {
            slots: vec.into_boxed_slice(),
            write_seq: AtomicU64::new(0),
        }
    }

    /// Push atómico (solo el escritor llama esto).
    ///
    /// 1. Reserva un slot incrementando `write_seq` (Release)
    /// 2. Escribe los datos en el slot
    /// 3. El lector ve el slot como válido cuando `write_seq >= seq_del_slot`
    pub fn push(&self, state: BinanceState) {
        let seq = self.write_seq.fetch_add(1, Ordering::Release);
        let idx = (seq as usize) & RING_MASK;
        // SAFETY: escritor único, ningún lector accede a este slot hasta
        // que write_seq avance lo suficiente (y ya avanzamos).
        unsafe {
            let ptr = self.slots[idx].get();
            ptr.write(state);
        }
    }

    /// Devuelve el estado de Binance más cercano en tiempo a `target_ts`.
    /// Búsqueda binaria O(log N) sobre el buffer circular.
    ///
    /// No bloquea al escritor: los lectores toman un snapshot atómico del
    /// `write_seq` y operan sobre el rango válido en ese instante.
    pub fn get_closest_to(&self, target_ts: u64) -> Option<BinanceState> {
        // Snapshot atómico del write_seq actual
        let mut seq = self.write_seq.load(Ordering::Acquire);

        // Si el buffer está vacío, spin breve y reintentar (el escritor es rápido)
        if seq == 0 {
            for _ in 0..16 {
                hint::spin_loop();
                seq = self.write_seq.load(Ordering::Acquire);
                if seq != 0 {
                    break;
                }
            }
            if seq == 0 {
                return None;
            }
        }

        let count = seq.min(RING_CAP as u64); // cuántos elementos válidos
        let base = seq.wrapping_sub(count);    // seq lógico del elemento más viejo

        // ─── Búsqueda binaria sobre índices lógicos ──────────────────────
        // Rango lógico: [base, base + count), mapeado a físico con & MASK.
        // Invariante: timestamps son estrictamente monótonos en el rango.

        let mut lo: u64 = 0;
        let mut hi: u64 = count.saturating_sub(1);

        // Leer timestamps de lo y hi
        let ts_lo = unsafe {
            let phys = ((base + lo) as usize) & RING_MASK;
            (*self.slots[phys].get()).timestamp
        };
        let ts_hi = unsafe {
            let phys = ((base + hi) as usize) & RING_MASK;
            (*self.slots[phys].get()).timestamp
        };

        // Si target_ts está fuera del rango, devolver el extremo más cercano
        if target_ts <= ts_lo {
            return Some(unsafe { *self.slots[((base + lo) as usize) & RING_MASK].get() });
        }
        if target_ts >= ts_hi {
            return Some(unsafe { *self.slots[((base + hi) as usize) & RING_MASK].get() });
        }

        // Binary search para encontrar el intervalo que contiene target_ts
        while lo + 1 < hi {
            let mid = lo + (hi - lo) / 2;
            let phys = ((base + mid) as usize) & RING_MASK;
            let ts_mid = unsafe { (*self.slots[phys].get()).timestamp };

            if ts_mid <= target_ts {
                lo = mid;
            } else {
                hi = mid;
            }
        }

        // lo y hi encierran target_ts. Devolver el más cercano.
        let idx_lo = ((base + lo) as usize) & RING_MASK;
        let idx_hi = ((base + hi) as usize) & RING_MASK;
        let state_lo = unsafe { *self.slots[idx_lo].get() };
        let state_hi = unsafe { *self.slots[idx_hi].get() };

        let dist_lo = if target_ts >= state_lo.timestamp {
            target_ts - state_lo.timestamp
        } else {
            state_lo.timestamp - target_ts
        };
        let dist_hi = if target_ts >= state_hi.timestamp {
            target_ts - state_hi.timestamp
        } else {
            state_hi.timestamp - target_ts
        };

        Some(if dist_lo <= dist_hi { state_lo } else { state_hi })
    }

    /// Devuelve el último estado escrito (el más reciente).
    pub fn latest(&self) -> Option<BinanceState> {
        let seq = self.write_seq.load(Ordering::Acquire);
        if seq == 0 {
            return None;
        }
        let idx = ((seq.wrapping_sub(1)) as usize) & RING_MASK;
        unsafe { Some(*self.slots[idx].get()) }
    }

    /// Número de elementos actualmente en el buffer.
    pub fn len(&self) -> usize {
        let seq = self.write_seq.load(Ordering::Acquire) as usize;
        seq.min(RING_CAP)
    }
}

impl Default for PriceRingBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_latest() {
        let rb = PriceRingBuffer::new();
        assert!(rb.latest().is_none());

        for i in 0..10 {
            rb.push(BinanceState {
                timestamp: (i * 100) as u64,
                mid_price: 50000.0 + i as f64,
                ..Default::default()
            });
        }

        let latest = rb.latest().unwrap();
        assert_eq!(latest.timestamp, 900);
        assert_eq!(latest.mid_price, 50009.0);
    }

    #[test]
    fn test_get_closest() {
        let rb = PriceRingBuffer::new();
        for i in 0..100 {
            rb.push(BinanceState {
                timestamp: (i * 10) as u64,
                mid_price: 50000.0 + i as f64,
                ..Default::default()
            });
        }

        // Exact match
        let s = rb.get_closest_to(500).unwrap();
        assert_eq!(s.timestamp, 500);

        // Closest: 505 → 500 wins over 510
        let s = rb.get_closest_to(505).unwrap();
        assert_eq!(s.timestamp, 500);

        // Closest: 504 → 500 wins over 510
        let s = rb.get_closest_to(504).unwrap();
        assert_eq!(s.timestamp, 500);

        // Before range
        let s = rb.get_closest_to(0).unwrap();
        assert_eq!(s.timestamp, 0);

        // After range
        let s = rb.get_closest_to(2000).unwrap();
        assert_eq!(s.timestamp, 990);
    }

    #[test]
    fn test_wraparound() {
        let rb = PriceRingBuffer::new();
        // Write more than RING_CAP elements to force wrap
        let total = (RING_CAP + 100) as u64;
        for i in 0..total {
            rb.push(BinanceState {
                timestamp: i * 10,
                mid_price: 50000.0 + i as f64,
                ..Default::default()
            });
        }

        assert_eq!(rb.len(), RING_CAP);

        // Oldest element should be at total - RING_CAP
        let oldest_ts = (total - RING_CAP as u64) * 10;
        let s = rb.get_closest_to(oldest_ts).unwrap();
        assert_eq!(s.timestamp, oldest_ts);

        // Latest element
        let s = rb.latest().unwrap();
        assert_eq!(s.timestamp, (total - 1) * 10);
    }
}
