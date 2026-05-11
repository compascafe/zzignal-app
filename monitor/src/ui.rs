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

    if s.show_man {
        draw_man_page(f, area, s);
        return;
    }

    let pos_h = 3;
    let cmd_h = if s.input_mode == InputMode::Command { 3 } else { 0 };
    let trade_h = if s.mt_state > 0 || !s.trade_log.is_empty() { 6 } else { 0 };

    let mut constraints = vec![
        Constraint::Length(1),     // commit bar
        Constraint::Length(1),     // tab bar
        Constraint::Length(pos_h), // position bar
        Constraint::Min(4),        // dashboard
    ];
    if trade_h > 0 { constraints.push(Constraint::Length(trade_h)); }
    if cmd_h > 0 { constraints.push(Constraint::Length(cmd_h)); }
    constraints.push(Constraint::Length(10)); // footer: ayuda

    let chunks = Layout::default().direction(Direction::Vertical).constraints(constraints).split(area);
    let mut ci = 0;

    // ─── COMMIT BAR ────────────────────────────────────────────────────
    let commit = option_env!("GIT_HASH").unwrap_or("dev");
    let total_w = area.width as usize;
    let filler_w = total_w.saturating_sub(commit.len() + 19);
    let filler = " ".repeat(filler_w.min(80));
    f.render_widget(
        Paragraph::new(format!("|ZZIGNAL{filler}{commit}|"))
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

    // ─── TRADE LOG PANEL ──────────────────────────────────────────────
    if trade_h > 0 {
        draw_trade_log(f, chunks[ci], s);
        ci += 1;
    }

    // ─── COMMAND BAR (modal) ──────────────────────────────────────────
    if cmd_h > 0 {
        draw_command_bar(f, chunks[ci], s);
        ci += 1;
    }

    // ─── FOOTER ───────────────────────────────────────────────────────
    draw_footer(f, chunks[ci], s);
}

// ═══════════════════════════════════════════════════════════════════
// TAB 0/1: DASHBOARD
// ═══════════════════════════════════════════════════════════════════

fn draw_dashboard(f: &mut Frame, area: Rect, s: &State) {
    let constraints = vec![
        Constraint::Length(3),     // market info
        Constraint::Length(3),     // UP/DOWN price cards
        Constraint::Length(2),     // indicators
        Constraint::Length(2),     // manual trading status
        Constraint::Min(18),       // orderbook depth (17 lines fixed)
        Constraint::Length(6),     // orders + positions + events
    ];

    let m = Layout::default().direction(Direction::Vertical).constraints(constraints).split(area);
    let mut idx: usize = 0;

    draw_market_info(f, m[idx], s); idx += 1;
    draw_price_cards(f, m[idx], s); idx += 1;
    draw_indicators(f, m[idx], s); idx += 1;
    draw_manual_status(f, m[idx], s); idx += 1;
    draw_depth_panel(f, m[idx], s); idx += 1;

    let bottom = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(3,5), Constraint::Ratio(2,5)])
        .split(m[idx]);
    draw_positions_card(f, bottom[0], s);
    draw_events_card(f, bottom[1], s);
}

// ─── MARKET INFO BAR ──────────────────────────────────────────────

fn draw_market_info(f: &mut Frame, area: Rect, s: &State) {
    let cols = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,4); 4]).split(area);
    let big = Modifier::BOLD;

    let btc_ref = if s.session_open_btc > 0.0 { s.session_open_btc }
        else if s.btc_open > 0.0 { s.btc_open }
        else { s.btc };
    let btc_delta = s.btc - btc_ref;
    let btc_delta_pct = if btc_ref > 0.0 { (s.btc / btc_ref - 1.0) * 100.0 } else { 0.0 };
    let btc_c = if btc_delta > 0.0 { Color::Green } else if btc_delta < 0.0 { Color::Red } else { Color::Yellow };
    let arrow = if btc_delta > 0.0 { "↑" } else if btc_delta < 0.0 { "↓" } else { "→" };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(format!("${:.0}", s.btc), Style::default().fg(Color::White).add_modifier(big)),
                Span::styled(format!(" {arrow} {:+.0}", btc_delta), Style::default().fg(btc_c)),
            ]),
            Line::from(Span::styled(format!("abrio ${:.0}", btc_ref), Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled(format!("{:+.1}%", btc_delta_pct), Style::default().fg(btc_c))),
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

// ─── PRICE CARDS ──────────────────────────────────────────────────

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
                Span::styled(format!(" {:+.2}%", up_diff), Style::default().fg(up_c).add_modifier(big)),
            ]),
            Line::from(Span::styled(format!("init {:.4}  vol {:.0}  {}", up_init, s.hft.clob_trade_up_vol,
                if s.pos_sen_up||s.pos_h65_up||s.pos_odi_up {"▶ POS"}else{"—"}),
                Style::default().fg(Color::DarkGray))),
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

