use ratatui::style::Color;
use crate::api::*;
use crate::State;

// ═══════════════════════════════════════════════════════════════════
// COMMAND REGISTRY — for /man help page
// ═══════════════════════════════════════════════════════════════════

pub struct CmdDef {
    pub syntax: &'static str,
    pub desc: &'static str,
    pub category: &'static str,
}

pub const REGISTRY: &[CmdDef] = &[
    // ── ABRIR ──
    CmdDef { syntax: "/l<usd>up<cents>        ", desc: "BUY UP limit: $X a precio Y", category: "ABRIR" },
    CmdDef { syntax: "/l<usd>d<cents>         ", desc: "BUY DOWN limit", category: "ABRIR" },
    CmdDef { syntax: "/l<usd>up<cents>e<cents> ", desc: "BUY UP + exit automático", category: "ABRIR" },
    CmdDef { syntax: "/l<usd>d<cents>e<cents>s<cents>", desc: "BUY + exit + SL bracket", category: "ABRIR" },
    CmdDef { syntax: "/<usd>up<cents>          ", desc: "(legacy) BUY UP limit", category: "ABRIR" },
    // ── ALIAS ──
    CmdDef { syntax: "/b<usd>up<cents>         ", desc: "Alias: BUY (igual que /l)", category: "ALIAS" },
    CmdDef { syntax: "/k                      ", desc: "Alias: CANCEL (igual que /c)", category: "ALIAS" },
    CmdDef { syntax: "/x /xu70 /xd70          ", desc: "Alias: EXIT (liq a mercado o límite)", category: "ALIAS" },
    // ── CANCELAR ──
    CmdDef { syntax: "/c                      ", desc: "Cancelar orden activa + SL + TSL", category: "CANCELAR" },
    CmdDef { syntax: "/c<id>                  ", desc: "Cancelar orden por ID", category: "CANCELAR" },
    // ── LIQUIDAR ──
    CmdDef { syntax: "/lup<cents>             ", desc: "Vender UP a precio límite", category: "LIQUIDAR" },
    CmdDef { syntax: "/ld<cents>              ", desc: "Vender DOWN a precio límite", category: "LIQUIDAR" },
    CmdDef { syntax: "/lm                     ", desc: "Vender a MERCADO", category: "LIQUIDAR" },
    // ── CANC+LIQ ──
    CmdDef { syntax: "/clup<cents>            ", desc: "Cancelar todo + vender UP límite", category: "CANC+LIQ" },
    CmdDef { syntax: "/cld<cents>             ", desc: "Cancelar todo + vender DOWN límite", category: "CANC+LIQ" },
    CmdDef { syntax: "/clm                    ", desc: "Cancelar todo + vender MERCADO", category: "CANC+LIQ" },
    // ── SL / TSL ──
    CmdDef { syntax: "/sl                     ", desc: "Alternar SL MARKET/LIMIT (OFF por defecto)", category: "SL / TSL" },
    CmdDef { syntax: "/sl<X>                  ", desc: "SL al X% (1–50)", category: "SL / TSL" },
    CmdDef { syntax: "/nsl                    ", desc: "Desactivar SL", category: "SL / TSL" },
    CmdDef { syntax: "/tsl<X>                 ", desc: "Trailing stop al X%", category: "SL / TSL" },
    CmdDef { syntax: "/ntsl                   ", desc: "Desactivar trailing stop", category: "SL / TSL" },
    // ── EMERGENCIA ──
    CmdDef { syntax: "/p                      ", desc: "PANIC: liquidar todo", category: "EMERGENCIA" },
    CmdDef { syntax: "/co                     ", desc: "CASH OUT: market sell + PANIC — todo a USD", category: "EMERGENCIA" },
    // ── INFO ──
    CmdDef { syntax: "/pos                    ", desc: "Ver posición actual (tamaño, entry, P&L)", category: "INFO" },
    CmdDef { syntax: "/alert up 0.70          ", desc: "Alerta visual+sonido al tocar precio", category: "INFO" },
    CmdDef { syntax: "/alert clear            ", desc: "Borrar todas las alertas", category: "INFO" },
    // ── META ──
    CmdDef { syntax: "/u                      ", desc: "Undo: cancelar última orden", category: "META" },
    CmdDef { syntax: "/man                    ", desc: "Ver página de ayuda", category: "META" },
    CmdDef { syntax: "/quit                   ", desc: "Salir de /man", category: "META" },
];

// ═══════════════════════════════════════════════════════════════════
// PARSED COMMAND
// ═══════════════════════════════════════════════════════════════════

enum Parsed {
    BuyManual { amount: f64, side: String, price: f64, exit: Option<f64>, sl: Option<f64> },
    CancelActive,
    CancelId(String),
    CancelLiquidateLimit { outcome: String, price: f64 },
    CancelLiquidateMarket,
    LiquidateLimit { outcome: String, price: f64 },
    LiquidateMarket,
    Panic,
    SlToggle,
    SlSet(f64),
    SlOff,
    ShowMan,
    QuitMan,
    ShowPosition,
    TrailingStop(f64),
    TrailingStopOff,
    AlertSet { outcome: String, price: f64 },
    AlertClear,
    Undo,
    CashOut,
    Unknown(String),
}

// ═══════════════════════════════════════════════════════════════════
// MAIN DISPATCHER — called from main loop
// ═══════════════════════════════════════════════════════════════════

pub async fn dispatch(input: &str, s: &mut State) {
    let cmd = parse(input.trim());
    execute(cmd, s).await;
}

