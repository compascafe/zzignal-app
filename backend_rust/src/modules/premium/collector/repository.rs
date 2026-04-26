use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::models::OrderBookCandle;

/// Inserta una vela de order book finalizada.
/// ON CONFLICT evita duplicados (mismo intervalo + side + open_time).
pub async fn insert_candle(pool: &PgPool, candle: &OrderBookCandle) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO ob_timeframes
            (interval, side, open_time,
             bid_open, bid_high, bid_low, bid_close,
             ask_open, ask_high, ask_low, ask_close,
             spread_open, spread_high, spread_low, spread_close,
             mid_open, mid_high, mid_low, mid_close,
             bid_volume, ask_volume, tick_count)
        VALUES ($1,$2,$3, $4,$5,$6,$7, $8,$9,$10,$11, $12,$13,$14,$15, $16,$17,$18,$19, $20,$21,$22)
        ON CONFLICT (interval, side, open_time) DO NOTHING
        "#,
    )
    .bind(&candle.interval)
    .bind(&candle.side)
    .bind(candle.open_time)
    .bind(candle.bid_open)
    .bind(candle.bid_high)
    .bind(candle.bid_low)
    .bind(candle.bid_close)
    .bind(candle.ask_open)
    .bind(candle.ask_high)
    .bind(candle.ask_low)
    .bind(candle.ask_close)
    .bind(candle.spread_open)
    .bind(candle.spread_high)
    .bind(candle.spread_low)
    .bind(candle.spread_close)
    .bind(candle.mid_open)
    .bind(candle.mid_high)
    .bind(candle.mid_low)
    .bind(candle.mid_close)
    .bind(candle.bid_volume)
    .bind(candle.ask_volume)
    .bind(candle.tick_count)
    .execute(pool)
    .await?;
    Ok(())
}

/// Query candles para visualización / export
pub async fn query_candles(
    pool:     &PgPool,
    interval: &str,
    side:     Option<&str>,
    limit:    i64,
    from:     Option<DateTime<Utc>>,
    to:       Option<DateTime<Utc>>,
) -> Result<Vec<OrderBookCandle>> {
    let rows = sqlx::query_as::<_, OrderBookCandle>(
        r#"
        SELECT * FROM ob_timeframes
        WHERE interval = $1
          AND ($2::varchar IS NULL OR side = $2)
          AND ($3::timestamptz IS NULL OR open_time >= $3)
          AND ($4::timestamptz IS NULL OR open_time <= $4)
        ORDER BY open_time DESC
        LIMIT $5
        "#,
    )
    .bind(interval)
    .bind(side)
    .bind(from)
    .bind(to)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Cuenta candles por intervalo (para el dashboard)
pub async fn count_by_interval(pool: &PgPool) -> Result<Vec<(String, i64)>> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT interval, COUNT(*) FROM ob_timeframes GROUP BY interval ORDER BY interval"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
