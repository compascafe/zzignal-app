use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tracing::{info, warn};

use crate::modules::core::state::AppState;

use super::engine;
use super::repository;

/// Background task: cada 1s evalúa todas las estrategias habilitadas
/// y ejecuta las acciones correspondientes si las condiciones se cumplen.
pub async fn run_executor(state: Arc<AppState>) {
    let pool = match state.db.as_ref().map(|p| p.clone()) {
        Some(p) => p,
        None => {
            warn!("Executor: PostgreSQL no disponible. El módulo NO se iniciará.");
            return;
        }
    };

    // Run migration
    if let Err(e) = sqlx::query(include_str!("migrations/001_strategies.sql"))
        .execute(&pool).await
    {
        warn!("Executor: migración fallida: {e}");
        return;
    }
    info!("Executor: engine iniciado. Evaluando estrategias cada 1s...");

    let mut tick = tokio::time::interval(Duration::from_secs(1));

    loop {
        tick.tick().await;

        let strategies = match repository::get_enabled_strategies(&pool).await {
            Ok(s) => s,
            Err(e) => {
                warn!("Executor: error cargando estrategias: {e}");
                continue;
            }
        };

        if strategies.is_empty() {
            continue;
        }

        let now = Utc::now();

        for strategy in &strategies {
            // Cooldown check
            if let Some(last_exec) = strategy.last_executed_at {
                let elapsed = (now - last_exec).num_seconds();
                if elapsed < strategy.cooldown_secs as i64 {
                    continue;
                }
            }

            // Evaluate conditions
            let conditions = strategy.conditions_parsed();
            if engine::evaluate_conditions(&state, &conditions) {
                let log = engine::execute_action(&state, strategy).await;

                if log.result == "success" {
                    if let Err(e) = repository::update_last_executed(&pool, strategy.id).await {
                        warn!("Executor: error actualizando last_executed: {e}");
                    }
                }
            }
        }
    }
}
