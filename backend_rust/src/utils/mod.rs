/// Utility modules: infrastructure helpers with zero domain logic.
///
/// - `persistence`  — Database migrations and historical CRUD
/// - `ring_buffer`  — Lock-free circular buffer for price history

pub mod persistence;
pub mod ring_buffer;