// ─── INDICATORS — 4 blank cards ───────────────────────────────

fn draw_indicators(f: &mut Frame, area: Rect, s: &State) {
    let cols = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,4); 4]).split(area);

    let b = Modifier::BOLD;

    // S1 = GEMINI
    {
        let gemini_lines: Vec<Line> = if s.gemini_active && !s.gemini_triggered {
            let mut lines = vec![
                Line::from(Span::styled("GEMINI", Style::default().fg(Color::Magenta).add_modifier(b))),
                Line::from(Span::styled(
                    format!("@{:.2}\u{2192}{:.2} ${:.0}", s.gemini_trigger, s.gemini_target, s.gemini_budget),
                    Style::default().fg(Color::White))),
            ];
            if s.gemini_exit > 0.0 {
                lines.push(Line::from(Span::styled(
                    format!("EXIT @{:.2}", s.gemini_exit),
                    Style::default().fg(Color::Cyan))));
            }
            lines
        } else if s.gemini_triggered || s.mt_state >= 1 {
            let mut lines = vec![
                Line::from(Span::styled("GEMINI", Style::default().fg(Color::Magenta).add_modifier(b))),
            ];
            if !s.gemini_outcome.is_empty() {
                let out_c = if s.gemini_outcome == "up" { Color::Green } else { Color::Red };
                let sz = if s.mt_size > 0.0 { format!("sz={:.0}", s.mt_size) } else { String::new() };
                lines.push(Line::from(Span::styled(
                    format!("{} @{:.4} {}", s.gemini_outcome.to_uppercase(), s.gemini_target, sz),
                    Style::default().fg(out_c))));
            }
            match s.mt_state {
                1 => lines.push(Line::from(Span::styled("TRIGGERED", Style::default().fg(Color::Yellow).add_modifier(b)))),
                2 => lines.push(Line::from(Span::styled("ACTIVE", Style::default().fg(Color::Green).add_modifier(b)))),
                3 => lines.push(Line::from(Span::styled("EXITING", Style::default().fg(Color::Cyan).add_modifier(b)))),
                _ => {}
            }
            lines
        } else {
            vec![
                Line::from(Span::styled("S1", Style::default().fg(Color::DarkGray))),
                Line::from(Span::styled("\u{2014}", Style::default().fg(Color::Rgb(20, 28, 40)))),
            ]
        };
        let border_c = if s.gemini_active || s.gemini_triggered { Color::Magenta } else { Color::Rgb(20, 30, 45) };
        let title = if s.gemini_active || s.gemini_triggered { "GEMINI" } else { "S1" };
        f.render_widget(
            Paragraph::new(gemini_lines)
                .block(Block::default().borders(Borders::ALL).title(title).border_style(Style::default().fg(border_c))),
            cols[0]);
    }

    let labels = ["S2", "S3", "S4"];
    for (i, label) in labels.iter().enumerate() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(*label, Style::default().fg(Color::DarkGray))),
                Line::from(Span::styled("\u{2014}", Style::default().fg(Color::Rgb(20, 28, 40)))),
            ]).block(Block::default().borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(20, 30, 45)))),
            cols[i + 1]);
    }
}

// ─── MANUAL TRADING STATUS BAR ────────────────────────────────────

