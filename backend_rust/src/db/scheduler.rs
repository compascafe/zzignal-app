use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};
use serde_json::json;
use chrono::Utc;

use crate::models::state::AppState;
use crate::controllers::worker::{CmdMsg, OrderSide, Outcome};
use crate::db::{api, repository};
use crate::services::risk::warmup_fetch_and_compute;

/// Corre en background:
///  1. Cada 10s guarda snapshot del order book en PostgreSQL
///  2. Cada 5s revisa scheduled_executions pendientes y las ejecuta
///  3. Cada 1s revisa sesiones de grabación programadas (inicia 5s antes, termina en scheduled_end)
pub async fn run_scheduler(state: Arc<AppState>) {
    info!("[SCHEDULER] Started — book=10s exec=5s session=1s");
    let mut book_timer   = tokio::time::interval(Duration::from_secs(10));
    let mut exec_timer   = tokio::time::interval(Duration::from_secs(5));
    let mut session_timer= tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            _ = book_timer.tick() => {
                save_book_snapshot(Arc::clone(&state)).await;
            }
            _ = exec_timer.tick() => {
                process_pending_executions(Arc::clone(&state)).await;
            }
            _ = session_timer.tick() => {
                let s = Arc::clone(&state);
                tokio::spawn(async move {
                    process_sessions(s).await;
                });
            }
        }
    }
}

async fn save_book_snapshot(state: Arc<AppState>) {
    let pool = state.db.as_ref();
    if pool.is_none() { return; }

    let book_up   = state.book_up.read().await.clone();
    let book_down = state.book_down.read().await.clone();

    for (side, book) in [("up", book_up), ("down", book_down)] {
        if let Some(b) = book {
            let best_bid    = b.bids.first().map(|l| l.price);
            let best_bid_sz = b.bids.first().map(|l| l.size);
            let best_ask    = b.asks.first().map(|l| l.price);
            let best_ask_sz = b.asks.first().map(|l| l.size);
            let spread = best_bid.and_then(|bb| best_ask.map(|ba| ba - bb));

            let depth_bids = json!(b.bids.iter().take(5).map(|l| json!({"p":l.price,"s":l.size})).collect::<Vec<_>>());
            let depth_asks = json!(b.asks.iter().take(5).map(|l| json!({"p":l.price,"s":l.size})).collect::<Vec<_>>());

            if let Err(e) = repository::insert_snapshot(
                pool, side, best_bid, best_bid_sz, best_ask, best_ask_sz, spread,
                Some(depth_bids), Some(depth_asks),
            ).await {
                warn!("DB snapshot {}: {}", side, e);
            }
        }
    }
}

async fn process_pending_executions(state: Arc<AppState>) {
    let pool = state.db.as_ref();
    if pool.is_none() { return; }

    let pending = match repository::query_pending_executions(pool).await {
        Ok(list) => list,
        Err(e)   => { warn!("Scheduler query pending: {}", e); return; }
    };

    for ex in pending {
        let id = ex.id;
        info!("Ejecutando scheduled #{}: {} {} @ {:?}", id, ex.side, ex.order_type, ex.price);

        let side    = parse_side(&ex.side);
        let outcome = parse_outcome(&ex.outcome);

        let result = match (side, outcome) {
            (Some(s), Some(o)) => {
                let cmd = match ex.order_type.as_str() {
                    "limit" => {
                        match (ex.price, ex.size) {
                            (Some(p), Some(sz)) => Some(CmdMsg::PlaceLimitOrder { side: s, outcome: o, price: p, size: sz }),
                            _ => None,
                        }
                    }
                    "market" => {
                        match ex.amount_usdc {
                            Some(amt) => Some(CmdMsg::PlaceMarketOrder { side: s, outcome: o, amount_usdc: amt }),
                            _ => None,
                        }
                    }
                    "scalp" => {
                        match (ex.price, ex.size, ex.target_price) {
                            (Some(p), Some(sz), Some(tp)) => Some(CmdMsg::ScalpBuy { outcome: o, price: p, size: sz, target_price: tp }),
                            _ => None,
                        }
                    }
                    _ => None,
                };

                match cmd {
                    Some(c) => {
                        let _ = state.cmd_tx.send(c);
                        Ok(())
                    }
                    None => Err("Parámetros incompletos".to_string()),
                }
            }
            _ => Err("side/outcome inválido".to_string()),
        };

        match result {
            Ok(_) => {
                if let Err(e) = repository::mark_executed(pool, id, None).await {
                    error!("Mark executed #{}: {}", id, e);
                }
            }
            Err(msg) => {
                warn!("Scheduled #{} falló: {}", id, msg);
                if let Err(e) = repository::mark_executed(pool, id, Some(&msg)).await {
                    error!("Mark failed #{}: {}", id, e);
                }
            }
        }
    }
}