fn parse(input: &str) -> Parsed {
    if input.is_empty() { return Parsed::Unknown(String::new()); }
    let first = input.chars().next().unwrap();
    let rest = &input[1..];

    // ─── ALIASES ────────────────────────────────────────────────
    match first {
        'b' => return parse(&format!("l{rest}")),      // /b → /l
        'k' if rest.is_empty() => return Parsed::CancelActive,
        'k' => return parse(&format!("cl{rest}")),     // /kup65 → /clup65
        'x' if rest.is_empty() => return Parsed::LiquidateMarket,
        'x' => {
            // /xu70 /xd70 → exit limit. Also /x70 = shorthand (interpret as need context)
            let liq_rest = rest;
            if liq_rest.starts_with("up") {
                if let Some((price, _)) = parse_cents(&liq_rest[2..]) {
                    return Parsed::LiquidateLimit { outcome: "up".into(), price };
                }
            } else if liq_rest.starts_with('u') {
                if let Some((price, _)) = parse_cents(&liq_rest[1..]) {
                    return Parsed::LiquidateLimit { outcome: "up".into(), price };
                }
            } else if liq_rest.starts_with('d') {
                let after = &liq_rest[1..];
                let ps = if after.starts_with("own") { &after[3..] } else { after };
                if let Some((price, _)) = parse_cents(ps) {
                    return Parsed::LiquidateLimit { outcome: "down".into(), price };
                }
            }
        }
        _ => {}
    }

    match first {
        'p' if rest.is_empty() => Parsed::Panic,
        'c' if rest == "o" => Parsed::CashOut,
        'c' => parse_c_group(rest),
        's' if input.len() >= 2 && input.as_bytes()[1] == b'l' => parse_sl(&input[2..]),
        'n' if input == "nsl" => Parsed::SlOff,
        'n' if input == "ntsl" => Parsed::TrailingStopOff,
        'l' => parse_l_group(rest),
        'm' if input == "man" => Parsed::ShowMan,
        'm' if rest == "an" => Parsed::ShowMan,
        'q' if input == "quit" => Parsed::QuitMan,
        'p' if rest == "os" => Parsed::ShowPosition,
        't' if rest.starts_with("sl") => {
            let tsl_rest = &rest[2..];
            if let Ok(pct) = tsl_rest.parse::<f64>() {
                Parsed::TrailingStop(pct.max(1.0).min(50.0))
            } else {
                Parsed::Unknown(input.to_string())
            }
        }
        'a' if rest.starts_with("lert") => {
            // /alert up 0.70  or  /alert clear
            let alert_rest = rest[4..].trim();
            if alert_rest == "clear" || alert_rest.is_empty() {
                Parsed::AlertClear
            } else if alert_rest.starts_with("up ") {
                if let Ok(price) = alert_rest[3..].parse::<f64>() {
                    Parsed::AlertSet { outcome: "up".into(), price }
                } else { Parsed::Unknown(input.to_string()) }
            } else if alert_rest.starts_with("down ") {
                if let Ok(price) = alert_rest[5..].parse::<f64>() {
                    Parsed::AlertSet { outcome: "down".into(), price }
                } else { Parsed::Unknown(input.to_string()) }
            } else {
                Parsed::Unknown(input.to_string())
            }
        }
        'u' if rest.is_empty() => Parsed::Undo,
        _ if first.is_ascii_digit() => parse_buy_legacy(input),
        _ => Parsed::Unknown(input.to_string()),
    }
}

// ═══════════════════════════════════════════════════════════════════
// PARSERS
// ═══════════════════════════════════════════════════════════════════

fn parse_c_group(rest: &str) -> Parsed {
    if rest.is_empty() { return Parsed::CancelActive; }

    // /clup65 /cld70 /clm
    if rest.starts_with('l') {
        let liq = &rest[1..];
        if liq.starts_with("up") {
            if let Some((price, _)) = parse_cents(&liq[2..]) {
                return Parsed::CancelLiquidateLimit { outcome: "up".into(), price };
            }
        } else if liq.starts_with('d') {
            let after = &liq[1..];
            let ps = if after.starts_with("own") { &after[3..] } else { after };
            if let Some((price, _)) = parse_cents(ps) {
                return Parsed::CancelLiquidateLimit { outcome: "down".into(), price };
            }
        } else if liq == "m" {
            return Parsed::CancelLiquidateMarket;
        }
    }

    // /c<id>
    if rest.len() >= 10 {
        Parsed::CancelId(rest.to_string())
    } else {
        Parsed::CancelActive
    }
}

fn parse_l_group(rest: &str) -> Parsed {
    if rest.is_empty() { return Parsed::Unknown("l".into()); }

    // /lup70
    if rest.starts_with("up") {
        if let Some((price, _)) = parse_cents(&rest[2..]) {
            return Parsed::LiquidateLimit { outcome: "up".into(), price };
        }
        return Parsed::Unknown(rest.to_string());
    }

    // /ld70 or /ldown70
    if rest.starts_with('d') {
        let after = &rest[1..];
        let ps = if after.starts_with("own") { &after[3..] } else { after };
        if let Some((price, _)) = parse_cents(ps) {
            return Parsed::LiquidateLimit { outcome: "down".into(), price };
        }
        return Parsed::Unknown(rest.to_string());
    }

    // /lm
    if rest == "m" { return Parsed::LiquidateMarket; }

    // /l10up65e70 → buy with exit
    parse_buy_with_prefix(rest, "l")
}

fn parse_buy_legacy(input: &str) -> Parsed {
    parse_buy_inner(input, "")
}

fn parse_buy_with_prefix(input: &str, _prefix: &str) -> Parsed {
    parse_buy_inner(input, _prefix)
}

fn parse_buy_inner(input: &str, _prefix: &str) -> Parsed {
    let (amount, rem) = match parse_amount(input) {
        Some(v) => v,
        None => return Parsed::Unknown(input.to_string()),
    };
    let (side, rem) = match parse_side_name(rem) {
        Some(v) => v,
        None => return Parsed::Unknown(input.to_string()),
    };
    let (price, rem) = match parse_cents(rem) {
        Some(v) => v,
        None => return Parsed::Unknown(input.to_string()),
    };
    let exit = if rem.starts_with('e') {
        parse_cents(&rem[1..]).map(|(p, _)| p)
    } else { None };

    // Bracket: after eXX, check for sXX (SL override)
    let sl = if let Some(_exit_px) = exit {
        let after_exit = &rem[1..];
        let after_exit_digits = after_exit.find(|c: char| !c.is_ascii_digit()).unwrap_or(after_exit.len());
        let sl_input = &after_exit[after_exit_digits..];
        if sl_input.starts_with('s') {
            parse_cents(&sl_input[1..]).map(|(p, _)| p)
        } else { None }
    } else { None };

    Parsed::BuyManual { amount, side: side.to_string(), price, exit, sl }
}

fn parse_sl(rest: &str) -> Parsed {
    if rest.is_empty() {
        Parsed::SlToggle
    } else if let Ok(pct) = rest.parse::<f64>() {
        let pct = pct.max(1.0).min(50.0);
        Parsed::SlSet(pct)
    } else {
        Parsed::Unknown(format!("sl{}", rest))
    }
}

// ═══════════════════════════════════════════════════════════════════
// PARSER HELPERS
// ═══════════════════════════════════════════════════════════════════

fn parse_amount(s: &str) -> Option<(f64, &str)> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if end == 0 { return None; }
    let n: f64 = s[..end].parse().ok()?;
    if n < 1.0 || n > 200.0 { return None; }
    Some((n, &s[end..]))
}

fn parse_side_name(s: &str) -> Option<(&str, &str)> {
    if s.starts_with("up") { Some(("up", &s[2..])) }
    else if s.starts_with('d') { Some(("down", &s[1..])) }
    else { None }
}

fn parse_cents(s: &str) -> Option<(f64, &str)> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if end == 0 { return None; }
    let n: f64 = s[..end].parse().ok()?;
    let price = n / 100.0;
    if price <= 0.0 || price >= 1.0 { return None; }
    Some((price, &s[end..]))
}

// ═══════════════════════════════════════════════════════════════════
// EXECUTOR
// ═══════════════════════════════════════════════════════════════════