fn draw_manual_status(f: &mut Frame, area: Rect, s: &State) {
    let b = Modifier::BOLD;
    let mut spans: Vec<Span> = Vec::new();

    spans.push(Span::styled("MANUAL: ", Style::default().fg(Color::DarkGray).add_modifier(b)));

    match s.mt_state {
        0 => {
            spans.push(Span::styled("IDLE  ", Style::default().fg(Color::DarkGray)));
            if s.mt_pnl_cum != 0.0 {
                let c = if s.mt_pnl_cum >= 0.0 { Color::Green } else { Color::Red };
                spans.push(Span::styled(format!("ΣP&L {:+.2}  ", s.mt_pnl_cum), Style::default().fg(c).add_modifier(b)));
                spans.push(Span::styled(format!("{}T/{}/{}W", s.mt_trades, s.mt_trades - s.mt_wins, s.mt_wins),
                    Style::default().fg(Color::DarkGray)));
            }
        }
        1 => {
            spans.push(Span::styled("PENDING ", Style::default().fg(Color::Yellow).add_modifier(b)));
            let outcome_c = if s.mt_outcome == "up" { Color::Green } else { Color::Red };
            spans.push(Span::styled(format!("{} ", s.mt_outcome.to_uppercase()), Style::default().fg(outcome_c).add_modifier(b)));
            spans.push(Span::styled(format!("BUY @{:.4} sz={:.0} ${:.2}",
                s.mt_entry, s.mt_size, s.mt_budget), Style::default().fg(Color::White)));
            if s.mt_last_fill_pct > 0.0 {
                spans.push(Span::styled(format!("  [{:.0}% filled]", s.mt_last_fill_pct),
                    Style::default().fg(Color::Yellow)));
            }
            if s.mt_exit_price > 0.0 {
                spans.push(Span::styled(format!("  TP@{:.4}", s.mt_exit_price),
                    Style::default().fg(Color::Cyan)));
            }
            spans.push(Span::styled("  /c=CANCELAR", Style::default().fg(Color::DarkGray)));
        }
        2 => {
            spans.push(Span::styled("ACTIVE ", Style::default().fg(Color::Green).add_modifier(b)));
            let outcome_c = if s.mt_outcome == "up" { Color::Green } else { Color::Red };
            spans.push(Span::styled(format!("{} ", s.mt_outcome.to_uppercase()), Style::default().fg(outcome_c).add_modifier(b)));

            let current_px = if s.mt_outcome == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn };
            let upnl = s.mt_size * (current_px - s.mt_entry);
            let upnl_pct = if s.mt_entry > 0.0 { (current_px / s.mt_entry - 1.0) * 100.0 } else { 0.0 };
            let pnl_c = if upnl >= 0.0 { Color::Green } else { Color::Red };

            spans.push(Span::styled(format!("entry:{:.4}→{:.4} ", s.mt_entry, current_px),
                Style::default().fg(Color::White)));
            spans.push(Span::styled(format!("uP&L {:+.2} ({:+.1}%) ", upnl, upnl_pct),
                Style::default().fg(pnl_c).add_modifier(b)));

            let liq_cmd = if s.mt_outcome == "up" { "/lupXX" } else { "/ldXX" };
            spans.push(Span::styled(format!("| {} /lm /c", liq_cmd), Style::default().fg(Color::DarkGray)));
        }
        3 => {
            spans.push(Span::styled("EXITING ", Style::default().fg(Color::Cyan).add_modifier(b)));
            let outcome_c = if s.mt_outcome == "up" { Color::Green } else { Color::Red };
            spans.push(Span::styled(format!("{} ", s.mt_outcome.to_uppercase()), Style::default().fg(outcome_c).add_modifier(b)));

            let exit_px = if s.mt_exit_price > 0.0 { s.mt_exit_price } else {
                if s.mt_outcome == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn }
            };
            let expected = s.mt_size * (exit_px - s.mt_entry);
            let pnl_c = if expected >= 0.0 { Color::Green } else { Color::Red };
            spans.push(Span::styled(format!("exit @{:.4}  exp.P&L {:+.2}",
                exit_px, expected), Style::default().fg(pnl_c).add_modifier(b)));
            spans.push(Span::styled("  /c=CANCELAR EXIT", Style::default().fg(Color::DarkGray)));
        }
        _ => {}
    }

    if s.mt_pnl_cum != 0.0 {
        let c = if s.mt_pnl_cum >= 0.0 { Color::Green } else { Color::Red };
        spans.push(Span::styled(format!("  Σ{:+.2}", s.mt_pnl_cum), Style::default().fg(c).add_modifier(b)));
    }

    // ─── SL INDICATOR ───
    let sl_span = if s.sl_pct > 0.0 {
        let sl_price = s.mt_entry * (1.0 - s.sl_pct / 100.0);
        let sl_type = if s.sl_market { "MKT" } else { "LMT" };
        let has_sl_order = !s.mt_sl_order_id.is_empty();
        let sl_c = if s.mt_state == 2 && has_sl_order { Color::Green }
            else if s.mt_state == 2 { Color::Yellow }
            else if s.sl_pct > 0.0 { Color::DarkGray }
            else { Color::Red };
        if s.mt_state == 2 {
            Span::styled(
                format!("  🛡 SL:{:.0}%{} @{:.4} {}", s.sl_pct, sl_type, sl_price,
                    if has_sl_order {"✓"}else{"..."}),
                Style::default().fg(sl_c).add_modifier(b))
        } else {
            Span::styled(
                format!("  SL:{:.0}%{}", s.sl_pct, sl_type),
                Style::default().fg(Color::DarkGray))
        }
    } else {
        Span::styled("  SL:OFF", Style::default().fg(Color::Red).add_modifier(b))
    };
    spans.push(sl_span);

    let (border_c, title) = match s.mt_state {
        1 => (Color::Yellow, "TRADING — PENDING"),
        2 => (Color::Green, "TRADING — ACTIVE"),
        3 => (Color::Cyan, "TRADING — EXIT"),
        _ if s.mt_pnl_cum != 0.0 => {
            let c = if s.mt_pnl_cum >= 0.0 { Color::Green } else { Color::Red };
            (c, "TRADING — RESULTS")
        }
        _ => (Color::DarkGray, "TRADING"),
    };

    f.render_widget(
        Paragraph::new(Line::from(spans))
            .block(Block::default().borders(Borders::ALL).title(title).border_style(Style::default().fg(border_c))),
        area);
}

