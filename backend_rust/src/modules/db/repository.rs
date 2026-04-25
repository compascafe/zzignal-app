use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;

use crate::modules::db::models::{ScheduledExecution, RecordingSession, SessionSnapshot, SessionTrade};

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

// ─── Recording Sessions ───────────────────────────────────────────────────────

pub async fn create_session(pool: Option<&PgPool>, name: &str, duration_min: i32, depth_levels: i32, btc_price: Option<f64>) -> Result<i32> {
    let Some(pool) = pool else { return Ok(0) };
    let row: (i32,) = sqlx::query_as(
        r#"
        INSERT INTO recording_sessions (name, duration_min, depth_levels, btc_price_start, status)
        VALUES ($1, $2, $3, $4, 'recording')
        RETURNING id
        "#,
    )
    .bind(name)
    .bind(duration_min)
    .bind(depth_levels)
    .bind(btc_price)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn get_active_session(pool: Option<&PgPool>) -> Result<Option<RecordingSession>> {
    let Some(pool) = pool else { return Ok(None) };
    let row = sqlx::query_as::<_, RecordingSession>(
        "SELECT * FROM recording_sessions WHERE status = 'recording' ORDER BY started_at DESC LIMIT 1"
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_sessions(pool: Option<&PgPool>, limit: i64) -> Result<Vec<RecordingSession>> {
    let Some(pool) = pool else { return Ok(vec![]) };
    let rows = sqlx::query_as::<_, RecordingSession>(
        "SELECT * FROM recording_sessions ORDER BY started_at DESC LIMIT $1"
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn stop_session(pool: Option<&PgPool>, id: i32, final_price: Option<f64>, btc_price_end: Option<f64>) -> Result<()> {
    let Some(pool) = pool else { return Ok(()) };

    // Determinar outcome comparando strike_price vs final_price
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
    Ok(())
}

pub async fn delete_session(pool: Option<&PgPool>, id: i32) -> Result<()> {
    let Some(pool) = pool else { return Ok(()) };
    sqlx::query("DELETE FROM recording_sessions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ─── Session Snapshots ────────────────────────────────────────────────────────

pub async fn insert_session_snapshot(
    pool: Option<&PgPool>,
    session_id: i32,
    side: &str,
    best_bid: Option<f64>,
    best_bid_sz: Option<f64>,
    best_ask: Option<f64>,
    best_ask_sz: Option<f64>,
    spread: Option<f64>,
    mid_price: Option<f64>,
    bid_volume: Option<f64>,
    ask_volume: Option<f64>,
    depth_bids: Option<serde_json::Value>,
    depth_asks: Option<serde_json::Value>,
    btc_price: Option<f64>,
) -> Result<()> {
    let Some(pool) = pool else { return Ok(()) };
    sqlx::query(
        r#"
        INSERT INTO session_snapshots
            (session_id, side, best_bid, best_bid_sz, best_ask, best_ask_sz, spread, mid_price,
             bid_volume, ask_volume, depth_bids, depth_asks, btc_price)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
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
    .bind(bid_volume)
    .bind(ask_volume)
    .bind(depth_bids)
    .bind(depth_asks)
    .bind(btc_price)
    .execute(pool)
    .await?;

    // Increment tick_count
    sqlx::query("UPDATE recording_sessions SET tick_count = tick_count + 1 WHERE id = $1")
        .bind(session_id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn list_session_snapshots(pool: Option<&PgPool>, session_id: i32) -> Result<Vec<SessionSnapshot>> {
    let Some(pool) = pool else { return Ok(vec![]) };
    let rows = sqlx::query_as::<_, SessionSnapshot>(
        "SELECT * FROM session_snapshots WHERE session_id = $1 ORDER BY ts ASC"
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ─── Session Trades ───────────────────────────────────────────────────────────

pub async fn insert_session_trade(
    pool: Option<&PgPool>,
    session_id: i32,
    side: &str,
    trade_side: &str,
    price: f64,
    size: f64,
    btc_price: Option<f64>,
) -> Result<()> {
    let Some(pool) = pool else { return Ok(()) };
    sqlx::query(
        "INSERT INTO session_trades (session_id, side, trade_side, price, size, btc_price) VALUES ($1, $2, $3, $4, $5, $6)"
    )
    .bind(session_id)
    .bind(side)
    .bind(trade_side)
    .bind(price)
    .bind(size)
    .bind(btc_price)
    .execute(pool)
    .await?;

    sqlx::query("UPDATE recording_sessions SET trade_count = trade_count + 1 WHERE id = $1")
        .bind(session_id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn list_session_trades(pool: Option<&PgPool>, session_id: i32) -> Result<Vec<SessionTrade>> {
    let Some(pool) = pool else { return Ok(vec![]) };
    let rows = sqlx::query_as::<_, SessionTrade>(
        "SELECT * FROM session_trades WHERE session_id = $1 ORDER BY ts ASC"
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
