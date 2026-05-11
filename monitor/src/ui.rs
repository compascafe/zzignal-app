use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::InputMode;
use crate::State;

const TAB_NAMES: &[&str] = &["DINERO REAL", "PAPER MONEY", "GRAFICOS"];

pub fn draw(f: &mut Frame, s: &State) {
    let area = f.area();

    let pos_h = 1;
    let cmd_h = if s.input_mode == InputMode::Command { 3 } else { 0 };

    let mut constraints = vec![
        Constraint::Length(1),     // commit bar
        Constraint::Length(1),     // tab bar
        Constraint::Length(pos_h), // position bar
        Constraint::Min(1),        // dashboard
    ];
    if cmd_h > 0 { constraints.push(Constraint::Length(cmd_h)); }
    constraints.push(Constraint::Length(2)); // footer

    let chunks = Layout::default().direction(Direction::Vertical).constraints(constraints).split(area);
    let mut ci = 0;

    // ─── COMMIT BAR ────────────────────────────────────────────────────
    let commit = option_env!("GIT_HASH").unwrap_or("dev");
    f.render_widget(
        Paragraph::new(format!("DAVID@ZZIGNAL || on the other side  {}", commit))
            .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        chunks[ci],
    );
    ci += 1;

    // ─── TAB BAR ──────────────────────────────────────────────────────
    let tab_spans: Vec<Span> = TAB_NAMES.iter().enumerate().flat_map(|(i, name)| {
        let (fg, bg) = if i == s.tab {
            match i {
                0 => (Color::White, Color::Red),
                1 => (Color::White, Color::Green),
                _ => (Color::White, Color::Blue),
            }
        } else {
            (Color::Gray, Color::Reset)
        };
        vec![
            Span::styled(" ", Style::default()),
            Span::styled(*name, Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD)),
        ]
    }).collect();
    f.render_widget(
        Paragraph::new(Line::from(tab_spans)),
        chunks[ci],
    );
    ci += 1;

    // ─── POSITION BAR ─────────────────────────────────────────────────
    draw_position_bar(f, chunks[ci], s); ci += 1;

    // ─── MAIN ─────────────────────────────────────────────────────────
    if s.tab == 2 {
        draw_graficos(f, chunks[ci], s);
    } else {
        draw_dashboard(f, chunks[ci], s);
    }
    ci += 1;

    // ─── COMMAND BAR (modal) ────────────────────────────────────
    if cmd_h > 0 {
        draw_command_bar(f, chunks[ci], s);
        ci += 1;
    }

    // ─── FOOTER ───────────────────────────────────────────────────────
    draw_footer(f, chunks[ci], s);
}

// ═══════════════════════════════════════════════════════════════════
// TAB 0/1: DASHBOARD — Polymarket style
// ═══════════════════════════════════════════════════════════════════

fn draw_dashboard(f: &mut Frame, area: Rect, s: &State) {
    let constraints = vec![
        Constraint::Length(3),     // market info (BTC, open, countdown, balance)
        Constraint::Length(3),     // UP/DOWN price cards with init + diff
        Constraint::Min(6),        // orderbook depth
        Constraint::Length(3),     // positions + events
    ];

    let m = Layout::default().direction(Direction::Vertical).constraints(constraints).split(area);
    let mut idx: usize = 0;

    draw_market_info(f, m[idx], s); idx += 1;
    draw_price_cards(f, m[idx], s); idx += 1;
    draw_depth_panel(f, m[idx], s); idx += 1;

    let bottom = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)])
        .split(m[idx]);
    draw_positions_card(f, bottom[0], s);
    draw_events_card(f, bottom[1], s);
}

// ─── MARKET INFO BAR: 4 cards — BTC | Open | Countdown | Balance ───

