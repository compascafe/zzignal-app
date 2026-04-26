use std::collections::VecDeque;
use chrono::Utc;
use serde_json::json;

use crate::modules::core::worker::PriceLevel;
use super::models::{Alert, DetectorConfig, PatternType, Signal};

/// Estado interno del detector por side (up/down)
pub struct SideDetector {
    pub side:               String,
    pub config:             DetectorConfig,
    pub recent_spreads:     VecDeque<f64>,
    pub recent_bid_volumes: VecDeque<f64>,
    pub recent_ask_volumes: VecDeque<f64>,
    pub recent_mids:        VecDeque<f64>,
    pub max_history:        usize,
    pub wall_history:       VecDeque<(chrono::DateTime<Utc>, f64, f64)>, // (ts, price, size)
    pub last_alert_ts:      Option<chrono::DateTime<Utc>>,
    pub signal_count:       u64,
}

impl SideDetector {
    pub fn new(side: &str, config: DetectorConfig) -> Self {
        Self {
            side: side.to_string(),
            config,
            recent_spreads:     VecDeque::with_capacity(200),
            recent_bid_volumes: VecDeque::with_capacity(200),
            recent_ask_volumes: VecDeque::with_capacity(200),
            recent_mids:        VecDeque::with_capacity(200),
            max_history: 200,
            wall_history:       VecDeque::with_capacity(100),
            last_alert_ts:      None,
            signal_count:       0,
        }
    }

    /// Alimenta un nuevo tick del order book y devuelve las señales detectadas
    pub fn feed(
        &mut self,
        best_bid: f64,
        best_bid_sz: f64,
        best_ask: f64,
        best_ask_sz: f64,
        bids: &[PriceLevel],
        asks: &[PriceLevel],
    ) -> Vec<Signal> {
        let spread = best_ask - best_bid;
        let mid = (best_bid + best_ask) / 2.0;
        let bid_vol: f64 = bids.iter().map(|l| l.size).sum();
        let ask_vol: f64 = asks.iter().map(|l| l.size).sum();

        // Actualizar buffers
        push_capped(&mut self.recent_spreads, spread, self.max_history);
        push_capped(&mut self.recent_bid_volumes, bid_vol, self.max_history);
        push_capped(&mut self.recent_ask_volumes, ask_vol, self.max_history);
        push_capped(&mut self.recent_mids, mid, self.max_history);

        let mut signals = Vec::new();

        // 1. Wall detection
        if let Some(s) = self.detect_wall(bids, asks) {
            signals.push(s);
        }

        // 2. Spread anomaly
        if let Some(s) = self.detect_spread_anomaly(spread) {
            signals.push(s);
        }

        // 3. Depth imbalance
        if let Some(s) = self.detect_imbalance(bid_vol, ask_vol) {
            signals.push(s);
        }

        // 4. Spoof detection (wall aparece y desaparece rápido)
        if let Some(s) = self.detect_spoof(best_bid, best_bid_sz, best_ask, best_ask_sz) {
            signals.push(s);
        }

        // 5. Momentum shift
        if let Some(s) = self.detect_momentum() {
            signals.push(s);
        }

        // Cooldown: máximo 1 alerta por segundo por side
        let now = Utc::now();
        if !signals.is_empty() {
            if let Some(last) = self.last_alert_ts {
                if (now - last).num_milliseconds() < 800 {
                    return vec![]; // suprimir por cooldown
                }
            }
            self.last_alert_ts = Some(now);
            self.signal_count += 1;
        }

        signals
    }

    fn detect_wall(&self, bids: &[PriceLevel], asks: &[PriceLevel]) -> Option<Signal> {
        // Un wall es una orden grande (> wall_threshold) en el top 5 del book
        for level in bids.iter().take(5) {
            if level.size >= self.config.wall_threshold {
                return Some(self.make_signal(
                    PatternType::Wall { side: "bid".into() },
                    "medium",
                    format!("Bid wall: {:.0} shares @ {:.4}", level.size, level.price),
                    json!({"price": level.price, "size": level.size, "type": "bid_wall"}),
                ));
            }
        }
        for level in asks.iter().take(5) {
            if level.size >= self.config.wall_threshold {
                return Some(self.make_signal(
                    PatternType::Wall { side: "ask".into() },
                    "medium",
                    format!("Ask wall: {:.0} shares @ {:.4}", level.size, level.price),
                    json!({"price": level.price, "size": level.size, "type": "ask_wall"}),
                ));
            }
        }
        None
    }

    fn detect_spread_anomaly(&self, spread: f64) -> Option<Signal> {
        if self.recent_spreads.len() < 20 {
            return None;
        }
        let mean: f64 = self.recent_spreads.iter().sum::<f64>() / self.recent_spreads.len() as f64;
        let variance: f64 = self.recent_spreads.iter()
            .map(|s| (s - mean).powi(2))
            .sum::<f64>() / self.recent_spreads.len() as f64;
        let std_dev = variance.sqrt().max(0.0001);

        let threshold = mean + self.config.spread_std_multiplier * std_dev;

        if spread > threshold {
            Some(self.make_signal(
                PatternType::SpreadAnomaly,
                "high",
                format!("Spread anomaly: {:.4} (mean: {:.4}, σ: {:.4})", spread, mean, std_dev),
                json!({"spread": spread, "mean": mean, "std_dev": std_dev}),
            ))
        } else {
            None
        }
    }

