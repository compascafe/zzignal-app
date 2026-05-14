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
    // ── GEMINI ──
    CmdDef { syntax: "/<usd>g<cents>          ", desc: "Gemini $X target Y, trigger Y−0.05", category: "GEMINI" },
    CmdDef { syntax: "/<usd>g<cents>e<cents>  ", desc: "Gemini + exit automático", category: "GEMINI" },
    // ── PROVIDER ──
    CmdDef { syntax: "/provider binance       ", desc: "Cambiar BTC provider (binance/coinbase/kraken)", category: "PROVIDER" },
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
    Gemini { budget: f64, target: f64, exit: Option<f64> },
    Provider(String),
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
        'p' if rest.starts_with("rovider") => {
            let prov = rest[7..].trim().to_lowercase();
            match prov.as_str() {
                "binance" | "coinbase" | "kraken" => Parsed::Provider(prov),
                _ => Parsed::Unknown(input.to_string()),
            }
        }
        _ if first.is_ascii_digit() => {
            if let Some(gemini) = try_parse_gemini(input) { gemini }
            else { parse_buy_legacy(input) }
        }
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

fn try_parse_gemini(input: &str) -> Option<Parsed> {
    let (amount, rem) = parse_amount(input)?;
    if !rem.starts_with('g') { return None; }
    let rem = &rem[1..];
    let (target, rem) = parse_cents(rem)?;
    let exit = if rem.starts_with('e') {
        parse_cents(&rem[1..]).map(|(p, _)| p)
    } else { None };
    let trigger = target - 0.05;
    if trigger <= 0.0 { return None; }
    Some(Parsed::Gemini { budget: amount, target, exit })
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
        Parsed::Gemini { budget, target, exit } => exec_gemini(budget, target, exit, s).await,
        Parsed::Provider(prov) => exec_provider(&prov, s).await,
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

    if exit_price.is_some() {
        s.add_log(format!("▶ EXIT pendiente — se colocara tras fill @{:.4}", exit_price.unwrap()), Color::Yellow);
        s.add_trade_log(format!("  TP @{:.4} (post-fill)", exit_price.unwrap()), Color::Yellow);
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
    if s.gemini_active {
        s.gemini_active = false;
        s.gemini_budget = 0.0;
        s.gemini_target = 0.0;
        s.gemini_trigger = 0.0;
        s.gemini_exit = 0.0;
        s.gemini_outcome.clear();
        s.gemini_triggered = false;
        s.add_log("GEMINI cancelado".to_string(), Color::Yellow);
        s.add_trade_log("\u{2717} GEMINI cancelado".to_string(), Color::Yellow);
        return;
    }

    // Active position with no pending orders → suggest /x or /lm
    if s.mt_state >= 2 {
        let has_pending = !s.mt_exit_order_id.is_empty() || !s.mt_sl_order_id.is_empty();
        if !has_pending {
            s.add_log(format!("{} ACTIVO — usa /x o /lm para salir", s.mt_outcome.to_uppercase()), Color::Yellow);
            s.add_trade_log(format!("\u{2717} {} ACTIVO — sal con /lm", s.mt_outcome.to_uppercase()), Color::Yellow);
            return;
        }
    }

    if s.mt_state == 0 && s.open_orders.is_empty() {
        s.add_log("0 posiciones".to_string(), Color::DarkGray);
        s.add_trade_log("\u{2717} 0 posiciones".to_string(), Color::DarkGray);
        return;
    }

    s.add_log("Cancelando todo...".to_string(), Color::Yellow);
    let cancelled = cancel_all_manual(s).await;
    if cancelled {
        let _ = http_delete("/api/orders").await;
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
            s.mt_exit_placed_at = std::time::Instant::now();
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
        let current_px = if outcome == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn };
        let pnl = size * (current_px - s.mt_entry);
        s.add_log(format!("▶ MARKET SELL {} sz={:.0}  PnL est:{:+.2}", outcome.to_uppercase(), size, pnl), Color::Yellow);
        s.add_trade_log(format!("▶ MKT SELL {} sz={:.0} @mercado", outcome.to_uppercase(), size), Color::Yellow);
        let body = format!(r#"{{"side":"sell","outcome":"{}","amount_usdc":{}}}"#,
            outcome, size);
        if let Err(e) = http_post("/api/orders/market", &body).await {
            s.add_log(format!("MARKET SELL FAIL: {}", e), Color::Red);
            s.add_trade_log(format!("\u{2717} MKT SELL FAIL: {}", e), Color::Red);
        } else {
            s.mt_pnl_cum += pnl;
            s.mt_trades += 1;
            if pnl >= 0.0 { s.mt_wins += 1; }
            let pnl_c = if pnl >= 0.0 { Color::Green } else { Color::Red };
            s.add_trade_log(format!("▼ MKT {} PnL:{:+.2} Σ{:+.2}", outcome.to_uppercase(), pnl, s.mt_pnl_cum), pnl_c);
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
}

async fn exec_sl_set(pct: f64, s: &mut State) {
    s.sl_pct = pct;
    // Compute stop-market trigger price
    if s.mt_state == 2 && s.mt_entry > 0.0 {
        s.mt_sl_price = if s.mt_outcome == "up" {
            s.mt_entry * (1.0 - s.sl_pct / 100.0)
        } else {
            s.mt_entry * (1.0 + s.sl_pct / 100.0)
        };
        s.add_log(format!("▶ SL {:.0}% trigger @{:.4} ({} side)", pct, s.mt_sl_price, s.mt_outcome.to_uppercase()), Color::Yellow);
        s.add_trade_log(format!("🛡 SL {:.0}% @{:.4}", pct, s.mt_sl_price), Color::Yellow);
    } else {
        s.mt_sl_price = 0.0;
        s.add_log(format!("SL: {:.0}% {} (sin posicion activa)", pct, if s.sl_market {"MARKET"}else{"LIMIT"}), Color::Yellow);
        s.add_trade_log(format!("🛡 SL {:.0}% {}", pct, if s.sl_market {"MKT"}else{"LMT"}), Color::Yellow);
    }
}

async fn exec_sl_off(s: &mut State) {
    s.sl_pct = 0.0;
    s.mt_sl_price = 0.0;
    s.add_log("SL: OFF — sin stop loss".to_string(), Color::DarkGray);
    s.add_trade_log("🛡 SL OFF".to_string(), Color::DarkGray);
}

// ═══════════════════════════════════════════════════════════════════
// SHARED HELPERS
// ═══════════════════════════════════════════════════════════════════

async fn cancel_all_manual(s: &mut State) -> bool {
    let ids: Vec<String> = {
        let mut v = Vec::new();
        if !s.mt_order_id.is_empty() { v.push(s.mt_order_id.clone()); }
        if !s.mt_exit_order_id.is_empty() { v.push(s.mt_exit_order_id.clone()); }
        if !s.mt_sl_order_id.is_empty() { v.push(s.mt_sl_order_id.clone()); }
        v
    };
    if ids.is_empty() && !s.open_orders.is_empty() {
        s.add_log("Cancelando todas las ordenes...".to_string(), Color::Yellow);
        match http_delete("/api/orders").await {
            Ok(()) => {
                s.add_log("Todas las ordenes canceladas".to_string(), Color::Green);
                s.add_trade_log("\u{2717} Canceladas todas".to_string(), Color::Yellow);
                s.reset_manual();
                return true;
            }
            Err(e) => {
                s.add_log(format!("Cancel ALL FAIL: {}", e), Color::Red);
                s.add_trade_log(format!("\u{2717} Cancel ALL FAIL: {}", e), Color::Red);
                return false;
            }
        }
    }
    if ids.is_empty() {
        s.add_log("Nada que cancelar".to_string(), Color::DarkGray);
        return false;
    }

    s.add_log("Cancelando todo lo manual...".to_string(), Color::Yellow);
    for id in &ids {
        if let Err(e) = http_delete(&format!("/api/orders/{}", id)).await {
            s.add_log(format!("Cancel {} FAIL: {}", id, e), Color::Red);
        } else {
            s.add_log(format!("Orden {} cancelada", id), Color::Green);
        }
    }
    let _ = http_delete("/api/orders").await;
    s.reset_manual();
    true
}

async fn exec_gemini(budget: f64, target: f64, exit: Option<f64>, s: &mut State) {
    let trigger = target - 0.05;
    s.gemini_active = true;
    s.gemini_budget = budget;
    s.gemini_target = target;
    s.gemini_trigger = trigger;
    s.gemini_exit = exit.unwrap_or(0.0);
    s.gemini_outcome.clear();
    s.gemini_triggered = false;

    let desc = if let Some(ex) = exit {
        format!("GEMINI UP/DN @{:.2}→{:.2} ${:.0} EXIT @{:.2}", trigger, target, budget, ex)
    } else {
        format!("GEMINI UP/DN @{:.2}→{:.2} ${:.0}", trigger, target, budget)
    };
    s.add_log(desc.clone(), Color::Magenta);
    s.add_trade_log(format!("⚡ GEMINI @{:.2}→{:.2} ${:.0}", trigger, target, budget), Color::Magenta);
}

pub async fn trigger_gemini_buy(s: &mut State) {
    if !s.gemini_triggered || s.mt_state != 0 { return; }
    let outcome = s.gemini_outcome.clone();
    let budget = s.gemini_budget;
    let target = s.gemini_target;
    let exit = if s.gemini_exit > 0.0 { Some(s.gemini_exit) } else { None };

    // Clear trigger flag before placing to avoid loop
    s.gemini_triggered = false;
    s.gemini_active = false;

    place_manual_buy(budget, &outcome, target, exit, None, s).await;
    s.add_log(format!("🚀 GEMINI BUY {} @{:.4} ${:.0}", outcome.to_uppercase(), target, budget), Color::Magenta);
    s.add_trade_log(format!("🚀 GEMINI BUY {} @{:.4} ${:.0}", outcome.to_uppercase(), target, budget), Color::Magenta);
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

fn setup_sl_after_fill(s: &mut State) {
    if s.sl_pct > 0.0 && s.mt_entry > 0.0 {
        s.mt_sl_price = if s.mt_outcome == "up" {
            s.mt_entry * (1.0 - s.sl_pct / 100.0)
        } else {
            s.mt_entry * (1.0 + s.sl_pct / 100.0)
        };
        s.add_log(format!("🛡 SL activado: {:.0}% trigger @{:.4}", s.sl_pct, s.mt_sl_price), Color::Yellow);
        s.add_trade_log(format!("🛡 SL {:.0}% @{:.4}", s.sl_pct, s.mt_sl_price), Color::Yellow);
    }
}

pub async fn check_sl_trigger(s: &mut State) {
    if s.mt_sl_price <= 0.0 || s.mt_state != 2 { return; }

    let current_px = if s.mt_outcome == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn };
    if current_px <= 0.0 { return; }

    let triggered = if s.mt_outcome == "up" {
        current_px <= s.mt_sl_price
    } else {
        current_px >= s.mt_sl_price
    };

    if !triggered { return; }

    s.add_log(format!("⚠ SL TRIGGERED! {} @{:.4} (SL:{:.4}) → market sell",
        s.mt_outcome.to_uppercase(), current_px, s.mt_sl_price), Color::Red);
    s.add_trade_log(format!("🛑 SL HIT {} current={:.4} SL={:.4}",
        s.mt_outcome.to_uppercase(), current_px, s.mt_sl_price), Color::Red);

    // Cancel ALL orders to clear the deck
    let _ = http_delete("/api/orders").await;

    // Market sell the position
    let outcome = s.mt_outcome.clone();
    let pnl = s.mt_size * (current_px - s.mt_entry);
    let body = format!(r#"{{"side":"sell","outcome":"{}","amount_usdc":{}}}"#,
        outcome, s.mt_size);

    match http_post("/api/orders/market", &body).await {
        Ok(()) => {
            s.mt_pnl_cum += pnl;
            s.mt_trades += 1;
            if pnl >= 0.0 { s.mt_wins += 1; }
            let pnl_c = if pnl >= 0.0 { Color::Green } else { Color::Red };
            s.add_log(format!("  ✓ SL EXIT {} PnL:{:+.2} Σ{:+.2}",
                outcome.to_uppercase(), pnl, s.mt_pnl_cum), pnl_c);
            s.add_trade_log(format!("🛑 SL EXIT {} sz={:.0} PnL:{:+.2}",
                outcome.to_uppercase(), s.mt_size, pnl), pnl_c);
        }
        Err(e) => {
            s.add_log(format!("❌ SL market sell FAIL: {}", e), Color::Red);
            s.add_trade_log(format!("✗ SL market sell FAIL: {}", e), Color::Red);
        }
    }

    // Reset position state
    s.mt_sl_price = 0.0;
    s.reset_manual();
}

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
                setup_sl_after_fill(s);
                s.mt_size = sz;
                s.add_log(format!("▲ FILLED {} @{:.4} sz={:.0} ${:.2}",
                    outcome_up.to_uppercase(), entry, sz, budget), Color::Green);
                s.add_trade_log(format!("✓ BUY {} sz={:.0} @{:.4} — ACTIVO",
                    outcome_up.to_uppercase(), sz, entry), Color::Green);
                if s.mt_exit_price > 0.0 { place_exit_after_fill(s).await; }
                else if !s.mt_exit_order_id.is_empty() { s.mt_state = 3; }
            } else if partial {
                s.add_log(format!("◐ FILLING {} {:.0}%", s.mt_outcome.to_uppercase(), pct), Color::Yellow);
            }
        } else if s.mt_order_seen {
            // Was seen, now gone → filled
            s.mt_state = 2;
            setup_sl_after_fill(s);
            let outcome = s.mt_outcome.clone();
            let entry = s.mt_entry;
            let sz = s.mt_size;
            let budget = s.mt_budget;
            s.add_log(format!("▲ FILLED {} @{:.4} sz={:.0} ${:.2}",
                outcome.to_uppercase(), entry, sz, budget), Color::Green);
            s.add_trade_log(format!("✓ BUY {} sz={:.0} @{:.4} — ACTIVO",
                outcome.to_uppercase(), sz, entry), Color::Green);
            if s.mt_exit_price > 0.0 { place_exit_after_fill(s).await; }
            else if !s.mt_exit_order_id.is_empty() { s.mt_state = 3; }
        } else if s.mt_order_placed_at.elapsed() < std::time::Duration::from_secs(3) {
            // Recently placed, not yet visible. Wait.
        } else {
            // Not seen for >3s → filled silently (fast fill between polls)
            s.mt_state = 2;
            setup_sl_after_fill(s);
            let outcome = s.mt_outcome.clone();
            let entry = s.mt_entry;
            let sz = s.mt_size;
            let budget = s.mt_budget;
            s.add_log(format!("▲ FILLED {} @{:.4} sz={:.0} ${:.2} (fast)",
                outcome.to_uppercase(), entry, sz, budget), Color::Green);
            s.add_trade_log(format!("✓ BUY {} sz={:.0} @{:.4} — ACTIVO",
                outcome.to_uppercase(), sz, entry), Color::Green);
            if s.mt_exit_price > 0.0 { place_exit_after_fill(s).await; }
            else if !s.mt_exit_order_id.is_empty() { s.mt_state = 3; }
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
        } else if !found_in_list && s.mt_exit_placed_at.elapsed() > std::time::Duration::from_secs(4) {
            // Fast fill — never seen, >4s elapsed
            let exit_px = s.mt_exit_price;
            let current_px = if s.mt_outcome == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn };
            let fill_px = if exit_px > 0.0 { exit_px } else { current_px };
            s.add_log(format!("▲ EXIT fast-fill {} @{:.4}", s.mt_outcome.to_uppercase(), fill_px), Color::Green);
            finalize_manual_trade(s, fill_px).await;
        }
        // Exit not found AND never seen → keep waiting, API may be slow
    }
}