fn draw_market_info(f: &mut Frame, area: Rect, s: &State) {
    let cols = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,4); 4]).split(area);
    let big = Modifier::BOLD;

    let btc_delta = if s.btc_open > 0.0 { s.btc - s.btc_open }
        else if s.btc_entry > 0.0 { s.btc - s.btc_entry } else { 0.0 };
    let btc_c = if btc_delta > 0.0 { Color::Green } else if btc_delta < 0.0 { Color::Red } else { Color::Yellow };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(format!("${:.0}", s.btc), Style::default().fg(Color::White).add_modifier(big))),
            Line::from(Span::styled(format!("{:+.0} USD", btc_delta), Style::default().fg(btc_c))),
            Line::from(Span::styled("BTC", Style::default().fg(Color::DarkGray))),
        ]).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(btc_c))),
        cols[0]);

    let open_c = if s.session_open_btc > 0.0 { Color::Cyan } else { Color::DarkGray };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(format!("${:.0}", s.session_open_btc), Style::default().fg(Color::White).add_modifier(big))),
            Line::from(Span::styled(if s.btc_open > 0.0 && s.session_open_btc == 0.0 {
                format!("ref ${:.0}", s.btc_open) } else { "".into() },
                Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled("INICIAL", Style::default().fg(open_c))),
        ]).block(Block::default().borders(Borders::ALL)),
        cols[1]);

    let sl = s.hft.secs_left;
    let min = sl / 60; let sec = sl % 60;
    let sl_c = if sl > 300 { Color::Green } else if sl > 60 { Color::Yellow } else if sl > 0 { Color::Red } else { Color::DarkGray };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(format!("{}:{:02}", min, sec), Style::default().fg(sl_c).add_modifier(big))),
            Line::from(Span::styled(if sl > 0 { "restantes" } else { "FINALIZADA" }, Style::default().fg(sl_c))),
            Line::from(Span::styled("CUENTA REGRESIVA", Style::default().fg(Color::DarkGray))),
        ]).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(sl_c))),
        cols[2]);

    let bal_c = if s.bal > 100.0 { Color::Green } else if s.bal > 50.0 { Color::Yellow } else { Color::Red };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(format!("${:.2}", s.bal), Style::default().fg(Color::White).add_modifier(big))),
            Line::from(Span::styled(format!("ordenes: {}", s.orders), Style::default().fg(if s.orders>0{Color::Yellow}else{Color::DarkGray}))),
            Line::from(Span::styled("BALANCE", Style::default().fg(bal_c))),
        ]).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(bal_c))),
        cols[3]);
}

// ─── PRICE CARDS: UP / DOWN — big numbers, init, diff ─────────────

fn draw_price_cards(f: &mut Frame, area: Rect, s: &State) {
    let cols = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)])
        .split(area);
    let big = Modifier::BOLD;

    let up_px = s.hft.clob_trade_up;
    let up_init = if s.session_open_up > 0.0 { s.session_open_up } else { up_px };
    let up_diff = if up_init > 0.0 { (up_px - up_init) / up_init * 100.0 } else { 0.0 };
    let up_c = if up_diff > 0.0 { Color::Green } else if up_diff < 0.0 { Color::Red } else { Color::Yellow };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("▲ UP  ", Style::default().fg(Color::Green).add_modifier(big)),
                Span::styled(format!("{:.4}", up_px), Style::default().fg(Color::White).add_modifier(big)),
                Span::styled(format!("  {:+.2}%", up_diff), Style::default().fg(up_c).add_modifier(big)),
            ]),
            Line::from(Span::styled(format!("INIT {:.4}  |  vol {:.0}", up_init, s.hft.clob_trade_up_vol),
                Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled(if s.pos_sen_up || s.pos_h65_up || s.pos_odi_up {
                format!("▶ POSICION ABIERTA")
            } else { "— sin posicion".into() },
                Style::default().fg(if s.pos_sen_up||s.pos_h65_up||s.pos_odi_up {Color::Green}else{Color::DarkGray}))),
        ]).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Green))),
        cols[0]);

    let dn_px = s.hft.clob_trade_dn;
    let dn_init = if s.session_open_dn > 0.0 { s.session_open_dn } else { dn_px };
    let dn_diff = if dn_init > 0.0 { (dn_px - dn_init) / dn_init * 100.0 } else { 0.0 };
    let dn_c = if dn_diff > 0.0 { Color::Green } else if dn_diff < 0.0 { Color::Red } else { Color::Yellow };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("▼ DN  ", Style::default().fg(Color::Red).add_modifier(big)),
                Span::styled(format!("{:.4}", dn_px), Style::default().fg(Color::White).add_modifier(big)),
                Span::styled(format!("  {:+.2}%", dn_diff), Style::default().fg(dn_c).add_modifier(big)),
            ]),
            Line::from(Span::styled(format!("INIT {:.4}  |  vol {:.0}", dn_init, s.hft.clob_trade_dn_vol),
                Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled(if s.pos_sen_dn || s.pos_h65_dn || s.pos_odi_dn {
                format!("▶ POSICION ABIERTA")
            } else { "— sin posicion".into() },
                Style::default().fg(if s.pos_sen_dn||s.pos_h65_dn||s.pos_odi_dn {Color::Red}else{Color::DarkGray}))),
        ]).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Red))),
        cols[1]);
}