    fn detect_imbalance(&self, bid_vol: f64, ask_vol: f64) -> Option<Signal> {
        if bid_vol < 1.0 || ask_vol < 1.0 {
            return None;
        }
        let ratio = if ask_vol > 0.0 { bid_vol / ask_vol } else { bid_vol };
        let reverse_ratio = if bid_vol > 0.0 { ask_vol / bid_vol } else { ask_vol };

        if ratio >= self.config.imbalance_ratio {
            Some(self.make_signal(
                PatternType::DepthImbalance { bias: "bid".into() },
                "medium",
                format!("Bid depth {:.1}x ask depth (bid: {:.0}, ask: {:.0})", ratio, bid_vol, ask_vol),
                json!({"bid_vol": bid_vol, "ask_vol": ask_vol, "ratio": ratio}),
            ))
        } else if reverse_ratio >= self.config.imbalance_ratio {
            Some(self.make_signal(
                PatternType::DepthImbalance { bias: "ask".into() },
                "medium",
                format!("Ask depth {:.1}x bid depth (ask: {:.0}, bid: {:.0})", reverse_ratio, ask_vol, bid_vol),
                json!({"bid_vol": bid_vol, "ask_vol": ask_vol, "ratio": reverse_ratio}),
            ))
        } else {
            None
        }
    }

    fn detect_spoof(&mut self, _best_bid: f64, best_bid_sz: f64, _best_ask: f64, best_ask_sz: f64) -> Option<Signal> {
        let now = Utc::now();
        let lookback = chrono::Duration::seconds(self.config.spoof_lookback_secs);

        // Guardar este tick
        self.wall_history.push_back((now, _best_bid, best_bid_sz.max(best_ask_sz)));

        // Limpiar entradas viejas
        while self.wall_history.front().map_or(false, |(ts, _, _)| now - *ts > lookback) {
            self.wall_history.pop_front();
        }

        // Buscar: una orden grande que ya no está
        let max_recent = self.wall_history.iter()
            .map(|(_, _, sz)| *sz)
            .fold(0.0_f64, f64::max);

        if max_recent >= self.config.wall_threshold
            && best_bid_sz < self.config.wall_threshold * 0.5
            && best_ask_sz < self.config.wall_threshold * 0.5
        {
            Some(self.make_signal(
                PatternType::Spoof,
                "high",
                format!("Possible spoof: wall of {:.0} shares disappeared", max_recent),
                json!({"max_recent_size": max_recent}),
            ))
        } else {
            None
        }
    }

    fn detect_momentum(&self) -> Option<Signal> {
        let window = self.config.momentum_window as usize;
        if self.recent_mids.len() < window + 10 {
            return None;
        }

        // Comparar media de los últimos N ticks vs media de los N ticks anteriores
        let recent: Vec<f64> = self.recent_mids.iter().rev().take(window).copied().collect();
        let older: Vec<f64> = self.recent_mids.iter().rev().skip(window).take(window).copied().collect();

        if recent.len() < window || older.len() < window {
            return None;
        }

        let recent_mean = recent.iter().sum::<f64>() / recent.len() as f64;
        let older_mean = older.iter().sum::<f64>() / older.len() as f64;

        let change_pct = ((recent_mean - older_mean) / older_mean) * 100.0;

        if change_pct.abs() > 0.5 {
            let direction = if change_pct > 0.0 { "up" } else { "down" };
            Some(self.make_signal(
                PatternType::MomentumShift { direction: direction.into() },
                "medium",
                format!("Momentum shift: {:.2}% ({})", change_pct, direction),
                json!({"change_pct": change_pct, "direction": direction, "recent_mean": recent_mean, "older_mean": older_mean}),
            ))
        } else {
            None
        }
    }

    fn make_signal(&self, pattern: PatternType, severity: &str, description: String, data: serde_json::Value) -> Signal {
        Signal {
            id: 0,
            pattern: pattern.as_str().into(),
            side: self.side.clone(),
            severity: severity.into(),
            description,
            data,
            ts: Utc::now(),
            created_at: Utc::now(),
        }
    }

    /// Convierte una Signal en una Alert para broadcast al frontend
    pub fn to_alert(&self, signal: &Signal, best_bid: f64, best_ask: f64) -> Alert {
        Alert {
            pattern:  signal.pattern.clone(),
            side:     signal.side.clone(),
            severity: signal.severity.clone(),
            message:  signal.description.clone(),
            price:    (best_bid + best_ask) / 2.0,
            ts:       signal.ts,
        }
    }
}

fn push_capped<T>(deque: &mut VecDeque<T>, value: T, max: usize) {
    deque.push_back(value);
    if deque.len() > max {
        deque.pop_front();
    }
}
