/// Database layer: PostgreSQL models, repositories, scheduler, and API.
///
/// Handles persistence for:
/// - Order book snapshots (every 10s)
/// - Scheduled executions
/// - Recording sessions with chunked CSV generation
/// - Session trades and snapshots

pub mod api;
pub mod models;
pub mod repository;
pub mod scheduler;