// ─── POSITIONS CARD ──────────────────────────────────────────────

fn draw_positions_card(f: &mut Frame, area: Rect, s: &State) {
    let mut lines: Vec<Line> = Vec::new();
    let b = Modifier::BOLD;

    if s.pos_sen_up {
        let entry = s.pos_sen_entry_up; let cur = s.hft.clob_trade_up;
        let pnl = if entry > 0.0 && cur > 0.0 { s.sen_budget * (cur / entry - 1.0) } else { 0.0 };
        let pnl_pct = if entry > 0.0 { (cur / entry - 1.0) * 100.0 } else { 0.0 };
        let pc = if pnl >= 0.0 { Color::Green } else { Color::Red };
        lines.push(Line::from(vec![
            Span::styled("▲ SENNA UP  ", Style::default().fg(Color::Green).add_modifier(b)),
            Span::styled(format!("entry:{:.4}  bid:{:.4}", entry, cur), Style::default().fg(Color::White)),
        ]));
        lines.push(Line::from(Span::styled(
            format!("  PnL {:+.2} ({:+.1}%)", pnl, pnl_pct), Style::default().fg(pc).add_modifier(b))));
    }
    if s.pos_sen_dn {
        let entry = s.pos_sen_entry_dn; let cur = s.hft.clob_trade_dn;
        let pnl = if entry > 0.0 && cur > 0.0 { s.sen_budget * (cur / entry - 1.0) } else { 0.0 };
        let pnl_pct = if entry > 0.0 { (cur / entry - 1.0) * 100.0 } else { 0.0 };
        let pc = if pnl >= 0.0 { Color::Green } else { Color::Red };
        lines.push(Line::from(vec![
            Span::styled("▼ SENNA DN  ", Style::default().fg(Color::Red).add_modifier(b)),
            Span::styled(format!("entry:{:.4}  bid:{:.4}", entry, cur), Style::default().fg(Color::White)),
        ]));
        lines.push(Line::from(Span::styled(
            format!("  PnL {:+.2} ({:+.1}%)", pnl, pnl_pct), Style::default().fg(pc).add_modifier(b))));
    }
    if s.pos_h65_up {
        lines.push(Line::from(Span::styled(
            format!("▲ H65 UP  entry:{:.4}  bid:{:.4}", s.pos_h65_entry_up, s.hft.clob_trade_up),
            Style::default().fg(Color::Green).add_modifier(b))));
    }
    if s.pos_h65_dn {
        lines.push(Line::from(Span::styled(
            format!("▼ H65 DN  entry:{:.4}  bid:{:.4}", s.pos_h65_entry_dn, s.hft.clob_trade_dn),
            Style::default().fg(Color::Red).add_modifier(b))));
    }
    if s.pos_odi_up {
        lines.push(Line::from(Span::styled(
            format!("▲ O83 UP  entry:{:.4}  bid:{:.4}", s.pos_odi_entry_up, s.hft.clob_trade_up),
            Style::default().fg(Color::Green))));
    }
    if s.pos_odi_dn {
        lines.push(Line::from(Span::styled(
            format!("▼ O83 DN  entry:{:.4}  bid:{:.4}", s.pos_odi_entry_dn, s.hft.clob_trade_dn),
            Style::default().fg(Color::Red))));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled("— sin posiciones activas", Style::default().fg(Color::DarkGray))));
        if s.sen_enabled && s.sen_budget > 0.0 {
            lines.push(Line::from(Span::styled(format!("SENNA ${:.0} esperando senal", s.sen_budget),
                Style::default().fg(Color::Cyan))));
        }
    }

    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("POSICIONES")),
        area);
}

