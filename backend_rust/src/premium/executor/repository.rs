use anyhow::Result;
use chrono::Utc;
use sqlx::PgPool;

use super::models::{ExecutionLog, NewStrategy, Strategy};

pub async fn insert_strategy(pool: &PgPool, s: &NewStrategy) -> Result<i64> {
    let conditions = serde_json::to_value(&s.conditions)?;
    let action = serde_json::to_value(&s.action)?;
    let row: (i64,) = sqlx::query_as(
        r#"
        INSERT INTO strategies (name, description, enabled, conditions, action,
            cooldown_secs, max_positions, max_size_total, stop_loss_pct, take_profit_pct)
        VALUES ($1, $2, true, $3, $4, $5, $6, $7, $8, $9)
        RETURNING id
        "#,
    )
    .bind(&s.name)
    .bind(&s.description)
    .bind(&conditions)
    .bind(&action)
    .bind(s.cooldown_secs.unwrap_or(30))
    .bind(s.max_positions.unwrap_or(3))
    .bind(s.max_size_total.unwrap_or(100.0))
    .bind(s.stop_loss_pct.unwrap_or(5.0))
    .bind(s.take_profit_pct.unwrap_or(10.0))
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn list_strategies(pool: &PgPool) -> Result<Vec<Strategy>> {
    let rows = sqlx::query_as::<_, Strategy>(
        r#"
        SELECT id, name, description, enabled, conditions, action,
               cooldown_secs, max_positions, max_size_total,
               stop_loss_pct, take_profit_pct, created_at, last_executed_at
        FROM strategies ORDER BY id
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_enabled_strategies(pool: &PgPool) -> Result<Vec<Strategy>> {
    let rows = sqlx::query_as::<_, Strategy>(
        r#"
        SELECT id, name, description, enabled, conditions, action,
               cooldown_secs, max_positions, max_size_total,
               stop_loss_pct, take_profit_pct, created_at, last_executed_at
        FROM strategies WHERE enabled = true ORDER BY id
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn update_last_executed(pool: &PgPool, id: i64) -> Result<()> {
    sqlx::query("UPDATE strategies SET last_executed_at = $1 WHERE id = $2")
        .bind(Utc::now())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn toggle_strategy(pool: &PgPool, id: i64, enabled: bool) -> Result<()> {
    sqlx::query("UPDATE strategies SET enabled = $1 WHERE id = $2")
        .bind(enabled)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_strategy(pool: &PgPool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM strategies WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn insert_execution_log(pool: &PgPool, log: &ExecutionLog) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO execution_logs (strategy_id, strategy_name, outcome, side, order_type, price, size, result, reason, btc_price, ts)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        "#,
    )
    .bind(log.strategy_id)
    .bind(&log.strategy_name)
    .bind(&log.outcome)
    .bind(&log.side)
    .bind(&log.order_type)
    .bind(log.price)
    .bind(log.size)
    .bind(&log.result)
    .bind(&log.reason)
    .bind(log.btc_price)
    .bind(log.ts)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn query_execution_logs(pool: &PgPool, limit: i64) -> Result<Vec<ExecutionLog>> {
    let rows = sqlx::query_as::<_, ExecutionLog>(
        "SELECT * FROM execution_logs ORDER BY ts DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