async fn process_sessions(state: Arc<AppState>) {
    use std::collections::HashSet;
    // 1. Detener sesiones que ya pasaron su scheduled_end
    let mut to_stop = match repository::get_sessions_to_stop(&state).await {
        Ok(list) => list,
        Err(e)   => { warn!("Session scheduler stop query: {}", e); return; }
    };
    let mut seen: HashSet<i32> = to_stop.iter().map(|s| s.id).collect();

    // Also force-stop children that exceeded their duration (fallback for missed scheduled_end)
    if let Ok(orphans) = repository::get_stale_children(&state).await {
        for o in orphans {
            if seen.insert(o.id) {
                to_stop.push(o);
            }
        }
    }

    let mut parents_to_replenish: Vec<i32> = Vec::new();
    let mut stopped_ids: Vec<i32> = Vec::new(); // defer recording_sessions cleanup

    for session in to_stop {
        let btc_price = *state.btc_price.read().await;
        let parent_id = session.parent_id;
        info!("[SESSION STOP] #{} | dur={}min | end={}",
            session.id, session.duration_min,
            session.scheduled_end.format("%H:%M:%S"));

        // Compute actual outcome BEFORE stop (strike vs current BTC price)
        let actual_outcome = match (session.strike_price, btc_price) {
            (Some(strike), Some(current)) if current > strike => "up".to_string(),
            (Some(strike), Some(current)) if current < strike => "down".to_string(),
            (Some(_), Some(_)) => "tie".to_string(),
            _ => "tie".to_string(),
        };

        // ─── T-5 Certainty Strategy: resolve prediction ──────────────────────
        let final_poly = if actual_outcome == "up" { 1.0 } else if actual_outcome == "down" { 0.0 } else { 0.5 };
        let t5_result = state.t5_manager.on_session_close(session.id, &actual_outcome, final_poly);
        if let Some(ref snap) = t5_result {
            if !snap.prediction.is_empty() {
                let csv_path = state.session_manager.session_path(session.id);
                if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&csv_path) {
                    use std::io::Write;
                    let _ = writeln!(file, "# T5_RESULT: session_id={} prediction={} actual={} correct={} pnl={:.4} entry={:.4} exit={:?} vol={:.6}",
                        session.id, snap.prediction, snap.actual_outcome, snap.correct, snap.virtual_pnl,
                        snap.entry_price, snap.exit_price, snap.volatility_2min);
                }
            }
        }
        // ─── T-3 Aggressive Strategy: resolve prediction ────────────────────
        let t3_result = state.t3_manager.on_session_close(session.id, &actual_outcome, final_poly);
        if let Some(ref snap) = t3_result {
            if !snap.prediction.is_empty() {
                let csv_path = state.session_manager.session_path(session.id);
                if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&csv_path) {
                    use std::io::Write;
                    let _ = writeln!(file, "# T3_RESULT: session_id={} prediction={} actual={} correct={} pnl={:.4} entry={:.4} exit={:?}",
                        session.id, snap.prediction, snap.actual_outcome, snap.correct, snap.virtual_pnl,
                        snap.entry_price, snap.exit_price);
                }
            }
        }
        // ─── Hydra No Return: flush PNR data ────────────────────────────────
        state.pnr_manager.flush_session(session.id, &actual_outcome);
        // ─── Insight Strategies: Cerbero + Fenix ────────────────────────────
        state.insight_manager.on_session_close(session.id, &actual_outcome);
        // ─── Fenix Trading: settle paper trades ─────────────────────────────
        state.fenix_trading.on_session_close(session.id, &actual_outcome);
        // ─── Odiseo Trading: settle paper trades ────────────────────────────
        state.odiseo_trading.on_session_close(session.id, &actual_outcome);

        // ─── STOP in DB (always, before any other writes) ────────────────────
        if let Err(e) = repository::stop_session(&state, session.id, btc_price, btc_price).await {
            warn!("stop_session #{} FAILED: {}", session.id, e);
        } else {
            info!("[SESSION STOPPED] #{} → completed", session.id);
            stopped_ids.push(session.id);
        }
        if let Some(pid) = parent_id {
            parents_to_replenish.push(pid);
        }

        // ─── Feedback (runs in own spawn, lock().await is safe) ─────────────
        let mut eng = state.adaptive_engine.lock().await;
        let predicted_bias = eng.predicted_bias().to_string();
        if predicted_bias.is_empty() || predicted_bias == "IDLE" { continue; }

        let accuracy = predicted_bias.eq_ignore_ascii_case(&actual_outcome);

        // Insert/complete session log
        let start_price = session.btc_price_start;
        {
            let ctx = state.macro_ctx.read().await;
            let _ = repository::insert_session_log(
                state.db.as_ref(), session.id, &predicted_bias,
                Some(eng.macd_hist()), Some(eng.rsi_value()), Some(eng.vfi_value()),
                Some(eng.macro_slope()), start_price,
                Some(eng.cp_quantile()), Some(eng.cp_alpha()),
                Some(ctx.vfi_confidence), Some(ctx.db_accuracy_factor), Some(ctx.dynamic_rsi),
            ).await;
            let _ = repository::complete_session_log(
                state.db.as_ref(), session.id, &actual_outcome, accuracy,
            ).await;
        }

        // Reinforce learning
        eng.cp_mut().record_accuracy(accuracy);
        if !accuracy { eng.cp_mut().robbins_monro_update(0.5); }
        if eng.cp_mut().should_auto_widen() { eng.cp_mut().auto_widen(); }
        let vfi_sign = if eng.vfi_value() > 0.1 { 1.0 } else if eng.vfi_value() < -0.1 { -1.0 } else { 0.0 };
        let sma_sign = if eng.macro_slope() > 0.0001 { 1.0 } else if eng.macro_slope() < -0.0001 { -1.0 } else { 0.0 };
        eng.cp_mut().reinforce_weights(accuracy, vfi_sign, sma_sign);
        eng.cp_mut().adjust_confidence_level();
        drop(eng);
    }

    // 2. Auto-generar siguiente hijo para padres indefinidos
    for pid in parents_to_replenish {
        auto_generate_child(&state, pid).await;
    }

    // 3. Restart recovery: padres grabando sin hijos activos
    recover_orphaned_parents(&state).await;

    // 4. Iniciar sesiones programadas (multi-sesión: todas las que toque)
    let to_start = match repository::get_sessions_to_start(&state).await {
        Ok(list) => list,
        Err(e)   => { warn!("Session scheduler start query: {}", e); return; }
    };
    for session in to_start {
        let btc_price = *state.btc_price.read().await;
        info!("[SESSION START] Duration set to: {} minutes | Iniciando grabación sesión #{} (programada para {})", session.duration_min, session.id, session.scheduled_start.format("%H:%M:%S"));

        // Reset session baselines so normalize price_gap_ratio starts fresh
        state.tracking_state.reset_session_baselines();
        state.recording_sessions.write().await.push(session.id);

        // ─── Adaptive Risk Engine: refresh macro warm‑up + feedback from history ──
        {
            // Load recent accuracy from past sessions for CP feedback
            let recent_accuracy = repository::get_recent_accuracy(state.db.as_ref(), 16).await;
            let mut eng = state.adaptive_engine.lock().await;
            // Replay historical accuracy into CP engine (fast, no I/O)
            for correct in &recent_accuracy {
                eng.cp_mut().record_accuracy(*correct);
            }
            if eng.cp_mut().should_auto_widen() {
                eng.cp_mut().auto_widen();
            }
            // Pre-trade calibration: accuracy < 65% → widen CP
            let mut ctx = state.macro_ctx.write().await;
            eng.pre_trade_calibrate(&mut ctx, &recent_accuracy);
            // VFI-weighted bias with divergence detection
            eng.compute_weighted_bias(&mut ctx);
        }
        // Fire‑and‑forget: warmup refresh (lock‑free HTTP, only brief lock for apply)
        {
            let warm_state = Arc::clone(&state);
            let session_dur = session.duration_min;
            tokio::spawn(async move {
                info!("AdaptiveRiskEngine: refreshing warm‑up for {}-min session...", session_dur);
                match warmup_fetch_and_compute().await {
                    Ok(result) => {
                        warm_state.adaptive_engine.lock().await.apply_warmup_result(result);
                        info!("AdaptiveRiskEngine: warm‑up refreshed for {}-min session", session_dur);
                    }
                    Err(e) => warn!("AdaptiveRiskEngine session-start refresh failed: {e}"),
                }
            });
        }

        // New session: truncate and write fresh header
        if let Err(e) = state.session_manager.start_session(session.id, &session.name) {
            warn!("SessionManager start #{}: {}", session.id, e);
        }

        // ─── T-5 + T-3 Strategies: register session ───────────────────────
        state.t5_manager.on_session_start(session.id, session.scheduled_end);
        state.t3_manager.on_session_start(session.id, session.scheduled_end);

        // Drain tick channel to prevent residual events leaking (ring persists for warm lookbacks)
        state.tick_drain.store(true, std::sync::atomic::Ordering::Release);

        if let Err(e) = repository::start_session_recording(&state, session.id, btc_price).await {
            warn!("No se pudo iniciar sesión #{}: {}", session.id, e);
            state.recording_sessions.write().await.retain(|&sid| sid != session.id);
            let _ = state.session_manager.stop_session(session.id);
        } else {
            info!("Sesión #{} grabando. Strike price (BTC): {:?}", session.id, btc_price);
        }
    }

    // 5. Deferred cleanup: flush and close OLD sessions AFTER new ones are started
    for sid in &stopped_ids {
        let _ = state.session_manager.flush(*sid);
        state.session_manager.stop_session(*sid).ok();
    }
    state.recording_sessions.write().await.retain(|sid| !stopped_ids.contains(sid));
}