// ─── EVENTS CARD — last log entries ──────────────────────────────

fn draw_events_card(f: &mut Frame, area: Rect, s: &State) {
    let event_lines: Vec<Line> = s.log.iter().take(3).map(|e| Line::from(vec![
        Span::styled(format!("{} ", e.ts), Style::default().fg(Color::DarkGray)),
        Span::styled(&e.text, Style::default().fg(e.color)),
    ])).collect();
    let lines = if event_lines.is_empty() {
        vec![Line::from(Span::styled("  esperando eventos...", Style::default().fg(Color::DarkGray)))]
    } else { event_lines };

    // Health + connection mini-indicator in event area
    let latency_ms = s.last_api_ok.elapsed().as_millis() as u64;
    let (health_txt, health_c) = if !s.connected {
        ("NO CONEXION", Color::Red)
    } else if latency_ms > 10_000 {
        ("SIN DATOS", Color::Red)
    } else if latency_ms > 2_000 {
        ("LAG", Color::Yellow)
    } else {
        ("OK", Color::Green)
    };

    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)
            .title(format!("EVENTOS [{}]", health_txt))
            .border_style(Style::default().fg(health_c))),
        area);
}

// ═══════════════════════════════════════════════════════════════════
// ORDERBOOK DEPTH — 20 levels bids/asks
// ═══════════════════════════════════════════════════════════════════

fn draw_depth_panel(f: &mut Frame, area: Rect, s: &State) {
    let chunks = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)])
        .split(area);

    let bar_w = chunks[0].width.saturating_sub(14) as usize;

    // ─── UP depth ───
    draw_book_side(f, chunks[0], bar_w, "UP", Color::Green, &s.book_up, &s.hft.depth_up_bids, &s.hft.depth_up_asks);

    // ─── DOWN depth ───
    draw_book_side(f, chunks[1], bar_w, "DOWN", Color::Red, &s.book_dn, &s.hft.depth_dn_bids, &s.hft.depth_dn_asks);
}

fn draw_book_side(f: &mut Frame, area: Rect, bar_w: usize, label: &str, border_c: Color,
                   book: &crate::api::BookDepth, bids_fb: &[(f64,f64)], asks_fb: &[(f64,f64)]) {
    // Prevent excessive CPU on large books (session transitions could flood)
    let mut bids: Vec<(f64,f64)> = if !book.bids.is_empty() {
        book.bids.iter().take(200).map(|l| (l.price, l.size)).collect()
    } else {
        bids_fb.iter().take(200).copied().collect()
    };
    let mut asks: Vec<(f64,f64)> = if !book.asks.is_empty() {
        book.asks.iter().take(200).map(|l| (l.price, l.size)).collect()
    } else {
        asks_fb.iter().take(200).copied().collect()
    };

    let best_bid = bids.iter().map(|&(p,_)| p).fold(f64::NEG_INFINITY, f64::max);
    let best_ask = asks.iter().map(|&(p,_)| p).fold(f64::INFINITY, f64::min);

    // Available content lines = area height - 2 (borders) - 1 (spread)
    let avail = (area.height as usize).saturating_sub(3);
    let half = (avail / 2).max(4).min(8);  // 4-8 per side, both sides fit

    asks.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let top_asks: Vec<_> = asks.into_iter().take(half).rev().collect();

    bids.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let top_bids: Vec<_> = bids.into_iter().take(half).collect();

    let max_size = top_asks.iter().map(|&(_,s)| s)
        .chain(top_bids.iter().map(|&(_,s)| s))
        .fold(0.0f64, f64::max).max(1.0);

    let spread = if best_bid > 0.0 && best_ask > 0.0 { best_ask - best_bid } else { 0.0 };
    let mid = if best_bid > 0.0 && best_ask > 0.0 { (best_bid + best_ask) / 2.0 } else { 0.0 };

    let mut lines: Vec<Line> = Vec::new();

    // Asks (venta) — rojo, descienden hacia el ceiling
    for &(price, size) in &top_asks {
        let w = ((size / max_size) * bar_w as f64) as usize;
        let bar = "█".repeat(w.min(bar_w));
        let is_ceiling = (price - best_ask).abs() < 0.0001;
        let c = if is_ceiling { Color::Yellow } else { Color::Red };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<8.4} ", price), Style::default().fg(c)),
            Span::styled(bar, Style::default().fg(Color::Red)),
            Span::styled(format!(" {:.0}", size), Style::default().fg(Color::DarkGray)),
        ]));
    }

    // ─── SPREAD ZONE ─── ceiling / floor
    if best_bid > 0.0 && best_ask > 0.0 {
        let spread_str = format!("{:.4}", spread);
        let mid_str = format!("{:.4}", mid);
        let s_label = format!("── SPREAD {spread_str} ── MID {mid_str} ──");
        lines.push(Line::from(vec![
            Span::styled(s_label, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
    }

    // Bids (compra) — verde, ascienden desde el floor
    for &(price, size) in &top_bids {
        let w = ((size / max_size) * bar_w as f64) as usize;
        let bar = "█".repeat(w.min(bar_w));
        let is_floor = (price - best_bid).abs() < 0.0001;
        let c = if is_floor { Color::Yellow } else { Color::Green };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<8.4} ", price), Style::default().fg(c)),
            Span::styled(bar, Style::default().fg(Color::Green)),
            Span::styled(format!(" {:.0}", size), Style::default().fg(Color::DarkGray)),
        ]));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled("  esperando...", Style::default().fg(Color::DarkGray))));
    }

    let title = if best_bid > 0.0 && best_ask > 0.0 {
        format!("{label}  ceil:{best_ask:.4}  floor:{best_bid:.4}")
    } else {
        format!("{label} Book")
    };

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default().borders(Borders::ALL).title(title).border_style(Style::default().fg(border_c))
        ),
        area,
    );
}

