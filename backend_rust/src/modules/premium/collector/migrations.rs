/// Registra las migraciones del módulo collector en el sistema de migraciones.
/// Se llama desde `persistence::run_migrations()` cuando el feature está activo.
///
/// La migración se ejecuta vía sqlx::query porque sqlx::migrate! solo soporta
/// una carpeta de migraciones. Las migraciones del collector van en
/// `src/modules/premium/collector/migrations/`.

use sqlx::PgPool;

pub async fn run(pool: &PgPool) -> Result<(), sqlx::Error> {
    // 001_collector_timeframes.sql — crea la tabla ob_timeframes
    sqlx::query(include_str!("migrations/001_collector_timeframes.sql"))
        .execute(pool)
        .await?;
    Ok(())
}
