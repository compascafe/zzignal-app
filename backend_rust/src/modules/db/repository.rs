use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;

use crate::modules::core::state::AppState;
use crate::modules::db::models::{ScheduledExecution, RecordingSession, SessionSnapshot, SessionTrade};

/// Explicit column list for RecordingSession queries.
/// Uses COALESCE for tag/tag_color so queries work even before migration 009/010.
const SESS_COLS: &str = "SELECT id, parent_id, name, scheduled_start, scheduled_end, started_at, stopped_at,\
    duration_min, market_id, market_title, capture_mode, depth_levels,\
    strike_price, final_price, outcome_result, btc_price_start, btc_price_end,\
    status, tick_count, trade_count, tag, tag_color, created_at FROM recording_sessions";

// ─── Order Book Snapshots ────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct OrderBookSnapshotRow {
    pub id:          i32,
    pub ts:          DateTime<Utc>,
    pub side:        String,
    pub best_bid:    Option<f64>,
    pub best_bid_sz: Option<f64>,
    pub best_ask:    Option<f64>,
    pub best_ask_sz: Option<f64>,
    pub spread:      Option<f64>,
    pub depth_bids:  Option<Value>,
    pub depth_asks:  Option<Value>,
}

pub async fn insert_snapshot(
    pool: Option<&PgPool>,
    side: &str,
    best_bid: Option<f64>,
    best_bid_sz: Option<f64>,
    best_ask: Option<f64>,
    best_ask_sz: Option<f64>,
    spread: Option<f64>,
    depth_bids: Option<Value>,
    depth_asks: Option<Value>,
) -> Result<()> {
    let Some(pool) = pool else { return Ok(()) };
    sqlx::query(
        r#"
        INSERT INTO order_book_snapshots
            (side, best_bid, best_bid_sz, best_ask, best_ask_sz, spread, depth_bids, depth_asks)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(side)
    .bind(best_bid)
    .bind(best_bid_sz)
    .bind(best_ask)
    .bind(best_ask_sz)
    .bind(spread)
    .bind(depth_bids)
    .bind(depth_asks)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn query_snapshots(
    pool:     Option<&PgPool>,
    side:     Option<&str>,
    limit:    i64,
    from:     Option<DateTime<Utc>>,
    to:       Option<DateTime<Utc>>,
) -> Result<Vec<OrderBookSnapshotRow>> {
    let Some(pool) = pool else { return Ok(vec![]) };
    let rows = sqlx::query_as::<_, OrderBookSnapshotRow>(
        r#"
        SELECT id, ts, side, best_bid, best_bid_sz, best_ask, best_ask_sz, spread, depth_bids, depth_asks
        FROM   order_book_snapshots
        WHERE  ($1::varchar IS NULL OR side = $1)
          AND  ($2::timestamptz IS NULL OR ts >= $2)
          AND  ($3::timestamptz IS NULL OR ts <= $3)
        ORDER  BY ts DESC
        LIMIT  $4
        "#,
    )
    .bind(side)
    .bind(from)
    .bind(to)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn query_latest_snapshot(pool: Option<&PgPool>, side: &str) -> Result<Option<OrderBookSnapshotRow>> {
    let Some(pool) = pool else { return Ok(None) };
    let row = sqlx::query_as::<_, OrderBookSnapshotRow>(
        r#"
        SELECT id, ts, side, best_bid, best_bid_sz, best_ask, best_ask_sz, spread, depth_bids, depth_asks
        FROM   order_book_snapshots
        WHERE  side = $1
        ORDER  BY ts DESC
        LIMIT  1
        "#,
    )
    .bind(side)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

// ─── Scheduled Executions ────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ScheduledExecutionRow {
    pub id:            i32,
    pub created_at:    DateTime<Utc>,
    pub scheduled_at:  DateTime<Utc>,
    pub executed_at:   Option<DateTime<Utc>>,
    pub status:        String,
    pub side:          String,
    pub outcome:       String,
    pub order_type:    String,
    pub price:         Option<f64>,
    pub size:          Option<f64>,
    pub amount_usdc:   Option<f64>,
    pub target_price:  Option<f64>,
    pub notes:         Option<String>,
    pub error_message: Option<String>,
}

pub async fn insert_execution(pool: Option<&PgPool>, e: &ScheduledExecution) -> Result<i32> {
    let Some(pool) = pool else { return Ok(0) };
    let row: (i32,) = sqlx::query_as(
        r#"
        INSERT INTO scheduled_executions
            (scheduled_at, status, side, outcome, order_type, price, size, amount_usdc, target_price, notes)
        VALUES ($1, 'pending', $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING id
        "#,
    )
    .bind(e.scheduled_at)
    .bind(&e.side)
    .bind(&e.outcome)
    .bind(&e.order_type)
    .bind(e.price)
    .bind(e.size)
    .bind(e.amount_usdc)
    .bind(e.target_price)
    .bind(&e.notes)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn query_executions(
    pool:   Option<&PgPool>,
    status: Option<&str>,
    limit:  i64,
) -> Result<Vec<ScheduledExecutionRow>> {
    let Some(pool) = pool else { return Ok(vec![]) };
    let rows = sqlx::query_as::<_, ScheduledExecutionRow>(
        r#"
        SELECT id, created_at, scheduled_at, executed_at, status, side, outcome, order_type,
               price, size, amount_usdc, target_price, notes, error_message
        FROM   scheduled_executions
        WHERE  ($1::varchar IS NULL OR status = $1)
        ORDER  BY scheduled_at DESC
        LIMIT  $2
        "#,
    )
    .bind(status)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn query_pending_executions(pool: Option<&PgPool>) -> Result<Vec<ScheduledExecutionRow>> {
    let Some(pool) = pool else { return Ok(vec![]) };
    let rows = sqlx::query_as::<_, ScheduledExecutionRow>(
        r#"
        SELECT id, created_at, scheduled_at, executed_at, status, side, outcome, order_type,
               price, size, amount_usdc, target_price, notes, error_message
        FROM   scheduled_executions
        WHERE  status = 'pending' AND scheduled_at <= NOW()
        ORDER  BY scheduled_at ASC
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn mark_executed(pool: Option<&PgPool>, id: i32, error: Option<&str>) -> Result<()> {
    let Some(pool) = pool else { return Ok(()) };
    let status = if error.is_some() { "failed" } else { "executed" };
    sqlx::query(
        "UPDATE scheduled_executions SET status=$1, executed_at=NOW(), error_message=$3 WHERE id=$2"
    )
    .bind(status)
    .bind(id)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn cancel_execution(pool: Option<&PgPool>, id: i32) -> Result<()> {
    let Some(pool) = pool else { return Ok(()) };
    sqlx::query("UPDATE scheduled_executions SET status='cancelled' WHERE id=$1 AND status='pending'")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_execution(pool: Option<&PgPool>, id: i32) -> Result<()> {
    let Some(pool) = pool else { return Ok(()) };
    sqlx::query("DELETE FROM scheduled_executions WHERE id=$1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ─── Recording Sessions (DB + In-Memory fallback) ────────────────────────────

fn next_mem_id(sessions: &[RecordingSession]) -> i32 {
    sessions.iter().map(|s| s.id).max().unwrap_or(0) + 1
}

fn next_snap_id(snapshots: &[SessionSnapshot]) -> i32 {
    snapshots.iter().map(|s| s.id).max().unwrap_or(0) + 1
}

fn next_trade_id(trades: &[SessionTrade]) -> i32 {
    trades.iter().map(|s| s.id).max().unwrap_or(0) + 1
}

pub async fn create_session(state: &AppState, name: &str, scheduled_start: DateTime<Utc>, scheduled_end: DateTime<Utc>, duration_min: i32, depth_levels: i32, parent_id: Option<i32>) -> Result<i32> {
    if let Some(pool) = state.db.as_ref() {
        let row: (i32,) = sqlx::query_as(
            r#"
            INSERT INTO recording_sessions (name, scheduled_start, scheduled_end, duration_min, depth_levels, status, parent_id)
            VALUES ($1, $2, $3, $4, $5, 'scheduled', $6)
            RETURNING id
            "#,
        )
        .bind(name)
        .bind(scheduled_start)
        .bind(scheduled_end)
        .bind(duration_min)
        .bind(depth_levels)
        .bind(parent_id)
        .fetch_one(pool)
        .await?;
        return Ok(row.0);
    }

    // In-memory fallback
    let mut sessions = state.mem_sessions.write().await;
    let id = next_mem_id(&sessions);
    sessions.push(RecordingSession {
        id,
        parent_id,
        name: name.into(),
        scheduled_start,
        scheduled_end,
        started_at: None,
        stopped_at: None,
        duration_min,
        market_id: None,
        market_title: None,
        capture_mode: "tick".into(),
        depth_levels,
        strike_price: None,
        final_price: None,
        outcome_result: None,
        btc_price_start: None,
        btc_price_end: None,
        status: "scheduled".into(),
        tick_count: 0,
        trade_count: 0,
        tag: None,
        tag_color: "#3b82f6".into(),
        created_at: Utc::now(),
    });
    Ok(id)
}

/// Creates a parent session and auto-splits into N children of chunk_duration_min each.
/// Returns (parent_id, Vec<child_id>)
pub async fn create_session_batch(state: &AppState, name: &str, scheduled_start: DateTime<Utc>, total_duration_min: i32, depth_levels: i32, chunk_duration_min: i32) -> Result<(i32, Vec<i32>)> {
    use chrono::Duration;

    let scheduled_end = scheduled_start + Duration::minutes(total_duration_min as i64);

    // 1. Create parent session
    let parent_id = create_session(state, name, scheduled_start, scheduled_end, total_duration_min, depth_levels, None).await?;

    // 2. Create children (each chunk_duration_min long)
    let num_children = (total_duration_min as f64 / chunk_duration_min as f64).ceil() as i32;
    let mut child_ids = Vec::with_capacity(num_children as usize);
    let slot = chunk_duration_min.max(1);

    for i in 0..num_children {
        let child_start = scheduled_start + Duration::minutes((i * slot) as i64);
        let child_end   = scheduled_start + Duration::minutes(((i + 1) * slot) as i64);
        // Cap last child at parent end
        let child_end = if child_end > scheduled_end { scheduled_end } else { child_end };
        let child_dur = ((child_end - child_start).num_seconds() / 60).max(1) as i32;
        let child_name = format!("{}-{:02}", name, i + 1);
        let child_id = create_session(state, &child_name, child_start, child_end, child_dur, depth_levels, Some(parent_id)).await?;
        child_ids.push(child_id);
    }

    Ok((parent_id, child_ids))
}

/// Lists children of a parent session
pub async fn list_session_children(state: &AppState, parent_id: i32) -> Result<Vec<RecordingSession>> {
    if let Some(pool) = state.db.as_ref() {
        let rows = sqlx::query_as::<_, RecordingSession>(
            &format!("{SESS_COLS} WHERE parent_id = $1 ORDER BY scheduled_start ASC")
        )
        .bind(parent_id)
        .fetch_all(pool)
        .await?;
        return Ok(rows);
    }

    let sessions = state.mem_sessions.read().await;
    let mut children: Vec<_> = sessions.iter()
        .filter(|s| s.parent_id == Some(parent_id))
        .cloned()
        .collect();
    children.sort_by(|a, b| a.scheduled_start.cmp(&b.scheduled_start));
    Ok(children)
}

pub async fn get_active_session(state: &AppState) -> Result<Option<RecordingSession>> {
    if let Some(pool) = state.db.as_ref() {
        let row = sqlx::query_as::<_, RecordingSession>(
            &format!("{SESS_COLS} WHERE status = 'recording' ORDER BY scheduled_start DESC LIMIT 1")
        )
        .fetch_optional(pool)
        .await?;
        return Ok(row);
    }

    let sessions = state.mem_sessions.read().await;
    Ok(sessions.iter()
        .find(|s| s.status == "recording")
        .cloned())
}

pub async fn get_session_by_id(state: &AppState, session_id: i32) -> Result<Option<RecordingSession>> {
    if let Some(pool) = state.db.as_ref() {
        let row = sqlx::query_as::<_, RecordingSession>(
            &format!("{SESS_COLS} WHERE id = $1")
        )
        .bind(session_id)
        .fetch_optional(pool)
        .await?;
        return Ok(row);
    }
    let sessions = state.mem_sessions.read().await;
    Ok(sessions.iter().find(|s| s.id == session_id).cloned())
}

pub async fn list_sessions(state: &AppState, limit: i64) -> Result<Vec<RecordingSession>> {
    if let Some(pool) = state.db.as_ref() {
        let rows = sqlx::query_as::<_, RecordingSession>(
            &format!("{SESS_COLS} ORDER BY scheduled_start DESC LIMIT $1")
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;
        return Ok(rows);
    }

    let sessions = state.mem_sessions.read().await;
    let mut result: Vec<_> = sessions.clone();
    result.sort_by(|a, b| b.scheduled_start.cmp(&a.scheduled_start));
    let limit = limit.max(0) as usize;
    if result.len() > limit { result.truncate(limit); }
    Ok(result)
}

pub async fn get_sessions_to_start(state: &AppState) -> Result<Vec<RecordingSession>> {
    if let Some(pool) = state.db.as_ref() {
        let rows = sqlx::query_as::<_, RecordingSession>(
            &format!("{SESS_COLS} WHERE status = 'scheduled' AND scheduled_start <= NOW() + INTERVAL '5 seconds' ORDER BY scheduled_start ASC")
        )
        .fetch_all(pool)
        .await?;
        return Ok(rows);
    }

    let now = Utc::now();
    let threshold = now + chrono::Duration::seconds(5);
    let sessions = state.mem_sessions.read().await;
    Ok(sessions.iter()
        .filter(|s| s.status == "scheduled" && s.scheduled_start <= threshold)
        .cloned()
        .collect())
}

pub async fn start_session_recording(state: &AppState, id: i32, btc_price: Option<f64>) -> Result<()> {
    if let Some(pool) = state.db.as_ref() {
        sqlx::query(
            r#"
            UPDATE recording_sessions
            SET status = 'recording',
                started_at = NOW(),
                strike_price = $2,
                btc_price_start = $2
            WHERE id = $1 AND status = 'scheduled'
            "#
        )
        .bind(id)
        .bind(btc_price)
        .execute(pool)
        .await?;
        return Ok(());
    }

    let mut sessions = state.mem_sessions.write().await;
    if let Some(s) = sessions.iter_mut().find(|s| s.id == id && s.status == "scheduled") {
        s.status = "recording".into();
        s.started_at = Some(Utc::now());
        s.strike_price = btc_price;
        s.btc_price_start = btc_price;
    }
    Ok(())
}

pub async fn get_sessions_to_stop(state: &AppState) -> Result<Vec<RecordingSession>> {
    if let Some(pool) = state.db.as_ref() {
        let rows = sqlx::query_as::<_, RecordingSession>(
            &format!("{SESS_COLS} WHERE status = 'recording' AND scheduled_end <= NOW() ORDER BY scheduled_end ASC")
        )
        .fetch_all(pool)
        .await?;
        return Ok(rows);
    }

    let now = Utc::now();
    let sessions = state.mem_sessions.read().await;
    Ok(sessions.iter()
        .filter(|s| s.status == "recording" && s.scheduled_end <= now)
        .cloned()
        .collect())
}

pub async fn stop_session(state: &AppState, id: i32, final_price: Option<f64>, btc_price_end: Option<f64>) -> Result<()> {
    if let Some(pool) = state.db.as_ref() {
        sqlx::query(
            r#"
            UPDATE recording_sessions
            SET status = 'completed',
                stopped_at = NOW(),
                final_price = $2,
                btc_price_end = $3,
                outcome_result = CASE
                    WHEN strike_price IS NULL OR $2 IS NULL THEN NULL
                    WHEN $2 > strike_price THEN 'up'
                    WHEN $2 < strike_price THEN 'down'
                    ELSE 'tie'
                END
            WHERE id = $1 AND status = 'recording'
            "#
        )
        .bind(id)
        .bind(final_price)
        .bind(btc_price_end)
        .execute(pool)
        .await?;
        return Ok(());
    }

    let mut sessions = state.mem_sessions.write().await;
    if let Some(s) = sessions.iter_mut().find(|s| s.id == id && s.status == "recording") {
        s.status = "completed".into();
        s.stopped_at = Some(Utc::now());
        s.final_price = final_price;
        s.btc_price_end = btc_price_end;
        s.outcome_result = match (s.strike_price, final_price) {
            (Some(strike), Some(final_p)) => {
                if final_p > strike { Some("up".into()) }
                else if final_p < strike { Some("down".into()) }
                else { Some("tie".into()) }
            }
            _ => None,
        };
    }
    Ok(())
}

pub async fn delete_session(state: &AppState, id: i32) -> Result<()> {
    if let Some(pool) = state.db.as_ref() {
        sqlx::query("DELETE FROM recording_sessions WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        return Ok(());
    }

    let mut sessions = state.mem_sessions.write().await;
    sessions.retain(|s| s.id != id);
    let mut snapshots = state.mem_snapshots.write().await;
    snapshots.retain(|s| s.session_id != id);
    let mut trades = state.mem_trades.write().await;
    trades.retain(|t| t.session_id != id);
    Ok(())
}

pub async fn update_session_tag(state: &AppState, id: i32, tag: Option<String>, tag_color: Option<String>) -> Result<bool> {
    if let Some(pool) = state.db.as_ref() {
        let rows = sqlx::query(
            "UPDATE recording_sessions SET tag = COALESCE($1, tag), tag_color = COALESCE($2, tag_color) WHERE id = $3"
        )
        .bind(&tag)
        .bind(&tag_color)
        .bind(id)
        .execute(pool)
        .await?;
        return Ok(rows.rows_affected() > 0);
    }
    let mut sessions = state.mem_sessions.write().await;
    if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
        if let Some(t) = tag { s.tag = Some(t); }
        if let Some(c) = tag_color { s.tag_color = c; }
        return Ok(true);
    }
    Ok(false)
}

// ─── Session Snapshots (DB + In-Memory) ──────────────────────────────────────

pub async fn insert_session_snapshot(
    state: &AppState,
    session_id: i32,
    side: &str,
    best_bid: Option<f64>,
    best_bid_sz: Option<f64>,
    best_ask: Option<f64>,
    best_ask_sz: Option<f64>,
    spread: Option<f64>,
    mid_price: Option<f64>,
    bid_volume_5: Option<f64>,
    ask_volume_5: Option<f64>,
    bid_volume_10: Option<f64>,
    ask_volume_10: Option<f64>,
    bid_volume: Option<f64>,
    ask_volume: Option<f64>,
    imbalance_ratio: Option<f64>,
    up_probability: Option<f64>,
    down_probability: Option<f64>,
    depth_bids: Option<Value>,
    depth_asks: Option<Value>,
    btc_price: Option<f64>,
) -> Result<()> {
    if let Some(pool) = state.db.as_ref() {
        sqlx::query(
            r#"
            INSERT INTO session_snapshots
                (session_id, side, best_bid, best_bid_sz, best_ask, best_ask_sz, spread, mid_price,
                 bid_volume_5, ask_volume_5, bid_volume_10, ask_volume_10, bid_volume, ask_volume,
                 imbalance_ratio, up_probability, down_probability, depth_bids, depth_asks, btc_price)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)
            "#
        )
        .bind(session_id)
        .bind(side)
        .bind(best_bid)
        .bind(best_bid_sz)
        .bind(best_ask)
        .bind(best_ask_sz)
        .bind(spread)
        .bind(mid_price)
        .bind(bid_volume_5)
        .bind(ask_volume_5)
        .bind(bid_volume_10)
        .bind(ask_volume_10)
        .bind(bid_volume)
        .bind(ask_volume)
        .bind(imbalance_ratio)
        .bind(up_probability)
        .bind(down_probability)
        .bind(depth_bids)
        .bind(depth_asks)
        .bind(btc_price)
        .execute(pool)
        .await?;

        sqlx::query("UPDATE recording_sessions SET tick_count = tick_count + 1 WHERE id = $1")
            .bind(session_id)
            .execute(pool)
            .await?;
        return Ok(());
    }

    // In-memory (lock ordering fix: always snapshots first, then sessions)
    let mut snapshots = state.mem_snapshots.write().await;
    let id = next_snap_id(&snapshots);
    snapshots.push(SessionSnapshot {
        id,
        session_id,
        ts: Utc::now(),
        side: side.into(),
        best_bid,
        best_bid_sz,
        best_ask,
        best_ask_sz,
        spread,
        mid_price,
        bid_volume_5,
        ask_volume_5,
        bid_volume_10,
        ask_volume_10,
        bid_volume,
        ask_volume,
        imbalance_ratio,
        up_probability,
        down_probability,
        depth_bids,
        depth_asks,
        btc_price,
        created_at: Utc::now(),
    });
    // increment tick_count — hold snapshots lock until done with sessions to maintain ordering
    let sessions_lock = state.mem_sessions.write();
    drop(snapshots); // release snapshots FIRST to avoid deadlock with delete_session
    let mut sessions = sessions_lock.await;
    if let Some(s) = sessions.iter_mut().find(|s| s.id == session_id) {
        s.tick_count += 1;
    }
    Ok(())
}

pub async fn list_session_snapshots(state: &AppState, session_id: i32) -> Result<Vec<SessionSnapshot>> {
    if let Some(pool) = state.db.as_ref() {
        let rows = sqlx::query_as::<_, SessionSnapshot>(
            "SELECT * FROM session_snapshots WHERE session_id = $1 ORDER BY ts ASC"
        )
        .bind(session_id)
        .fetch_all(pool)
        .await?;
        return Ok(rows);
    }

    let snapshots = state.mem_snapshots.read().await;
    Ok(snapshots.iter()
        .filter(|s| s.session_id == session_id)
        .cloned()
        .collect())
}

// ─── Session Trades (DB + In-Memory) ─────────────────────────────────────────

pub async fn insert_session_trade(
    state: &AppState,
    session_id: i32,
    side: &str,
    trade_side: &str,
    price: f64,
    size: f64,
    btc_price: Option<f64>,
) -> Result<()> {
    if let Some(pool) = state.db.as_ref() {
        // ON CONFLICT DO NOTHING prevents fill duplication (BUG FIX: fills were inserted ~180x per session)
        let result = sqlx::query(
            "INSERT INTO session_trades (session_id, side, trade_side, price, size, btc_price) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING"
        )
        .bind(session_id)
        .bind(side)
        .bind(trade_side)
        .bind(price)
        .bind(size)
        .bind(btc_price)
        .execute(pool)
        .await?;

        // Only increment counter if a new row was actually inserted
        if result.rows_affected() > 0 {
            sqlx::query("UPDATE recording_sessions SET trade_count = trade_count + 1 WHERE id = $1")
                .bind(session_id)
                .execute(pool)
                .await?;
        }
        return Ok(());
    }

    // In-memory (lock ordering: trades → sessions, consistent with delete_session: sessions → trades → snapshots)
    // SAFETY: consumer task is single-threaded, so no actual deadlock risk. Still, release one lock before next.
    let mut trades = state.mem_trades.write().await;
    // Dedup: skip if already exists
    if trades.iter().any(|t| t.session_id == session_id && t.side == side && t.trade_side == trade_side && t.price == price && t.size == size) {
        return Ok(());
    }
    let id = next_trade_id(&trades);
    trades.push(SessionTrade {
        id,
        session_id,
        ts: Utc::now(),
        side: side.into(),
        trade_side: trade_side.into(),
        price,
        size,
        btc_price,
        created_at: Utc::now(),
    });
    drop(trades);

    let mut sessions = state.mem_sessions.write().await;
    if let Some(s) = sessions.iter_mut().find(|s| s.id == session_id) {
        s.trade_count += 1;
    }
    Ok(())
}

pub async fn list_session_trades(state: &AppState, session_id: i32) -> Result<Vec<SessionTrade>> {
    if let Some(pool) = state.db.as_ref() {
        let rows = sqlx::query_as::<_, SessionTrade>(
            "SELECT * FROM session_trades WHERE session_id = $1 ORDER BY ts ASC"
        )
        .bind(session_id)
        .fetch_all(pool)
        .await?;
        return Ok(rows);
    }

    let trades = state.mem_trades.read().await;
    Ok(trades.iter()
        .filter(|t| t.session_id == session_id)
        .cloned()
        .collect())
}