// ─── POSITIONS & ORDERS CARD ──────────────────────────────────────

fn draw_positions_card(f: &mut Frame, area: Rect, s: &State) {
    let mut lines: Vec<Line> = Vec::new();
    let b = Modifier::BOLD;

    // ─── OPEN ORDERS ──────────────────────────────────────────
    if !s.open_orders.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("── ORDENES ({}) ──", s.open_orders.len()),
            Style::default().fg(Color::Cyan).add_modifier(b))));
        for o in &s.open_orders {
            let side_txt = if o.side == "buy" { "BUY" } else { "SELL" };
            let outcome_txt = o.outcome.to_uppercase();
            let outcome_c = if o.outcome == "up" { Color::Green } else { Color::Red };
            let side_c = if o.side == "buy" { Color::Green } else { Color::Red };

            let (fill_txt, fill_c, icon) = if o.is_filled() {
                ("FILLED", Color::Green, "✓")
            } else if o.is_partial() {
                ("FILLING", Color::Yellow, "◐")
            } else {
                ("PENDING", Color::Red, "✗")
            };

            let pct = if o.size_orig > 0.0 { (o.size_matched / o.size_orig * 100.0) as i64 } else { 0 };

            lines.push(Line::from(vec![
                Span::styled(format!("{icon} "), Style::default().fg(fill_c).add_modifier(b)),
                Span::styled(format!("{} ", side_txt), Style::default().fg(side_c).add_modifier(b)),
                Span::styled(format!("{}  ", outcome_txt), Style::default().fg(outcome_c).add_modifier(b)),
                Span::styled(format!("@{:.4}  ", o.price), Style::default().fg(Color::White)),
                Span::styled(format!("{:.0}/{:.0} [{pct}%]", o.size_matched, o.size_orig), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("  {fill_txt}"), Style::default().fg(fill_c)),
            ]));
            // Show order ID for manual cancel
            lines.push(Line::from(Span::styled(
                format!("  id:{}", &o.id[..o.id.len().min(20)]),
                Style::default().fg(Color::Rgb(30, 40, 55)))));
        }
        lines.push(Line::from(""));
    }

    // ─── STRATEGY POSITIONS ──────────────────────────────────────
    let has_positions = s.pos_sen_up || s.pos_sen_dn || s.pos_h65_up
        || s.pos_h65_dn || s.pos_odi_up || s.pos_odi_dn;

    if has_positions {
        lines.push(Line::from(Span::styled("── POSICIONES STRATEGY ──",
            Style::default().fg(Color::Yellow).add_modifier(b))));
    }

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
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("ORDENES / POSICIONES")),
        area);
}

// ─── EVENTS CARD ──────────────────────────────────────────────────