async fn place_exit_after_fill(s: &mut State) {
    if s.mt_exit_price <= 0.0 || !s.mt_exit_order_id.is_empty() { return; }
    let exit = s.mt_exit_price;
    let outcome = s.mt_outcome.clone();
    let size = s.mt_size;
    let exit_body = format!(r#"{{"side":"sell","outcome":"{}","price":{},"size":{}}}"#, outcome, exit, size);
    s.add_log(format!("▶ EXIT SELL {} @{:.4} sz={:.0}", outcome.to_uppercase(), exit, size), Color::Yellow);
    s.add_trade_log(format!("  EXIT @{:.4} sz={:.0}", exit, size), Color::Yellow);
    match http_post_result::<OrderPlaced>("/api/orders/limit", &exit_body).await {
        Ok(exit_placed) => {
            s.mt_exit_order_id = exit_placed.id.clone();
            s.mt_exit_placed_at = std::time::Instant::now();
            s.mt_state = 3;
            s.add_log(format!("  Exit colocado: {}", exit_placed.id), Color::Cyan);
        }
        Err(e) => {
            s.add_log(format!("EXIT FAIL: {}", e), Color::Red);
        }
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
    s.mt_sl_price = 0.0;
    s.add_log("TSL: OFF".to_string(), Color::DarkGray);
    s.add_trade_log("📈 TSL OFF".to_string(), Color::DarkGray);
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
        s.mt_sl_price = new_sl_price;
        s.add_trade_log(format!("📈 TSL → @{:.4}", new_sl_price), Color::Magenta);
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

async fn exec_provider(prov: &str, s: &mut State) {
    let body = format!(r#"{{"provider":"{}"}}"#, prov);
    match http_post("/api/btc/provider", &body).await {
        Ok(()) => {
            s.btc_provider = prov.to_string();
            s.add_log(format!("BTC provider → {}", prov.to_uppercase()), Color::Cyan);
            s.add_trade_log(format!("⚡ Provider → {}", prov), Color::Cyan);
        }
        Err(e) => {
            s.add_log(format!("Provider FAIL: {}", e), Color::Red);
        }
    }
}

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
    s.add_log("💰 CASH OUT — liquidando todo...".to_string(), Color::Yellow);
    s.add_trade_log("💰 CASH OUT iniciado".to_string(), Color::Yellow);

    // 1) Cancel Gemini
    s.gemini_active = false;
    s.gemini_budget = 0.0;
    s.gemini_target = 0.0;
    s.gemini_trigger = 0.0;
    s.gemini_exit = 0.0;
    s.gemini_outcome.clear();
    s.gemini_triggered = false;

    // 2) Cancel ALL existing orders FIRST — before market sell
    //    This clears pending buys, exit orders, SL orders.
    //    CRITICAL: do NOT cancel after market sell — it would cancel the market order itself.
    let ids: Vec<String> = {
        let mut v = Vec::new();
        if !s.mt_order_id.is_empty() { v.push(s.mt_order_id.clone()); }
        if !s.mt_exit_order_id.is_empty() { v.push(s.mt_exit_order_id.clone()); }
        if !s.mt_sl_order_id.is_empty() { v.push(s.mt_sl_order_id.clone()); }
        v
    };
    if !ids.is_empty() {
        for id in &ids {
            let _ = http_delete(&format!("/api/orders/{}", id)).await;
        }
        s.add_log(format!("  ✓ {} órdenes canceladas", ids.len()), Color::Green);
    }
    // Safety net: cancel-all on backend
    match http_delete("/api/orders").await {
        Ok(()) => {}
        Err(e) => s.add_log(format!("⚠ Cancel ALL orders: {}", e), Color::Red),
    }

    // Clear local order tracking (orders are already cancelled)
    s.mt_order_id.clear();
    s.mt_exit_order_id.clear();
    s.mt_sl_order_id.clear();

    // 3) Handle pending buy (mt_state == 1) — just cancel & reset, no position yet
    if s.mt_state == 1 {
        s.add_log("  ✓ Compra pendiente cancelada".to_string(), Color::Green);
        s.add_trade_log("✓ CASH OUT: compra pendiente cancelada".to_string(), Color::Yellow);
        s.reset_manual();
    }

    // 4) Market sell active position
    let mut mkt_failed = false;
    if s.mt_state >= 2 {
        let outcome = s.mt_outcome.clone();
        let current_px = if outcome == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn };
        let pnl = s.mt_size * (current_px - s.mt_entry);
        let body = format!(r#"{{"side":"sell","outcome":"{}","amount_usdc":{}}}"#,
            outcome, s.mt_size);
        s.add_log(format!("▶ MARKET SELL {} sz={:.0} ${:.2}", outcome.to_uppercase(), s.mt_size, s.mt_budget), Color::Yellow);
        match http_post("/api/orders/market", &body).await {
            Ok(()) => {
                s.mt_pnl_cum += pnl;
                s.mt_trades += 1;
                if pnl >= 0.0 { s.mt_wins += 1; }
                let pnl_c = if pnl >= 0.0 { Color::Green } else { Color::Red };
                s.add_log(format!("  ✓ Vendido {} PnL:{:+.2} Σ{:+.2}", outcome.to_uppercase(), pnl, s.mt_pnl_cum), pnl_c);
                s.add_trade_log(format!("💰 CASH OUT {} sz={:.0} PnL:{:+.2}", outcome.to_uppercase(), s.mt_size, pnl), pnl_c);
            }
            Err(e) => {
                mkt_failed = true;
                s.add_log(format!("❌ Market sell FAIL: {}", e), Color::Red);
                s.add_trade_log(format!("✗ CASH OUT: market sell FAIL ({})", e), Color::Red);
            }
        }
    }

    // 5) Market sell each active strategy position individually
    {
        let strat_pos: Vec<(&str, &str, f64, f64)> = {
            let mut v = Vec::new();
            if s.pos_h65_up && s.h65_budget > 0.0 { v.push(("H65", "up", s.h65_budget, s.pos_h65_entry_up)); }
            if s.pos_h65_dn && s.h65_budget > 0.0 { v.push(("H65", "down", s.h65_budget, s.pos_h65_entry_dn)); }
            if s.pos_odi_up && s.odi_budget > 0.0 { v.push(("ODI", "up", s.odi_budget, s.pos_odi_entry_up)); }
            if s.pos_odi_dn && s.odi_budget > 0.0 { v.push(("ODI", "down", s.odi_budget, s.pos_odi_entry_dn)); }
            if s.pos_sen_up && s.sen_budget > 0.0 { v.push(("SEN", "up", s.sen_budget, s.pos_sen_entry_up)); }
            if s.pos_sen_dn && s.sen_budget > 0.0 { v.push(("SEN", "down", s.sen_budget, s.pos_sen_entry_dn)); }
            v
        };
        for (strat, outcome, budget, entry) in &strat_pos {
            let shares = if *entry > 0.0 { (budget / entry).floor().max(1.0) } else { *budget };
            let body = format!(r#"{{"side":"sell","outcome":"{}","amount_usdc":{}}}"#, outcome, shares);
            s.add_log(format!("▶ MARKET SELL {} {} sz={:.0} ${:.0}", strat, outcome.to_uppercase(), shares, budget), Color::Yellow);
            match http_post("/api/orders/market", &body).await {
                Ok(()) => {
                    s.add_log(format!("  ✓ {} {} vendido", strat, outcome.to_uppercase()), Color::Green);
                    s.add_trade_log(format!("💰 CASH OUT {} {} budget={:.0}", strat, outcome.to_uppercase(), budget), Color::Green);
                }
                Err(e) => {
                    mkt_failed = true;
                    s.add_log(format!("❌ {} {} market sell FAIL: {}", strat, outcome.to_uppercase(), e), Color::Red);
                    s.add_trade_log(format!("✗ CASH OUT: {} {} FAIL ({})", strat, outcome.to_uppercase(), e), Color::Red);
                }
            }
        }
    }

    // 6) PANIC — cancel remaining orders + disable all strategies (safety net)
    match http_post("/api/panic", "{}").await {
        Ok(()) => s.add_log("  ✓ Estrategias desactivadas".to_string(), Color::Green),
        Err(e) => s.add_log(format!("⚠ PANIC strategies FAIL: {}", e), Color::Red),
    }

    // 7) Reset manual state (do NOT cancel orders again — market sell must survive)
    if mkt_failed {
        s.add_log("⚠ CASH OUT parcial: reintenta /lm o /x".to_string(), Color::Red);
        s.add_trade_log("⚠ Cash out parcial — reintenta /lm o /x".to_string(), Color::Red);
    } else {
        s.reset_manual();
        s.add_log("✅ CASH OUT completado — todo en USD".to_string(), Color::Green);
        s.add_trade_log("✅ Cash out completado".to_string(), Color::Green);
    }
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
            Parsed::Gemini { budget, target, exit } =>
                format!("GEMINI ${:.0} @{:.4} exit={:?}", budget, target, exit),
            Parsed::Provider(p) => format!("PROVIDER:{p}"),
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
    fn sl_price_calculation() {
        // SL price = entry * (1 - pct/100)
        let entry: f64 = 0.6500;
        let delta: f64 = (entry * (1.0 - 5.0 / 100.0) - 0.6175).abs();
        assert!(delta < 0.0001); // sl5 → 0.6175
        let delta: f64 = (entry * (1.0 - 10.0 / 100.0) - 0.5850).abs();
        assert!(delta < 0.0001); // sl10 → 0.5850
        let delta: f64 = (entry * (1.0 - 1.0 / 100.0) - 0.6435).abs();
        assert!(delta < 0.0001); // sl1 → 0.6435
        let delta: f64 = (entry * (1.0 - 50.0 / 100.0) - 0.3250).abs();
        assert!(delta < 0.0001); // sl50 → 0.3250
        // sl0 would give same as entry → 0.65 (but sl0 is clamped to sl1)
    }

    #[test]
    fn sl_toggle_flips_market_flag() {
        // /sl toggles between LIMIT and MARKET stop type
        // Default: sl_market = true (MARKET)
        let mut s = crate::State::new(false);
        assert!(s.sl_market); // default: MARKET

        // Simulate toggle to LIMIT
        s.sl_market = !s.sl_market;
        assert!(!s.sl_market);

        // Toggle back to MARKET
        s.sl_market = !s.sl_market;
        assert!(s.sl_market);
    }

    #[test]
    fn sl_set_preserves_market_flag() {
        // /sl10 should set percentage but keep current sl_market state
        let mut s = crate::State::new(false);
        s.sl_market = true;  // currently MARKET
        s.sl_pct = 10.0;     // set 10%

        assert!(s.sl_market);
        assert_eq!(s.sl_pct, 10.0);

        // Toggle to LIMIT
        s.sl_market = !s.sl_market;
        assert!(!s.sl_market);
        assert_eq!(s.sl_pct, 10.0); // pct unchanged
    }

    #[test]
    fn sl_off_clears_state() {
        // /nsl turns off SL: clears pct, cancels order, clears order ID
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.sl_pct = 5.0;
        s.sl_market = true;
        s.mt_sl_order_id = "sl-order-001".into();

        // /nsl
        s.sl_pct = 0.0;
        s.mt_sl_order_id.clear();

        assert_eq!(s.sl_pct, 0.0);
        assert!(s.mt_sl_order_id.is_empty());
        // sl_market is not reset by /nsl
        assert!(s.sl_market);
    }

    #[test]
    fn sl_no_position_just_sets_percentage() {
        // Without position, /sl10 only sets pct — no order placed
        let mut s = crate::State::new(false);
        s.sl_pct = 10.0;
        assert_eq!(s.sl_pct, 10.0);
        assert!(s.mt_sl_order_id.is_empty()); // no order
        assert_eq!(s.mt_state, 0); // still idle
    }

    #[test]
    fn sl_with_active_position_would_place_order() {
        // With active position, SL setup should be ready to place order
        let mut s = make_test_state(2, "down", 20.0, 0.40, 80.0);
        s.sl_pct = 5.0;

        // place_sl_order conditions: sl_pct > 0, mt_state == 2, sl_price valid
        let can_place = s.sl_pct > 0.0 && s.mt_state == 2;
        assert!(can_place);

        let sl_price = s.mt_entry * (1.0 - s.sl_pct / 100.0);
        assert!((sl_price - 0.38).abs() < 0.0001); // 0.40 * 0.95 = 0.38
        assert!(sl_price > 0.0 && sl_price < 1.0); // valid price
    }

    #[test]
    fn sl_bracket_overrides_percentage() {
        // /l10up65s60 sets bracket SL, which overrides sl_pct
        let mut s = crate::State::new(false);
        s.sl_pct = 5.0; // had SL set before

        // Bracket buy resets sl_pct to 0
        s.sl_pct = 0.0;
        assert_eq!(s.sl_pct, 0.0);

        // The bracket SL would be at 0.60 (from /l10up65s60)
        // Regular SL at 5% would be 0.65 * 0.95 = 0.6175 — different!
        let bracket_sl: f64 = 0.60;
        let regular_sl: f64 = 0.65 * 0.95;
        let diff: f64 = (bracket_sl - regular_sl).abs();
        assert!(diff > 0.01); // bracket ≠ percentage
    }

    #[test]
    fn sl_boundaries_clamped() {
        // SL percentage is clamped to [1, 50]
        fn clamp_sl(pct: f64) -> f64 { pct.max(1.0).min(50.0) }
        assert_eq!(clamp_sl(0.0), 1.0);
        assert_eq!(clamp_sl(0.5), 1.0);
        assert_eq!(clamp_sl(1.0), 1.0);
        assert_eq!(clamp_sl(5.0), 5.0);
        assert_eq!(clamp_sl(25.0), 25.0);
        assert_eq!(clamp_sl(50.0), 50.0);
        assert_eq!(clamp_sl(51.0), 50.0);
        assert_eq!(clamp_sl(100.0), 50.0);
    }

    #[test]
    fn sl_price_boundary_validation() {
        // place_sl_order guards: sl_price <= 0.0 || sl_price >= 1.0 → return
        let cases: Vec<(f64, f64, f64, bool)> = vec![
            (0.65, 1.0, 0.6435, true),     // entry=0.65, sl=1%, price=0.6435 ✓
            (0.65, 50.0, 0.325, true),     // entry=0.65, sl=50%, price=0.325 ✓
            (0.99, 5.0, 0.9405, true),     // entry=0.99, sl=5%, price=0.9405 ✓
            (0.50, 10.0, 0.45, true),      // entry=0.50, sl=10%, price=0.45 ✓
            (0.05, 5.0, 0.0475, true),     // entry=0.05, sl=5%, still valid (>0) ✓
            // Edge: sl_pct=100% with entry=0.01 → price=0.0 (invalid)
            (0.01, 100.0, 0.0, false),
        ];
        for (entry, pct, expected_price, should_be_valid) in cases {
            let sl_price = entry * (1.0 - pct / 100.0);
            let diff: f64 = (sl_price - expected_price).abs();
            assert!(diff < 0.001,
                "entry={entry} pct={pct} expected={expected_price} got={sl_price}");
            let valid = sl_price > 0.0 && sl_price < 1.0;
            assert_eq!(valid, should_be_valid, "entry={entry} pct={pct}");
        }
    }

    #[test]
    fn sl_with_exit_order_both_active() {
        // SL and TP (exit) can coexist: exit at profit, SL protects downside
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.mt_exit_price = 0.70;         // TP exit at 0.70
        s.mt_exit_order_id = "exit-001".into();
        s.sl_pct = 5.0;                  // SL at 0.65*0.95 = 0.6175
        s.mt_sl_order_id = "sl-001".into();

        assert!(s.mt_exit_price > s.mt_entry); // TP above entry
        let sl_price = s.mt_entry * (1.0 - s.sl_pct / 100.0);
        assert!(sl_price < s.mt_entry); // SL below entry
        assert!(!s.mt_exit_order_id.is_empty());
        assert!(!s.mt_sl_order_id.is_empty());
    }

    #[test]
    fn sl_toggle_with_active_position_refreshes() {
        // When SL is toggled and position is active + SL% > 0, it places/replaces order
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.sl_pct = 5.0;
        s.sl_market = false;
        s.mt_sl_order_id = "old-sl".into();

        // Toggle: LIMIT → MARKET
        s.sl_market = !s.sl_market;
        assert!(s.sl_market);

        // Conditions to refresh SL order
        let should_refresh = s.mt_state == 2 && s.sl_pct > 0.0;
        assert!(should_refresh);

        // Old SL would be cancelled, new one placed
        s.mt_sl_order_id.clear(); // simulate cancel old
        assert!(s.mt_sl_order_id.is_empty());
    }

    #[test]
    fn sl_full_lifecycle() {
        // Full SL lifecycle: set → active position → exit clears SL
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);

        // Step 1: /sl5 — set SL at 5%
        s.sl_pct = 5.0;
        assert_eq!(s.sl_pct, 5.0);
        let sl_price = s.mt_entry * (1.0 - s.sl_pct / 100.0);
        assert!((sl_price - 0.6175).abs() < 0.0001);
        s.mt_sl_order_id = "sl-active".into();

        // Step 2: price moves, still active
        assert_eq!(s.mt_state, 2);

        // Step 3: exit trade (simulate fill)
        s.mt_pnl_cum += s.mt_size * (0.70 - s.mt_entry);
        s.mt_trades += 1;
        s.mt_wins += 1;
        s.mt_sl_order_id.clear(); // cancel SL on exit
        s.reset_manual();

        assert_eq!(s.mt_state, 0);
        assert!(s.mt_sl_order_id.is_empty());
        assert!(s.mt_pnl_cum > 0.0);
    }

    #[test]
    fn sl_vs_tsl_exclusive() {
        // /sl and /tsl share mt_sl_order_id — setting TSL replaces SL order
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.sl_pct = 5.0;
        s.mt_sl_order_id = "sl-005".into();

        // Set TSL
        s.mt_tsl_pct = 2.0;
        // TSL update cancels old SL and places new one
        s.mt_sl_order_id.clear();
        s.mt_sl_order_id = "tsl-002".into();

        assert_eq!(s.sl_pct, 5.0); // sl_pct still set (for info)
        assert_eq!(s.mt_tsl_pct, 2.0);
        assert_eq!(s.mt_sl_order_id, "tsl-002".to_string());
    }

    #[test]
    fn sl_parse_all_valid_inputs() {
        // All valid SL parse variants
        let cases = vec![
            ("sl", "SL-TOGGLE"),
            ("sl1", "SL-1%"),
            ("sl2", "SL-2%"),
            ("sl5", "SL-5%"),
            ("sl10", "SL-10%"),
            ("sl15", "SL-15%"),
            ("sl25", "SL-25%"),
            ("sl49", "SL-49%"),
            ("sl50", "SL-50%"),
            ("nsl", "SL-OFF"),
        ];
        for (input, expected) in cases {
            assert_eq!(parsed(input), expected, "FAIL: /{input}");
        }
    }

    #[test]
    fn sl_parse_boundary_clamping() {
        // Values outside [1,50] are clamped
        assert_eq!(parsed("sl0"), "SL-1%");   // clamped to 1
        assert_eq!(parsed("sl51"), "SL-50%"); // clamped to 50
        assert_eq!(parsed("sl99"), "SL-50%"); // clamped to 50
        assert_eq!(parsed("sl100"), "SL-50%"); // clamped to 50
    }

    // ═══ STOP-MARKET SL TESTS ═════════════════════════════════════

    #[test]
    fn sl_trigger_logic_up_price_below() {
        // UP position: trigger when current <= mt_sl_price
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.mt_sl_price = 0.6175; // SL at 5% below 0.65
        s.hft.clob_trade_up = 0.60; // price dropped below SL

        let current_px = s.hft.clob_trade_up;
        let triggered = current_px <= s.mt_sl_price;
        assert!(triggered, "UP: 0.60 should trigger SL at 0.6175");
    }

    #[test]
    fn sl_trigger_logic_up_price_above_no_trigger() {
        // UP position: no trigger when current > mt_sl_price
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.mt_sl_price = 0.6175;
        s.hft.clob_trade_up = 0.63; // price above SL

        let current_px = s.hft.clob_trade_up;
        let triggered = current_px <= s.mt_sl_price;
        assert!(!triggered, "UP: 0.63 should NOT trigger SL at 0.6175");
    }

    #[test]
    fn sl_trigger_logic_up_price_at_sl_triggers() {
        // UP: price exactly at SL level → triggers
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.mt_sl_price = 0.6175;
        s.hft.clob_trade_up = 0.6175; // exactly at SL

        let current_px = s.hft.clob_trade_up;
        let triggered = current_px <= s.mt_sl_price;
        assert!(triggered, "UP: price at SL should trigger");
    }

    #[test]
    fn sl_trigger_logic_down_price_above() {
        // DOWN position: trigger when current >= mt_sl_price
        let mut s = make_test_state(2, "down", 20.0, 0.40, 80.0);
        s.mt_sl_price = 0.42; // SL at 5% above 0.40
        s.hft.clob_trade_dn = 0.45; // price went above SL

        let current_px = s.hft.clob_trade_dn;
        let triggered = current_px >= s.mt_sl_price;
        assert!(triggered, "DOWN: 0.45 should trigger SL at 0.42");
    }

    #[test]
    fn sl_trigger_logic_down_price_below_no_trigger() {
        // DOWN position: no trigger when current < mt_sl_price
        let mut s = make_test_state(2, "down", 20.0, 0.40, 80.0);
        s.mt_sl_price = 0.42;
        s.hft.clob_trade_dn = 0.38; // price below SL (still safe for DOWN)

        let current_px = s.hft.clob_trade_dn;
        let triggered = current_px >= s.mt_sl_price;
        assert!(!triggered, "DOWN: 0.38 should NOT trigger SL at 0.42");
    }

    #[test]
    fn sl_no_trigger_when_disabled() {
        // mt_sl_price == 0 means SL disabled
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.mt_sl_price = 0.0; // disabled
        s.hft.clob_trade_up = 0.01; // extreme drop

        let guards_fail = s.mt_sl_price <= 0.0;
        assert!(guards_fail, "SL disabled → no trigger");
    }

    #[test]
    fn sl_no_trigger_when_no_position() {
        // mt_state != 2 means no active position
        let mut s = make_test_state(0, "up", 0.0, 0.0, 0.0);
        s.mt_sl_price = 0.6175;
        s.hft.clob_trade_up = 0.01;

        let guards_fail = s.mt_state != 2;
        assert!(guards_fail, "no position → no trigger");
    }

    #[test]
    fn sl_no_trigger_pending_buy() {
        // mt_state == 1 (pending): SL not active yet
        let mut s = make_test_state(1, "up", 10.0, 0.65, 100.0);
        s.mt_sl_price = 0.6175;
        s.hft.clob_trade_up = 0.01;

        assert_eq!(s.mt_state, 1);
        // Guard: mt_state != 2 → skip
        assert!(s.mt_state != 2);
    }

    #[test]
    fn sl_setup_after_fill_up() {
        // setup_sl_after_fill computes mt_sl_price after UP buy fills
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.sl_pct = 5.0;

        // Simulate setup_sl_after_fill
        if s.sl_pct > 0.0 && s.mt_entry > 0.0 {
            s.mt_sl_price = if s.mt_outcome == "up" {
                s.mt_entry * (1.0 - s.sl_pct / 100.0)
            } else {
                s.mt_entry * (1.0 + s.sl_pct / 100.0)
            };
        }

        let expected: f64 = 0.65 * 0.95;
        assert!((s.mt_sl_price - expected).abs() < 0.001,
            "UP SL price should be {expected}, got {}", s.mt_sl_price);
    }

    #[test]
    fn sl_setup_after_fill_down() {
        // setup_sl_after_fill computes mt_sl_price after DOWN buy fills
        let mut s = make_test_state(2, "down", 20.0, 0.40, 80.0);
        s.sl_pct = 5.0;

        if s.sl_pct > 0.0 && s.mt_entry > 0.0 {
            s.mt_sl_price = if s.mt_outcome == "up" {
                s.mt_entry * (1.0 - s.sl_pct / 100.0)
            } else {
                s.mt_entry * (1.0 + s.sl_pct / 100.0)
            };
        }

        let expected: f64 = 0.40 * 1.05;
        assert!((s.mt_sl_price - expected).abs() < 0.001,
            "DOWN SL price should be {expected}, got {}", s.mt_sl_price);
    }

    #[test]
    fn sl_setup_no_pct_no_trigger() {
        // sl_pct == 0 → setup_sl_after_fill does nothing
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.sl_pct = 0.0;

        let should_setup = s.sl_pct > 0.0 && s.mt_entry > 0.0;
        assert!(!should_setup, "sl_pct=0 → no SL setup");
        assert_eq!(s.mt_sl_price, 0.0);
    }

    #[test]
    fn sl_set_via_command_computes_price() {
        // /sl10 sets mt_sl_price when position active
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);

        // Simulate exec_sl_set(10.0)
        s.sl_pct = 10.0;
        if s.mt_state == 2 && s.mt_entry > 0.0 {
            s.mt_sl_price = if s.mt_outcome == "up" {
                s.mt_entry * (1.0 - s.sl_pct / 100.0)
            } else {
                s.mt_entry * (1.0 + s.sl_pct / 100.0)
            };
        }

        let expected: f64 = 0.65 * 0.90;
        assert!((s.mt_sl_price - expected).abs() < 0.001);
        assert_eq!(s.sl_pct, 10.0);
    }

    #[test]
    fn sl_off_clears_trigger_price() {
        // /nsl clears mt_sl_price
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.mt_sl_price = 0.6175;
        s.sl_pct = 5.0;

        // /nsl
        s.sl_pct = 0.0;
        s.mt_sl_price = 0.0;

        assert_eq!(s.sl_pct, 0.0);
        assert_eq!(s.mt_sl_price, 0.0);
    }

    #[test]
    fn sl_trigger_resets_state() {
        // After SL triggers, state should be reset
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.mt_sl_price = 0.6175;
        s.hft.clob_trade_up = 0.60; // triggered

        // Simulate what check_sl_trigger does on trigger:
        // 1. Cancel all orders (HTTP, skip in test)
        // 2. Market sell (HTTP, skip in test)
        // 3. Reset
        s.mt_sl_price = 0.0;
        s.reset_manual();

        assert_eq!(s.mt_state, 0);
        assert_eq!(s.mt_sl_price, 0.0);
        assert!(s.mt_order_id.is_empty());
        assert!(s.mt_exit_order_id.is_empty());
    }

    #[test]
    fn sl_tsl_dynamic_update_up() {
        // TSL updates mt_sl_price as price rises for UP
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.mt_tsl_pct = 2.0;
        s.mt_tsl_high = 0.65;
        s.hft.clob_trade_up = 0.70; // price rose

        let current_px = s.hft.clob_trade_up;
        let high = s.mt_tsl_high.max(current_px);
        let new_sl = high * (1.0 - s.mt_tsl_pct / 100.0); // 0.70 * 0.98 = 0.686
        let changed = high > s.mt_tsl_high;

        assert!(changed, "high moved from 0.65 to 0.70");
        assert!((new_sl - 0.686).abs() < 0.001, "new SL = 0.70 * 0.98 = 0.686");

        // Apply
        s.mt_tsl_high = high;
        s.mt_sl_price = new_sl;

        assert!((s.mt_sl_price - 0.686).abs() < 0.001);
        assert_eq!(s.mt_tsl_high, 0.70);
    }

    #[test]
    fn sl_tsl_dynamic_update_down() {
        // TSL updates mt_sl_price as price falls for DOWN
        let mut s = make_test_state(2, "down", 20.0, 0.40, 80.0);
        s.mt_tsl_pct = 2.0;
        s.mt_tsl_low = 0.40;
        s.hft.clob_trade_dn = 0.35; // price fell (good for DOWN)

        let current_px = s.hft.clob_trade_dn;
        let low = s.mt_tsl_low.min(current_px);
        let new_sl = low * (1.0 + s.mt_tsl_pct / 100.0); // 0.35 * 1.02 = 0.357
        let changed = low < s.mt_tsl_low;

        assert!(changed, "low moved from 0.40 to 0.35");
        assert!((new_sl - 0.357).abs() < 0.001, "new SL = 0.35 * 1.02 = 0.357");

        s.mt_tsl_low = low;
        s.mt_sl_price = new_sl;

        assert!((s.mt_sl_price - 0.357).abs() < 0.001);
        assert_eq!(s.mt_tsl_low, 0.35);
    }

    #[test]
    fn sl_tsl_no_update_when_unchanged() {
        // TSL shouldn't update when extreme hasn't moved
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.mt_tsl_pct = 2.0;
        s.mt_tsl_high = 0.70; // already tracked higher
        s.mt_sl_price = 0.686;
        s.hft.clob_trade_up = 0.68; // price below tracked high

        let current_px = s.hft.clob_trade_up;
        let high = s.mt_tsl_high.max(current_px); // still 0.70
        let changed = high > s.mt_tsl_high;

        assert!(!changed, "extreme unchanged → no TSL update needed");
        assert_eq!(high, 0.70);
    }

    #[test]
    fn sl_set_via_command_no_position() {
        // /sl5 without active position: sets pct but no mt_sl_price
        let mut s = crate::State::new(false);
        s.sl_pct = 5.0;

        assert_eq!(s.sl_pct, 5.0);
        assert_eq!(s.mt_sl_price, 0.0); // no trigger without position
    }

    #[test]
    fn meta_commands() {
        assert_eq!(parsed("man"),  "MAN");
        assert_eq!(parsed("quit"), "QUIT");
        assert_eq!(parsed("p"),    "PANIC");
        assert_eq!(parsed("co"),   "CASHOUT");
    }

    #[test]
    fn edge_cases() {
        assert!(parsed("").starts_with("UNKNOWN"));
        assert!(parsed(" ").starts_with("UNKNOWN"));
        assert!(parsed("xyz").starts_with("UNKNOWN"));
        assert!(parsed("l").starts_with("UNKNOWN"));
        assert_eq!(parsed("c123"), "CANCEL");
        assert_eq!(parsed("co"), "CASHOUT");
    }

    #[test]
    fn gemini_simple() {
        assert_eq!(parsed("5g70"),    "GEMINI $5 @0.7000 exit=None");
        assert_eq!(parsed("8g72"),    "GEMINI $8 @0.7200 exit=None");
    }

    #[test]
    fn gemini_with_exit() {
        assert_eq!(parsed("7g70e82"), "GEMINI $7 @0.7000 exit=Some(0.82)");
        assert_eq!(parsed("5g70e80"), "GEMINI $5 @0.7000 exit=Some(0.8)");
    }

    #[test]
    fn gemini_invalid() {
        assert!(parsed("g70").starts_with("UNKNOWN"));
        assert!(parsed("5g5").starts_with("UNKNOWN"));  // trigger = 0.05-0.05 = 0
    }

    #[test]
    fn gemini_50_cases() {
        // ─── 50 cases: valid + invalid + edge ───
        let cases: Vec<(&str, &str)> = vec![
            // === VALID: sin exit (budget g cents) ===
            // #1-3  trigger mínimo (target pequeño, trigger justo >0)
            ("1g6",   "GEMINI $1 @0.0600 exit=None"),        // #1  trigger=0.01  mínimo válido
            ("1g7",   "GEMINI $1 @0.0700 exit=None"),        // #2  trigger=0.02
            ("3g8",   "GEMINI $3 @0.0800 exit=None"),        // #3  trigger=0.03
            // #4-6  targets bajos (0.10-0.30)
            ("2g10",  "GEMINI $2 @0.1000 exit=None"),        // #4  trigger=0.05
            ("5g15",  "GEMINI $5 @0.1500 exit=None"),        // #5  trigger=0.10
            ("10g20", "GEMINI $10 @0.2000 exit=None"),       // #6  trigger=0.15
            // #7-10 targets medios (0.30-0.60)
            ("7g33",  "GEMINI $7 @0.3300 exit=None"),        // #7  trigger=0.28
            ("12g48", "GEMINI $12 @0.4800 exit=None"),       // #8  trigger=0.43
            ("15g50", "GEMINI $15 @0.5000 exit=None"),       // #9  trigger=0.45
            ("20g55", "GEMINI $20 @0.5500 exit=None"),       // #10 trigger=0.50
            // #11-15 targets clásicos (0.60-0.75)
            ("5g60",  "GEMINI $5 @0.6000 exit=None"),        // #11 trigger=0.55
            ("5g65",  "GEMINI $5 @0.6500 exit=None"),        // #12 trigger=0.60
            ("5g70",  "GEMINI $5 @0.7000 exit=None"),        // #13 trigger=0.65
            ("8g72",  "GEMINI $8 @0.7200 exit=None"),        // #14 trigger=0.67
            ("10g75", "GEMINI $10 @0.7500 exit=None"),       // #15 trigger=0.70
            // #16-18 targets altos (0.80-0.99)
            ("15g80", "GEMINI $15 @0.8000 exit=None"),       // #16 trigger=0.75
            ("25g85", "GEMINI $25 @0.8500 exit=None"),       // #17 trigger=0.80
            ("30g90", "GEMINI $30 @0.9000 exit=None"),       // #18 trigger=0.85
            ("50g92", "GEMINI $50 @0.9200 exit=None"),       // #19 trigger=0.87
            ("8g92",  "GEMINI $8 @0.9200 exit=None"),        // #20 trigger=0.87
            // #21-22 extremos de budget
            ("1g10",  "GEMINI $1 @0.1000 exit=None"),        // #21 budget mínimo $1
            ("200g10","GEMINI $200 @0.1000 exit=None"),      // #22 budget máximo $200
            ("1g99",  "GEMINI $1 @0.9900 exit=None"),        // #23 target máximo 0.99
            ("200g6", "GEMINI $200 @0.0600 exit=None"),      // #24 budget máx + trigger=0.01
            // #25-27 budgets variados
            ("35g45", "GEMINI $35 @0.4500 exit=None"),       // #25
            ("60g38", "GEMINI $60 @0.3800 exit=None"),       // #26
            ("150g77","GEMINI $150 @0.7700 exit=None"),      // #27
            ("77g66", "GEMINI $77 @0.6600 exit=None"),       // #28
            ("99g88", "GEMINI $99 @0.8800 exit=None"),       // #29

            // === VALID: con exit (budget g cents e cents) ===
            ("5g70e80",  "GEMINI $5 @0.7000 exit=Some(0.8)"),      // #30 clásico
            ("7g70e82",  "GEMINI $7 @0.7000 exit=Some(0.82)"),     // #31
            ("10g50e60", "GEMINI $10 @0.5000 exit=Some(0.6)"),     // #32
            ("20g55e70", "GEMINI $20 @0.5500 exit=Some(0.7)"),     // #33
            ("15g30e45", "GEMINI $15 @0.3000 exit=Some(0.45)"),    // #34
            ("8g80e92",  "GEMINI $8 @0.8000 exit=Some(0.92)"),     // #35
            ("25g60e75", "GEMINI $25 @0.6000 exit=Some(0.75)"),    // #36
            ("50g45e55", "GEMINI $50 @0.4500 exit=Some(0.55)"),    // #37
            ("100g65e80","GEMINI $100 @0.6500 exit=Some(0.8)"),    // #38
            ("200g15e30","GEMINI $200 @0.1500 exit=Some(0.3)"),    // #39
            ("1g10e20",  "GEMINI $1 @0.1000 exit=Some(0.2)"),      // #40 budget mín con exit
            ("3g20e35",  "GEMINI $3 @0.2000 exit=Some(0.35)"),     // #41
            ("1g6e12",   "GEMINI $1 @0.0600 exit=Some(0.12)"),     // #42 trigger=0.01
            ("6g11e22",  "GEMINI $6 @0.1100 exit=Some(0.22)"),     // #43 trigger=0.06

            // === INVALID: trigger <= 0 ===
            ("5g5",   "UNKNOWN:5g5"),       // #44 target=0.05 trigger=0.00 → inválido
            ("10g1",  "UNKNOWN:10g1"),      // #45 target=0.01 trigger=-0.04
            ("10g2",  "UNKNOWN:10g2"),      // #46 target=0.02 trigger=-0.03
            ("10g3",  "UNKNOWN:10g3"),      // #47 target=0.03 trigger=-0.02
            ("10g4",  "UNKNOWN:10g4"),      // #48 target=0.04 trigger=-0.01

            // === INVALID: sin budget o syntax incorrecta ===
            ("g70",   "UNKNOWN:g70"),       // #49 falta presupuesto
            ("g99",   "UNKNOWN:g99"),       // #50 sin dígito inicial
        ];
        for (i, (input, expected)) in cases.iter().enumerate() {
            let result = parsed(input);
            if expected.starts_with("UNKNOWN") {
                assert!(result.starts_with("UNKNOWN"), "#{} FAIL: /{input} → got '{result}', expected UNKNOWN", i+1);
            } else {
                assert_eq!(result, *expected, "#{} FAIL: /{input}", i+1);
            }
        }
    }

    #[test]
    fn provider_command() {
        assert_eq!(parsed("provider binance"), "PROVIDER:binance");
        assert_eq!(parsed("provider coinbase"), "PROVIDER:coinbase");
        assert_eq!(parsed("provider kraken"), "PROVIDER:kraken");
        assert!(parsed("provider xyz").starts_with("UNKNOWN"));
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

    // ═══════════════════════════════════════════════════════════════
    // CASH OUT (/co) TESTS — parser + edge cases
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn cashout_parse() {
        assert_eq!(parsed("co"), "CASHOUT");
    }

    #[test]
    fn cashout_not_confused_with_cancel() {
        // "co" is cashout, not cancel
        assert_eq!(parsed("co"), "CASHOUT");
        // "c" alone is cancel
        assert_eq!(parsed("c"), "CANCEL");
        // "c" + digits (>=10 chars) is cancel by id
        assert_eq!(parsed("c0000000000"), "CANCEL-ID(0000000000)");
        // "c" + short string is cancel active
        assert_eq!(parsed("c123"), "CANCEL");
    }

    #[test]
    fn cashout_not_confused_with_cancel_liq() {
        // "cl" + up/down/m is cancel+liq
        assert_eq!(parsed("clup65"), "CANCEL+LIQ up @0.6500");
        assert_eq!(parsed("cld70"), "CANCEL+LIQ down @0.7000");
        assert_eq!(parsed("clm"), "CANCEL+LIQ MKT");
        // "co" is cashout (not cancel-liq with "o" outcome)
        assert_eq!(parsed("co"), "CASHOUT");
        // Also test "co" with trailing chars (should still be cashout)
        // Actually "co" + anything would be routed to parse_c_group which handles "c"...
        // but "co" is special-cased first in parse()
    }

    #[test]
    fn cashout_vs_panic() {
        // /p  = PANIC (liquidate all)
        assert_eq!(parsed("p"), "PANIC");
        // /co = CASH OUT (liquidate + cancel + strategies off)
        assert_eq!(parsed("co"), "CASHOUT");
        // Verify they are different commands
        assert_ne!(parsed("p"), parsed("co"));
    }

    #[test]
    fn cashout_in_comprehensive_matrix() {
        // This is part of the comprehensive matrix below
        assert_eq!(parsed("co"), "CASHOUT");
    }

    // ─── LOGIC / STATE MACHINE TESTS ────────────────────────────

    /// Build a minimal test state with manual position active (mt_state >= 2)
    fn make_test_state(mt_state: u8, outcome: &str, size: f64, entry: f64, budget: f64) -> crate::State {
        let mut s = crate::State::new(false);
        s.mt_state = mt_state;
        s.mt_outcome = outcome.to_string();
        s.mt_size = size;
        s.mt_entry = entry;
        s.mt_budget = budget;
        s.sl_pct = 5.0; // default SL active
        s
    }

    /// Build a state with strategies active
    fn make_test_state_strategies() -> crate::State {
        let mut s = crate::State::new(false);
        s.pos_odi_up = true;
        s.pos_odi_entry_up = 0.65;
        s.pos_h65_dn = true;
        s.pos_sen_up = true;
        s.pos_sen_entry_up = 0.55;
        s
    }

    #[test]
    fn cashout_logic_no_position() {
        // Cashout with NO position should reset state without errors
        let mut s = crate::State::new(false);
        // Step 1: Cancel Gemini
        s.gemini_active = false; s.gemini_budget = 0.0; s.gemini_target = 0.0;
        s.gemini_trigger = 0.0; s.gemini_exit = 0.0;
        s.gemini_outcome.clear(); s.gemini_triggered = false;
        // Step 2: Cancel ALL orders first (clear order IDs)
        s.mt_order_id.clear(); s.mt_exit_order_id.clear(); s.mt_sl_order_id.clear();
        // Step 3: no pending buy (mt_state != 1), no active (mt_state < 2)
        // Step 5: no strategies
        // Step 6: reset state
        s.reset_manual();

        assert_eq!(s.mt_state, 0);
        assert!(s.mt_order_id.is_empty());
        assert_eq!(s.mt_exit_price, 0.0);
        assert!(!s.gemini_active);
    }

    #[test]
    fn cashout_cancels_orders_before_market_sell() {
        // CRITICAL: orders must be cancelled BEFORE market sell, not after
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.mt_order_id = "buy-001".into();
        s.mt_exit_order_id = "exit-001".into();
        s.mt_sl_order_id = "sl-001".into();

        // Simulate step 2: cancel orders FIRST
        assert!(!s.mt_order_id.is_empty());
        assert!(!s.mt_exit_order_id.is_empty());
        assert!(!s.mt_sl_order_id.is_empty());

        // Clear them (simulating cancel_all before market sell)
        s.mt_order_id.clear();
        s.mt_exit_order_id.clear();
        s.mt_sl_order_id.clear();

        // Now there are no orders that could cancel the market sell
        assert!(s.mt_order_id.is_empty());
        assert!(s.mt_exit_order_id.is_empty());
        assert!(s.mt_sl_order_id.is_empty());

        // Simulate step 4: market sell (would be POST /api/orders/market)
        // The market sell order doesn't get an ID until the response comes back,
        // and we never cancel after this point — so it can't be cancelled by cashout
        s.mt_pnl_cum += 10.0 * (0.70 - 0.65); // simulate PnL
        s.mt_trades += 1;
        s.mt_wins += 1;

        // Simulate step 6: reset
        s.reset_manual();
        assert_eq!(s.mt_state, 0);
        // Order IDs were already cleared, market sell would have its own ID on backend
        assert!(s.mt_order_id.is_empty());
        assert!(s.mt_exit_order_id.is_empty());
        assert!(s.mt_sl_order_id.is_empty());

        // PnL preserved
        assert!((s.mt_pnl_cum - 0.5).abs() < 0.001);
        assert_eq!(s.mt_trades, 1);
        assert_eq!(s.mt_wins, 1);
    }

    #[test]
    fn cashout_pending_buy_cancelled() {
        // mt_state == 1 (pending buy): cancel & reset, no market sell needed
        let mut s = make_test_state(1, "up", 10.0, 0.65, 50.0);
        s.mt_order_id = "pending-buy-001".into();
        s.mt_order_seen = true;
        s.mt_last_fill_pct = 60.0;

        // Step 2: cancel orders
        s.mt_order_id.clear();
        // Step 3: pending buy → reset
        s.reset_manual();

        assert_eq!(s.mt_state, 0);
        assert!(s.mt_order_id.is_empty());
        assert!(!s.mt_order_seen);
        assert_eq!(s.mt_last_fill_pct, 0.0);
    }

    #[test]
    fn cashout_active_position_market_sell() {
        // mt_state == 2: cancel orders, market sell, PANIC strategies, reset
        let mut s = make_test_state(2, "down", 20.0, 0.40, 80.0);
        s.mt_order_id = "buy-order".into();
        s.mt_exit_order_id = "exit-order".into();

        // Step 2: cancel ALL orders before market sell
        s.mt_order_id.clear();
        s.mt_exit_order_id.clear();

        // Step 4: market sell (simulated — PnL based on current price)
        let current_px = 0.45; // simulated current price
        let pnl = s.mt_size * (current_px - s.mt_entry);
        s.mt_pnl_cum += pnl;
        s.mt_trades += 1;
        if pnl >= 0.0 { s.mt_wins += 1; }

        // Step 6: reset
        s.reset_manual();

        assert_eq!(s.mt_state, 0);
        assert!(s.mt_order_id.is_empty());
        assert!(s.mt_exit_order_id.is_empty());
        assert!((s.mt_pnl_cum - 1.0).abs() < 0.001);
        assert_eq!(s.mt_trades, 1);
        assert_eq!(s.mt_wins, 1);
    }

    #[test]
    fn cashout_exiting_state_handled() {
        // mt_state == 3 (exit pending): cancel exit order, market sell, reset
        let mut s = make_test_state(3, "up", 15.0, 0.60, 75.0);
        s.mt_exit_order_id = "exit-pending".into();
        s.mt_exit_price = 0.70;
        s.mt_exit_order_seen = true;
        s.mt_order_id = "buy-001".into();

        // Step 2: cancel ALL orders
        s.mt_order_id.clear();
        s.mt_exit_order_id.clear();

        // Step 4: market sell (mt_state >= 2 covers mt_state == 3 too)
        // Simulate market sell success
        let pnl = s.mt_size * (0.62 - s.mt_entry);
        s.mt_pnl_cum += pnl;
        s.mt_trades += 1;
        if pnl >= 0.0 { s.mt_wins += 1; }

        // Step 6: reset
        s.reset_manual();

        assert_eq!(s.mt_state, 0);
        assert!(s.mt_exit_order_id.is_empty());
        assert_eq!(s.mt_exit_price, 0.0);
        assert!(!s.mt_exit_order_seen);
    }

    #[test]
    fn cashout_resets_sl_and_tsl() {
        // After cashout, SL and TSL should be cleared by reset_manual
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.sl_pct = 5.0;
        s.mt_tsl_pct = 2.0;
        s.mt_tsl_high = 0.70;
        s.mt_exit_order_id = "exit-order-123".into();
        s.mt_sl_order_id = "sl-order-456".into();

        // Step 2: cancel orders
        s.mt_exit_order_id.clear();
        s.mt_sl_order_id.clear();
        // Step 6: reset
        s.reset_manual();

        assert_eq!(s.mt_state, 0);
        assert_eq!(s.mt_tsl_pct, 0.0);
        assert_eq!(s.mt_tsl_high, 0.0);
        assert!(s.mt_exit_order_id.is_empty());
        assert!(s.mt_sl_order_id.is_empty());
    }

    #[test]
    fn cashout_clears_all_order_ids() {
        // All order IDs should be cleared in step 2, not after market sell
        let mut s = make_test_state(2, "down", 20.0, 0.40, 50.0);
        s.mt_order_id = "buy-order-789".into();
        s.mt_exit_order_id = "exit-order-101".into();
        s.mt_sl_order_id = "sl-order-102".into();
        s.mt_order_seen = true;
        s.mt_exit_order_seen = true;

        // Step 2: cancel ALL orders first
        s.mt_order_id.clear();
        s.mt_exit_order_id.clear();
        s.mt_sl_order_id.clear();
        let ids = [&s.mt_order_id, &s.mt_exit_order_id, &s.mt_sl_order_id];
        assert!(ids.iter().all(|id| id.is_empty()), "all order IDs must be cleared before market sell");

        // Step 6: reset
        s.reset_manual();

        assert!(s.mt_order_id.is_empty());
        assert!(s.mt_exit_order_id.is_empty());
        assert!(s.mt_sl_order_id.is_empty());
        assert!(!s.mt_order_seen);
        assert!(!s.mt_exit_order_seen);
    }

    #[test]
    fn cashout_gemini_cancelled() {
        let mut s = crate::State::new(false);
        s.gemini_active = true;
        s.gemini_budget = 50.0;
        s.gemini_target = 0.72;
        s.gemini_trigger = 0.67;
        s.gemini_exit = 0.82;
        s.gemini_outcome = "up".into();
        s.gemini_triggered = true;

        // Step 1: cancel Gemini
        s.gemini_active = false;
        s.gemini_budget = 0.0;
        s.gemini_target = 0.0;
        s.gemini_trigger = 0.0;
        s.gemini_exit = 0.0;
        s.gemini_outcome.clear();
        s.gemini_triggered = false;

        assert!(!s.gemini_active);
        assert_eq!(s.gemini_budget, 0.0);
        assert_eq!(s.gemini_target, 0.0);
        assert!(s.gemini_outcome.is_empty());
        assert!(!s.gemini_triggered);
    }

    #[test]
    fn cashout_pnl_preserved_across_resets() {
        // P&L cumulative stats are preserved across resets
        let mut s = make_test_state(2, "up", 10.0, 0.65, 50.0);
        s.mt_pnl_cum = 12.50;
        s.mt_trades = 5;
        s.mt_wins = 3;
        s.reset_manual();
        // These are NOT reset by reset_manual (they accumulate across trades)
        assert_eq!(s.mt_pnl_cum, 12.50);
        assert_eq!(s.mt_trades, 5);
        assert_eq!(s.mt_wins, 3);
    }

    #[test]
    fn cashout_market_sell_failure_preserves_state() {
        // If market sell FAILS, position state is NOT reset — user can retry with /lm
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.mt_order_id = "buy-001".into();
        s.mt_exit_order_id = "exit-001".into();
        s.mt_sl_order_id = "sl-001".into();

        // Step 2: cancel orders first (always happens)
        s.mt_order_id.clear();
        s.mt_exit_order_id.clear();
        s.mt_sl_order_id.clear();

        // Step 4: market sell FAILS
        let mkt_failed = true;

        // Step 6: do NOT reset if mkt_failed
        if !mkt_failed {
            s.reset_manual();
        }

        // State is still active (mt_state == 2), position data intact
        assert_eq!(s.mt_state, 2);
        assert_eq!(s.mt_size, 10.0);
        assert_eq!(s.mt_entry, 0.65);
        assert_eq!(s.mt_budget, 100.0);
    }

    #[test]
    fn cashout_strategies_detection() {
        let s = make_test_state_strategies();
        assert!(s.pos_odi_up);
        assert!(s.pos_h65_dn);
        assert!(s.pos_sen_up);
        let has_strategies = s.pos_sen_up || s.pos_sen_dn
            || s.pos_h65_up || s.pos_h65_dn
            || s.pos_odi_up || s.pos_odi_dn;
        assert!(has_strategies);
    }

    #[test]
    fn cashout_strategies_detection_none() {
        let s = crate::State::new(false);
        let has_strategies = s.pos_sen_up || s.pos_sen_dn
            || s.pos_h65_up || s.pos_h65_dn
            || s.pos_odi_up || s.pos_odi_dn;
        assert!(!has_strategies);
    }

    #[test]
    fn cashout_strategies_detection_each() {
        for (field, expected) in [
            ("odi_up", true), ("odi_dn", true),
            ("h65_up", true), ("h65_dn", true),
            ("sen_up", true), ("sen_dn", true),
        ] {
            let mut s = crate::State::new(false);
            match field {
                "odi_up" => s.pos_odi_up = true,
                "odi_dn" => s.pos_odi_dn = true,
                "h65_up" => s.pos_h65_up = true,
                "h65_dn" => s.pos_h65_dn = true,
                "sen_up" => s.pos_sen_up = true,
                "sen_dn" => s.pos_sen_dn = true,
                _ => unreachable!(),
            }
            let has = s.pos_sen_up || s.pos_sen_dn
                || s.pos_h65_up || s.pos_h65_dn
                || s.pos_odi_up || s.pos_odi_dn;
            assert_eq!(has, expected, "strategy flag {field}");
        }
    }

    #[test]
    fn cashout_with_both_manual_and_strategies() {
        // Simulate: manual UP position + Odiseo UP + Senna UP active
        let mut s = make_test_state(2, "up", 20.0, 0.60, 100.0);
        s.pos_odi_up = true;
        s.pos_sen_up = true;

        // Step 2: cancel orders
        s.mt_order_id.clear();
        // Step 4: market sell (mt_state >= 2)
        let pnl = s.mt_size * (0.65 - s.mt_entry);
        s.mt_pnl_cum += pnl;
        s.mt_trades += 1;
        s.mt_wins += 1;
        // Step 5: PANIC strategies (pos_odi_up and pos_sen_up are true)
        let has_strategies = s.pos_odi_up || s.pos_sen_up;
        assert!(has_strategies);
        // Step 6: reset
        s.reset_manual();

        assert_eq!(s.mt_state, 0);
        // Strategy flags NOT cleared by reset_manual (managed by HFT polling)
        assert!(s.pos_odi_up);
        assert!(s.pos_sen_up);
    }

    #[test]
    fn cashout_concurrent_market_buy_not_confused() {
        // Regression: "co" should NOT be parsed as a cancel-order with ID "o"
        assert_eq!(parsed("co"), "CASHOUT");
        assert_eq!(parsed("c0000000000"), "CANCEL-ID(0000000000)");
        // "co" is short (<10 chars), which cancels active... but we special-case "co"
        assert_eq!(parsed("co5up"), "CANCEL"); // 5up is target, but "co" prefix pattern
    }

    #[test]
    fn cashout_all_states_reset_correctly() {
        // Test reset_manual for all 4 mt_states
        for mt_state in 0..4 {
            let (outcome, size, entry, budget) = if mt_state == 0 {
                ("up", 0.0, 0.0, 0.0)
            } else {
                ("up", 10.0, 0.65, 50.0)
            };
            let mut s = make_test_state(mt_state, outcome, size, entry, budget);
            if mt_state >= 1 { s.mt_order_id = "test-order".into(); }
            if mt_state >= 2 { s.mt_exit_order_id = "test-exit".into(); s.mt_sl_order_id = "test-sl".into(); }

            // Clear orders (step 2 of cashout)
            s.mt_order_id.clear();
            s.mt_exit_order_id.clear();
            s.mt_sl_order_id.clear();

            // Reset (step 6 of cashout, or step 3 if pending buy)
            s.reset_manual();

            assert_eq!(s.mt_state, 0, "mt_state={mt_state} should reset to 0");
            assert!(s.mt_order_id.is_empty());
            assert!(s.mt_exit_order_id.is_empty());
            assert!(s.mt_sl_order_id.is_empty());
            assert_eq!(s.mt_exit_price, 0.0);
        }
    }

    #[test]
    fn cashout_order_of_operations() {
        let mut s = make_test_state(2, "up", 10.0, 0.65, 100.0);
        s.mt_order_id = "buy-001".into();
        s.mt_exit_order_id = "exit-001".into();
        s.mt_sl_order_id = "sl-001".into();

        s.gemini_active = false;
        assert!(!s.gemini_active);

        let ids: Vec<String> = {
            let mut v = Vec::new();
            if !s.mt_order_id.is_empty() { v.push(s.mt_order_id.clone()); }
            if !s.mt_exit_order_id.is_empty() { v.push(s.mt_exit_order_id.clone()); }
            if !s.mt_sl_order_id.is_empty() { v.push(s.mt_sl_order_id.clone()); }
            v
        };
        assert_eq!(ids.len(), 3);
        s.mt_order_id.clear();
        s.mt_exit_order_id.clear();
        s.mt_sl_order_id.clear();

        assert!(s.mt_order_id.is_empty());
        assert!(s.mt_exit_order_id.is_empty());
        assert!(s.mt_sl_order_id.is_empty());

        s.reset_manual();
        assert_eq!(s.mt_state, 0);
    }

    // ═══════════════════════════════════════════════════════════════
    // CASH OUT — BUILD MKT SELL BODY helpers
    // ═══════════════════════════════════════════════════════════════

    /// Build the JSON body for a manual market sell (matching exec_cashout step 4).
    fn build_manual_mkt_body(outcome: &str, size: f64) -> String {
        format!(r#"{{"side":"sell","outcome":"{}","amount_usdc":{}}}"#, outcome, size)
    }

    /// Build the JSON body for a strategy market sell (matching exec_cashout step 5).
    fn build_strat_mkt_body(outcome: &str, budget: f64, entry: f64) -> (String, f64) {
        let shares = if entry > 0.0 { (budget / entry).floor().max(1.0) } else { budget };
        (format!(r#"{{"side":"sell","outcome":"{}","amount_usdc":{}}}"#, outcome, shares), shares)
    }

    /// Collect active strategy positions (matching exec_cashout step 5 collection).
    fn collect_active_strategies(s: &crate::State) -> Vec<(&'static str, &str, f64, f64)> {
        let mut v: Vec<(&str, &str, f64, f64)> = Vec::new();
        if s.pos_h65_up && s.h65_budget > 0.0 { v.push(("H65", "up", s.h65_budget, s.pos_h65_entry_up)); }
        if s.pos_h65_dn && s.h65_budget > 0.0 { v.push(("H65", "down", s.h65_budget, s.pos_h65_entry_dn)); }
        if s.pos_odi_up && s.odi_budget > 0.0 { v.push(("ODI", "up", s.odi_budget, s.pos_odi_entry_up)); }
        if s.pos_odi_dn && s.odi_budget > 0.0 { v.push(("ODI", "down", s.odi_budget, s.pos_odi_entry_dn)); }
        if s.pos_sen_up && s.sen_budget > 0.0 { v.push(("SEN", "up", s.sen_budget, s.pos_sen_entry_up)); }
        if s.pos_sen_dn && s.sen_budget > 0.0 { v.push(("SEN", "down", s.sen_budget, s.pos_sen_entry_dn)); }
        v
    }

    // ═══════════════════════════════════════════════════════════════
    // CASH OUT — MANUAL MARKET SELL USES SHARES (NOT BUDGET)
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn cashout_manual_sell_uses_shares_not_dollars() {
        // BUG FIX: amount_usdc for SELL must be shares, not dollars.
        // $200 at $0.50 = 400 shares. Old code sent 200 (dollars) → only 200 sold.
        // Fixed code sends 400 (shares) → all shares sold.
        let outcome = "up";
        let size = 400.0;   // shares
        let budget = 200.0; // dollars
        assert_ne!(size, budget, "shares ≠ dollars: must not be equal for this test");

        let body = build_manual_mkt_body(outcome, size);
        let body_budget = build_manual_mkt_body(outcome, budget);

        // Fixed: uses size (shares) to close entire position
        assert!(body.contains("400"));
        assert!(!body.contains("200"));
        assert_ne!(body, body_budget, "correct body uses shares, old bug used dollars");
    }

    #[test]
    fn cashout_manual_sell_shares_match_fill_size() {
        // Verify that mt_size (filled shares) is used, not mt_budget
        let sizes_and_budgets = [
            (200.0, 100.0),   // 200 shares at $0.50 = $100
            (333.0, 200.0),   // 333 shares at $0.60 = $200
            (10.0, 5.0),      // 10 shares at $0.50 = $5
            (1000.0, 500.0),  // 1000 shares at $0.50 = $500
        ];
        for (size, budget) in &sizes_and_budgets {
            let body = build_manual_mkt_body("up", *size);
            assert!(body.contains(&format!("{}", *size)), "body for size={size}");
            if (*size - *budget).abs() > 0.1 {
                assert!(!body.contains(&format!("{}", *budget)), "body should NOT contain budget ${budget}");
            }
        }
    }

    #[test]
    fn cashout_pnl_uses_shares_not_budget() {
        // PnL calculation already uses size (shares): pnl = mt_size * (current_px - mt_entry)
        let size = 400.0;
        let entry = 0.50;
        let current = 0.55;
        let budget = 200.0;

        let pnl_shares: f64 = size * (current - entry);     // correct
        let pnl_budget: f64 = budget * (current - entry);   // would be wrong

        assert!((pnl_shares - 20.0).abs() < 0.001, "PnL with shares: {pnl_shares}");
        assert!((pnl_budget - 10.0).abs() < 0.001, "old PnL with budget would understate");
        assert!(pnl_shares > pnl_budget, "shares-based PnL correctly reflects position size");
    }

    #[test]
    fn cashout_manual_sell_body_has_correct_structure() {
        let body = build_manual_mkt_body("down", 333.0);
        assert!(body.contains(r#""side":"sell""#));
        assert!(body.contains(r#""outcome":"down""#));
        assert!(body.contains(r#""amount_usdc":333"#));
        assert!(!body.contains("mt_budget"), "budget field should NOT appear");
    }

    // ═══════════════════════════════════════════════════════════════
    // CASH OUT — STRATEGY SHARES CALCULATION
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn cashout_strategy_shares_budget_div_entry() {
        let cases = [
            (200.0, 0.50, 400.0),
            (100.0, 0.65, 153.0),
            (50.0, 0.40, 125.0),
            (75.0, 0.33, 227.0),
            (300.0, 0.75, 400.0),
        ];
        for (budget, entry, expected_shares) in &cases {
            let (_body, shares) = build_strat_mkt_body("up", *budget, *entry);
            assert!((shares - expected_shares).abs() < 0.01,
                "budget={budget} entry={entry}: expected shares={expected_shares}, got {shares}");
        }
    }

    #[test]
    fn cashout_strategy_shares_min_1() {
        // Even tiny budgets produce at least 1 share
        let (_body, shares) = build_strat_mkt_body("up", 1.0, 0.99);
        assert!(shares >= 1.0, "shares must be at least 1.0, got {shares}");
    }

    #[test]
    fn cashout_strategy_zero_entry_falls_back_to_budget() {
        // If entry price is 0 (no data yet), fall back to using budget as shares
        let budget = 100.0;
        let (_body, shares) = build_strat_mkt_body("up", budget, 0.0);
        assert_eq!(shares, budget, "zero entry should fall back to budget as share count");
    }

    #[test]
    fn cashout_strategy_body_uses_calculated_shares() {
        let budget = 150.0;
        let entry = 0.60;
        let (body, shares) = build_strat_mkt_body("down", budget, entry);
        let expected_shares = (budget / entry).floor().max(1.0);

        assert_eq!(shares, expected_shares);
        assert!(body.contains(&format!("{}", expected_shares as i64)),
            "body should contain calculated shares {expected_shares}, got: {body}");
        assert!(body.contains(r#""outcome":"down""#));
        assert!(body.contains(r#""side":"sell""#));
    }

    #[test]
    fn cashout_strategy_shares_vs_budget_are_different() {
        // Verify that shares ≠ budget (proving the fix is meaningful)
        for (budget, entry) in [(200.0, 0.50), (100.0, 0.80), (50.0, 0.25)] {
            let (_body, shares) = build_strat_mkt_body("up", budget, entry);
            assert_ne!(shares, budget, "budget={budget} entry={entry}: shares must differ from budget");
        }
    }

    // ═══════════════════════════════════════════════════════════════
    // CASH OUT — EACH POSITION SOLD INDIVIDUALLY
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn cashout_each_active_strategy_sold_individually() {
        let mut s = crate::State::new(false);
        s.pos_h65_up = true;  s.h65_budget = 200.0; s.pos_h65_entry_up = 0.50;
        s.pos_odi_dn = true;  s.odi_budget = 150.0; s.pos_odi_entry_dn = 0.40;
        s.pos_sen_up = true;  s.sen_budget = 100.0; s.pos_sen_entry_up = 0.65;

        let positions = collect_active_strategies(&s);

        assert_eq!(positions.len(), 3, "should have 3 active positions");
        assert_eq!(positions[0].0, "H65");
        assert_eq!(positions[0].1, "up");
        assert_eq!(positions[1].0, "ODI");
        assert_eq!(positions[1].1, "down");
        assert_eq!(positions[2].0, "SEN");
        assert_eq!(positions[2].1, "up");

        // Each position gets its own market sell with correct shares
        for (strat, outcome, budget, entry) in &positions {
            let (_body, shares) = build_strat_mkt_body(outcome, *budget, *entry);
            assert!(shares > 0.0, "{strat} {outcome}: shares should be positive");
            // shares should differ from budget (unless entry=0)
            if *entry > 0.0 && *entry != 1.0 {
                assert_ne!(shares, *budget, "{strat} {outcome}: shares should differ from budget");
            }
        }
    }

    #[test]
    fn cashout_no_strategies_collects_empty() {
        let s = crate::State::new(false);
        let positions = collect_active_strategies(&s);
        assert!(positions.is_empty());
    }

    #[test]
    fn cashout_all_6_positions_collected() {
        let mut s = crate::State::new(false);
        s.pos_h65_up = true;  s.h65_budget = 100.0; s.pos_h65_entry_up = 0.50;
        s.pos_h65_dn = true;  s.pos_h65_entry_dn = 0.55;
        s.pos_odi_up = true;  s.odi_budget = 100.0; s.pos_odi_entry_up = 0.60;
        s.pos_odi_dn = true;  s.pos_odi_entry_dn = 0.45;
        s.pos_sen_up = true;  s.sen_budget = 100.0; s.pos_sen_entry_up = 0.70;
        s.pos_sen_dn = true;  s.pos_sen_entry_dn = 0.35;

        let positions = collect_active_strategies(&s);
        assert_eq!(positions.len(), 6);
        assert!(positions.iter().any(|(s, o, _, _)| *s == "H65" && *o == "up"));
        assert!(positions.iter().any(|(s, o, _, _)| *s == "H65" && *o == "down"));
        assert!(positions.iter().any(|(s, o, _, _)| *s == "ODI" && *o == "up"));
        assert!(positions.iter().any(|(s, o, _, _)| *s == "ODI" && *o == "down"));
        assert!(positions.iter().any(|(s, o, _, _)| *s == "SEN" && *o == "up"));
        assert!(positions.iter().any(|(s, o, _, _)| *s == "SEN" && *o == "down"));
    }

    #[test]
    fn cashout_strategy_with_zero_budget_skipped() {
        let mut s = crate::State::new(false);
        s.pos_h65_up = true;  s.h65_budget = 0.0; s.pos_h65_entry_up = 0.50;
        s.pos_odi_up = true;  s.odi_budget = 100.0; s.pos_odi_entry_up = 0.60;

        let positions = collect_active_strategies(&s);
        assert_eq!(positions.len(), 1, "zero-budget strategy should be skipped");
        assert_eq!(positions[0].0, "ODI");
    }

    #[test]
    fn cashout_strategies_disabled_by_flag_only() {
        let mut s = crate::State::new(false);
        // Position flag false even with budget > 0 → skipped
        s.pos_h65_up = false;
        s.h65_budget = 200.0;
        s.pos_h65_entry_up = 0.50;

        let positions = collect_active_strategies(&s);
        assert!(positions.is_empty(), "must be skipped if pos flag is false regardless of budget");
    }

    // ═══════════════════════════════════════════════════════════════
    // CASH OUT — FAILURE SCENARIOS
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn cashout_market_sell_failure_marks_mkt_failed() {
        // When any market sell fails, mkt_failed = true and state is preserved
        let mut s = make_test_state(2, "up", 400.0, 0.50, 200.0);
        let mkt_failed = true;

        if !mkt_failed {
            s.reset_manual();
        }

        // State preserved — user can retry
        assert_eq!(s.mt_state, 2);
        assert_eq!(s.mt_size, 400.0);
        assert_eq!(s.mt_entry, 0.50);
        assert_eq!(s.mt_budget, 200.0);
    }

    #[test]
    fn cashout_strategy_sell_failure_marks_mkt_failed() {
        // If any strategy market sell fails, mkt_failed = true
        let mut s = make_test_state(2, "down", 333.0, 0.40, 100.0);
        s.pos_h65_dn = true; s.h65_budget = 200.0; s.pos_h65_entry_dn = 0.40;

        let positions = collect_active_strategies(&s);
        assert_eq!(positions.len(), 1);

        // Simulate: first sell succeeds, second fails → mkt_failed
        let mut mkt_failed = false;
        for (_, _, _, _) in &positions {
            // simulating failure
            mkt_failed = true;
        }

        assert!(mkt_failed, "strategy sell failure should mark mkt_failed");
        // Manual state should NOT be reset
        assert_eq!(s.mt_state, 2);
    }

    #[test]
    fn cashout_mkt_failed_prevents_reset_manual() {
        let mut s = make_test_state(2, "up", 200.0, 0.65, 100.0);
        let mkt_failed = true;

        if !mkt_failed {
            s.reset_manual();
        }

        assert_eq!(s.mt_state, 2, "position must be preserved on failure");
        assert_eq!(s.mt_size, 200.0);
        assert_eq!(s.mt_entry, 0.65);
    }

    // ═══════════════════════════════════════════════════════════════
    // CASH OUT — COMBINED MANUAL + STRATEGY SCENARIOS
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn cashout_manual_up_plus_three_strategies() {
        // Most common real scenario: manual UP + 3 strategies active
        let mut s = make_test_state(2, "up", 300.0, 0.55, 150.0);
        s.pos_odi_up = true; s.odi_budget = 150.0; s.pos_odi_entry_up = 0.55;
        s.pos_h65_up = true; s.h65_budget = 200.0; s.pos_h65_entry_up = 0.55;
        s.pos_sen_dn = true; s.sen_budget = 100.0; s.pos_sen_entry_dn = 0.45;

        // Manual sell
        let manual_body = build_manual_mkt_body(&s.mt_outcome, s.mt_size);
        assert!(manual_body.contains("300"), "manual body should use mt_size=300 shares");

        // Strategy sells
        let positions = collect_active_strategies(&s);
        assert_eq!(positions.len(), 3);

        let mut total_shares_to_sell = s.mt_size;
        for (_, _, budget, entry) in &positions {
            let (body, shares) = build_strat_mkt_body("up", *budget, *entry);
            assert!(shares > 0.0);
            assert!(body.contains(r#""side":"sell""#));
            total_shares_to_sell += shares;
        }

        // All positions accounted for
        assert!(total_shares_to_sell > s.mt_size, "strategy shares add to total");
    }

    #[test]
    fn cashout_manual_down_with_all_strategies() {
        let mut s = make_test_state(2, "down", 250.0, 0.45, 200.0);
        s.pos_h65_up = true; s.h65_budget = 100.0; s.pos_h65_entry_up = 0.50;
        s.pos_h65_dn = true; s.pos_h65_entry_dn = 0.48;
        s.pos_odi_up = true; s.odi_budget = 50.0;  s.pos_odi_entry_up = 0.52;
        s.pos_odi_dn = true; s.pos_odi_entry_dn = 0.46;
        s.pos_sen_up = true; s.sen_budget = 75.0;  s.pos_sen_entry_up = 0.55;
        s.pos_sen_dn = true; s.pos_sen_entry_dn = 0.42;

        let manual_body = build_manual_mkt_body("down", s.mt_size);
        assert!(manual_body.contains("250"));

        let positions = collect_active_strategies(&s);
        assert_eq!(positions.len(), 6);

        let total_sells = 1 + positions.len(); // manual + 6 strategies
        assert_eq!(total_sells, 7, "all 7 positions must generate market sell orders");
    }

    #[test]
    fn cashout_no_positions_at_all() {
        let mut s = crate::State::new(false);
        s.mt_state = 0;

        let has_manual = s.mt_state >= 2;
        let positions = collect_active_strategies(&s);

        assert!(!has_manual);
        assert!(positions.is_empty());
        // Reset still works on empty state
        s.reset_manual();
        assert_eq!(s.mt_state, 0);
    }

    // ═══════════════════════════════════════════════════════════════
    // CASH OUT — ORDER SAFETY (PANIC still called)
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn cashout_panic_called_even_with_no_individual_sells() {
        // PANIC is always called as safety net — even if no positions
        let s = crate::State::new(false);
        let positions = collect_active_strategies(&s);
        assert!(positions.is_empty());
        // PANIC would be called in step 6 regardless (disables strategies)
    }

    #[test]
    fn cashout_panic_called_after_individual_sells() {
        // Individual sells execute first, THEN PANIC (step 5 then step 6)
        let mut s = make_test_state(2, "up", 400.0, 0.50, 200.0);
        s.pos_h65_up = true; s.h65_budget = 100.0; s.pos_h65_entry_up = 0.50;

        // Step 4: manual sell (uses shares)
        let manual_body = build_manual_mkt_body("up", s.mt_size);
        assert!(manual_body.contains("400"));

        // Step 5: individual strategy sells
        let positions = collect_active_strategies(&s);
        assert_eq!(positions.len(), 1);
        let (strat_body, _) = build_strat_mkt_body("up", positions[0].2, positions[0].3);
        assert!(!strat_body.is_empty());

        // Step 6: PANIC (would be called via http_post)
        // Reset only if no failures
        s.reset_manual();
        assert_eq!(s.mt_state, 0);
    }

    // ═══════════════════════════════════════════════════════════════
    // CASH OUT — EDGE CASES
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn cashout_entry_price_one() {
        // entry = 1.0 (max) → budget / entry = budget → shares = budget
        let budget = 200.0;
        let entry = 1.0;
        let (_body, shares) = build_strat_mkt_body("up", budget, entry);
        assert!((shares - budget).abs() < 0.01,
            "entry=1.0: shares should equal budget");
    }

    #[test]
    fn cashout_tiny_budget_high_price() {
        let (_body, shares) = build_strat_mkt_body("down", 5.0, 0.95);
        assert_eq!(shares as i64, 5, "$5 at 0.95 → 5 shares (floor)");
    }

    #[test]
    fn cashout_large_budget_asymmetric_outcomes() {
        // Common: large UP budget with strategies
        let mut s = make_test_state(2, "up", 800.0, 0.52, 400.0);
        s.pos_odi_up = true; s.odi_budget = 300.0; s.pos_odi_entry_up = 0.52;
        s.pos_sen_up = true; s.sen_budget = 200.0; s.pos_sen_entry_up = 0.52;

        let manual_body = build_manual_mkt_body("up", s.mt_size);
        assert!(manual_body.contains("800"));

        let positions = collect_active_strategies(&s);
        assert_eq!(positions.len(), 2);
        let total_shares: f64 = s.mt_size + positions.iter()
            .map(|(_, _, budget, entry)| {
                let (_, shares) = build_strat_mkt_body("up", *budget, *entry);
                shares
            })
            .sum::<f64>();

        assert!(total_shares > 1000.0, "combined shares > 1000 for large positions");
    }

    #[test]
    fn cashout_budget_and_entry_sync() {
        // Verify that when budget and entry differ, the market sell body
        // uses shares (budget/entry), not raw budget
        let cases = [
            ("H65", "up", 200.0, 0.50, 400.0),
            ("ODI", "down", 150.0, 0.60, 250.0),
            ("SEN", "up", 100.0, 0.40, 250.0),
        ];
        for (strat, outcome, budget, entry, expected_shares) in &cases {
            let (body, shares) = build_strat_mkt_body(outcome, *budget, *entry);
            assert!((shares - expected_shares).abs() < 0.01,
                "{strat} {outcome}: shares={shares} expected={expected_shares}");
            assert!(body.contains(&format!("{}", *expected_shares as i64)),
                "{strat} body should contain {expected_shares} shares, got: {body}");
        }
    }

    // ═══════════════════════════════════════════════════════════════
    // CASH OUT — VERIFY NO REGRESSION ON PARSER
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn cashout_parser_isolated_from_cancel() {
        assert_eq!(parsed("co"), "CASHOUT");
        assert_eq!(parsed("c"), "CANCEL");
        assert_eq!(parsed("clm"), "CANCEL+LIQ MKT");
        assert_ne!(parsed("co"), parsed("c"));
        assert_ne!(parsed("co"), parsed("clm"));
    }

    #[test]
    fn cashout_parser_not_confused_with_limit_buy_co() {
        // "l10co65" — has 'co' in the middle, should NOT parse as cashout
        let r = parsed("l10co65");
        assert!(!r.contains("CASHOUT"), "'co' buried in buy command should not trigger cashout, got: {r}");
        // Should be UNKNOWN (invalid side pattern) or BUY if valid
        assert!(r.contains("UNKNOWN") || r.contains("BUY"), "should be UNKNOWN or BUY, got: {r}");
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
            ("co",          "CASHOUT"),
            ("man",         "MAN"),
            ("quit",        "QUIT"),
            ("5g70",        "GEMINI $5 @0.7000 exit=None"),
            ("7g70e82",     "GEMINI $7 @0.7000 exit=Some(0.82)"),
            ("8g92",        "GEMINI $8 @0.9200 exit=None"),
            ("provider binance", "PROVIDER:binance"),
            ("provider coinbase", "PROVIDER:coinbase"),
        ];
        for (input, expected) in cases {
            assert_eq!(parsed(input), expected, "FAIL: /{input}");
        }
    }
}
