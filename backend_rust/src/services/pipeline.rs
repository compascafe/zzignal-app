use chrono::Utc;

use crate::controllers::worker::PriceLevel;
use crate::models::hft::{CsvRecord, EventType, PolyDepthFrame};
use crate::models::state::AppState;
use crate::services::metrics;

pub async fn capture_book_db(state: &AppState, side: &str, bids: &[PriceLevel], asks: &[PriceLevel]) {
    let _ = (state, side, bids, asks);
}

pub async fn push_depth_frame(state: &AppState, side: u8, bids: &[PriceLevel], asks: &[PriceLevel]) {
    let mut history = state.poly_depth_history.write().await;
    let frame = PolyDepthFrame {
        ts_unix_ms: Utc::now().timestamp_millis(),
        side,
        bids: bids.to_vec(),
        asks: asks.to_vec(),
    };
    if history.len() >= 300 {
        history.pop_front();
    }
    history.push_back(frame);
}

pub async fn capture_combined(
    state: &AppState, _side: &str,
    poly_bids: &[PriceLevel], poly_asks: &[PriceLevel],
    evt_type: EventType, trade_side: &str, trade_price: f64, trade_size: f64,
) {
    let poly_ts = Utc::now().timestamp_millis();
    let binance_opt = state.binance_depth.read().await.clone();

    let mut rec = if let Some(ref binance) = binance_opt {
        match evt_type {
            EventType::BookUpdate => metrics::build_book_update(
                binance, &state.binance_ring, poly_bids, poly_asks, &state.tracking_state, poly_ts,
            ),
            EventType::Trade => metrics::build_trade_record(
                binance, &state.binance_ring, poly_bids, poly_asks, &state.tracking_state,
                poly_ts, trade_side, trade_price, trade_size,
            ),
            _ => unreachable!(),
        }
    } else {
        let pb_bid = poly_bids.first().map(|l| l.price).unwrap_or(0.0);
        let pb_ask = poly_asks.first().map(|l| l.price).unwrap_or(0.0);
        let pb_mid = if pb_bid > 0.0 && pb_ask > 0.0 { (pb_bid + pb_ask) / 2.0 } else { 0.0 };
        let pb_vol_bid: f64 = poly_bids.iter().map(|l| l.size).sum();
        let pb_vol_ask: f64 = poly_asks.iter().map(|l| l.size).sum();
        let pb_imb = if pb_vol_ask > 0.0 { pb_vol_bid / pb_vol_ask } else { 0.0 };
        let fallback_btc = *state.btc_price.read().await;

        let mut rec = CsvRecord {
            event_type: evt_type,
            ts_local: Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            poly_bid: pb_bid,
            poly_ask: pb_ask,
            poly_mid: pb_mid,
            poly_spread: if pb_bid > 0.0 && pb_ask > 0.0 { pb_ask - pb_bid } else { 0.0 },
            poly_bid_vol_all: pb_vol_bid,
            poly_ask_vol_all: pb_vol_ask,
            poly_imbalance: if pb_imb.is_finite() { pb_imb } else { 0.0 },
            ..Default::default()
        };
        if fallback_btc.unwrap_or(0.0) > 0.0 {
            rec.binance_price = fallback_btc.unwrap();
        }
        if evt_type == EventType::Trade {
        }
        rec
    };

    let session_ids = state.recording_sessions.read().await.clone();
    let active_sid = session_ids.last().copied()
        .or_else(|| state.session_manager.active_ids().last().copied())
        .unwrap_or(0);
    rec.session_id = active_sid;

    // ─── Trade Window Prices ──────────────────────────────────────────
    {
        let window_up = state.trade_window_up.read().await;
        let window_dn = state.trade_window_dn.read().await;
        if !window_up.is_empty() {
            let total: f64 = window_up.iter().map(|(p,_)| p).sum();
            rec.clob_trade_up = total / window_up.len() as f64;
            rec.clob_trade_up_vol = window_up.iter().map(|(_,s)| s).sum();
            rec.clob_trade_count_up = window_up.len() as u16;
        }
        if !window_dn.is_empty() {
            let total: f64 = window_dn.iter().map(|(p,_)| p).sum();
            rec.clob_trade_dn = total / window_dn.len() as f64;
            rec.clob_trade_dn_vol = window_dn.iter().map(|(_,s)| s).sum();
            rec.clob_trade_count_dn = window_dn.len() as u16;
        }
    }

    rec.poly_spread = rec.clob_trade_up - rec.clob_trade_dn;

    rec.pnr_seconds_left = 0;

    // ─── Anti-Flash Dump metrics ───────────────────────────────────────
    {
        use std::sync::atomic::AtomicI64;
        static LAST_TICK_MS: AtomicI64 = AtomicI64::new(0);
        let now_ms = Utc::now().timestamp_millis();
        let last = LAST_TICK_MS.swap(now_ms, std::sync::atomic::Ordering::Relaxed);
        rec.tick_gap_ms = if last > 0 { now_ms - last } else { 0 };

        {
            let dt_secs = (rec.tick_gap_ms as f64 / 1000.0).max(0.001);
            let prev_vel = *state.prev_btc_vel.read().await;
            rec.btc_acel = (rec.price_velocity - prev_vel) / dt_secs;
            *state.prev_btc_vel.write().await = rec.price_velocity;
        }

        {
            let alpha = 0.1;
            let abs_vel = rec.price_velocity.abs();
            let mut vol_ema = state.btc_vol_ema.write().await;
            *vol_ema = alpha * abs_vel + (1.0 - alpha) * *vol_ema;
            rec.btc_volatility = *vol_ema;
        }

        {
            let ob_vol = rec.poly_bid_vol_all + rec.poly_ask_vol_all;
            rec.btc_vol_ratio = if ob_vol > 0.0 { rec.btc_vol / ob_vol } else { 0.0 };
        }

        {
            let trade_vol = rec.clob_trade_up_vol + rec.clob_trade_dn_vol;
            rec.price_impact = if trade_vol > 0.0 {
                (rec.clob_trade_up - rec.clob_trade_dn).abs() / trade_vol.max(0.01)
            } else { 0.0 };
            let total_depth = rec.poly_bid_vol_all + rec.poly_ask_vol_all;
            rec.depth_concentration = if total_depth > 0.0 {
                rec.poly_bid_vol_all.max(rec.poly_ask_vol_all) / total_depth
            } else { 0.5 };
        }

        {
            use std::sync::atomic::AtomicU8;
            static ASK_WALL_COUNT: AtomicU8 = AtomicU8::new(0);
            let is_wall = rec.poly_ask_vol_all > 0.0 && rec.poly_ask_vol_all > rec.poly_bid_vol_all * 3.0;
            if is_wall {
                let c = ASK_WALL_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed).saturating_add(1);
                rec.ask_wall = if c >= 3 { 1 } else { 0 };
            } else {
                ASK_WALL_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
                rec.ask_wall = 0;
            }
        }
        rec.dump_score = if rec.poly_bid == 0.0 { 3 }
                    else if rec.tick_gap_ms > 2000 { 3 }
                    else if rec.ask_wall == 1 { 2 }
                    else if rec.tick_gap_ms > 500 { 1 }
                    else { 0 };

        {
            use std::sync::atomic::{AtomicU64, Ordering};
            static IMB_SUM: AtomicU64 = AtomicU64::new(0);
            static IMB_COUNT: AtomicU64 = AtomicU64::new(0);
            let bits = rec.poly_imbalance.to_bits();
            let prev_sum = IMB_SUM.fetch_add(bits, Ordering::Relaxed);
            let count = IMB_COUNT.fetch_add(1, Ordering::Relaxed).saturating_add(1);
            if count >= 3 {
                let avg_bits = (prev_sum + bits) / 3;
                rec.poly_imbalance = f64::from_bits(avg_bits);
                IMB_SUM.store(avg_bits * 3, Ordering::Relaxed);
                IMB_COUNT.store(2, Ordering::Relaxed);
            } else {
                rec.poly_imbalance = if count > 1 { f64::from_bits(prev_sum / count) } else { rec.poly_imbalance };
            }
        }

        {
            use std::sync::atomic::AtomicU8;
            static SPOOF_COUNT: AtomicU8 = AtomicU8::new(0);
            if rec.spoofing_flag == 1 {
                let c = SPOOF_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed).saturating_add(1);
                rec.spoofing_flag = if c >= 3 { 1 } else { 0 };
            } else {
                SPOOF_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
                rec.spoofing_flag = 0;
            }
        }
    }

    // ─── LIVE money trace ──────────────────────────────────────────────
    rec.btc_vol = *state.btc_volume.read().await;

    // ─── Token momentum ───────────────────────────────────────────────
    {
        let raw_up = *state.raw_trade_up.read().await;
        let raw_dn = *state.raw_trade_dn.read().await;
        let prev_up = *state.prev_raw_up.read().await;
        let prev_dn = *state.prev_raw_dn.read().await;
        rec.token_momentum = if raw_up > 0.0 && prev_up > 0.0 { raw_up - prev_up }
                        else if raw_dn > 0.0 && prev_dn > 0.0 { raw_dn - prev_dn }
                        else { 0.0 };
    }

    // ─── Update latest HFT state ───────────────────────────────────────
    {
        let mut hft = state.latest_hft.write().await;
        hft.time = rec.ts_local.clone();
        hft.event = rec.event_type.as_str().to_string();
        hft.btc_price = rec.binance_price;
        hft.mid = rec.poly_mid;
        hft.spread = rec.poly_spread;
        hft.imbalance = rec.poly_imbalance;
        hft.bid_vol = rec.poly_bid_vol_all;
        hft.ask_vol = rec.poly_ask_vol_all;
        hft.clob_trade_up = rec.clob_trade_up;
        hft.clob_trade_dn = rec.clob_trade_dn;
        hft.clob_trade_up_vol = rec.clob_trade_up_vol;
        hft.clob_trade_dn_vol = rec.clob_trade_dn_vol;
        hft.btc_vel = rec.price_velocity;
        hft.btc_acel = rec.btc_acel;
        hft.btc_volatility = rec.btc_volatility;
        hft.btc_volume_24h = rec.binance_vol_24h;
        hft.btc_vol = rec.btc_vol;
        hft.btc_vol_1m = *state.btc_vol_1m.read().await;
        hft.btc_vol_ses = *state.btc_vol_ses.read().await;
        hft.spoof = rec.spoofing_flag;
        hft.dump_score = rec.dump_score;
        hft.ask_wall = rec.ask_wall;
        hft.tick_gap_ms = rec.tick_gap_ms;
        hft.secs_left = rec.pnr_seconds_left;

        // ── Update btc_open on session boundary ──
        // Detects when secs_left jumps from near 0 to ~900 (new 15-min session),
        // refreshing the "price to beat" reference used by the dashboard delta.
        {
            use std::sync::atomic::{AtomicI32, Ordering};
            static PREV_SESSION_SECS: AtomicI32 = AtomicI32::new(-1);
            let prev = PREV_SESSION_SECS.swap(rec.pnr_seconds_left, Ordering::Relaxed);
            if prev >= 0 && rec.pnr_seconds_left > prev + 60 && rec.binance_price > 0.0 {
                *state.btc_open.write().await = Some(rec.binance_price);
            }
            if prev < 0 && rec.binance_price > 0.0 {
                // Initial set on first tick if not yet from Gamma/Pyth
                let current = *state.btc_open.read().await;
                if current.is_none() {
                    *state.btc_open.write().await = Some(rec.binance_price);
                }
            }
        }

        {
            let up = state.book_up.read().await;
            let dn = state.book_down.read().await;

            use std::sync::atomic::AtomicU64;
            static PREV_UP_BEST_BID: AtomicU64 = AtomicU64::new(0);
            static PREV_UP_BEST_ASK: AtomicU64 = AtomicU64::new(0);
            static PREV_DN_BEST_BID: AtomicU64 = AtomicU64::new(0);
            static PREV_DN_BEST_ASK: AtomicU64 = AtomicU64::new(0);

            let (up_best_bid_sz, up_best_ask_sz, dn_best_bid_sz, dn_best_ask_sz, up_bid_px, up_ask_px, dn_bid_px, dn_ask_px) =
            {
                let mut ubb: f64 = 0.0; let mut ubp: f64 = 0.0;
                let mut uba: f64 = 0.0; let mut uap: f64 = 0.0;
                let mut dbb: f64 = 0.0; let mut dbp: f64 = 0.0;
                let mut dba: f64 = 0.0; let mut dap: f64 = 0.0;
                if let Some(ref b) = *up {
                    hft.depth_up_bids = b.bids.iter().take(30).map(|l| (l.price, l.size)).collect();
                    hft.depth_up_asks = b.asks.iter().take(30).map(|l| (l.price, l.size)).collect();
                    if let Some(bb) = b.bids.first() { ubb = bb.size; ubp = bb.price; }
                    if let Some(ba) = b.asks.first() { uba = ba.size; uap = ba.price; }
                }
                if let Some(ref b) = *dn {
                    hft.depth_dn_bids = b.bids.iter().take(30).map(|l| (l.price, l.size)).collect();
                    hft.depth_dn_asks = b.asks.iter().take(30).map(|l| (l.price, l.size)).collect();
                    if let Some(bb) = b.bids.first() { dbb = bb.size; dbp = bb.price; }
                    if let Some(ba) = b.asks.first() { dba = ba.size; dap = ba.price; }
                }
                (ubb, uba, dbb, dba, ubp, uap, dbp, dap)
            };

            let prev_ubb = f64::from_bits(PREV_UP_BEST_BID.swap(up_best_bid_sz.to_bits(), std::sync::atomic::Ordering::Relaxed));
            let prev_uba = f64::from_bits(PREV_UP_BEST_ASK.swap(up_best_ask_sz.to_bits(), std::sync::atomic::Ordering::Relaxed));
            let prev_dbb = f64::from_bits(PREV_DN_BEST_BID.swap(dn_best_bid_sz.to_bits(), std::sync::atomic::Ordering::Relaxed));
            let prev_dba = f64::from_bits(PREV_DN_BEST_ASK.swap(dn_best_ask_sz.to_bits(), std::sync::atomic::Ordering::Relaxed));
            hft.ofi_up = (up_best_bid_sz - prev_ubb) - (up_best_ask_sz - prev_uba);
            hft.ofi_dn = (dn_best_bid_sz - prev_dbb) - (dn_best_ask_sz - prev_dba);

            hft.micro_price_up = if up_best_bid_sz + up_best_ask_sz > 0.0 && up_bid_px > 0.0 && up_ask_px > 0.0 {
                (up_bid_px * up_best_ask_sz + up_ask_px * up_best_bid_sz) / (up_best_bid_sz + up_best_ask_sz)
            } else { up_bid_px.max(up_ask_px).max(0.0) };
            hft.micro_price_dn = if dn_best_bid_sz + dn_best_ask_sz > 0.0 && dn_bid_px > 0.0 && dn_ask_px > 0.0 {
                (dn_bid_px * dn_best_ask_sz + dn_ask_px * dn_best_bid_sz) / (dn_best_bid_sz + dn_best_ask_sz)
            } else { dn_bid_px.max(dn_ask_px).max(0.0) };
        }
    }

    // ── Sanitize NaN/Infinity before broadcast (prevents JSON deser failures) ──
    {
        let mut hft = state.latest_hft.write().await;
        if hft.btc_volatility.is_nan() || hft.btc_volatility.is_infinite() { hft.btc_volatility = 0.0; }
        if hft.imbalance.is_nan() || hft.imbalance.is_infinite() { hft.imbalance = 0.0; }
        if hft.sen_clob_delta.is_nan() || hft.sen_clob_delta.is_infinite() { hft.sen_clob_delta = 0.0; }
        if hft.sen_btc_vel.is_nan() || hft.sen_btc_vel.is_infinite() { hft.sen_btc_vel = 0.0; }
        if hft.ofi_up.is_nan() || hft.ofi_up.is_infinite() { hft.ofi_up = 0.0; }
        if hft.ofi_dn.is_nan() || hft.ofi_dn.is_infinite() { hft.ofi_dn = 0.0; }
    }

    if let Ok(json) = serde_json::to_string(&*state.latest_hft.read().await) {
        let _ = state.broadcast_tx.send(format!("{{\"type\":\"hft_state\",\"data\":{json}}}"));
    }

    // ─── Cross-book imbalance depth profile ────────────────────────────
    {
        let dn_book = state.book_down.read().await;
        let up_book = state.book_up.read().await;

        let up_bids_all: Vec<(f64, f64)> = up_book.as_ref()
            .map(|b| b.bids.iter().map(|l| (l.price, l.size)).collect())
            .unwrap_or_default();
        let up_asks_all: Vec<(f64, f64)> = up_book.as_ref()
            .map(|b| b.asks.iter().map(|l| (l.price, l.size)).collect())
            .unwrap_or_default();
        let dn_bids_all: Vec<(f64, f64)> = dn_book.as_ref()
            .map(|b| b.bids.iter().map(|l| (l.price, l.size)).collect())
            .unwrap_or_default();
        let dn_asks_all: Vec<(f64, f64)> = dn_book.as_ref()
            .map(|b| b.asks.iter().map(|l| (l.price, l.size)).collect())
            .unwrap_or_default();

        let cross_book = |n: usize| -> f64 {
            let ub: f64 = up_bids_all.iter().take(n).map(|(_, s)| s).sum();
            let ua: f64 = up_asks_all.iter().take(n).map(|(_, s)| s).sum();
            let db: f64 = dn_bids_all.iter().take(n).map(|(_, s)| s).sum();
            let da: f64 = dn_asks_all.iter().take(n).map(|(_, s)| s).sum();
            let bull = ub + da;
            let bear = ua + db;
            if bear > 0.0 { bull / bear } else { 1.0 }
        };

        rec.comb_imb_d10 = cross_book(10);
        rec.comb_imb_d20 = cross_book(20);
        rec.comb_imb_d30 = cross_book(30);
        rec.imb_gradient = rec.comb_imb_d30 - rec.comb_imb_d10;

        // Imbalance velocity & acceleration (static trackers)
        {
            use std::sync::atomic::{AtomicU64, AtomicI64, Ordering};
            static PREV_IMB: AtomicU64 = AtomicU64::new(f64::to_bits(1.0));
            static PREV_VEL: AtomicU64 = AtomicU64::new(0u64);
            static PREV_TS:  AtomicI64 = AtomicI64::new(0);
            let now_ms = chrono::Utc::now().timestamp_millis();

            let prev_imb = f64::from_bits(PREV_IMB.swap(rec.comb_imb_d10.to_bits(), Ordering::Relaxed));
            let prev_vel = f64::from_bits(PREV_VEL.load(Ordering::Relaxed));
            let prev_ts  = PREV_TS.swap(now_ms, Ordering::Relaxed);

            let dt = ((now_ms - prev_ts) as f64 / 1000.0).max(0.01);
            rec.imb_velocity = (rec.comb_imb_d10 - prev_imb) / dt;
            rec.imb_accel = (rec.imb_velocity - prev_vel) / dt;
            PREV_VEL.store(rec.imb_velocity.to_bits(), Ordering::Relaxed);
        }

        // Wall score: max deviation from average in best 30 levels across all 4 books
        {
            let all_vols: Vec<f64> = up_bids_all.iter().take(30).map(|(_, s)| *s)
                .chain(up_asks_all.iter().take(30).map(|(_, s)| *s))
                .chain(dn_bids_all.iter().take(30).map(|(_, s)| *s))
                .chain(dn_asks_all.iter().take(30).map(|(_, s)| *s))
                .collect();
            if !all_vols.is_empty() {
                let avg: f64 = all_vols.iter().sum::<f64>() / all_vols.len() as f64;
                if avg > 0.0 {
                    let (max_val, max_idx) = all_vols.iter().enumerate()
                        .fold((0.0f64, 0usize), |(m, mi), (i, &v)| if v > m { (v, i) } else { (m, mi) });
                    rec.wall_score = max_val / avg;

                    let n30 = 30.min(up_bids_all.len());
                    if max_idx < n30 { rec.wall_side = 1; }
                    else if max_idx < n30*2 { rec.wall_side = 2; }
                    else if max_idx < n30*3 { rec.wall_side = 3; }
                    else { rec.wall_side = 4; }
                }
            }
        }
    }

    state.session_manager.push(&rec);

    state.mem_hft.write().await.push(rec.clone());

    // DB removed — no-op

}

pub async fn capture_fills_csv(state: &AppState, fills: &[crate::controllers::worker::RecentFill]) {
    let up_book   = state.book_up.read().await.clone();
    let down_book = state.book_down.read().await.clone();

    for fill in fills {
        let (bids, asks) = match fill.outcome.as_str() {
            "Up" | "up" => (
                up_book.as_ref().map(|b| &b.bids[..]).unwrap_or(&[]),
                up_book.as_ref().map(|b| &b.asks[..]).unwrap_or(&[]),
            ),
            _ => (
                down_book.as_ref().map(|b| &b.bids[..]).unwrap_or(&[]),
                down_book.as_ref().map(|b| &b.asks[..]).unwrap_or(&[]),
            ),
        };

        let trade_side = match fill.side {
            crate::controllers::worker::OrderSide::Buy => "BUY",
            crate::controllers::worker::OrderSide::Sell => "SELL",
        };

        capture_combined(
            state, &fill.outcome, bids, asks,
            EventType::Trade, trade_side, fill.price, fill.size,
        ).await;
    }
}