fn draw_events_card(f: &mut Frame, area: Rect, s: &State) {
    let mut lines: Vec<Line> = Vec::new();

    // Show trade log first (clean trading events)
    for e in s.trade_log.iter().take(2) {
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", e.ts), Style::default().fg(Color::DarkGray)),
            Span::styled(&e.text, Style::default().fg(e.color)),
        ]));
    }

    // Fill remaining with critical log entries
    for e in s.log.iter().filter(|e| e.text.contains("FAIL") || e.text.contains("PANIC")).take(2) {
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", e.ts), Style::default().fg(Color::DarkGray)),
            Span::styled(&e.text, Style::default().fg(e.color)),
        ]));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled("  esperando eventos...", Style::default().fg(Color::DarkGray))));
    }

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

// ─── TRADE LOG PANEL ──────────────────────────────────────────────

fn draw_trade_log(f: &mut Frame, area: Rect, s: &State) {
    let max_n = (area.height as usize).saturating_sub(2).min(5);
    let lines: Vec<Line> = s.trade_log.iter().take(max_n).map(|e| {
        Line::from(vec![
            Span::styled(format!("{} ", e.ts), Style::default().fg(Color::DarkGray)),
            Span::styled(&e.text, Style::default().fg(e.color)),
        ])
    }).collect();

    let title = if s.mt_pnl_cum != 0.0 {
        let c = if s.mt_pnl_cum >= 0.0 { "▲" } else { "▼" };
        format!("TRADE LOG {c} Σ{:+.2}  {}/{}W",
            s.mt_pnl_cum, s.mt_trades, s.mt_wins)
    } else {
        "TRADE LOG".into()
    };

    let border_c = if s.mt_state == 1 { Color::Yellow }
        else if s.mt_state == 2 { Color::Green }
        else if s.mt_state == 3 { Color::Cyan }
        else if s.mt_pnl_cum > 0.0 { Color::Green }
        else if s.mt_pnl_cum < 0.0 { Color::Red }
        else { Color::DarkGray };

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default().borders(Borders::ALL).title(title).border_style(Style::default().fg(border_c))
        ),
        area);
}

// ═══════════════════════════════════════════════════════════════════
// ORDERBOOK DEPTH
// ═══════════════════════════════════════════════════════════════════

fn draw_depth_panel(f: &mut Frame, area: Rect, s: &State) {
    let chunks = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)])
        .split(area);

    let bar_w = chunks[0].width.saturating_sub(14) as usize;

    draw_book_side(f, chunks[0], bar_w, "UP", Color::Green, &s.book_up, &s.hft.depth_up_bids, &s.hft.depth_up_asks);
    draw_book_side(f, chunks[1], bar_w, "DOWN", Color::Red, &s.book_dn, &s.hft.depth_dn_bids, &s.hft.depth_dn_asks);
}

fn draw_book_side(f: &mut Frame, area: Rect, bar_w: usize, label: &str, border_c: Color,
                   book: &crate::api::BookDepth, bids_fb: &[(f64,f64)], asks_fb: &[(f64,f64)]) {
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

    let avail = (area.height as usize).saturating_sub(3);
    let half = 8; // fixed: 8 asks + 1 spread + 8 bids = 17 lines

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

    for &(price, size) in top_asks.iter() {
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

    if best_bid > 0.0 && best_ask > 0.0 {
        let spread_str = format!("{:.4}", spread);
        let mid_str = format!("{:.4}", mid);
        let s_label = format!("── SPREAD {spread_str} ── MID {mid_str} ──");
        lines.push(Line::from(vec![
            Span::styled(s_label, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
    }

    for &(price, size) in top_bids.iter() {
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

    draw_dom(f, vert[0], s);
    draw_tap(f, vert[1], s);

    let bot = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)])
        .split(vert[2]);
    draw_area_acumulada(f, bot[0], s);
    draw_histograma(f, bot[1], s);
}

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

    let buckets = avail_h.min(20);
    let mut rows: Vec<(f64, f64, f64)> = Vec::new();
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
        Block::default().borders(Borders::ALL).title("Area Acumulada")), area);
}

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
        Block::default().borders(Borders::ALL).title("Histograma")), area);
}


// ═══════════════════════════════════════════════════════════════════
// POSITION BAR
// ═══════════════════════════════════════════════════════════════════

