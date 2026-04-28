/// Runner manual de migraciones para el módulo collector.
/// Se llama desde el scheduler cuando el feature premium-collector está activo.
///
/// Todas las migraciones son idempotentes — se ejecutan en cada arranque.

use sqlx::PgPool;

pub async fn run(pool: &PgPool) -> Result<(), sqlx::Error> {
    // 001_collector_timeframes.sql — crea la tabla ob_timeframes
    sqlx::query(include_str!("migrations/001_collector_timeframes.sql"))
        .execute(pool)
        .await?;
    Ok(())
}