// ═══════════════════════════════════════════════════════════════════
// TAB GRAFICOS — DOM + TAP + Area Acumulada + Histograma
// ═══════════════════════════════════════════════════════════════════

fn draw_graficos(f: &mut Frame, area: Rect, s: &State) {
    let vert = Layout::default().direction(Direction::Vertical)
        .constraints([Constraint::Ratio(2,5), Constraint::Ratio(2,5), Constraint::Ratio(1,5)])
        .split(area);

    // Top: DOM full width
    draw_dom(f, vert[0], s);

    // Middle: TAP full width
    draw_tap(f, vert[1], s);

    // Bottom: Area Acumulada | Histograma
    let bot = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)])
        .split(vert[2]);
    draw_area_acumulada(f, bot[0], s);
    draw_histograma(f, bot[1], s);
}

// ─── DOM — Depth of Market: bids/asks ladder compacto para UP y DOWN ──

fn draw_dom(f: &mut Frame, area: Rect, s: &State) {
    let cols = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)])
        .split(area);

    draw_dom_side(f, cols[0], "UP", Color::Green, &s.book_up);
    draw_dom_side(f, cols[1], "DOWN", Color::Red, &s.book_dn);
}

fn draw_dom_side(f: &mut Frame, area: Rect, label: &str, c: Color, book: &crate::api::BookDepth) {
    let max_n = (area.height as usize).saturating_sub(4) / 2;
    let n = max_n.min(10).max(3);

    let mut bids: Vec<(f64,f64)> = book.bids.iter().map(|l| (l.price, l.size)).collect();
    let mut asks: Vec<(f64,f64)> = book.asks.iter().map(|l| (l.price, l.size)).collect();
    bids.sort_unstable_by(|a,b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    asks.sort_unstable_by(|a,b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let best_bid = bids.first().map(|&(p,_)| p).unwrap_or(0.0);
    let best_ask = asks.first().map(|&(p,_)| p).unwrap_or(0.0);
    let max_size = bids.iter().chain(asks.iter()).map(|&(_,s)| s).fold(0.0f64, f64::max).max(1.0);

    let bar_w = area.width.saturating_sub(18) as usize;
    let mut lines: Vec<Line> = Vec::new();
    let bid_bg = if label == "UP" { Color::Green } else { Color::Red };

    for &(price, size) in asks.iter().take(n).rev() {
        let w = ((size / max_size) * bar_w as f64) as usize;
        lines.push(Line::from(vec![
            Span::styled(format!("{:<8.4}", price), Style::default().fg(Color::Red)),
            Span::styled(format!("{:>7.0}", size), Style::default().fg(Color::DarkGray)),
            Span::styled("█".repeat(w.min(bar_w)), Style::default().fg(Color::Red)),
        ]));
    }
    if best_ask > 0.0 && best_bid > 0.0 {
        lines.push(Line::from(Span::styled(
            format!("── {:.4} ──", best_ask - best_bid),
            Style::default().fg(Color::Yellow))));
    }
    for &(price, size) in bids.iter().take(n) {
        let w = ((size / max_size) * bar_w as f64) as usize;
        lines.push(Line::from(vec![
            Span::styled(format!("{:<8.4}", price), Style::default().fg(bid_bg)),
            Span::styled(format!("{:>7.0}", size), Style::default().fg(Color::DarkGray)),
            Span::styled("█".repeat(w.min(bar_w)), Style::default().fg(bid_bg)),
        ]));
    }
    let title = if best_bid > 0.0 && best_ask > 0.0 {
        format!("{label} {:.4}/{:.4}", best_bid, best_ask)
    } else { format!("{label} DOM") };
    f.render_widget(Paragraph::new(lines).block(
        Block::default().borders(Borders::ALL).title(title).border_style(Style::default().fg(c))), area);
}

// ─── TAP — Time & Sales: últimos trades ──────────────────────────────

fn draw_tap(f: &mut Frame, area: Rect, s: &State) {
    let max_n = (area.height as usize).saturating_sub(3).min(20);
    let mut lines: Vec<Line> = Vec::new();
    for t in s.trades.iter().take(max_n) {
        let col = if t.side == "UP" { Color::Green } else { Color::Red };
        let ts = if t.ts.len() > 12 { &t.ts[t.ts.len()-12..] } else { &t.ts };
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", ts), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:>6.4} ", t.price), Style::default().fg(col).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:>6.0} ", t.size), Style::default().fg(Color::DarkGray)),
            Span::styled(&t.side, Style::default().fg(col)),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled("  esperando trades...", Color::DarkGray)));
    }
    f.render_widget(Paragraph::new(lines).block(
        Block::default().borders(Borders::ALL).title("TAP — Time & Sales")), area);
}

