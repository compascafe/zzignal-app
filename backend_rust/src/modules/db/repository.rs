use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;

use crate::modules::db::models::ScheduledExecution;

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