fn draw_position_bar(f: &mut Frame, area: Rect, s: &State) {
    let b = Modifier::BOLD;

    // ─── Manual position ALWAYS shown ───
    let (pos_text, pos_style) = if s.mt_state >= 2 {
        let current_px = if s.mt_outcome == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn };
        let entry = if s.mt_fill_avg > 0.0 { s.mt_fill_avg } else { s.mt_entry };
        let pnl = s.mt_size * (current_px - entry);
        let pnl_pct = if entry > 0.0 { (current_px / entry - 1.0) * 100.0 } else { 0.0 };
        let gain = pnl >= 0.0;
        let bg = if gain { Color::Green } else { Color::Red };
        let side_sym = if s.mt_outcome == "up" { "▲" } else { "▼" };
        let tsl = if s.mt_tsl_pct > 0.0 { format!(" TSL:{:.0}%", s.mt_tsl_pct) } else { String::new() };
        let sl = if s.sl_pct > 0.0 { format!(" SL:{:.0}%", s.sl_pct) } else { String::new() };
        let exit_info = if s.mt_exit_price > 0.0 { format!(" TP:{:.4}", s.mt_exit_price) } else { String::new() };
        let txt = format!("{side_sym} POS {} sz={:.0} entry={:.4}→{:.4} PnL:{:+.2} ({:+.1}%) Σ{:+.2}{tsl}{sl}{exit_info}",
            s.mt_outcome.to_uppercase(), s.mt_size, entry, current_px, pnl, pnl_pct, s.mt_pnl_cum);
        (txt, Style::default().fg(Color::Black).bg(bg).add_modifier(b))
    } else if s.mt_state == 1 {
        let side_sym = if s.mt_outcome == "up" { "▲" } else { "▼" };
        let txt = format!("{side_sym} PENDING {} sz={:.0} @{:.4} ${:.2}  [{:.0}% filled]  /c=CANCELAR",
            s.mt_outcome.to_uppercase(), s.mt_size, s.mt_entry, s.mt_budget, s.mt_last_fill_pct);
        (txt, Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(b))
    } else if s.pos_sen_up {
        let entry = s.pos_sen_entry_up; let cur = s.hft.clob_trade_up;
        let sz = if entry > 0.0 { (s.sen_budget / entry).floor() as i64 } else { 0 };
        let pnl = if entry > 0.0 && cur > 0.0 { s.sen_budget * (cur / entry - 1.0) } else { 0.0 };
        let gain = pnl >= 0.0;
        let bg = if gain { Color::Green } else { Color::Red };
        (format!("▲ SENNA UP  {:.0}ct→{:.4}  PnL:{:+.2}  {:.1}%  ▶ /p EXIT",
            sz, cur, pnl, if entry>0.0{(cur/entry-1.0)*100.0}else{0.0}),
         Style::default().fg(Color::Black).bg(bg).add_modifier(b))
    } else if s.pos_sen_dn {
        let entry = s.pos_sen_entry_dn; let cur = s.hft.clob_trade_dn;
        let sz = if entry > 0.0 { (s.sen_budget / entry).floor() as i64 } else { 0 };
        let pnl = if entry > 0.0 && cur > 0.0 { s.sen_budget * (cur / entry - 1.0) } else { 0.0 };
        let gain = pnl >= 0.0;
        let bg = if gain { Color::Green } else { Color::Red };
        (format!("▼ SENNA DN  {:.0}ct→{:.4}  PnL:{:+.2}  {:.1}%  ▶ /p EXIT",
            sz, cur, pnl, if entry>0.0{(cur/entry-1.0)*100.0}else{0.0}),
         Style::default().fg(Color::Black).bg(bg).add_modifier(b))
    } else if s.sen_enabled && s.sen_budget > 0.0 {
        (format!("⚡ SENNA activo  ${:.0}  esperando momentum...", s.sen_budget),
         Style::default().fg(Color::Cyan).add_modifier(b))
    } else {
        (format!("POS: 0  —  /l10up65 para abrir  |  /man = ayuda"),
         Style::default().fg(Color::DarkGray))
    };

    // Alert indicators
    let alert_info = if !s.alerts.is_empty() {
        let alert_list: Vec<String> = s.alerts.iter().map(|a|
            format!("{}@{:.4}", a.outcome.to_uppercase(), a.price)
        ).collect();
        format!(" | 🔔 {}", alert_list.join(", "))
    } else { String::new() };

    let combined = format!("{pos_text}{alert_info}");

    f.render_widget(
        Paragraph::new(combined).style(pos_style)
            .block(Block::default().borders(Borders::ALL).title("POSICION")),
        area);
}

