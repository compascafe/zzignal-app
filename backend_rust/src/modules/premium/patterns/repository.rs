use anyhow::Result;
use sqlx::PgPool;

use super::models::Signal;

pub async fn insert_signal(pool: &PgPool, signal: &Signal) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO pattern_signals (pattern, side, severity, description, data, ts)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(&signal.pattern)
    .bind(&signal.side)
    .bind(&signal.severity)
    .bind(&signal.description)
    .bind(&signal.data)
    .bind(signal.ts)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn query_signals(pool: &PgPool, side: Option<&str>, limit: i64) -> Result<Vec<Signal>> {
    let rows = sqlx::query_as::<_, Signal>(
        r#"
        SELECT id, pattern, side, severity, description, data, ts, created_at
        FROM pattern_signals
        WHERE ($1::varchar IS NULL OR side = $1)
        ORDER BY ts DESC
        LIMIT $2
        "#,
    )
    .bind(side)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
