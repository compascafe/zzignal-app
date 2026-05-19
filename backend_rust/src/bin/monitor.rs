//! zzignal-monitor — Terminal UI launcher binary.
//!
//! Requires `--features tui` at compile time.
//! Connects to a running backend via WebSocket and renders real-time
//! market data, signals, and position tracking in a Bloomberg-style layout.

/// zzignal-monitor — Terminal UI launcher binary.
///
/// Requires `--features tui` at compile time.
/// Connects to a running backend via WebSocket and renders real-time
/// market data, signals, and position tracking in a Bloomberg-style layout.

#[tokio::main]
async fn main() {
    if let Err(e) = polymarket_backend::views::app::run().await {
        eprintln!("TUI error: {e}");
        std::process::exit(1);
    }
}