// ═══════════════════════════════════════════════════════════════════
// COMMAND BAR MODAL
// ═══════════════════════════════════════════════════════════════════

fn draw_command_bar(f: &mut Frame, area: Rect, s: &State) {
    let help = "/l10up65e70  BUY+EXIT  |  /clup65  cancel+liq  |  /lup70 /ld70  liq limit";
    let help2 = "/c  cancel  |  /lm  liq mercado  |  /clm  cancel+mkt  |  /p  PANIC";
    let text = format!(
        "▶ /{}_\n{help}\n{help2}",
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
// /man PAGE — full-screen command reference
// ═══════════════════════════════════════════════════════════════════

fn draw_man_page(f: &mut Frame, area: Rect, _s: &State) {
    use crate::commands::REGISTRY;
    use std::collections::BTreeMap;

    // Group by category
    let mut cats: BTreeMap<&str, Vec<&crate::commands::CmdDef>> = BTreeMap::new();
    for cmd in REGISTRY {
        cats.entry(cmd.category).or_default().push(cmd);
    }

    let mut lines: Vec<Line> = Vec::new();
    let b = Modifier::BOLD;

    lines.push(Line::from(Span::styled(
        "╔══════════════════════════════════════════════════════════════╗",
        Style::default().fg(Color::Yellow).add_modifier(b))));
    lines.push(Line::from(Span::styled(
        "║              ZZIGNAL MONITOR — COMANDOS /man               ║",
        Style::default().fg(Color::Yellow).add_modifier(b))));
    lines.push(Line::from(Span::styled(
        "╚══════════════════════════════════════════════════════════════╝",
        Style::default().fg(Color::Yellow).add_modifier(b))));
    lines.push(Line::from(""));

    for (cat, cmds) in &cats {
        lines.push(Line::from(Span::styled(
            format!("── {cat} ──"),
            Style::default().fg(Color::Cyan).add_modifier(b))));
        for cmd in cmds {
            lines.push(Line::from(vec![
                Span::styled(cmd.syntax, Style::default().fg(Color::Green).add_modifier(b)),
                Span::styled(cmd.desc, Style::default().fg(Color::Gray)),
            ]));
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "/quit = salir    Esc/q = cerrar    [/] = nuevo comando",
        Style::default().fg(Color::DarkGray))));

    f.render_widget(
        Paragraph::new(lines),
        area,
    );
}

// ═══════════════════════════════════════════════════════════════════
// FOOTER — COMMAND LEGEND
// ═══════════════════════════════════════════════════════════════════

fn draw_footer(f: &mut Frame, area: Rect, s: &State) {
    let sl_info = if s.sl_pct > 0.0 {
        format!("{:.0}%{}", s.sl_pct, if s.sl_market {" MKT"}else{" LMT"})
    } else { "OFF".into() };
    let tsl_info = if s.mt_tsl_pct > 0.0 { format!("{}%", s.mt_tsl_pct) } else { "OFF".into() };

    let lines = vec![
        Line::from(vec![
            Span::styled(" /b10up65e70  ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled("BUY+exit       ", Style::default().fg(Color::Gray)),
            Span::styled("/b10up65e70s50", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(" bracket       ", Style::default().fg(Color::Gray)),
            Span::styled("/k", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" cancel  ", Style::default().fg(Color::Gray)),
            Span::styled("/x", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::styled(" exit mkt  ", Style::default().fg(Color::Gray)),
            Span::styled("/u", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(" undo", Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled(format!(" SL:{sl_info} ", sl_info = sl_info), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("TSL:{tsl_info} ", tsl_info = tsl_info), Style::default().fg(Color::DarkGray)),
            Span::styled(" /sl /sl10 /nsl  ", Style::default().fg(Color::DarkGray)),
            Span::styled("/tsl5 /ntsl  ", Style::default().fg(Color::DarkGray)),
            Span::styled("/alert up 0.70  ", Style::default().fg(Color::DarkGray)),
            Span::styled("/co /p /pos /man", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled(" /5g70 /5g70e80  ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::styled("Gemini trigger  ", Style::default().fg(Color::DarkGray)),
            Span::styled(" [/]comando  [Tab]vista  [Esc/q]salir", Style::default().fg(Color::Rgb(30, 40, 55))),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), area);
}