async fn execute(cmd: Parsed, s: &mut State) {
    match cmd {
        Parsed::Panic => exec_panic(s).await,
        Parsed::CancelActive => cancel_active(s).await,
        Parsed::CancelId(id) => cancel_by_id(&id, s).await,
        Parsed::BuyManual { amount, side, price, exit, sl } => {
            place_manual_buy(amount, &side, price, exit, sl, s).await;
        }
        Parsed::CancelLiquidateLimit { outcome, price } => exec_cancel_liq(outcome, Some(price), false, s).await,
        Parsed::CancelLiquidateMarket => exec_cancel_liq(String::new(), None, true, s).await,
        Parsed::LiquidateLimit { outcome, price } => exec_liq_limit(&outcome, price, s).await,
        Parsed::LiquidateMarket => exec_liq_market(s).await,
        Parsed::SlToggle => exec_sl_toggle(s).await,
        Parsed::SlSet(pct) => exec_sl_set(pct, s).await,
        Parsed::SlOff => exec_sl_off(s).await,
        Parsed::ShowMan => { s.show_man = true; }
        Parsed::QuitMan => { s.show_man = false; }
        Parsed::ShowPosition => { /* handled in UI */ }
        Parsed::TrailingStop(pct) => exec_tsl_set(pct, s).await,
        Parsed::TrailingStopOff => exec_tsl_off(s).await,
        Parsed::AlertSet { outcome, price } => exec_alert_set(outcome, price, s),
        Parsed::AlertClear => exec_alert_clear(s),
        Parsed::Undo => exec_undo(s).await,
        Parsed::CashOut => exec_cashout(s).await,
        Parsed::Unknown(input) => {
            s.add_log(format!("?: /{} — desconocido", input), Color::Red);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// EXECUTORS: TRADING
// ═══════════════════════════════════════════════════════════════════

async fn place_manual_buy(amount: f64, side: &str, price: f64, exit_price: Option<f64>, sl_price: Option<f64>, s: &mut State) {
    let outcome = if side == "up" { "up" } else { "down" };
    let size = (amount / price).floor().max(1.0);

    s.add_log(format!("▶ BUY {} ${:.0} @{:.4} sz={:.0}",
        outcome.to_uppercase(), amount, price, size),
        if side == "up" { Color::Green } else { Color::Red });
    s.add_trade_log(format!("▶ BUY {} ${:.0} @{:.4} sz={:.0}",
        outcome.to_uppercase(), amount, price, size),
        if side == "up" { Color::Green } else { Color::Red });

    let body = format!(r#"{{"side":"buy","outcome":"{}","price":{},"size":{}}}"#, outcome, price, size);
    let order_id = match http_post_result::<OrderPlaced>("/api/orders/limit", &body).await {
        Ok(placed) => {
            if placed.id.is_empty() {
                s.add_log("  Orden colocada (sin ID)".to_string(), Color::Cyan);
            } else {
                s.add_log(format!("  Orden: {}", placed.id), Color::Cyan);
            }
            placed.id
        }
        Err(e) => {
            // Order might still have been placed — proceed without ID, track by price
            s.add_log(format!("⚠ BUY OK pero no se pudo leer ID: {}", e), Color::Yellow);
            s.add_trade_log(format!("⚠ BUY OK (sin ID)"), Color::Yellow);
            String::new()
        }
    };

    s.last_order_id = order_id.clone();
    s.last_order_type = "buy".to_string();

    s.mt_state = 1;
    s.mt_outcome = outcome.to_string();
    s.mt_entry = price;
    s.mt_order_placed_at = std::time::Instant::now();
    s.mt_fill_avg = price; // start at limit, update on partial fills
    s.mt_fill_count = 0;
    s.mt_size = size;
    s.mt_budget = amount;
    s.mt_order_id = order_id;
    s.mt_order_seen = false;
    s.mt_last_fill_pct = 0.0;
    s.mt_exit_price = exit_price.unwrap_or(0.0);
    s.mt_exit_order_id.clear();
    s.mt_sl_order_id.clear();
    s.mt_tsl_pct = 0.0;
    s.mt_tsl_high = price;
    s.mt_tsl_low = price;

    if let Some(exit) = exit_price {
        let exit_body = format!(r#"{{"side":"sell","outcome":"{}","price":{},"size":{}}}"#, outcome, exit, size);
        s.add_log(format!("▶ EXIT SELL {} @{:.4} (take-profit)", outcome.to_uppercase(), exit), Color::Yellow);
        s.add_trade_log(format!("  TP @{:.4}", exit), Color::Yellow);
        match http_post_result::<OrderPlaced>("/api/orders/limit", &exit_body).await {
            Ok(exit_placed) => {
                s.mt_exit_order_id = exit_placed.id.clone();
                s.add_log(format!("  Exit ID: {}", exit_placed.id), Color::Cyan);
            }
            Err(e) => {
                s.add_log(format!("EXIT FAIL: {}", e), Color::Red);
            }
        }
    }

    // Bracket SL override or default SL
    if let Some(sl) = sl_price {
        s.sl_pct = 0.0; // bracket SL overrides percentage
        let sl_body = format!(r#"{{"side":"sell","outcome":"{}","price":{},"size":{}}}"#, outcome, sl, size);
        s.add_log(format!("▶ SL BRACKET {} @{:.4}", outcome.to_uppercase(), sl), Color::Yellow);
        s.add_trade_log(format!("🛡 SL bracket @{:.4}", sl), Color::Yellow);
        match http_post_result::<OrderPlaced>("/api/orders/limit", &sl_body).await {
            Ok(sl_placed) => {
                s.mt_sl_order_id = sl_placed.id.clone();
                s.add_log(format!("  SL ID: {}", sl_placed.id), Color::Cyan);
            }
            Err(e) => {
                s.add_log(format!("SL bracket FAIL: {}", e), Color::Red);
            }
        }
    } else if s.sl_pct > 0.0 {
        // SL will be placed after fill (via track_manual_fills)
        s.add_log(format!("  SL {:.0}% pendiente de fill", s.sl_pct), Color::DarkGray);
    }
}

// ═══════════════════════════════════════════════════════════════════
// EXECUTORS: CANCEL
// ═══════════════════════════════════════════════════════════════════

async fn cancel_active(s: &mut State) {
    if s.mt_state == 0 && s.open_orders.is_empty() {
        s.add_log("Nada que cancelar", Color::DarkGray);
        s.add_trade_log("\u{2717} Cancel: sin ordenes activas", Color::DarkGray);
        return;
    }

    let cancel_id = if !s.mt_order_id.is_empty() {
        s.mt_order_id.clone()
    } else if let Some(o) = s.open_orders.first() {
        o.id.clone()
    } else {
        s.add_log("Nada que cancelar", Color::DarkGray);
        return;
    };

    s.add_log("Cancelando orden...".to_string(), Color::Yellow);
    let mut cancelled = false;
    if let Err(e) = http_delete(&format!("/api/orders/{}", cancel_id)).await {
        s.add_log(format!("Cancel FAIL: {}", e), Color::Red);
        s.add_trade_log(format!("\u{2717} Cancel FAIL: {}", e), Color::Red);
    } else {
        s.add_log(format!("Orden {} cancelada", cancel_id), Color::Green);
        s.add_trade_log("\u{2717} Orden cancelada".to_string(), Color::Yellow);
        cancelled = true;
    }

    if !s.mt_exit_order_id.is_empty() {
        if let Err(e) = http_delete(&format!("/api/orders/{}", s.mt_exit_order_id)).await {
            s.add_log(format!("Cancel EXIT FAIL: {}", e), Color::Red);
        } else {
            s.add_log("Exit cancelado".to_string(), Color::Green);
        }
        s.mt_exit_order_id.clear();
    }

    if !s.mt_sl_order_id.is_empty() {
        if let Err(e) = http_delete(&format!("/api/orders/{}", s.mt_sl_order_id)).await {
            s.add_log(format!("Cancel SL FAIL: {}", e), Color::Red);
        } else {
            s.add_log("SL cancelado".to_string(), Color::Green);
        }
        s.mt_sl_order_id.clear();
    }

    if cancelled {
        if s.mt_state == 1 {
            s.reset_manual();
        } else if s.mt_state == 3 {
            s.mt_state = 2;
            s.mt_exit_order_id.clear();
        }
    }
}

async fn cancel_by_id(id: &str, s: &mut State) {
    if let Err(e) = http_delete(&format!("/api/orders/{}", id)).await {
        s.add_log(format!("Cancel FAIL: {}", e), Color::Red);
        s.add_trade_log(format!("\u{2717} Cancel {} FAIL: {}", id, e), Color::Red);
    } else {
        s.add_log(format!("Orden {} cancelada", id), Color::Green);
        s.add_trade_log(format!("\u{2717} Cancel {}", id), Color::Yellow);
        if s.mt_order_id == id { s.mt_order_id.clear(); }
        if s.mt_exit_order_id == id { s.mt_exit_order_id.clear(); }
        if s.mt_sl_order_id == id { s.mt_sl_order_id.clear(); }
    }
}

// ═══════════════════════════════════════════════════════════════════
// EXECUTORS: LIQUIDATE
// ═══════════════════════════════════════════════════════════════════

async fn exec_liq_limit(outcome: &str, price: f64, s: &mut State) {
    let current_px = if outcome == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn };
    let best_bid = if outcome == "up" {
        s.book_up.bids.first().map(|l| l.price).unwrap_or(0.0)
    } else {
        s.book_dn.bids.first().map(|l| l.price).unwrap_or(0.0)
    };

    let size = if s.mt_state >= 2 && s.mt_outcome == outcome {
        s.mt_size
    } else {
        if s.mt_state == 1 && s.mt_outcome == outcome {
            s.add_log(format!("Orden {} aun no ha llenado. Espera fill o /c para cancelar", outcome.to_uppercase()), Color::Yellow);
        } else if s.mt_state == 0 {
            s.add_log(format!("Sin posicion {} activa.", outcome.to_uppercase()), Color::Red);
        } else {
            s.add_log(format!("Sin posicion {} para liquidar", outcome.to_uppercase()), Color::Red);
        }
        s.add_trade_log(format!("\u{2717} Liquidar {}: sin posicion", outcome.to_uppercase()), Color::Red);
        return;
    };

    s.add_log(format!("▶ LIQUIDAR {} sz={:.0} @{:.4}  [bid:{:.4} px:{:.4}]",
        outcome.to_uppercase(), size, price, best_bid, current_px), Color::Yellow);
    s.add_trade_log(format!("▶ LIQ {} sz={:.0} @{:.4}", outcome.to_uppercase(), size, price), Color::Yellow);

    let body = format!(r#"{{"side":"sell","outcome":"{}","price":{},"size":{}}}"#, outcome, price, size);
    match http_post_result::<OrderPlaced>("/api/orders/limit", &body).await {
        Ok(placed) => {
            s.add_log(format!("  Liquidacion: {}", placed.id), Color::Cyan);
            s.mt_exit_order_id = placed.id;
            s.mt_exit_price = price;
            s.mt_state = 3;
        }
        Err(e) => {
            s.add_log(format!("LIQUIDAR FAIL: {}", e), Color::Red);
            s.add_trade_log(format!("\u{2717} LIQUIDAR FAIL: {}", e), Color::Red);
        }
    }
}

async fn exec_liq_market(s: &mut State) {
    let mut liquidated = false;

    if s.mt_state >= 2 {
        let outcome = s.mt_outcome.clone();
        let size = s.mt_size;
        s.add_log(format!("▶ MARKET SELL {} sz={:.0}", outcome.to_uppercase(), size), Color::Yellow);
        s.add_trade_log(format!("▶ MKT SELL {} sz={:.0} @mercado", outcome.to_uppercase(), size), Color::Yellow);
        let body = format!(r#"{{"side":"sell","outcome":"{}","amount_usdc":{}}}"#,
            outcome, s.mt_budget);
        if let Err(e) = http_post("/api/orders/market", &body).await {
            s.add_log(format!("MARKET SELL FAIL: {}", e), Color::Red);
            s.add_trade_log(format!("\u{2717} MKT SELL FAIL: {}", e), Color::Red);
        } else {
            liquidated = true;
        }
    }

    if s.pos_sen_up || s.pos_sen_dn || s.pos_h65_up || s.pos_h65_dn || s.pos_odi_up || s.pos_odi_dn {
        s.add_log("Liquidando estrategias via PANIC...".to_string(), Color::Yellow);
        let _ = http_post("/api/panic", "{}").await;
        liquidated = true;
    }

    if liquidated {
        s.reset_manual();
    } else {
        s.add_log("Sin posiciones para liquidar a mercado".to_string(), Color::DarkGray);
        s.add_trade_log("\u{2717} MKT: sin posiciones".to_string(), Color::DarkGray);
    }
}

async fn exec_panic(s: &mut State) {
    s.add_log("PANIC — liquidando TODO".to_string(), Color::Red);
    s.add_trade_log("PANIC — liquidando todo".to_string(), Color::Red);
    s.reset_manual();
    if let Err(e) = http_post("/api/panic", "{}").await {
        s.add_log(format!("PANIC FAIL: {}", e), Color::Red);
    } else {
        s.add_log("TODO LIQUIDADO".to_string(), Color::Green);
    }
}

// ═══════════════════════════════════════════════════════════════════
// EXECUTORS: CANCEL + LIQUIDATE
// ═══════════════════════════════════════════════════════════════════

async fn exec_cancel_liq(outcome: String, price_opt: Option<f64>, market: bool, s: &mut State) {
    // Save position info BEFORE cancel
    let saved_outcome = s.mt_outcome.clone();
    let saved_size = s.mt_size;
    let saved_budget = s.mt_budget;
    let saved_entry = s.mt_entry;
    let saved_state = s.mt_state;

    // Cancel everything
    cancel_all_manual(s).await;

    if market {
        // Restore position info for market sell
        if saved_state >= 2 {
            s.mt_outcome = saved_outcome;
            s.mt_size = saved_size;
            s.mt_budget = saved_budget;
            s.mt_entry = saved_entry;
            s.mt_state = 2;
        }
        exec_liq_market(s).await;
        return;
    }

    if let Some(price) = price_opt {
        // Determine outcome: use saved, or infer from command context
        let liq_outcome = if !outcome.is_empty() { outcome } else { saved_outcome.clone() };

        if saved_state >= 2 && saved_outcome == liq_outcome {
            // Restore position tracking for the liquidation
            s.mt_outcome = liq_outcome.clone();
            s.mt_size = saved_size;
            s.mt_budget = saved_budget;
            s.mt_entry = saved_entry;
            s.mt_state = 2;
        }
        exec_liq_limit(&liq_outcome, price, s).await;
    }
}

// ═══════════════════════════════════════════════════════════════════
// EXECUTORS: SL
// ═══════════════════════════════════════════════════════════════════

async fn exec_sl_toggle(s: &mut State) {
    s.sl_market = !s.sl_market;
    s.add_log(format!("SL: {} {}", if s.sl_market {"MARKET"}else{"LIMIT"}, if s.sl_pct > 0.0 {format!("{}%", s.sl_pct)}else{"OFF".into()}), Color::Yellow);

    // Refresh SL order if position is active
    if s.mt_state == 2 && s.sl_pct > 0.0 {
        place_sl_order(s).await;
    }
}

async fn exec_sl_set(pct: f64, s: &mut State) {
    s.sl_pct = pct;
    s.add_log(format!("SL: {}% {}", s.sl_pct, if s.sl_market {"MARKET"}else{"LIMIT"}), Color::Yellow);
    s.add_trade_log(format!("🛡 SL {:.0}% {}", pct, if s.sl_market {"MKT"}else{"LMT"}), Color::Yellow);

    if s.mt_state == 2 {
        place_sl_order(s).await;
    }
}

async fn exec_sl_off(s: &mut State) {
    s.sl_pct = 0.0;
    s.add_log("SL: OFF — sin stop loss".to_string(), Color::DarkGray);
    s.add_trade_log("🛡 SL OFF".to_string(), Color::DarkGray);

    if !s.mt_sl_order_id.is_empty() {
        if let Err(e) = http_delete(&format!("/api/orders/{}", s.mt_sl_order_id)).await {
            s.add_log(format!("Cancel SL FAIL: {}", e), Color::Red);
        } else {
            s.add_log("SL cancelado".to_string(), Color::Green);
        }
        s.mt_sl_order_id.clear();
    }
}

// ═══════════════════════════════════════════════════════════════════
// SHARED HELPERS
// ═══════════════════════════════════════════════════════════════════

async fn cancel_all_manual(s: &mut State) {
    s.add_log("Cancelando todo lo manual...".to_string(), Color::Yellow);
    let ids: Vec<String> = {
        let mut v = Vec::new();
        if !s.mt_order_id.is_empty() { v.push(s.mt_order_id.clone()); }
        if !s.mt_exit_order_id.is_empty() { v.push(s.mt_exit_order_id.clone()); }
        if !s.mt_sl_order_id.is_empty() { v.push(s.mt_sl_order_id.clone()); }
        v
    };
    for id in &ids {
        if let Err(e) = http_delete(&format!("/api/orders/{}", id)).await {
            s.add_log(format!("Cancel {} FAIL: {}", id, e), Color::Red);
        } else {
            s.add_log(format!("Orden {} cancelada", id), Color::Green);
        }
    }
    s.reset_manual();
}

pub async fn place_sl_order(s: &mut State) {
    if s.sl_pct <= 0.0 || s.mt_state != 2 { return; }

    let sl_price = s.mt_entry * (1.0 - s.sl_pct / 100.0);
    if sl_price <= 0.0 || sl_price >= 1.0 { return; }

    if !s.mt_sl_order_id.is_empty() {
        let _ = http_delete(&format!("/api/orders/{}", s.mt_sl_order_id)).await;
        s.mt_sl_order_id.clear();
    }

    let body = format!(r#"{{"side":"sell","outcome":"{}","price":{},"size":{}}}"#,
        s.mt_outcome, sl_price, s.mt_size);

    s.add_log(format!("▶ SL {}:{}% @{:.4} sz={:.0}",
        if s.sl_market {"MKT"}else{"LMT"}, s.sl_pct, sl_price, s.mt_size),
        Color::Yellow);
    s.add_trade_log(format!("🛡 SL {:.0}% @{:.4}  {}",
        s.sl_pct, sl_price, if s.sl_market {"MARKET"}else{"LIMIT"}), Color::Yellow);

    match http_post_result::<OrderPlaced>("/api/orders/limit", &body).await {
        Ok(placed) => {
            let sl_id = placed.id;
            s.mt_sl_order_id = sl_id.clone();
            s.add_log(format!("  SL ID: {}", sl_id), Color::Cyan);
        }
        Err(e) => {
            s.add_log(format!("SL FAIL: {}", e), Color::Red);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// FILL TRACKER — called from main.rs on each orders poll
// ═══════════════════════════════════════════════════════════════════

pub async fn track_manual_fills(s: &mut State) {
    if s.mt_state == 1 {
        let order_info: Option<(String, f64, f64, f64, bool, bool)> = {
            let our_order = if !s.mt_order_id.is_empty() {
                s.open_orders.iter().find(|o| o.id == s.mt_order_id)
            } else {
                s.open_orders.iter().find(|o| {
                    o.outcome == s.mt_outcome
                    && o.side == "buy"
                    && (o.price - s.mt_entry).abs() < 0.001
                })
            };
            our_order.map(|o| (
                o.id.clone(), o.size_orig, o.size_matched, o.price,
                o.is_filled(), o.is_partial()
            ))
        };

        if let Some((order_id, size_orig, size_matched, _price, filled, partial)) = order_info {
            s.mt_order_seen = true;
            if !order_id.is_empty() { s.mt_order_id = order_id; }
            let pct = if size_orig > 0.0 { (size_matched / size_orig * 100.0).min(100.0) } else { 0.0 };
            s.mt_last_fill_pct = pct;

            if filled {
                let outcome_up = s.mt_outcome.clone();
                let entry = s.mt_entry;
                let budget = s.mt_budget;
                let sz = size_matched;
                s.mt_state = 2;
                s.mt_size = sz;
                s.add_log(format!("▲ FILLED {} @{:.4} sz={:.0} ${:.2}",
                    outcome_up.to_uppercase(), entry, sz, budget), Color::Green);
                s.add_trade_log(format!("✓ BUY {} sz={:.0} @{:.4} — ACTIVO",
                    outcome_up.to_uppercase(), sz, entry), Color::Green);
                if !s.mt_exit_order_id.is_empty() { s.mt_state = 3; }
            } else if partial {
                s.add_log(format!("◐ FILLING {} {:.0}%", s.mt_outcome.to_uppercase(), pct), Color::Yellow);
            }
        } else if s.mt_order_seen {
            // Was seen, now gone → filled
            s.mt_state = 2;
            let outcome = s.mt_outcome.clone();
            let entry = s.mt_entry;
            let sz = s.mt_size;
            let budget = s.mt_budget;
            s.add_log(format!("▲ FILLED {} @{:.4} sz={:.0} ${:.2}",
                outcome.to_uppercase(), entry, sz, budget), Color::Green);
            s.add_trade_log(format!("✓ BUY {} sz={:.0} @{:.4} — ACTIVO",
                outcome.to_uppercase(), sz, entry), Color::Green);
            if !s.mt_exit_order_id.is_empty() { s.mt_state = 3; }
        } else if s.mt_order_placed_at.elapsed() < std::time::Duration::from_secs(3) {
            // Recently placed, not yet visible. Wait.
        } else {
            // Not seen for >3s → filled silently (fast fill between polls)
            s.mt_state = 2;
            let outcome = s.mt_outcome.clone();
            let entry = s.mt_entry;
            let sz = s.mt_size;
            let budget = s.mt_budget;
            s.add_log(format!("▲ FILLED {} @{:.4} sz={:.0} ${:.2} (fast)",
                outcome.to_uppercase(), entry, sz, budget), Color::Green);
            s.add_trade_log(format!("✓ BUY {} sz={:.0} @{:.4} — ACTIVO",
                outcome.to_uppercase(), sz, entry), Color::Green);
            if !s.mt_exit_order_id.is_empty() { s.mt_state = 3; }
        }
    }

    if s.mt_state == 3 && !s.mt_exit_order_id.is_empty() {
        let exit_id = s.mt_exit_order_id.clone();
        let (found_in_list, is_filled, is_partial, partial_pct) = {
            if let Some(o) = s.open_orders.iter().find(|o| o.id == exit_id) {
                let pct = if o.size_orig > 0.0 { (o.size_matched / o.size_orig * 100.0).min(100.0) } else { 0.0 };
                (true, o.is_filled(), o.is_partial(), pct)
            } else { (false, false, false, 0.0) }
        };

        if found_in_list { s.mt_exit_order_seen = true; }

        if is_partial {
            s.add_log(format!("◐ EXIT FILLING {:.0}%", partial_pct), Color::Yellow);
        }

        if is_filled {
            let exit_px = s.mt_exit_price;
            finalize_manual_trade(s, exit_px).await;
        } else if !found_in_list && s.mt_exit_order_seen {
            // Was seen before, now gone → filled
            let exit_px = s.mt_exit_price;
            finalize_manual_trade(s, exit_px).await;
        }
        // Exit not found AND never seen → keep waiting, API may be slow
    }
}

pub async fn finalize_manual_trade(s: &mut State, exit_price: f64) {
    let entry = s.mt_entry;
    let size = s.mt_size;
    let pnl = size * (exit_price - entry);
    let pnl_pct = if entry > 0.0 { (exit_price / entry - 1.0) * 100.0 } else { 0.0 };

    s.mt_pnl_cum += pnl;
    s.mt_trades += 1;
    if pnl >= 0.0 { s.mt_wins += 1; }

    let pnl_c = if pnl >= 0.0 { Color::Green } else { Color::Red };
    s.add_log(format!("▼ EXIT {} @{:.4}  PnL:{:+.2} ({:+.1}%)  Cum:{:+.2}",
        s.mt_outcome.to_uppercase(), exit_price, pnl, pnl_pct, s.mt_pnl_cum), pnl_c);
    s.add_trade_log(format!("▼ EXIT {} sz={:.0} @{:.4}  PnL:{:+.2} ({:+.1}%)  Σ:{:+.2}",
        s.mt_outcome.to_uppercase(), size, exit_price, pnl, pnl_pct, s.mt_pnl_cum), pnl_c);

    // Cancel SL if exists
    if !s.mt_sl_order_id.is_empty() {
        let sl_id = s.mt_sl_order_id.clone();
        let _ = http_delete(&format!("/api/orders/{}", sl_id)).await;
        s.mt_sl_order_id.clear();
    }

    s.reset_manual();
}

// ═══════════════════════════════════════════════════════════════════
// TRAILING STOP
// ═══════════════════════════════════════════════════════════════════

async fn exec_tsl_set(pct: f64, s: &mut State) {
    s.mt_tsl_pct = pct;
    s.add_log(format!("▶ TSL: {:.0}% trailing", pct), Color::Magenta);
    s.add_trade_log(format!("📈 TSL {:.0}%", pct), Color::Magenta);
    if s.mt_state == 2 {
        update_trailing_stop(s).await;
    }
}

async fn exec_tsl_off(s: &mut State) {
    s.mt_tsl_pct = 0.0;
    s.add_log("TSL: OFF".to_string(), Color::DarkGray);
    s.add_trade_log("📈 TSL OFF".to_string(), Color::DarkGray);
    if !s.mt_sl_order_id.is_empty() {
        let _ = http_delete(&format!("/api/orders/{}", s.mt_sl_order_id)).await;
        s.mt_sl_order_id.clear();
    }
}

pub async fn update_trailing_stop(s: &mut State) {
    if s.mt_tsl_pct <= 0.0 || s.mt_state != 2 { return; }

    let current_px = if s.mt_outcome == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn };
    if current_px <= 0.0 { return; }

    let (new_extreme, new_sl_price) = if s.mt_outcome == "up" {
        let high = s.mt_tsl_high.max(current_px);
        let sl = high * (1.0 - s.mt_tsl_pct / 100.0);
        (high, sl)
    } else {
        let low = s.mt_tsl_low.min(current_px);
        let sl = low * (1.0 + s.mt_tsl_pct / 100.0);
        (low, sl)
    };

    let changed = (s.mt_outcome == "up" && new_extreme > s.mt_tsl_high)
        || (s.mt_outcome == "down" && new_extreme < s.mt_tsl_low);
    if s.mt_outcome == "up" { s.mt_tsl_high = new_extreme; } else { s.mt_tsl_low = new_extreme; }

    if changed {
        // Cancel old + place new SL
        if !s.mt_sl_order_id.is_empty() {
            let _ = http_delete(&format!("/api/orders/{}", s.mt_sl_order_id)).await;
            s.mt_sl_order_id.clear();
        }
        let body = format!(r#"{{"side":"sell","outcome":"{}","price":{},"size":{}}}"#,
            s.mt_outcome, new_sl_price, s.mt_size);
        match http_post_result::<OrderPlaced>("/api/orders/limit", &body).await {
            Ok(placed) => {
                s.mt_sl_order_id = placed.id.clone();
                s.add_log(format!("📈 TSL {}→SL @{:.4}",
                    if s.mt_outcome=="up" { format!("{:.4}", s.mt_tsl_high) } else { format!("{:.4}", s.mt_tsl_low) },
                    new_sl_price), Color::Magenta);
            }
            Err(e) => {
                s.add_log(format!("TSL FAIL: {}", e), Color::Red);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// ALERTS
// ═══════════════════════════════════════════════════════════════════

fn exec_alert_set(outcome: String, price: f64, s: &mut State) {
    let outcome_up = outcome.to_uppercase();
    s.alerts.push(Alert { outcome, price, triggered: false });
    s.add_log(format!("🔔 ALERT {} @{:.4}", outcome_up, price), Color::Cyan);
    s.add_trade_log(format!("🔔 Alert {} @{:.4}", outcome_up, price), Color::Cyan);
}

fn exec_alert_clear(s: &mut State) {
    let n = s.alerts.len();
    s.alerts.clear();
    s.add_log(format!("🔔 {} alertas borradas", n), Color::DarkGray);
}

pub fn check_alerts(s: &mut State) {
    let mut triggered: Vec<(String, f64, f64)> = Vec::new();
    for a in &mut s.alerts {
        if a.triggered { continue; }
        let current_px = if a.outcome == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn };
        if current_px <= 0.0 { continue; }
        let crossed = if a.outcome == "up" { current_px >= a.price } else { current_px <= a.price };
        if crossed {
            a.triggered = true;
            triggered.push((a.outcome.clone(), a.price, current_px));
        }
    }
    for (outcome, price, current_px) in triggered {
        print!("\x07");
        s.add_log(format!("🔔🔔 ALERTA {} @{:.4} TOCADO! px:{:.4}", outcome.to_uppercase(), price, current_px), Color::Red);
        s.add_trade_log(format!("🔔 ALERTA {} @{:.4}!", outcome.to_uppercase(), price), Color::Red);
    }
    s.alerts.retain(|a| !a.triggered);
}

// ═══════════════════════════════════════════════════════════════════
// UNDO
// ═══════════════════════════════════════════════════════════════════

async fn exec_undo(s: &mut State) {
    if s.last_order_id.is_empty() {
        s.add_log("Nada que deshacer".to_string(), Color::DarkGray);
        return;
    }
    let id = s.last_order_id.clone();
    let typ = s.last_order_type.clone();
    s.add_log(format!("↩ Undo {} {}", typ, id), Color::Yellow);
    if let Err(e) = http_delete(&format!("/api/orders/{}", id)).await {
        s.add_log(format!("Undo FAIL: {}", e), Color::Red);
    } else {
        s.add_log(format!("{} cancelado", id), Color::Green);
        s.add_trade_log(format!("↩ Undo {}", id), Color::Yellow);
    }
    s.last_order_id.clear();
    s.last_order_type.clear();
}

// ═══════════════════════════════════════════════════════════════════
// CASH OUT — /co
// ═══════════════════════════════════════════════════════════════════

async fn exec_cashout(s: &mut State) {
    s.add_log("💰 CASH OUT — liquidando todo".to_string(), Color::Yellow);
    s.add_trade_log("💰 CASH OUT — todo a USD".to_string(), Color::Yellow);

    // 1) Market sell manual position
    if s.mt_state >= 2 {
        let outcome = s.mt_outcome.clone();
        let body = format!(r#"{{"side":"sell","outcome":"{}","amount_usdc":{}}}"#,
            outcome, s.mt_budget);
        match http_post_result::<OrderPlaced>("/api/orders/market", &body).await {
            Ok(_) => {
                s.add_log(format!("Market sell {} OK", outcome.to_uppercase()), Color::Green);
                s.add_trade_log(format!("💰 MKT {} liquidado", outcome.to_uppercase()), Color::Green);
            }
            Err(e) => {
                s.add_log(format!("Market sell FAIL: {}", e), Color::Red);
                s.add_trade_log(format!("\u{2717} CashOut FAIL: {}", e), Color::Red);
            }
        }
    }

    // 2) Cancel pending manual orders
    cancel_all_manual(s).await;

    // 3) PANIC strategies
    if s.pos_sen_up || s.pos_sen_dn || s.pos_h65_up || s.pos_h65_dn || s.pos_odi_up || s.pos_odi_dn {
        s.add_log("PANIC estrategias...".to_string(), Color::Yellow);
        let _ = http_post("/api/panic", "{}").await;
    }

    s.reset_manual();
    s.add_log("💰 CASH OUT completo".to_string(), Color::Green);
    s.add_trade_log("💰 Cash out completo".to_string(), Color::Green);
}

// ═══════════════════════════════════════════════════════════════════
// ALERT STRUCT (re-exported for State)
// ═══════════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct Alert {
    pub outcome: String,
    pub price: f64,
    pub triggered: bool,
}

// ═══════════════════════════════════════════════════════════════════
// UNIT TESTS
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(input: &str) -> String {
        match parse(input) {
            Parsed::BuyManual { amount, side, price, exit, sl } =>
                format!("BUY ${amount:.0} {side} @{price:.4} exit={:?} sl={:?}", exit, sl),
            Parsed::CancelActive => "CANCEL".into(),
            Parsed::CancelId(id) => format!("CANCEL-ID({id})"),
            Parsed::CancelLiquidateLimit { outcome, price } =>
                format!("CANCEL+LIQ {outcome} @{price:.4}"),
            Parsed::CancelLiquidateMarket => "CANCEL+LIQ MKT".into(),
            Parsed::LiquidateLimit { outcome, price } =>
                format!("LIQ {outcome} @{price:.4}"),
            Parsed::LiquidateMarket => "LIQ MKT".into(),
            Parsed::Panic => "PANIC".into(),
            Parsed::SlToggle => "SL-TOGGLE".into(),
            Parsed::SlSet(pct) => format!("SL-{pct:.0}%"),
            Parsed::SlOff => "SL-OFF".into(),
            Parsed::ShowMan => "MAN".into(),
            Parsed::QuitMan => "QUIT".into(),
            Parsed::ShowPosition => "POS".into(),
            Parsed::TrailingStop(pct) => format!("TSL-{pct:.0}%"),
            Parsed::TrailingStopOff => "TSL-OFF".into(),
            Parsed::AlertSet { outcome, price } => format!("ALERT {outcome} @{price:.4}"),
            Parsed::AlertClear => "ALERT-CLEAR".into(),
            Parsed::Undo => "UNDO".into(),
            Parsed::CashOut => "CASHOUT".into(),
            Parsed::Unknown(s) => format!("UNKNOWN:{s}"),
        }
    }

    #[test]
    fn buy_up_limit() {
        assert_eq!(parsed("l10up65"),    "BUY $10 up @0.6500 exit=None sl=None");
        assert_eq!(parsed("l25up50"),    "BUY $25 up @0.5000 exit=None sl=None");
        assert_eq!(parsed("l100d55"),    "BUY $100 down @0.5500 exit=None sl=None");
    }

    #[test]
    fn buy_up_with_exit() {
        assert_eq!(parsed("l10up65e70"), "BUY $10 up @0.6500 exit=Some(0.7) sl=None");
        assert_eq!(parsed("l20d50e60"),  "BUY $20 down @0.5000 exit=Some(0.6) sl=None");
    }

    #[test]
    fn buy_bracket() {
        assert_eq!(parsed("l10up65e70s50"), "BUY $10 up @0.6500 exit=Some(0.7) sl=Some(0.5)");
        assert_eq!(parsed("l20d50e60s40"),  "BUY $20 down @0.5000 exit=Some(0.6) sl=Some(0.4)");
    }

    #[test]
    fn buy_legacy_format() {
        assert_eq!(parsed("10up65"),     "BUY $10 up @0.6500 exit=None sl=None");
        assert_eq!(parsed("15d40"),      "BUY $15 down @0.4000 exit=None sl=None");
        assert_eq!(parsed("10up65e70"),  "BUY $10 up @0.6500 exit=Some(0.7) sl=None");
    }

    #[test]
    fn aliases() {
        assert_eq!(parsed("b10up65"),     "BUY $10 up @0.6500 exit=None sl=None");
        assert_eq!(parsed("b10up65e70"),  "BUY $10 up @0.6500 exit=Some(0.7) sl=None");
        assert_eq!(parsed("k"),           "CANCEL");
        assert_eq!(parsed("kup65"),       "CANCEL+LIQ up @0.6500");
        assert_eq!(parsed("kd70"),        "CANCEL+LIQ down @0.7000");
        assert_eq!(parsed("km"),          "CANCEL+LIQ MKT");
        assert_eq!(parsed("x"),           "LIQ MKT");
        assert_eq!(parsed("xu70"),        "LIQ up @0.7000");
        assert_eq!(parsed("xd70"),        "LIQ down @0.7000");
    }

    #[test]
    fn tsl_commands() {
        assert_eq!(parsed("tsl5"),  "TSL-5%");
        assert_eq!(parsed("tsl10"), "TSL-10%");
        assert_eq!(parsed("tsl0"),  "TSL-1%"); // clamped
        assert_eq!(parsed("ntsl"),  "TSL-OFF");
    }

    #[test]
    fn alert_commands() {
        assert_eq!(parsed("alert up 0.70"), "ALERT up @0.7000");
        assert_eq!(parsed("alert down 0.50"), "ALERT down @0.5000");
        assert_eq!(parsed("alert clear"), "ALERT-CLEAR");
    }

    #[test]
    fn pos_command() {
        assert_eq!(parsed("pos"), "POS");
    }

    #[test]
    fn undo_command() {
        assert_eq!(parsed("u"), "UNDO");
    }

    #[test]
    fn buy_boundaries() {
        assert_eq!(parsed("l1up50"),   "BUY $1 up @0.5000 exit=None sl=None");
        assert_eq!(parsed("l200up99"), "BUY $200 up @0.9900 exit=None sl=None");
        assert_eq!(parsed("l10up99"),  "BUY $10 up @0.9900 exit=None sl=None");
        assert_eq!(parsed("l10up1"),   "BUY $10 up @0.0100 exit=None sl=None");
    }

    #[test]
    fn buy_invalid() {
        assert!(parsed("l0up50").starts_with("UNKNOWN"));
        assert!(parsed("l201up50").starts_with("UNKNOWN"));
        assert!(parsed("l10up100").starts_with("UNKNOWN"));
        assert!(parsed("l10up0").starts_with("UNKNOWN"));
    }

    #[test]
    fn liquidate_commands() {
        assert_eq!(parsed("lup70"), "LIQ up @0.7000");
        assert_eq!(parsed("ld70"),  "LIQ down @0.7000");
        assert_eq!(parsed("ldown70"), "LIQ down @0.7000");
        assert_eq!(parsed("lm"),    "LIQ MKT");
    }

    #[test]
    fn cancel_commands() {
        assert_eq!(parsed("c"), "CANCEL");
        assert_eq!(parsed("cabc123def456789"), "CANCEL-ID(abc123def456789)");
    }

    #[test]
    fn cancel_liq_commands() {
        assert_eq!(parsed("clup65"),  "CANCEL+LIQ up @0.6500");
        assert_eq!(parsed("cld70"),   "CANCEL+LIQ down @0.7000");
        assert_eq!(parsed("cldown70"), "CANCEL+LIQ down @0.7000");
        assert_eq!(parsed("clm"),     "CANCEL+LIQ MKT");
    }

    #[test]
    fn sl_commands() {
        assert_eq!(parsed("sl"),   "SL-TOGGLE");
        assert_eq!(parsed("sl10"), "SL-10%");
        assert_eq!(parsed("sl5"),  "SL-5%");
        assert_eq!(parsed("sl50"), "SL-50%");
        assert_eq!(parsed("sl0"),  "SL-1%");
        assert_eq!(parsed("sl51"), "SL-50%");
        assert_eq!(parsed("nsl"),  "SL-OFF");
    }

    #[test]
    fn meta_commands() {
        assert_eq!(parsed("man"),  "MAN");
        assert_eq!(parsed("quit"), "QUIT");
        assert_eq!(parsed("p"),    "PANIC");
    }

    #[test]
    fn edge_cases() {
        assert!(parsed("").starts_with("UNKNOWN"));
        assert!(parsed(" ").starts_with("UNKNOWN"));
        assert!(parsed("xyz").starts_with("UNKNOWN"));
        assert!(parsed("l").starts_with("UNKNOWN"));
        assert_eq!(parsed("c123"), "CANCEL");
    }

    #[test]
    fn parser_helpers() {
        assert_eq!(parse_amount("10up65"), Some((10.0, "up65")));
        assert_eq!(parse_amount("200d50"), Some((200.0, "d50")));
        assert_eq!(parse_amount("0up50"), None);
        assert_eq!(parse_amount("abc"), None);
        assert_eq!(parse_side_name("up65"), Some(("up", "65")));
        assert_eq!(parse_side_name("d40"), Some(("down", "40")));
        assert_eq!(parse_side_name("xx"), None);
        assert_eq!(parse_cents("65"), Some((0.65, "")));
        assert_eq!(parse_cents("65e70"), Some((0.65, "e70")));
        assert_eq!(parse_cents("99"), Some((0.99, "")));
        assert_eq!(parse_cents("100"), None);
        assert_eq!(parse_cents("0"), None);
    }

    #[test]
    fn comprehensive_matrix() {
        let cases = vec![
            ("l10up65",     "BUY $10 up @0.6500 exit=None sl=None"),
            ("l10up65e70",  "BUY $10 up @0.6500 exit=Some(0.7) sl=None"),
            ("l10up65e70s50", "BUY $10 up @0.6500 exit=Some(0.7) sl=Some(0.5)"),
            ("10up65",      "BUY $10 up @0.6500 exit=None sl=None"),
            ("15d40e50",    "BUY $15 down @0.4000 exit=Some(0.5) sl=None"),
            ("b10up65",     "BUY $10 up @0.6500 exit=None sl=None"),
            ("k",           "CANCEL"),
            ("x",           "LIQ MKT"),
            ("xu70",        "LIQ up @0.7000"),
            ("lup70",       "LIQ up @0.7000"),
            ("ld70",        "LIQ down @0.7000"),
            ("ldown70",     "LIQ down @0.7000"),
            ("lm",          "LIQ MKT"),
            ("c",           "CANCEL"),
            ("clup65",      "CANCEL+LIQ up @0.6500"),
            ("cld70",       "CANCEL+LIQ down @0.7000"),
            ("cldown80",    "CANCEL+LIQ down @0.8000"),
            ("clm",         "CANCEL+LIQ MKT"),
            ("sl",          "SL-TOGGLE"),
            ("sl10",        "SL-10%"),
            ("nsl",         "SL-OFF"),
            ("tsl5",        "TSL-5%"),
            ("ntsl",        "TSL-OFF"),
            ("alert up 0.70", "ALERT up @0.7000"),
            ("alert clear", "ALERT-CLEAR"),
            ("pos",         "POS"),
            ("u",           "UNDO"),
            ("p",           "PANIC"),
            ("man",         "MAN"),
            ("quit",        "QUIT"),
        ];
        for (input, expected) in cases {
            assert_eq!(parsed(input), expected, "FAIL: /{input}");
        }
    }
}