// ─── AREA ACUMULADA — cumulative bid/ask volume curve ────────────────

fn draw_area_acumulada(f: &mut Frame, area: Rect, s: &State) {
    let avail_h = (area.height as usize).saturating_sub(3).max(1);
    let avail_w = (area.width as usize).saturating_sub(16).max(1);

    let bids: Vec<(f64,f64)> = s.book_up.bids.iter().map(|l| (l.price, l.size)).collect();
    let asks: Vec<(f64,f64)> = s.book_up.asks.iter().map(|l| (l.price, l.size)).collect();

    let all: Vec<(f64,f64)> = bids.iter().copied().chain(asks.iter().copied()).collect();
    let max_cum = bids.iter().map(|&(_,s)| s).sum::<f64>().max(asks.iter().map(|&(_,s)| s).sum::<f64>()).max(1.0);
    let min_p = all.iter().map(|&(p,_)| p).fold(f64::INFINITY, f64::min).max(0.0);
    let max_p = all.iter().map(|&(p,_)| p).fold(f64::NEG_INFINITY, f64::max).min(1.0);
    let p_range = (max_p - min_p).max(0.01);

    // Build cumulative curves as rows (one per price bucket)
    let buckets = avail_h.min(20);
    let mut rows: Vec<(f64, f64, f64)> = Vec::new(); // (price_mid, bid_cum, ask_cum)
    for i in 0..buckets {
        let p = min_p + (p_range * (buckets - 1 - i) as f64 / (buckets - 1).max(1) as f64);
        let bid_cum: f64 = bids.iter().filter(|&&(bp,_)| bp >= p).map(|&(_,s)| s).sum();
        let ask_cum: f64 = asks.iter().filter(|&&(ap,_)| ap <= p).map(|&(_,s)| s).sum();
        rows.push((p, bid_cum, ask_cum));
    }

    let mut lines: Vec<Line> = Vec::new();
    for (p, bid_cum, ask_cum) in &rows {
        let bw = ((bid_cum / max_cum) * avail_w as f64) as usize;
        let aw = ((ask_cum / max_cum) * avail_w as f64) as usize;
        let mid_w = avail_w.saturating_sub(bw + aw);
        lines.push(Line::from(vec![
            Span::styled(format!("{:<7.4}", p), Style::default().fg(Color::DarkGray)),
            Span::styled("█".repeat(bw.min(avail_w)), Style::default().fg(Color::Green)),
            Span::styled("░".repeat(mid_w), Style::default().fg(Color::DarkGray)),
            Span::styled("█".repeat(aw.min(avail_w)), Style::default().fg(Color::Red)),
            Span::styled(format!(" {:.0}", bid_cum + ask_cum), Style::default().fg(Color::DarkGray)),
        ]));
    }
    f.render_widget(Paragraph::new(lines).block(
        Block::default().borders(Borders::ALL).title("Area Acumulada — bids █ / asks █")), area);
}

