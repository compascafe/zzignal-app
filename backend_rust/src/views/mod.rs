/// View layer: Terminal UI powered by ratatui + crossterm.
///
/// Connects to the backend via WebSocket and renders:
/// - Real-time market data (BTC price, orderbook depth)
/// - Signal indicators (momentum, velocity, imbalance, OFI)
/// - Position tracking and manual trading controls
/// - Session management tabs
///
/// Enabled via `--features tui` at compile time.

pub mod api;
pub mod app;
pub mod commands;
pub mod ui;