/// Crea el siguiente hijo para un padre indefinido si no tiene hijos activos.
/// La duración del chunk se lee de parent.duration_min.
async fn auto_generate_child(state: &AppState, parent_id: i32) {
    let parent = match repository::get_session_by_id(state, parent_id).await {
        Ok(Some(p)) => p,
        _ => return,
    };
    if parent.status != "recording" {
        return;
    }
    let chunk_min = parent.duration_min.max(1);
    let children = repository::list_session_children(state, parent_id).await.unwrap_or_default();
    let has_active = children.iter().any(|c| c.status == "recording" || c.status == "scheduled");
    if has_active {
        return;
    }
    let last_end = children.iter().map(|c| c.scheduled_end).max().unwrap_or_else(Utc::now);
    // Always align to chunk boundaries (15-min blocks: :00 :15 :30 :45)
    let start = snap_to_next_chunk(last_end, chunk_min);
    create_next_child(state, parent_id, parent.depth_levels, start, chunk_min).await;
}

/// Redondea a la siguiente frontera de chunk si no está ya alineado.
/// Ej: 12:55 con chunk=15 → 13:00. 13:15 con chunk=15 → 13:15 (ya alineado).
pub(crate) fn snap_to_next_chunk(ts: chrono::DateTime<Utc>, chunk_min: i32) -> chrono::DateTime<Utc> {
    let chunk_secs = (chunk_min as i64) * 60;
    let secs = ts.timestamp();
    if secs % chunk_secs == 0 {
        ts
    } else {
        let bucket = ((secs / chunk_secs) + 1) * chunk_secs;
        chrono::DateTime::from_timestamp(bucket, 0)
            .unwrap_or(ts + chrono::Duration::minutes(chunk_min as i64))
    }
}