// ─── HISTOGRAMA — volume bars per price level ────────────────────────

fn draw_histograma(f: &mut Frame, area: Rect, s: &State) {
    let avail_h = (area.height as usize).saturating_sub(3).max(1);
    let avail_w = (area.width as usize).saturating_sub(14).max(1);

    let bids: Vec<(f64,f64)> = s.book_up.bids.iter().map(|l| (l.price, l.size)).collect();
    let asks: Vec<(f64,f64)> = s.book_up.asks.iter().map(|l| (l.price, l.size)).collect();

    let max_vol = bids.iter().map(|&(_,s)| s).chain(asks.iter().map(|&(_,s)| s)).fold(0.0f64, f64::max).max(1.0);
    let all: Vec<(f64,f64)> = bids.iter().copied().chain(asks.iter().copied()).collect();
    let min_p = all.iter().map(|&(p,_)| p).fold(f64::INFINITY, f64::min).max(0.0);
    let max_p = all.iter().map(|&(p,_)| p).fold(f64::NEG_INFINITY, f64::max).min(1.0);
    let p_range = (max_p - min_p).max(0.01);

    let log_max = (max_vol + 1.0).ln().max(1.0);

    let buckets = avail_h.min(30);
    let grid_every = (buckets / 4).max(1);
    let mut lines: Vec<Line> = Vec::new();

    for i in 0..buckets {
        let p_lo = min_p + (p_range * i as f64 / buckets as f64);
        let p_hi = min_p + (p_range * (i + 1) as f64 / buckets as f64);
        let vol: f64 = bids.iter().filter(|&&(bp,_)| bp >= p_lo && bp < p_hi).map(|&(_,s)| s).sum::<f64>()
            + asks.iter().filter(|&&(ap,_)| ap >= p_lo && ap < p_hi).map(|&(_,s)| s).sum::<f64>();

        let w = if vol > 0.0 {
            ((vol + 1.0).ln() / log_max * avail_w as f64).min(avail_w as f64) as usize
        } else { 0 };

        let is_grid = i > 0 && i % grid_every == 0;
        let c = if vol > 0.0 { Color::Cyan } else { Color::DarkGray };

        let bar = if w > 0 && vol > 0.0 { "█".repeat(w.min(avail_w)) } else { String::new() };
        let fill = if w < avail_w { "░".repeat(avail_w.saturating_sub(w)) } else { String::new() };

        if is_grid {
            let grid_w = avail_w.min(area.width.saturating_sub(14) as usize);
            lines.push(Line::from(vec![
                Span::styled(format!("{:<7.4} ", p_lo), Style::default().fg(Color::DarkGray)),
                Span::styled(bar, Style::default().fg(c)),
                Span::styled(fill, Style::default().fg(Color::Rgb(18,18,28))),
                Span::styled(format!(" {:.0}", vol), Style::default().fg(Color::DarkGray)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("        ", Style::default()),
                Span::styled("·".repeat(grid_w), Style::default().fg(Color::Rgb(30,30,42))),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!("{:<7.4} ", p_lo), Style::default().fg(Color::DarkGray)),
                Span::styled(bar, Style::default().fg(c)),
                Span::styled(fill, Style::default().fg(Color::Rgb(18,18,28))),
                Span::styled(format!(" {:.0}", vol), Style::default().fg(Color::DarkGray)),
            ]));
        }
    }
    f.render_widget(Paragraph::new(lines).block(
        Block::default().borders(Borders::ALL).title("Histograma — vol x nivel (log)")), area);
}