/// Recupera padres indefinidos huérfanos (ej. tras reinicio del backend)
async fn recover_orphaned_parents(state: &AppState) {
    let parents = match repository::list_sessions(state, 200).await {
        Ok(list) => list,
        Err(_) => return,
    };
    for parent in &parents {
        if parent.status != "recording" || parent.parent_id.is_some() {
            continue;
        }
        let chunk_min = parent.duration_min.max(1);
        let children = repository::list_session_children(state, parent.id).await.unwrap_or_default();
        let has_active = children.iter().any(|c| c.status == "recording" || c.status == "scheduled");
        if has_active {
            continue;
        }
        let last_end = children.iter().map(|c| c.scheduled_end).max().unwrap_or_else(Utc::now);
        let next_start = snap_to_next_chunk(last_end, chunk_min);
        create_next_child(state, parent.id, parent.depth_levels, next_start, chunk_min).await;
    }
}

async fn create_next_child(state: &AppState, parent_id: i32, depth_levels: i32, start: chrono::DateTime<Utc>, chunk_min: i32) {
    // Belt-and-suspenders: always snap to chunk boundary regardless of caller
    let start = snap_to_next_chunk(start, chunk_min);
    let end = start + chrono::Duration::minutes(chunk_min as i64);
    let name = api::child_session_name(start, chunk_min);
    info!("Auto-gen child aligned → {} ({}→{})", name, start.format("%H:%M"), end.format("%H:%M"));
    match repository::create_session(state, &name, start, end, chunk_min, depth_levels, Some(parent_id)).await {
        Ok(child_id) => info!("Auto-gen child #{} for parent #{}: {}", child_id, parent_id, name),
        Err(e) => warn!("Failed to auto-gen child for parent #{}: {}", parent_id, e),
    }
}

fn parse_side(s: &str) -> Option<OrderSide> {
    match s.to_lowercase().as_str() {
        "buy"  => Some(OrderSide::Buy),
        "sell" => Some(OrderSide::Sell),
        _      => None,
    }
}

fn parse_outcome(s: &str) -> Option<Outcome> {
    match s.to_lowercase().as_str() {
        "up"   => Some(Outcome::Up),
        "down" => Some(Outcome::Down),
        _      => None,
    }
}