// ═══════════════════════════════════════════════════════════════════
// POSITION BAR — visible in ALL tabs
// ═══════════════════════════════════════════════════════════════════

fn draw_position_bar(f: &mut Frame, area: Rect, s: &State) {
    let (sen_pos, sen_style) = if s.pos_sen_up {
        let entry = s.pos_sen_entry_up;
        let current = s.hft.clob_trade_up;
        let sz = if entry > 0.0 { (s.sen_budget / entry).floor() as i64 } else { 0 };
        let pnl = if entry > 0.0 && current > 0.0 { s.sen_budget * (current / entry - 1.0) } else { 0.0 };
        let gain = pnl >= 0.0;
        let bg = if gain { Color::Green } else { Color::Red };
        let txt = format!("▲ SENNA UP  {:.0}ct→{:.4}  PnL:{:+.2}  {:.1}%  ▶ /p EXIT",
            sz, current, pnl, if entry>0.0{(current/entry-1.0)*100.0}else{0.0});
        (txt, Style::default().fg(Color::Black).bg(bg).add_modifier(Modifier::BOLD))
    } else if s.pos_sen_dn {
        let entry = s.pos_sen_entry_dn;
        let current = s.hft.clob_trade_dn;
        let sz = if entry > 0.0 { (s.sen_budget / entry).floor() as i64 } else { 0 };
        let pnl = if entry > 0.0 && current > 0.0 { s.sen_budget * (current / entry - 1.0) } else { 0.0 };
        let gain = pnl >= 0.0;
        let bg = if gain { Color::Green } else { Color::Red };
        let txt = format!("▼ SENNA DN  {:.0}ct→{:.4}  PnL:{:+.2}  {:.1}%  ▶ /p EXIT",
            sz, current, pnl, if entry>0.0{(current/entry-1.0)*100.0}else{0.0});
        (txt, Style::default().fg(Color::Black).bg(bg).add_modifier(Modifier::BOLD))
    } else if s.sen_enabled && s.sen_budget > 0.0 {
        (format!("⚡ SENNA activo  ${:.0}  esperando momentum...", s.sen_budget),
         Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
    } else if s.sen_enabled {
        (format!("⚡ SENNA activo  sin presupuesto"),
         Style::default().fg(Color::Cyan))
    } else {
        (format!("○ SENNA OFF  /s5..s100 para activar"),
         Style::default().fg(Color::DarkGray))
    };

    f.render_widget(
        Paragraph::new(sen_pos).style(sen_style)
            .block(Block::default().borders(Borders::ALL).title("Senna")),
        area);
}

// ═══════════════════════════════════════════════════════════════════
// BUDGET INPUT MODAL
// ═══════════════════════════════════════════════════════════════════

fn draw_command_bar(f: &mut Frame, area: Rect, s: &State) {
    let text = format!(
        "▶ /{}_\n/h5..h100 /o5..o100 /p /h /o",
        s.input_buf
    );
    f.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Green)).title("CMD")),
        area,
    );
}

// ═══════════════════════════════════════════════════════════════════
// FOOTER — hotkeys permanentes
// ═══════════════════════════════════════════════════════════════════

fn draw_footer(f: &mut Frame, area: Rect, s: &State) {
    let sl_info = if s.sl_pct > 0.0 {
        format!("{:.0}% {}", s.sl_pct, if s.sl_market {"MKT"}else{"LMT"})
    } else { "OFF".into() };
    let line1 = format!("TRADE: /10up65 /15d40  |  EXIT: /20up65e70  |  SL:{} /sl /sl10 /nsl  |  /p=PANIC", sl_info);
    let line2 = "[/]abrir comandos  [Tab]DINERO REAL/PAPER  [q]salir";
    f.render_widget(
        Paragraph::new(format!("{}\n{}", line1, line2))
            .style(Style::default().fg(Color::DarkGray)),
        area,
    );
}
