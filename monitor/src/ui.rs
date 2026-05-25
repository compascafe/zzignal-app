use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use std::collections::VecDeque;

use crate::InputMode;
use crate::State;

const TAB_NAMES: &[&str] = &["DINERO REAL", "PAPER MONEY", "GRAFICOS", "MICRO", "OFI"];

// ─── Bloomberg Terminal Palette ────────────────────────────────────
const BB_BG:       Color = Color::Reset;           // terminal default dark
const BB_CARD:     Color = Color::Rgb(10, 14, 22); // subtle card bg
const BB_AMBER:    Color = Color::Rgb(255, 179, 0);
const BB_AMBER_DIM:Color = Color::Rgb(180, 130, 30);
const BB_GREEN:    Color = Color::Rgb(0, 210, 90);
const BB_RED:      Color = Color::Rgb(255, 65, 65);
const BB_WHITE:    Color = Color::White;
const BB_GRAY:     Color = Color::Rgb(120, 135, 155);
const BB_DIM:      Color = Color::Rgb(60, 68, 80);
const BB_BORDER:   Color = Color::Rgb(35, 42, 55);
const BB_CYAN:     Color = Color::Rgb(0, 200, 220);
const BB_MAGENTA:  Color = Color::Rgb(210, 80, 255);

pub fn draw(f: &mut Frame, s: &State) {
    let area = f.area();

    if s.show_man {
        draw_man_page(f, area, s);
        return;
    }

    let pos_h = 3;
    let cmd_h = if s.input_mode == InputMode::Command { 3 } else { 0 };
    let gem_h = if s.gemini_active || s.gemini_triggered || s.mt_state >= 1 { 2 } else { 0 };

    let mut constraints = vec![
        Constraint::Length(1),     // commit bar
        Constraint::Length(1),     // tab bar
        Constraint::Length(pos_h), // position bar
        Constraint::Min(4),        // dashboard
    ];
    if gem_h > 0 { constraints.push(Constraint::Length(gem_h)); }
    if cmd_h > 0 { constraints.push(Constraint::Length(cmd_h)); }
    constraints.push(Constraint::Length(10)); // footer: ayuda

    let chunks = Layout::default().direction(Direction::Vertical).constraints(constraints).split(area);
    let mut ci = 0;

    // ─── COMMIT BAR ────────────────────────────────────────────────────
    let commit = option_env!("GIT_HASH").unwrap_or("dev");
    let total_w = area.width as usize;

    // Aggregate alert level: worst of all indicators
    let (alert_color, alert_blink) = aggregate_alert(s);

    let pulse = s.pulse_tick;
    let dot = if alert_blink && pulse % 8 < 5 { "●" } else if alert_blink { "○" } else { "●" };
    let dot_span = Span::styled(format!("{dot} "), Style::default().fg(alert_color).add_modifier(Modifier::BOLD));

    let filler_w = total_w.saturating_sub(commit.len() + 22);
    let filler = " ".repeat(filler_w.min(80));
    f.render_widget(
        Paragraph::new(Line::from(vec![
            dot_span,
            Span::styled(format!("|ZZIGNAL{filler}{commit}|"),
                Style::default().fg(BB_AMBER).add_modifier(Modifier::BOLD)),
        ])),
        chunks[ci],
    );
    ci += 1;

    // ─── TAB BAR ──────────────────────────────────────────────────────
    let tab_spans: Vec<Span> = TAB_NAMES.iter().enumerate().flat_map(|(i, name)| {
        let (fg, bg) = if i == s.tab {
            match i {
                0 => (BB_WHITE, BB_RED),
                1 => (BB_WHITE, BB_GREEN),
                2 => (BB_WHITE, BB_CYAN),
                3 => (BB_WHITE, BB_MAGENTA),
                _ => (BB_WHITE, BB_AMBER),
            }
        } else {
            (BB_GRAY, Color::Reset)
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
    } else if s.tab == 3 {
        draw_microestructura(f, chunks[ci], s);
    } else if s.tab == 4 {
        draw_ofi(f, chunks[ci], s);
    } else {
        draw_dashboard(f, chunks[ci], s);
    }
    ci += 1;

    // ─── GEMINI CARD ───────────────────────────────────────────────────
    if gem_h > 0 {
        draw_gemini_card(f, chunks[ci], s);
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
// AGGREGATE ALERT — status light for commit bar
// ═══════════════════════════════════════════════════════════════════

fn aggregate_alert(s: &State) -> (Color, bool) {
    // Check all indicator thresholds — worst wins (RED > YELLOW > GREEN)
    let spread = s.hft.spread;
    let dump = s.hft.dump_score;
    let gap = s.hft.tick_gap_ms;
    let spoof_risk = s.hft.spoof + s.hft.ask_wall;

    // RED conditions
    if spread >= 0.008 || dump >= 3 || gap >= 2000 || spoof_risk >= 2 {
        return (BB_RED, true); // blink fast
    }

    // YELLOW conditions
    if spread >= 0.003 || dump >= 1 || gap >= 500 || spoof_risk >= 1 {
        return (BB_AMBER, true); // blink medium
    }

    // Check aligned signal (S1/S2)
    let up_ref = if s.session_open_up > 0.0 { s.session_open_up } else { s.hft.clob_trade_up };
    let dn_ref = if s.session_open_dn > 0.0 { s.session_open_dn } else { s.hft.clob_trade_dn };
    let btc_o = if s.btc_open > 0.0 { s.btc_open } else if s.session_open_btc > 0.0 { s.session_open_btc } else { s.btc };
    let up_d = if up_ref > 0.0 { (s.hft.clob_trade_up / up_ref - 1.0) * 100.0 } else { 0.0 };
    let dn_d = if dn_ref > 0.0 { (s.hft.clob_trade_dn / dn_ref - 1.0) * 100.0 } else { 0.0 };
    let btc_d = if btc_o > 0.0 { (s.btc / btc_o - 1.0) * 100.0 } else { 0.0 };
    let clob_moved = up_d.abs().max(dn_d.abs()) > 0.05;
    let clob_up = up_d >= 0.0;
    let btc_up = btc_d >= 0.0;
    let aligned = (clob_up == btc_up) && clob_moved && btc_d.abs() > 0.01;

    if aligned {
        return (BB_GREEN, true); // blink slow — good signal
    }

    (BB_DIM, false) // everything quiet
}

// ═══════════════════════════════════════════════════════════════════
// TAB 0/1: DASHBOARD
// ═══════════════════════════════════════════════════════════════════

fn draw_dashboard(f: &mut Frame, area: Rect, s: &State) {
    let constraints = vec![
        Constraint::Length(4),     // market info
        Constraint::Length(4),     // UP/DOWN price cards
        Constraint::Length(5),     // indicators row 1 (S1..S6)
        Constraint::Length(5),     // indicators row 2 (S7..S12)
        Constraint::Length(3),     // manual trading status
        Constraint::Length(13),    // orderbook depth
        Constraint::Min(3),        // orders + positions + trade log
    ];

    let m = Layout::default().direction(Direction::Vertical).constraints(constraints).split(area);
    let mut idx: usize = 0;

    draw_market_info(f, m[idx], s); idx += 1;
    draw_price_cards(f, m[idx], s); idx += 1;
    draw_indicators(f, m[idx], s); idx += 1;
    draw_indicators_row2(f, m[idx], s); idx += 1;
    draw_manual_status(f, m[idx], s); idx += 1;
    draw_depth_panel(f, m[idx], s); idx += 1;

    let bottom = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)])
        .split(m[idx]);
    draw_positions_card(f, bottom[0], s);
    draw_trade_log_inline(f, bottom[1], s);
}

// ─── MARKET INFO BAR ──────────────────────────────────────────────

fn draw_market_info(f: &mut Frame, area: Rect, s: &State) {
    let cols = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,4); 4]).split(area);
    let big = Modifier::BOLD;

    let btc_ref = if s.btc_open > 0.0 { s.btc_open }
        else if s.session_open_btc > 0.0 { s.session_open_btc }
        else { s.btc };
    let btc_delta = s.btc - btc_ref;
    let btc_delta_pct = if btc_ref > 0.0 { (s.btc / btc_ref - 1.0) * 100.0 } else { 0.0 };
    let btc_up = btc_delta >= 0.0;
    let btc_c = if btc_up { BB_GREEN } else { BB_RED };
    let arrow = if btc_up { "▲" } else { "▼" };

    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![Span::styled(format!("${:.0}", s.btc),
                Style::default().fg(BB_WHITE).add_modifier(big))]),
            Line::from(Span::styled(format!("abrio ${:.0}", btc_ref),
                Style::default().fg(BB_GRAY))),
            Line::from(Span::styled(s.btc_provider.to_uppercase(),
                Style::default().fg(BB_DIM))),
        ]).block(Block::default().borders(Borders::ALL).title("BTC")
            .border_style(Style::default().fg(btc_c))
            .style(Style::default().bg(BB_CARD))),
        cols[0]);

    let delta_pulse = s.pulse_tick % 10 < 7;
    let delta_fg = if delta_pulse { BB_WHITE } else { btc_c };

    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(format!("{arrow} "), Style::default().fg(delta_fg).add_modifier(big)),
                Span::styled(format!("${:+.0}", btc_delta), Style::default().fg(delta_fg).add_modifier(big)),
            ]),
            Line::from(Span::styled(format!("{:+.1}%", btc_delta_pct),
                Style::default().fg(delta_fg).add_modifier(big))),
            Line::from(Span::styled(if btc_up {"▲ UP"} else {"▼ DN"},
                Style::default().fg(delta_fg))),
        ]).block(Block::default().borders(Borders::ALL).title("BTC Δ")
            .border_style(Style::default().fg(btc_c))
            .style(Style::default().bg(BB_CARD))),
        cols[1]);

    let sl = s.hft.secs_left;
    let min = sl / 60; let sec = sl % 60;
    let sl_c = if sl > 300 { BB_GREEN } else if sl > 60 { BB_AMBER } else if sl > 0 { BB_RED } else { BB_DIM };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(format!("{}:{:02}", min, sec),
                Style::default().fg(sl_c).add_modifier(big))),
            Line::from(Span::styled(if sl > 0 { "restantes" } else { "FINALIZADA" },
                Style::default().fg(sl_c))),
            Line::from(Span::styled("CUENTA REGRESIVA", Style::default().fg(BB_DIM))),
        ]).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(sl_c))
            .style(Style::default().bg(BB_CARD))),
        cols[2]);

    let bal_c = if s.bal > 100.0 { BB_GREEN } else if s.bal > 50.0 { BB_AMBER } else { BB_RED };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(format!("${:.2}", s.bal),
                Style::default().fg(BB_WHITE).add_modifier(big))),
            Line::from(Span::styled(format!("ordenes: {}", s.orders),
                Style::default().fg(if s.orders>0{BB_AMBER}else{BB_DIM}))),
            Line::from(Span::styled("BALANCE", Style::default().fg(bal_c))),
        ]).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(bal_c))
            .style(Style::default().bg(BB_CARD))),
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
    let up_c = if up_diff > 0.0 { BB_GREEN } else if up_diff < 0.0 { BB_RED } else { BB_AMBER };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("▲ UP  ", Style::default().fg(BB_GREEN).add_modifier(big)),
                Span::styled(format!("{:.4}", up_px), Style::default().fg(BB_WHITE).add_modifier(big)),
                Span::styled(format!(" {:+.2}%", up_diff), Style::default().fg(up_c).add_modifier(big)),
            ]),
            Line::from(Span::styled(format!("init {:.4}  vol {:.0}  {}", up_init, s.hft.clob_trade_up_vol,
                if s.pos_sen_up||s.pos_h65_up||s.pos_odi_up {"▶ POS"}else{"—"}),
                Style::default().fg(BB_DIM))),
        ]).block(Block::default().borders(Borders::ALL)
            .border_style(Style::default().fg(BB_GREEN))
            .style(Style::default().bg(BB_CARD))),
        cols[0]);

    let dn_px = s.hft.clob_trade_dn;
    let dn_init = if s.session_open_dn > 0.0 { s.session_open_dn } else { dn_px };
    let dn_diff = if dn_init > 0.0 { (dn_px - dn_init) / dn_init * 100.0 } else { 0.0 };
    let dn_c = if dn_diff > 0.0 { BB_GREEN } else if dn_diff < 0.0 { BB_RED } else { BB_AMBER };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("▼ DN  ", Style::default().fg(BB_RED).add_modifier(big)),
                Span::styled(format!("{:.4}", dn_px), Style::default().fg(BB_WHITE).add_modifier(big)),
                Span::styled(format!("  {:+.2}%", dn_diff), Style::default().fg(dn_c).add_modifier(big)),
            ]),
            Line::from(Span::styled(format!("INIT {:.4}  |  vol {:.0}", dn_init, s.hft.clob_trade_dn_vol),
                Style::default().fg(BB_DIM))),
            Line::from(Span::styled(if s.pos_sen_dn || s.pos_h65_dn || s.pos_odi_dn {
                format!("▶ POSICION ABIERTA")
            } else { "— sin posicion".into() },
                Style::default().fg(if s.pos_sen_dn||s.pos_h65_dn||s.pos_odi_dn {BB_RED}else{BB_DIM}))),
        ]).block(Block::default().borders(Borders::ALL)
            .border_style(Style::default().fg(BB_RED))
            .style(Style::default().bg(BB_CARD))),
        cols[1]);
}

// ─── INDICATORS — 6 cards ────────────────────────────────────

fn draw_indicators(f: &mut Frame, area: Rect, s: &State) {
    let cols = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,6); 6]).split(area);
    let b = Modifier::BOLD;

    let up_ref = if s.session_open_up > 0.0 { s.session_open_up } else { s.hft.clob_trade_up };
    let dn_ref = if s.session_open_dn > 0.0 { s.session_open_dn } else { s.hft.clob_trade_dn };
    let up_d = if up_ref > 0.0 { (s.hft.clob_trade_up / up_ref - 1.0) * 100.0 } else { 0.0 };
    let dn_d = if dn_ref > 0.0 { (s.hft.clob_trade_dn / dn_ref - 1.0) * 100.0 } else { 0.0 };
    let clob_up = up_d >= 0.0;
    let btc_o = if s.btc_open > 0.0 { s.btc_open } else { s.session_open_btc };
    let btc_d = if btc_o > 0.0 { (s.btc / btc_o - 1.0) * 100.0 } else { 0.0 };
    let btc_up = btc_d >= 0.0;
    let up_30s = if s.clob_up_30s > 0.0 { (s.hft.clob_trade_up / s.clob_up_30s - 1.0) * 100.0 } else { 0.0 };
    let dn_30s = if s.clob_dn_30s > 0.0 { (s.hft.clob_trade_dn / s.clob_dn_30s - 1.0) * 100.0 } else { 0.0 };
    let btc_30s = if s.btc_price_30s > 0.0 { (s.btc / s.btc_price_30s - 1.0) * 100.0 } else { 0.0 };
    let clob_moved = up_d.abs().max(dn_d.abs()) > 0.05;
    let aligned = (clob_up == btc_up) && clob_moved && btc_d.abs() > 0.01;
    let pulse_on = aligned && (s.pulse_tick % 12) < 8;

    // ── S1: CLOB MOM ──
    let s1_border = if aligned { if btc_up { BB_GREEN } else { BB_RED } } else { BB_BORDER };
    let s1_border_alert = if aligned && pulse_on { s1_border } else { BB_BORDER };
    let _s1_fg = if aligned { BB_WHITE } else { if btc_up { BB_GREEN } else { BB_RED } };
    let clob_dir = if up_d.abs() > dn_d.abs() {
        if clob_up { "▲UP" } else { "▼UP" }
    } else {
        if dn_d >= 0.0 { "▲DN" } else { "▼DN" }
    };
    let up_30s_c = if up_30s > 0.0 { BB_GREEN } else if up_30s < 0.0 { BB_RED } else { BB_DIM };
    let dn_30s_c = if dn_30s > 0.0 { BB_GREEN } else if dn_30s < 0.0 { BB_RED } else { BB_DIM };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("● ", Style::default().fg(s1_border_alert).add_modifier(b)),
                Span::styled("CLOB MOM", Style::default().fg(BB_AMBER).add_modifier(b)),
            ]),
            Line::from(vec![
                Span::styled(format!("{clob_dir} "), Style::default().fg(s1_border).add_modifier(b)),
                Span::styled(format!("ses UP{up_d:+.1}/DN{dn_d:+.1}%"), Style::default().fg(BB_WHITE)),
            ]),
            Line::from(vec![
                Span::styled("30s ", Style::default().fg(BB_DIM)),
                Span::styled(format!("UP{up_30s:+.1}"), Style::default().fg(up_30s_c)),
                Span::styled(format!("/DN{dn_30s:+.1}% "), Style::default().fg(dn_30s_c)),
                Span::styled(if aligned&&pulse_on {"✓ BTC ✓"}else{"—"},
                    Style::default().fg(if aligned&&pulse_on{BB_GREEN}else{BB_DIM}).add_modifier(b)),
            ]),
        ]).block(Block::default().borders(Borders::ALL).title("S1 CLOB")
            .border_style(Style::default().fg(s1_border_alert))
            .style(Style::default().bg(BB_CARD))),
        cols[0]);

    // ── S2: BTC MOM ──
    let s2_border = if aligned { if btc_up { BB_GREEN } else { BB_RED } } else { BB_BORDER };
    let s2_border_alert = if aligned && pulse_on { s2_border } else { BB_BORDER };
    let s2_fg = if aligned { BB_WHITE } else { if btc_up { BB_GREEN } else { BB_RED } };
    let btc_dir = if btc_d >= 0.0 { "▲BULL" } else { "▼BEAR" };
    let btc_30s_c = if btc_30s > 0.0 { BB_GREEN } else if btc_30s < 0.0 { BB_RED } else { BB_DIM };
    let clob_max_d = up_d.abs().max(dn_d.abs());
    let btc_lead = if btc_d.abs() > clob_max_d && btc_d.abs() > 0.05 {
        let lead = btc_d.abs() - clob_max_d;
        (lead > 0.0, lead)
    } else { (false, 0.0) };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("● ", Style::default().fg(s2_border_alert).add_modifier(b)),
                Span::styled("BTC MOM", Style::default().fg(BB_AMBER).add_modifier(b)),
            ]),
            Line::from(vec![
                Span::styled(format!("${:.0} ", s.btc), Style::default().fg(BB_WHITE)),
                Span::styled(format!("ses {btc_d:+.1}%"), Style::default().fg(if btc_up { BB_GREEN } else { BB_RED })),
            ]),
            Line::from(vec![
                Span::styled(format!("{btc_dir} 30s"), Style::default().fg(s2_fg).add_modifier(b)),
                Span::styled(format!("{btc_30s:+.1}%"), Style::default().fg(btc_30s_c)),
                if btc_lead.0 {
                    Span::styled(format!(" →CLOB+{:.1}%", btc_lead.1), Style::default().fg(BB_CYAN).add_modifier(b))
                } else { Span::styled("", Style::default()) },
            ]),
        ]).block(Block::default().borders(Borders::ALL).title("S2 BTC")
            .border_style(Style::default().fg(s2_border))
            .style(Style::default().bg(BB_CARD))),
        cols[1]);

    // ── S3: BTC Δ vs SESSION OPEN ──
    let btc_ref = if s.btc_open > 0.0 { s.btc_open }
        else if s.session_open_btc > 0.0 { s.session_open_btc }
        else { s.btc };
    let delta = s.btc - btc_ref;
    let delta_pct = if btc_ref > 0.0 { (s.btc / btc_ref - 1.0) * 100.0 } else { 0.0 };
    let total_secs = 900;
    let elapsed = if s.hft.secs_left >= 0 { (total_secs - s.hft.secs_left).max(0) } else { 0 };
    let elapsed_min = elapsed as f64 / 60.0;
    let (s3_border, s3_fg) = if elapsed_min <= 7.0 {
        (BB_AMBER, BB_AMBER)
    } else if elapsed_min <= 14.0 {
        if delta > 50.0 { (BB_GREEN, BB_GREEN) }
        else if delta >= -50.0 { (BB_AMBER, BB_AMBER) }
        else { (BB_RED, BB_RED) }
    } else {
        (BB_AMBER, BB_AMBER)
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("● ", Style::default().fg(s3_border).add_modifier(b)),
                Span::styled("BTC Δ OPEN", Style::default().fg(BB_AMBER).add_modifier(b)),
            ]),
            Line::from(Span::styled(format!("${:+.0}  {:+.1}%", delta, delta_pct),
                Style::default().fg(s3_fg).add_modifier(b))),
            Line::from(Span::styled(format!("min {:.0}/15  ref ${:.0}", elapsed_min, btc_ref),
                Style::default().fg(BB_DIM))),
        ]).block(Block::default().borders(Borders::ALL).title(format!("S3 Δ [{:.0}m]", elapsed_min))
            .border_style(Style::default().fg(s3_border))
            .style(Style::default().bg(BB_CARD))),
        cols[2]);

    // ── S4: VELOCIDAD + VOLUMEN ──
    let vel = s.btc_velocity;
    let acel = s.btc_acceleration;
    let vel_dir = if vel >= 0.0 { "▲" } else { "▼" };
    let acel_dir = if acel >= 0.0 { "▲" } else { "▼" };
    let vel_color = if vel > 0.5 { BB_GREEN } else if vel < -0.5 { BB_RED } else { BB_AMBER };
    let vol_1m = s.btc_vol_1m;
    let vol_ses = s.hft.btc_vol_ses;
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("● ", Style::default().fg(vel_color).add_modifier(b)),
                Span::styled("VEL+VOL", Style::default().fg(BB_AMBER).add_modifier(b)),
            ]),
            Line::from(vec![
                Span::styled(format!("{vel_dir} "), Style::default().fg(vel_color).add_modifier(b)),
                Span::styled(format!("${vel:+.2}/s  "), Style::default().fg(vel_color)),
                Span::styled(format!("{acel_dir} ${acel:+.2}/s²"), Style::default().fg(BB_MAGENTA)),
            ]),
            Line::from(Span::styled(format!("1m {vol_1m:.0} BTC"),
                Style::default().fg(if vol_1m > 25.0 { BB_GREEN } else if vol_1m > 10.0 { BB_AMBER } else { BB_DIM }))),
            Line::from(Span::styled(format!("ses {vol_ses:.0} BTC"),
                Style::default().fg(if vol_ses > 200.0 { BB_GREEN } else if vol_ses > 50.0 { BB_CYAN } else { BB_DIM }))),
        ]).block(Block::default().borders(Borders::ALL).title("S4 V+V")
            .border_style(Style::default().fg(BB_BORDER))
            .style(Style::default().bg(BB_CARD))),
        cols[3]);

    // ── S5: ORDER BOOK IMBALANCE ──
    let up_bid_v: f64 = s.hft.depth_up_bids.iter().map(|(_,s)| s).sum();
    let up_ask_v: f64 = s.hft.depth_up_asks.iter().map(|(_,s)| s).sum();
    let dn_bid_v: f64 = s.hft.depth_dn_bids.iter().map(|(_,s)| s).sum();
    let dn_ask_v: f64 = s.hft.depth_dn_asks.iter().map(|(_,s)| s).sum();
    let up_imb = if up_ask_v > 0.0 { up_bid_v / up_ask_v } else { 1.0 };
    let dn_imb = if dn_ask_v > 0.0 { dn_bid_v / dn_ask_v } else { 1.0 };
    let total_bull = up_bid_v + dn_ask_v;
    let total_bear = up_ask_v + dn_bid_v;
    let comb_imb = if total_bear > 0.0 { total_bull / total_bear } else { 1.0 };
    let (imb_border, imb_label) = if comb_imb > 1.15 {
        (BB_GREEN, "▲ UP")
    } else if comb_imb < 0.85 {
        (BB_RED, "▼ DN")
    } else {
        (BB_AMBER, "—")
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("● ", Style::default().fg(imb_border).add_modifier(b)),
                Span::styled("IMBALANCE", Style::default().fg(BB_AMBER).add_modifier(b)),
            ]),
            Line::from(Span::styled(format!("UP {up_imb:.2}x"),
                Style::default().fg(if up_imb>1.1{BB_GREEN}else if up_imb<0.9{BB_RED}else{BB_AMBER}).add_modifier(b))),
            Line::from(Span::styled(format!("{imb_label}  DN {dn_imb:.2}x"),
                Style::default().fg(BB_WHITE).add_modifier(b))),
        ]).block(Block::default().borders(Borders::ALL).title("S5 IMB")
            .border_style(Style::default().fg(imb_border))
            .style(Style::default().bg(BB_CARD))),
        cols[4]);

    // ── S6: WALL ──
    let all_vols: Vec<f64> = s.hft.depth_up_bids.iter().map(|(_, s)| *s)
        .chain(s.hft.depth_up_asks.iter().map(|(_, s)| *s))
        .chain(s.hft.depth_dn_bids.iter().map(|(_, s)| *s))
        .chain(s.hft.depth_dn_asks.iter().map(|(_, s)| *s))
        .collect();
    let (wall_score, wall_side) = if !all_vols.is_empty() {
        let avg = all_vols.iter().sum::<f64>() / all_vols.len() as f64;
        if avg > 0.0 {
            let (max_v, max_i) = all_vols.iter().enumerate()
                .fold((0.0f64, 0usize), |(m, mi), (i, &v)| if v > m { (v, i) } else { (m, mi) });
            let n30 = 30.min(s.hft.depth_up_bids.len());
            let side = if max_i < n30 { "UP_BID" }
                else if max_i < n30*2 { "UP_ASK" }
                else if max_i < n30*3 { "DN_BID" }
                else { "DN_ASK" };
            (max_v / avg, side.to_string())
        } else { (0.0, String::new()) }
    } else { (0.0, String::new()) };
    let w_color = if wall_score > 5.0 { BB_RED } else if wall_score > 2.5 { BB_AMBER } else { BB_GREEN };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("● ", Style::default().fg(w_color).add_modifier(b)),
                Span::styled("WALL", Style::default().fg(BB_CYAN).add_modifier(b)),
            ]),
            Line::from(Span::styled(format!("{wall_side} {wall_score:.1}×"),
                Style::default().fg(w_color).add_modifier(b))),
            Line::from(vec![
                Span::styled(format!("ASK⚡{}", if s.hft.ask_wall>0{"!"}else{""}), Style::default().fg(if s.hft.ask_wall>0{BB_RED}else{BB_DIM})),
                Span::styled(format!(" SPF{}", if s.hft.spoof>0{"!"}else{""}), Style::default().fg(if s.hft.spoof>0{BB_RED}else{BB_DIM})),
            ]),
        ]).block(Block::default().borders(Borders::ALL).title("S6 WALL")
            .border_style(Style::default().fg(w_color))
            .style(Style::default().bg(BB_CARD))),
        cols[5]);
}

// ─── INDICATORS — 6 cards row 2: DEPTH PROFILE ────────────

fn draw_indicators_row2(f: &mut Frame, area: Rect, s: &State) {
    let cols = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,6); 6]).split(area);
    let b = Modifier::BOLD;

    let cross_book = |n: usize| -> f64 {
        let up_bid: f64 = s.hft.depth_up_bids.iter().take(n).map(|(_, sz)| sz).sum();
        let up_ask: f64 = s.hft.depth_up_asks.iter().take(n).map(|(_, sz)| sz).sum();
        let dn_bid: f64 = s.hft.depth_dn_bids.iter().take(n).map(|(_, sz)| sz).sum();
        let dn_ask: f64 = s.hft.depth_dn_asks.iter().take(n).map(|(_, sz)| sz).sum();
        let bull = up_bid + dn_ask;
        let bear = up_ask + dn_bid;
        if bear > 0.0 { bull / bear } else { 1.0 }
    };

    let d10 = cross_book(10);
    let d20 = cross_book(20);
    let d30 = cross_book(30);
    let grad = d30 - d10;

    // Velocity: Δd10 / Δt (from imb_history)
    let vel = if s.imb_history.len() >= 2 {
        let now = *s.imb_history.back().unwrap_or(&d10);
        let prev = s.imb_history.iter().rev().nth(1).copied().unwrap_or(now);
        (now - prev) / 0.5_f64.max(1.0)  // approx 500ms between ticks
    } else { 0.0 };
    let accel = if s.imb_history.len() >= 3 {
        let now_v = vel;
        let v1 = if s.imb_history.len() >= 2 {
            let a = *s.imb_history.back().unwrap_or(&d10);
            let b = s.imb_history.iter().rev().nth(1).copied().unwrap_or(a);
            (a - b) / 0.5_f64.max(1.0)
        } else { 0.0 };
        (now_v - v1) / 0.5_f64.max(1.0)
    } else { 0.0 };

    // ── D10 ──
    let c10 = if d10 > 1.15 { BB_GREEN } else if d10 < 0.85 { BB_RED } else { BB_AMBER };
    let l10 = if d10 > 1.15 { "▲BULL" } else if d10 < 0.85 { "▼BEAR" } else { "—FLAT" };
    f.render_widget(Paragraph::new(vec![
        Line::from(vec![Span::styled("● ", Style::default().fg(c10).add_modifier(b)),
            Span::styled("IMB D10", Style::default().fg(BB_AMBER).add_modifier(b))]),
        Line::from(Span::styled(format!("{l10} {d10:.2}x"), Style::default().fg(c10).add_modifier(b))),
        Line::from(Span::styled("10 levels", Style::default().fg(BB_DIM))),
    ]).block(Block::default().borders(Borders::ALL).title("D10")
        .border_style(Style::default().fg(c10)).style(Style::default().bg(BB_CARD))), cols[0]);

    // ── D20 ──
    let c20 = if d20 > 1.15 { BB_GREEN } else if d20 < 0.85 { BB_RED } else { BB_AMBER };
    let l20 = if d20 > 1.15 { "▲BULL" } else if d20 < 0.85 { "▼BEAR" } else { "—FLAT" };
    f.render_widget(Paragraph::new(vec![
        Line::from(vec![Span::styled("● ", Style::default().fg(c20).add_modifier(b)),
            Span::styled("IMB D20", Style::default().fg(BB_AMBER).add_modifier(b))]),
        Line::from(Span::styled(format!("{l20} {d20:.2}x"), Style::default().fg(c20).add_modifier(b))),
        Line::from(Span::styled("20 levels", Style::default().fg(BB_DIM))),
    ]).block(Block::default().borders(Borders::ALL).title("D20")
        .border_style(Style::default().fg(c20)).style(Style::default().bg(BB_CARD))), cols[1]);

    // ── D30 ──
    let c30 = if d30 > 1.15 { BB_GREEN } else if d30 < 0.85 { BB_RED } else { BB_AMBER };
    let l30 = if d30 > 1.15 { "▲BULL" } else if d30 < 0.85 { "▼BEAR" } else { "—FLAT" };
    f.render_widget(Paragraph::new(vec![
        Line::from(vec![Span::styled("● ", Style::default().fg(c30).add_modifier(b)),
            Span::styled("IMB D30", Style::default().fg(BB_AMBER).add_modifier(b))]),
        Line::from(Span::styled(format!("{l30} {d30:.2}x"), Style::default().fg(c30).add_modifier(b))),
        Line::from(Span::styled("30 levels", Style::default().fg(BB_DIM))),
    ]).block(Block::default().borders(Borders::ALL).title("D30")
        .border_style(Style::default().fg(c30)).style(Style::default().bg(BB_CARD))), cols[2]);

    // ── GRAD ──
    let gc = if grad > 0.05 { BB_GREEN } else if grad < -0.05 { BB_RED } else { BB_AMBER };
    let gl = if grad > 0.05 { "▲DEEP BULL" } else if grad < -0.05 { "▼BEAR TRAP" } else { "—FLAT" };
    f.render_widget(Paragraph::new(vec![
        Line::from(vec![Span::styled("● ", Style::default().fg(gc).add_modifier(b)),
            Span::styled("GRAD", Style::default().fg(BB_CYAN).add_modifier(b))]),
        Line::from(Span::styled(format!("{gl}"), Style::default().fg(gc).add_modifier(b))),
        Line::from(Span::styled(format!("D30−D10={grad:+.2}"), Style::default().fg(gc))),
    ]).block(Block::default().borders(Borders::ALL).title("GRAD")
        .border_style(Style::default().fg(gc)).style(Style::default().bg(BB_CARD))), cols[3]);

    // ── VEL ──
    let vc = if vel > 0.02 { BB_GREEN } else if vel < -0.02 { BB_RED } else { BB_AMBER };
    let vl = if vel > 0.02 { "▲BULL" } else if vel < -0.02 { "▼BEAR" } else { "—" };
    f.render_widget(Paragraph::new(vec![
        Line::from(vec![Span::styled("● ", Style::default().fg(vc).add_modifier(b)),
            Span::styled("VEL", Style::default().fg(BB_MAGENTA).add_modifier(b))]),
        Line::from(Span::styled(format!("{vl} {vel:+.3}/s"), Style::default().fg(vc).add_modifier(b))),
        Line::from(Span::styled("Δ imb / Δt", Style::default().fg(BB_DIM))),
    ]).block(Block::default().borders(Borders::ALL).title("VEL")
        .border_style(Style::default().fg(vc)).style(Style::default().bg(BB_CARD))), cols[4]);

    // ── ACCEL ──
    let ac = if accel > 0.01 { BB_GREEN } else if accel < -0.01 { BB_RED } else { BB_AMBER };
    let al = if accel > 0.01 { "▲BUILDING" } else if accel < -0.01 { "▼FADING" } else { "—" };
    f.render_widget(Paragraph::new(vec![
        Line::from(vec![Span::styled("● ", Style::default().fg(ac).add_modifier(b)),
            Span::styled("ACCEL", Style::default().fg(BB_CYAN).add_modifier(b))]),
        Line::from(Span::styled(format!("{al} {accel:+.3}/s²"), Style::default().fg(ac).add_modifier(b))),
        Line::from(Span::styled("Δ vel / Δt", Style::default().fg(BB_DIM))),
    ]).block(Block::default().borders(Borders::ALL).title("ACCEL")
        .border_style(Style::default().fg(ac)).style(Style::default().bg(BB_CARD))), cols[5]);
}

// ─── MANUAL TRADING STATUS BAR ────────────────────────────────────

fn draw_manual_status(f: &mut Frame, area: Rect, s: &State) {
    let bb = Modifier::BOLD;
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled("MANUAL: ", Style::default().fg(BB_DIM).add_modifier(bb)));

    match s.mt_state {
        0 => {
            spans.push(Span::styled("IDLE", Style::default().fg(BB_DIM)));
            spans.push(Span::styled("  0 posiciones", Style::default().fg(BB_DIM)));
        }
        1 => {
            spans.push(Span::styled("PENDING ", Style::default().fg(BB_AMBER).add_modifier(bb)));
            let outcome_c = if s.mt_outcome == "up" { BB_GREEN } else { BB_RED };
            spans.push(Span::styled(format!("{} ", s.mt_outcome.to_uppercase()), Style::default().fg(outcome_c).add_modifier(bb)));
            spans.push(Span::styled(format!("BUY @{:.4} sz={:.0} ${:.2}", s.mt_entry, s.mt_size, s.mt_budget),
                Style::default().fg(BB_WHITE)));
            if s.mt_last_fill_pct > 0.0 {
                spans.push(Span::styled(format!("  [{:.0}% filled]", s.mt_last_fill_pct), Style::default().fg(BB_AMBER)));
            }
            spans.push(Span::styled("  /c=CANCELAR", Style::default().fg(BB_DIM)));
        }
        2 => {
            spans.push(Span::styled("ACTIVE ", Style::default().fg(BB_GREEN).add_modifier(bb)));
            let outcome_c = if s.mt_outcome == "up" { BB_GREEN } else { BB_RED };
            spans.push(Span::styled(format!("{} ", s.mt_outcome.to_uppercase()), Style::default().fg(outcome_c).add_modifier(bb)));
            let current_px = if s.mt_outcome == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn };
            spans.push(Span::styled(format!("entry:{:.4}→{:.4} ", s.mt_entry, current_px), Style::default().fg(BB_WHITE)));
            spans.push(Span::styled("| /xuXX /x /c", Style::default().fg(BB_DIM)));
        }
        3 => {
            spans.push(Span::styled("EXITING ", Style::default().fg(BB_CYAN).add_modifier(bb)));
            let outcome_c = if s.mt_outcome == "up" { BB_GREEN } else { BB_RED };
            spans.push(Span::styled(format!("{} ", s.mt_outcome.to_uppercase()), Style::default().fg(outcome_c).add_modifier(bb)));
            let exit_px = if s.mt_exit_price > 0.0 { s.mt_exit_price } else {
                if s.mt_outcome == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn }
            };
            spans.push(Span::styled(format!("exit @{:.4}  sz={:.0}", exit_px, s.mt_size), Style::default().fg(BB_WHITE)));
        }
        _ => {}
    }

    let (border_c, title) = match s.mt_state {
        1 => (BB_AMBER, "TRADING — PENDING"),
        2 => (BB_GREEN, "TRADING — ACTIVE"),
        3 => (BB_CYAN, "TRADING — EXIT"),
        _ if s.mt_pnl_cum != 0.0 => {
            let c = if s.mt_pnl_cum >= 0.0 { BB_GREEN } else { BB_RED };
            (c, "TRADING — RESULTS")
        }
        _ => (BB_DIM, "TRADING"),
    };

    f.render_widget(
        Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::ALL)
            .title(title).border_style(Style::default().fg(border_c))),
        area);
}

// ─── POSITIONS & ORDERS CARD ──────────────────────────────────────

fn draw_positions_card(f: &mut Frame, area: Rect, s: &State) {
    let mut lines: Vec<Line> = Vec::new();
    let bb = Modifier::BOLD;

    if !s.open_orders.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("── ORDENES ({}) ──", s.open_orders.len()),
            Style::default().fg(BB_CYAN).add_modifier(bb))));
        for o in &s.open_orders {
            let outcome_c = if o.outcome == "up" { BB_GREEN } else { BB_RED };
            let side_c = if o.side == "buy" { BB_GREEN } else { BB_RED };
            let (fill_txt, fill_c) = if o.is_filled() { ("FILLED", BB_GREEN) }
                else if o.is_partial() { ("FILLING", BB_AMBER) }
                else { ("PENDING", BB_RED) };
            let pct = if o.size_orig > 0.0 { (o.size_matched / o.size_orig * 100.0) as i64 } else { 0 };
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", o.side.to_uppercase()), Style::default().fg(side_c).add_modifier(bb)),
                Span::styled(format!("{}  ", o.outcome.to_uppercase()), Style::default().fg(outcome_c).add_modifier(bb)),
                Span::styled(format!("@{:.4}  ", o.price), Style::default().fg(BB_WHITE)),
                Span::styled(format!("{:.0}/{:.0} [{pct}%]  {fill_txt}", o.size_matched, o.size_orig),
                    Style::default().fg(fill_c)),
            ]));
        }
        lines.push(Line::from(""));
    }

    let has_positions = s.pos_sen_up || s.pos_sen_dn || s.pos_h65_up
        || s.pos_h65_dn || s.pos_odi_up || s.pos_odi_dn;
    if has_positions {
        lines.push(Line::from(Span::styled("── POSICIONES STRATEGY ──",
            Style::default().fg(BB_AMBER).add_modifier(bb))));
    }

    if s.pos_sen_up {
        let entry = s.pos_sen_entry_up; let cur = s.hft.clob_trade_up;
        let pnl = if entry > 0.0 && cur > 0.0 { s.sen_budget * (cur / entry - 1.0) } else { 0.0 };
        let pc = if pnl >= 0.0 { BB_GREEN } else { BB_RED };
        lines.push(Line::from(vec![
            Span::styled("▲ SENNA UP  ", Style::default().fg(BB_GREEN).add_modifier(bb)),
            Span::styled(format!("entry:{:.4}→{:.4}  PnL {:+.2}", entry, cur, pnl),
                Style::default().fg(pc).add_modifier(bb)),
        ]));
    }
    if s.pos_sen_dn {
        let entry = s.pos_sen_entry_dn; let cur = s.hft.clob_trade_dn;
        let pnl = if entry > 0.0 && cur > 0.0 { s.sen_budget * (cur / entry - 1.0) } else { 0.0 };
        let pc = if pnl >= 0.0 { BB_GREEN } else { BB_RED };
        lines.push(Line::from(vec![
            Span::styled("▼ SENNA DN  ", Style::default().fg(BB_RED).add_modifier(bb)),
            Span::styled(format!("entry:{:.4}→{:.4}  PnL {:+.2}", entry, cur, pnl),
                Style::default().fg(pc).add_modifier(bb)),
        ]));
    }
    if s.pos_h65_up {
        lines.push(Line::from(Span::styled(format!("▲ H65 UP  entry:{:.4}  bid:{:.4}",
            s.pos_h65_entry_up, s.hft.clob_trade_up), Style::default().fg(BB_GREEN).add_modifier(bb))));
    }
    if s.pos_h65_dn {
        lines.push(Line::from(Span::styled(format!("▼ H65 DN  entry:{:.4}  bid:{:.4}",
            s.pos_h65_entry_dn, s.hft.clob_trade_dn), Style::default().fg(BB_RED).add_modifier(bb))));
    }
    if s.pos_odi_up {
        lines.push(Line::from(Span::styled(format!("▲ O83 UP  entry:{:.4}  bid:{:.4}",
            s.pos_odi_entry_up, s.hft.clob_trade_up), Style::default().fg(BB_GREEN))));
    }
    if s.pos_odi_dn {
        lines.push(Line::from(Span::styled(format!("▼ O83 DN  entry:{:.4}  bid:{:.4}",
            s.pos_odi_entry_dn, s.hft.clob_trade_dn), Style::default().fg(BB_RED))));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled("— sin posiciones activas", Style::default().fg(BB_DIM))));
        if s.sen_enabled && s.sen_budget > 0.0 {
            lines.push(Line::from(Span::styled(format!("SENNA ${:.0} esperando senal", s.sen_budget),
                Style::default().fg(BB_CYAN))));
        }
    }
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)
            .title("ORDENES / POSICIONES").border_style(Style::default().fg(BB_BORDER))),
        area);
}

fn draw_trade_log_inline(f: &mut Frame, area: Rect, s: &State) {
    let max_n = (area.height as usize).saturating_sub(2).min(30);
    let lines: Vec<Line> = s.trade_log.iter().take(max_n).map(|e| {
        Line::from(vec![
            Span::styled(format!("{} ", e.ts), Style::default().fg(BB_DIM)),
            Span::styled(&e.text, Style::default().fg(e.color)),
        ])
    }).collect();

    let title = if s.mt_pnl_cum != 0.0 {
        let c = if s.mt_pnl_cum >= 0.0 { "▲" } else { "▼" };
        format!("TRADE LOG {c} Σ{:+.2}  {}/{}W", s.mt_pnl_cum, s.mt_trades, s.mt_wins)
    } else { "TRADE LOG".into() };

    let border_c = if s.mt_state == 1 { BB_AMBER }
        else if s.mt_state == 2 { BB_GREEN }
        else if s.mt_state == 3 { BB_CYAN }
        else if s.mt_pnl_cum > 0.0 { BB_GREEN }
        else if s.mt_pnl_cum < 0.0 { BB_RED }
        else { BB_DIM };

    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)
            .title(title).border_style(Style::default().fg(border_c))),
        area);
}

// ═══════════════════════════════════════════════════════════════════
// ORDERBOOK DEPTH
// ═══════════════════════════════════════════════════════════════════

fn draw_depth_panel(f: &mut Frame, area: Rect, s: &State) {
    let chunks = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)])
        .split(area);

    draw_book_side(f, chunks[0], "UP", Color::Green, &s.book_up, &s.hft.depth_up_bids, &s.hft.depth_up_asks);
    draw_book_side(f, chunks[1], "DOWN", Color::Red, &s.book_dn, &s.hft.depth_dn_bids, &s.hft.depth_dn_asks);
}

fn draw_book_side(f: &mut Frame, area: Rect, label: &str, border_c: Color,
                   book: &crate::api::BookDepth, bids_fb: &[(f64,f64)], asks_fb: &[(f64,f64)]) {
    let mut bids: Vec<(f64,f64)> = if !book.bids.is_empty() {
        book.bids.iter().take(200).map(|l| (l.price, l.size)).collect()
    } else { bids_fb.iter().take(200).copied().collect() };
    let mut asks: Vec<(f64,f64)> = if !book.asks.is_empty() {
        book.asks.iter().take(200).map(|l| (l.price, l.size)).collect()
    } else { asks_fb.iter().take(200).copied().collect() };

    let best_bid = bids.iter().map(|&(p,_)| p).fold(f64::NEG_INFINITY, f64::max);
    let best_ask = asks.iter().map(|&(p,_)| p).fold(f64::INFINITY, f64::min);
    let avail = (area.height as usize).saturating_sub(2);
    let half = (avail.saturating_sub(1) / 2).min(9).max(3);
    asks.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let top_asks: Vec<_> = asks.into_iter().take(half).rev().collect();
    bids.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let top_bids: Vec<_> = bids.into_iter().take(half).collect();
    let max_size = top_asks.iter().map(|&(_,s)| s).chain(top_bids.iter().map(|&(_,s)| s)).fold(0.0f64, f64::max).max(1.0);
    let log_max = (max_size + 1.0).ln();
    let spread = if best_bid > 0.0 && best_ask > 0.0 { best_ask - best_bid } else { 0.0 };
    let mid = if best_bid > 0.0 && best_ask > 0.0 { (best_bid + best_ask) / 2.0 } else { 0.0 };
    let bar_w = area.width.saturating_sub(14) as usize;
    let mut lines: Vec<Line> = Vec::new();

    for &(price, size) in top_asks.iter() {
        let log_sz = (size + 1.0).ln();
        let w = if log_max > 0.0 { (log_sz / log_max * bar_w as f64) as usize } else { 0 };
        let bar = "█".repeat(w.min(bar_w));
        let is_ceiling = (price - best_ask).abs() < 0.0001;
        let c = if is_ceiling { BB_AMBER } else { BB_RED };
        lines.push(Line::from(vec![
            Span::styled(format!("{:.4} ", price), Style::default().fg(c)),
            Span::styled(bar, Style::default().fg(BB_RED)),
            Span::styled(format!(" {:.0}", size), Style::default().fg(BB_DIM)),
        ]));
    }
    if best_bid > 0.0 && best_ask > 0.0 {
        let s_label = format!("── SPREAD {spread:.4} ── MID {mid:.4} ──");
        lines.push(Line::from(vec![
            Span::styled(s_label, Style::default().fg(BB_AMBER).add_modifier(Modifier::BOLD)),
        ]));
    }
    for &(price, size) in top_bids.iter() {
        let log_sz = (size + 1.0).ln();
        let w = if log_max > 0.0 { (log_sz / log_max * bar_w as f64) as usize } else { 0 };
        let bar = "█".repeat(w.min(bar_w));
        let is_floor = (price - best_bid).abs() < 0.0001;
        let c = if is_floor { BB_AMBER } else { BB_GREEN };
        lines.push(Line::from(vec![
            Span::styled(format!("{:.4} ", price), Style::default().fg(c)),
            Span::styled(bar, Style::default().fg(BB_GREEN)),
            Span::styled(format!(" {:.0}", size), Style::default().fg(BB_DIM)),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled("  esperando...", Style::default().fg(BB_DIM))));
    }
    let title = if best_bid > 0.0 && best_ask > 0.0 {
        format!("{label}  ceil:{best_ask:.4}  floor:{best_bid:.4}")
    } else { format!("{label} Book") };
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)
            .title(title).border_style(Style::default().fg(border_c))
            .style(Style::default().bg(BB_CARD))),
        area,
    );
}

// ═══════════════════════════════════════════════════════════════════
// TAB GRAFICOS — DOM + TAP + Area Acumulada + Histograma
// ═══════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════
// TAB 2: OBI OSCILLATOR (Order Book Imbalance)
// ═══════════════════════════════════════════════════════════════════

fn draw_graficos(f: &mut Frame, area: Rect, s: &State) {
    let vert = Layout::default().direction(Direction::Vertical)
        .constraints([Constraint::Ratio(3,5), Constraint::Ratio(1,5), Constraint::Ratio(1,5)])
        .split(area);

    draw_obi_oscillator(f, vert[0], s);
    draw_obi_updn(f, vert[1], s);
    draw_obi_stats(f, vert[2], s);
}

fn draw_obi_oscillator(f: &mut Frame, area: Rect, s: &State) {
    let chart_h = (area.height as usize).saturating_sub(3).max(3);
    let chart_w = (area.width as usize).saturating_sub(12).max(10);

    let history: Vec<f64> = s.imb_history.iter().copied().collect();
    if history.len() < 2 {
        let lines = vec![Line::from(Span::styled("  esperando datos...",
            Style::default().fg(BB_DIM)))];
        f.render_widget(Paragraph::new(lines).block(
            Block::default().borders(Borders::ALL).title("OBI OSCILLATOR")
                .border_style(Style::default().fg(BB_AMBER))), area);
        return;
    }

    // Subsample to fit chart width
    let step = if history.len() > chart_w {
        (history.len() as f64 / chart_w as f64).max(1.0)
    } else { 1.0 };
    let mut sampled: Vec<f64> = Vec::with_capacity(chart_w);
    let mut idx = 0.0f64;
    while idx < history.len() as f64 && sampled.len() < chart_w {
        sampled.push(history[idx as usize]);
        idx += step;
    }

    let min_v = sampled.iter().cloned().fold(f64::INFINITY, f64::min).min(0.70);
    let max_v = sampled.iter().cloned().fold(f64::NEG_INFINITY, f64::max).max(1.30);
    let range = (max_v - min_v).max(0.01);

    // Build chart grid: row 0 = top (max_v) → row chart_h-1 = bottom (min_v)
    // Use a Vec<Vec<Option<char>>> for the chart with fill
    let mut grid: Vec<Vec<Option<(char, Color)>>> = vec![vec![None; sampled.len()]; chart_h];

    for (x, &val) in sampled.iter().enumerate() {
        let y = ((max_v - val) / range * (chart_h - 1) as f64).round() as usize;
        let y = y.min(chart_h - 1);
        let color = if val >= 1.0 { BB_GREEN } else { BB_RED };
        // Fill from line to center (1.0) for area effect
        let center_y = ((max_v - 1.0) / range * (chart_h - 1) as f64).round() as usize;
        let center_y = center_y.min(chart_h - 1);
        let (fill_start, fill_end) = if y <= center_y { (y, center_y) } else { (center_y, y) };
        for fy in fill_start..=fill_end {
            if fy < chart_h {
                let fill_color = if fy <= center_y { BB_GREEN } else { BB_RED };
                grid[fy][x] = Some(('█', fill_color));
            }
        }
        grid[y][x] = Some(('▀', color)); // line on top
    }

    // Highlight threshold lines
    let bull_y = ((max_v - 1.15) / range * (chart_h - 1) as f64).round() as usize;
    let bear_y = ((max_v - 0.85) / range * (chart_h - 1) as f64).round() as usize;
    let bull_y = bull_y.min(chart_h - 1);
    let bear_y = bear_y.min(chart_h - 1);

    let mut lines: Vec<Line> = Vec::new();

    for row in 0..chart_h {
        let val_at_row = max_v - (row as f64 / (chart_h - 1) as f64) * range;
        let is_center = row == ((max_v - 1.0) / range * (chart_h - 1) as f64).round() as usize;
        let is_bull = row == bull_y;
        let is_bear = row == bear_y;

        let label = if is_bull {
            Span::styled(format!("{:<7.2}─", 1.15), Style::default().fg(BB_GREEN).add_modifier(Modifier::BOLD))
        } else if is_center {
            Span::styled(format!("{:<7.2}─", 1.00), Style::default().fg(BB_AMBER).add_modifier(Modifier::BOLD))
        } else if is_bear {
            Span::styled(format!("{:<7.2}─", 0.85), Style::default().fg(BB_RED).add_modifier(Modifier::BOLD))
        } else if row % 3 == 0 {
            Span::styled(format!("{:<7.2} ", val_at_row), Style::default().fg(BB_DIM))
        } else {
            Span::styled("        ", Style::default())
        };

        let mut spans = vec![label];

        for x in 0..sampled.len() {
            if let Some((ch, c)) = grid[row][x] {
                if is_bull || is_bear || is_center {
                    // Threshold lines override with dotted style
                    spans.push(Span::styled("·", Style::default().fg(if is_bull { BB_GREEN } else if is_bear { BB_RED } else { BB_AMBER })));
                } else {
                    spans.push(Span::styled(ch.to_string(), Style::default().fg(c)));
                }
            } else if is_bull || is_bear || is_center {
                spans.push(Span::styled("·", Style::default().fg(if is_bull { BB_GREEN } else if is_bear { BB_RED } else { BB_AMBER })));
            } else {
                spans.push(Span::styled(" ", Style::default()));
            }
        }
        lines.push(Line::from(spans));
    }

    // Time axis labels
    let n = sampled.len();
    let mut axis_spans = vec![Span::styled("        ", Style::default())];
    let tick_interval = if n > 40 { n / 8 } else { n / 4 };
    for x in (0..n).step_by(tick_interval.max(1)) {
        let mut label_str = if x == 0 { format!("{:<tick_interval$}T-{}", "", n-x) }
            else { format!("{:<tick_interval$}T-{}", "", n-x) };
        // Truncate to fit
        if label_str.len() > tick_interval { label_str.truncate(tick_interval); }
        axis_spans.push(Span::styled(label_str, Style::default().fg(BB_DIM)));
    }
    lines.push(Line::from(axis_spans));

    // Current value
    let cur = history.last().copied().unwrap_or(1.0);
    let cur_label = if cur > 1.15 { "▲ BULLISH" } else if cur < 0.85 { "▼ BEARISH" } else { "— NEUTRAL" };
    let cur_color = if cur > 1.15 { BB_GREEN } else if cur < 0.85 { BB_RED } else { BB_AMBER };
    lines.push(Line::from(vec![
        Span::styled("  OBI ", Style::default().fg(BB_DIM)),
        Span::styled(format!("{:.3}x ", cur), Style::default().fg(BB_WHITE).add_modifier(Modifier::BOLD)),
        Span::styled(cur_label, Style::default().fg(cur_color).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  N={}  rango[{:.2},{:.2}]", history.len(), min_v, max_v),
            Style::default().fg(BB_DIM)),
    ]));

    f.render_widget(Paragraph::new(lines).block(
        Block::default().borders(Borders::ALL).title("OBI OSCILLATOR — Order Book Imbalance")
            .border_style(Style::default().fg(BB_AMBER))), area);
}

fn draw_obi_updn(f: &mut Frame, area: Rect, s: &State) {
    let chart_w = (area.width as usize).saturating_sub(12).max(10);

    let up_h: Vec<f64> = s.up_imb_history.iter().copied().collect();
    let dn_h: Vec<f64> = s.dn_imb_history.iter().copied().collect();
    let combined: Vec<f64> = s.imb_history.iter().copied().collect();

    let n = up_h.len().min(dn_h.len()).min(combined.len());
    if n < 2 {
        let lines = vec![Line::from(Span::styled("  esperando...", Style::default().fg(BB_DIM)))];
        f.render_widget(Paragraph::new(lines).block(
            Block::default().borders(Borders::ALL).title("UP vs DN IMBALANCE")
                .border_style(Style::default().fg(BB_BORDER))), area);
        return;
    }

    let step = if n > chart_w { (n as f64 / chart_w as f64).max(1.0) } else { 1.0 };

    let all_vals: Vec<f64> = up_h.iter().chain(dn_h.iter()).copied().collect();
    let min_v = all_vals.iter().cloned().fold(f64::INFINITY, f64::min).min(0.5);
    let max_v = all_vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max).max(2.0);
    let range = (max_v - min_v).max(0.01);
    let h = (area.height as usize).saturating_sub(3).max(3);

    let mut lines: Vec<Line> = Vec::new();

    // Draw two lines: UP (green ▀) and DN (red ▄) per row
    for row in 0..h {
        let val_lo = max_v - ((row as f64 + 1.0) / h as f64) * range;
        let val_hi = max_v - (row as f64 / h as f64) * range;

        let mut spans = vec![];
        let label = if row == 0 {
            Span::styled(format!("{:<7.2} ", max_v), Style::default().fg(BB_DIM))
        } else if row == h / 2 {
            Span::styled(format!("{:<7.2} ", (max_v + min_v) / 2.0), Style::default().fg(BB_DIM))
        } else if row == h - 1 {
            Span::styled(format!("{:<7.2} ", min_v), Style::default().fg(BB_DIM))
        } else {
            Span::styled("        ", Style::default())
        };
        spans.push(label);

        let mut idx_f = 0.0;
        while idx_f < n as f64 && spans.len() < 12 + chart_w {
            let i = idx_f as usize;
            let up = up_h[i];
            let dn = dn_h[i];
            let up_in = up >= val_lo && up < val_hi;
            let dn_in = dn >= val_lo && dn < val_hi;
            let ch = if up_in && dn_in { "█" }
                else if up_in { "▀" }
                else if dn_in { "▄" }
                else { " " };
            let color = if up_in && dn_in { BB_AMBER }
                else if up_in { BB_GREEN }
                else if dn_in { BB_RED }
                else { Color::Reset };
            spans.push(Span::styled(ch, Style::default().fg(color)));
            idx_f += step;
        }
        lines.push(Line::from(spans));
    }

    // Legend
    let cur_up = up_h.last().copied().unwrap_or(1.0);
    let cur_dn = dn_h.last().copied().unwrap_or(1.0);
    lines.push(Line::from(vec![
        Span::styled(" ▀ UP ", Style::default().fg(BB_GREEN).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:.2}x  ", cur_up), Style::default().fg(if cur_up > 1.0 { BB_GREEN } else { BB_RED })),
        Span::styled("▄ DN ", Style::default().fg(BB_RED).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:.2}x  ", cur_dn), Style::default().fg(if cur_dn > 1.0 { BB_GREEN } else { BB_RED })),
        Span::styled("█ BOTH", Style::default().fg(BB_AMBER)),
        Span::styled(format!("  N={n}", ), Style::default().fg(BB_DIM)),
    ]));

    f.render_widget(Paragraph::new(lines).block(
        Block::default().borders(Borders::ALL).title("UP vs DN IMBALANCE")
            .border_style(Style::default().fg(BB_BORDER))), area);
}

fn draw_obi_stats(f: &mut Frame, area: Rect, s: &State) {
    let b = Modifier::BOLD;
    let h: Vec<f64> = s.imb_history.iter().copied().collect();
    let n = h.len();

    // Current values
    let cur = h.last().copied().unwrap_or(1.0);
    let cur_dir = if cur > 1.15 { "▲ BULL" } else if cur < 0.85 { "▼ BEAR" } else { "— NEUTRAL" };
    let cur_c = if cur > 1.15 { BB_GREEN } else if cur < 0.85 { BB_RED } else { BB_AMBER };

    // Stats over the history
    let (mean, min_val, max_val) = if n > 1 {
        let sum: f64 = h.iter().sum();
        let mn = h.iter().cloned().fold(f64::INFINITY, f64::min);
        let mx = h.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        (sum / n as f64, mn, mx)
    } else { (cur, cur, cur) };

    // Cross count: how many times crossed 1.0
    let crosses: usize = h.windows(2).filter(|w| (w[0] - 1.0).signum() != (w[1] - 1.0).signum()).count();

    // Time above/below 1.0
    let above = h.iter().filter(|&&v| v >= 1.0).count();
    let above_pct = if n > 0 { above as f64 / n as f64 * 100.0 } else { 50.0 };

    // Wall flags
    let wall_on = s.hft.ask_wall > 0;
    let spoof_on = s.hft.spoof > 0;

    let lines = vec![
        Line::from(vec![
            Span::styled("OBI ", Style::default().fg(BB_AMBER).add_modifier(b)),
            Span::styled(format!("{:.3}x", cur), Style::default().fg(BB_WHITE).add_modifier(b)),
            Span::styled(format!(" {cur_dir}  "), Style::default().fg(cur_c).add_modifier(b)),
            Span::styled(format!("avg {:.3}", mean), Style::default().fg(BB_DIM)),
        ]),
        Line::from(vec![
            Span::styled(format!("rango [{:.3}, {:.3}]  ", min_val, max_val), Style::default().fg(BB_DIM)),
            Span::styled(format!("bull↑ {:.0}% ", above_pct), Style::default().fg(BB_GREEN)),
            Span::styled(format!("bear↓ {:.0}%  ", 100.0 - above_pct), Style::default().fg(BB_RED)),
            Span::styled(format!("x1.0:{}", crosses), Style::default().fg(BB_CYAN)),
        ]),
        Line::from(vec![
            Span::styled(format!("N={n} muestras  ", ), Style::default().fg(BB_DIM)),
            Span::styled(if wall_on { "█ ASK WALL " } else { "— no wall " },
                Style::default().fg(if wall_on { BB_RED } else { BB_GREEN }).add_modifier(b)),
            Span::styled(if spoof_on { "⚠ SPOOF" } else { "" },
                Style::default().fg(BB_RED).add_modifier(b)),
        ]),
    ];

    f.render_widget(Paragraph::new(lines).block(
        Block::default().borders(Borders::ALL).title("OBI STATS")
            .border_style(Style::default().fg(BB_BORDER))), area);
}


// ═══════════════════════════════════════════════════════════════════
// TAB 3: MARKET MICROSTRUCTURE — session indicators
// ═══════════════════════════════════════════════════════════════════

fn draw_microestructura(f: &mut Frame, area: Rect, s: &State) {
    let vert = Layout::default().direction(Direction::Vertical)
        .constraints([Constraint::Ratio(2,5), Constraint::Ratio(1,5), Constraint::Ratio(2,5)])
        .split(area);

    let top = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)])
        .split(vert[0]);
    draw_micro_price(f, top[0], s, "UP", &s.sess_up_prices, &s.sess_mids, BB_GREEN);
    draw_micro_price(f, top[1], s, "DN", &s.sess_dn_prices, &s.sess_mids, BB_RED);

    draw_micro_depth(f, vert[1], s);

    let bot = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)])
        .split(vert[2]);
    draw_micro_spread(f, bot[0], s);
    draw_micro_metrics(f, bot[1], s);
}

fn draw_micro_price(f: &mut Frame, area: Rect, _s: &State, label: &str,
                     prices: &VecDeque<f64>, mids: &VecDeque<f64>, color: Color) {
    let chart_h = (area.height as usize).saturating_sub(3).max(3);
    let chart_w = (area.width as usize).saturating_sub(10).max(8);
    let vals: Vec<f64> = prices.iter().copied().collect();
    let mid_vals: Vec<f64> = mids.iter().copied().collect();

    if vals.len() < 2 {
        let lines = vec![Line::from(Span::styled("  esperando...", Style::default().fg(BB_DIM)))];
        f.render_widget(Paragraph::new(lines).block(
            Block::default().borders(Borders::ALL).title(format!("PRICE {label}"))
                .border_style(Style::default().fg(color))), area);
        return;
    }

    let n = vals.len().min(mid_vals.len());
    let step = if n > chart_w { (n as f64 / chart_w as f64).max(1.0) } else { 1.0 };
    let mut px: Vec<f64> = Vec::with_capacity(chart_w);
    let mut md: Vec<f64> = Vec::with_capacity(chart_w);
    let mut idx = 0.0f64;
    while idx < n as f64 && px.len() < chart_w {
        let i = idx as usize;
        px.push(vals[i]);
        md.push(if i < mid_vals.len() { mid_vals[i] } else { vals[i] });
        idx += step;
    }

    let all_max = px.iter().chain(md.iter()).cloned().fold(f64::NEG_INFINITY, f64::max);
    let all_min = px.iter().chain(md.iter()).cloned().fold(f64::INFINITY, f64::min);
    let margin = (all_max - all_min) * 0.1;
    let min_v = (all_min - margin).max(0.0);
    let max_v = all_max + margin;
    let range = (max_v - min_v).max(0.0001);

    let mut lines: Vec<Line> = Vec::new();
    for row in 0..chart_h {
        let val_at = max_v - (row as f64 / (chart_h - 1) as f64) * range;
        let mut spans = vec![
            Span::styled(format!("{:<6.4} ", val_at), Style::default().fg(if row % 3 == 0 { BB_DIM } else { Color::Reset })),
        ];
        for x in 0..px.len() {
            let p = px[x];
            let m = md[x];
            let p_y = ((max_v - p) / range * (chart_h - 1) as f64).round() as usize;
            let m_y = ((max_v - m) / range * (chart_h - 1) as f64).round() as usize;
            let ch = if p_y == row && m_y == row { "█" }
                else if p_y == row { "▀" }
                else if m_y == row { "·" }
                else { " " };
            let c = if p_y == row { color } else if m_y == row { BB_DIM } else { Color::Reset };
            spans.push(Span::styled(ch, Style::default().fg(c)));
        }
        lines.push(Line::from(spans));
    }

    let cur = vals.last().copied().unwrap_or(0.0);
    let open = *vals.first().unwrap_or(&cur);
    let delta = cur - open;
    let delta_pct = if open > 0.0 { (cur / open - 1.0) * 100.0 } else { 0.0 };
    lines.push(Line::from(vec![
        Span::styled(format!("  cur {:.4}", cur), Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  open {:.4}", open), Style::default().fg(BB_DIM)),
        Span::styled(format!("  Δ{:+.1}%", delta_pct), Style::default().fg(if delta >= 0.0 { BB_GREEN } else { BB_RED }).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  N={}", vals.len()), Style::default().fg(BB_DIM)),
    ]));

    f.render_widget(Paragraph::new(lines).block(
        Block::default().borders(Borders::ALL).title(format!("PRICE {label}  ▀=trade ·=mid"))
            .border_style(Style::default().fg(color))), area);
}

fn draw_micro_depth(f: &mut Frame, area: Rect, s: &State) {
    let avail_w = (area.width as usize).saturating_sub(14).max(20);
    let half_w = avail_w / 2;

    // UP book depth
    let up_bids: Vec<(f64, f64)> = s.book_up.bids.iter().map(|l| (l.price, l.size)).collect();
    let up_asks: Vec<(f64, f64)> = s.book_up.asks.iter().map(|l| (l.price, l.size)).collect();
    // DN book depth
    let dn_bids: Vec<(f64, f64)> = s.book_dn.bids.iter().map(|l| (l.price, l.size)).collect();
    let dn_asks: Vec<(f64, f64)> = s.book_dn.asks.iter().map(|l| (l.price, l.size)).collect();

    let all_sizes: Vec<f64> = up_bids.iter().chain(up_asks.iter()).chain(dn_bids.iter()).chain(dn_asks.iter())
        .map(|&(_, s)| s).collect();
    let max_sz = all_sizes.iter().cloned().fold(0.0f64, f64::max).max(1.0);

    // Build combined price list (sorted descending)
    let mut all_prices: Vec<f64> = Vec::new();
    for &(p, _) in &up_bids { all_prices.push(p); }
    for &(p, _) in &up_asks { all_prices.push(p); }
    for &(p, _) in &dn_bids { all_prices.push(p); }
    for &(p, _) in &dn_asks { all_prices.push(p); }
    all_prices.sort_unstable_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    all_prices.dedup_by(|a, b| (*a - *b).abs() < 0.0001);

    let chart_h = (area.height as usize).saturating_sub(3).max(4);
    let step = if all_prices.len() > chart_h {
        (all_prices.len() as f64 / chart_h as f64).max(1.0)
    } else { 1.0 };

    let mut lines: Vec<Line> = Vec::new();
    let mut idx = 0.0;
    while idx < all_prices.len() as f64 && lines.len() < chart_h {
        let i = idx as usize;
        let p = all_prices[i];

        let up_bid_cum: f64 = up_bids.iter().filter(|&&(bp, _)| bp >= p).map(|&(_, s)| s).sum();
        let up_ask_cum: f64 = up_asks.iter().filter(|&&(ap, _)| ap <= p).map(|&(_, s)| s).sum();
        let dn_bid_cum: f64 = dn_bids.iter().filter(|&&(bp, _)| bp >= p).map(|&(_, s)| s).sum();
        let dn_ask_cum: f64 = dn_asks.iter().filter(|&&(ap, _)| ap <= p).map(|&(_, s)| s).sum();

        let up_bid_w = ((up_bid_cum / max_sz) * half_w as f64) as usize;
        let dn_bid_w = ((dn_bid_cum / max_sz) * half_w as f64) as usize;
        let up_ask_w = ((up_ask_cum / max_sz) * half_w as f64) as usize;
        let dn_ask_w = ((dn_ask_cum / max_sz) * half_w as f64) as usize;

        let total_w = up_bid_w + dn_bid_w + up_ask_w + dn_ask_w;
        let scale = if total_w > 0 && total_w > avail_w {
            avail_w as f64 / total_w as f64
        } else { 1.0 };

        let up_bid_w = ((up_bid_w as f64 * scale) as usize).min(half_w);
        let dn_bid_w = ((dn_bid_w as f64 * scale) as usize).min(half_w);
        let up_ask_w = ((up_ask_w as f64 * scale) as usize).min(half_w);
        let dn_ask_w = ((dn_ask_w as f64 * scale) as usize).min(half_w);

        lines.push(Line::from(vec![
            Span::styled(format!("{:<7.4} ", p), Style::default().fg(BB_DIM)),
            Span::styled("█".repeat(up_bid_w), Style::default().fg(BB_GREEN)),
            Span::styled("▓".repeat(dn_bid_w), Style::default().fg(Color::Rgb(0, 150, 70))),
            Span::styled("▐".repeat(up_ask_w), Style::default().fg(BB_RED)),
            Span::styled("░".repeat(dn_ask_w), Style::default().fg(Color::Rgb(200, 40, 40))),
        ]));

        idx += step;
    }

    let up_bid_total: f64 = up_bids.iter().map(|&(_, s)| s).sum();
    let up_ask_total: f64 = up_asks.iter().map(|&(_, s)| s).sum();
    let dn_bid_total: f64 = dn_bids.iter().map(|&(_, s)| s).sum();
    let dn_ask_total: f64 = dn_asks.iter().map(|&(_, s)| s).sum();
    lines.insert(0, Line::from(vec![
        Span::styled(format!("UPbid{:.0}k", up_bid_total / 1000.0),
            Style::default().fg(BB_GREEN)),
        Span::styled(format!(" DNbid{:.0}k", dn_bid_total / 1000.0),
            Style::default().fg(Color::Rgb(0, 150, 70))),
        Span::styled(format!(" UPask{:.0}k", up_ask_total / 1000.0),
            Style::default().fg(BB_RED)),
        Span::styled(format!(" DNask{:.0}k", dn_ask_total / 1000.0),
            Style::default().fg(Color::Rgb(200, 40, 40))),
    ]));

    f.render_widget(Paragraph::new(lines).block(
        Block::default().borders(Borders::ALL).title("DEPTH PROFILE  █UPbid ▓DNbid ▐UPask ░DNask")
            .border_style(Style::default().fg(BB_BORDER))), area);
}

fn draw_micro_spread(f: &mut Frame, area: Rect, s: &State) {
    let chart_h = (area.height as usize).saturating_sub(3).max(3);
    let chart_w = (area.width as usize).saturating_sub(10).max(8);
    let vals: Vec<f64> = s.sess_spreads.iter().copied().collect();

    if vals.len() < 2 {
        let lines = vec![Line::from(Span::styled("  esperando...", Style::default().fg(BB_DIM)))];
        f.render_widget(Paragraph::new(lines).block(
            Block::default().borders(Borders::ALL).title("SPREAD EVOLUTION")
                .border_style(Style::default().fg(BB_BORDER))), area);
        return;
    }

    let step = if vals.len() > chart_w { (vals.len() as f64 / chart_w as f64).max(1.0) } else { 1.0 };
    let mut sampled: Vec<f64> = Vec::with_capacity(chart_w);
    let mut idx = 0.0;
    while idx < vals.len() as f64 && sampled.len() < chart_w {
        sampled.push(vals[idx as usize]);
        idx += step;
    }

    let max_v = sampled.iter().cloned().fold(f64::NEG_INFINITY, f64::max).max(0.01);
    let range = max_v.max(0.0001);

    let mut lines: Vec<Line> = Vec::new();
    for row in 0..chart_h {
        let thresh = max_v - (row as f64 / (chart_h - 1) as f64) * range;
        let mut spans = vec![
            Span::styled(format!("{:<6.4} ", thresh), Style::default().fg(if row % 3 == 0 { BB_DIM } else { Color::Reset })),
        ];
        for x in 0..sampled.len() {
            let v = sampled[x];
            let c = if v >= thresh {
                if v > 0.008 { BB_RED } else if v > 0.003 { BB_AMBER } else { BB_GREEN }
            } else { Color::Reset };
            spans.push(Span::styled(if v >= thresh { "█" } else { " " }, Style::default().fg(c)));
        }
        lines.push(Line::from(spans));
    }

    let cur = vals.last().copied().unwrap_or(0.0);
    let label = if cur < 0.003 { "TIGHT" } else if cur < 0.008 { "MED" } else { "WIDE" };
    let label_c = if cur < 0.003 { BB_GREEN } else if cur < 0.008 { BB_AMBER } else { BB_RED };
    lines.push(Line::from(vec![
        Span::styled(format!("  spread {:.4}  ", cur), Style::default().fg(label_c).add_modifier(Modifier::BOLD)),
        Span::styled(label, Style::default().fg(label_c).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  max{:.4} min{:.4}", s.sess_max_spread, s.sess_min_spread),
            Style::default().fg(BB_DIM)),
    ]));

    f.render_widget(Paragraph::new(lines).block(
        Block::default().borders(Borders::ALL).title("SPREAD EVOLUTION")
            .border_style(Style::default().fg(BB_BORDER))), area);
}

fn draw_micro_metrics(f: &mut Frame, area: Rect, s: &State) {
    let b = Modifier::BOLD;

    let total_trades = s.sess_trades_up + s.sess_trades_dn;
    let total_vol = s.sess_vol_up + s.sess_vol_dn;
    let up_ratio = if total_trades > 0 { s.sess_trades_up as f64 / total_trades as f64 * 100.0 } else { 50.0 };
    let vol_ratio = if total_vol > 0.0 { s.sess_vol_up / total_vol * 100.0 } else { 50.0 };
    let avg_spread: f64 = if !s.sess_spreads.is_empty() {
        s.sess_spreads.iter().sum::<f64>() / s.sess_spreads.len() as f64
    } else { 0.0 };

    let eff_spread = if s.sess_up_prices.len() > 1 && s.sess_mids.len() > 0 {
        let last_price = *s.sess_up_prices.back().unwrap_or(&0.0);
        let last_mid = *s.sess_mids.back().unwrap_or(&0.0);
        if last_mid > 0.0 { ((last_price - last_mid).abs() / last_mid) * 100.0 } else { 0.0 }
    } else { 0.0 };

    let up_color = if up_ratio > 55.0 { BB_GREEN } else if up_ratio < 45.0 { BB_RED } else { BB_AMBER };

    let lines = vec![
        Line::from(vec![
            Span::styled("Trades  ", Style::default().fg(BB_DIM)),
            Span::styled(format!("UP {}  ", s.sess_trades_up), Style::default().fg(BB_GREEN).add_modifier(b)),
            Span::styled(format!("DN {}  ", s.sess_trades_dn), Style::default().fg(BB_RED).add_modifier(b)),
            Span::styled(format!("total {}", total_trades), Style::default().fg(BB_WHITE)),
        ]),
        Line::from(vec![
            Span::styled("Volume  ", Style::default().fg(BB_DIM)),
            Span::styled(format!("UP {:.0}  ", s.sess_vol_up), Style::default().fg(BB_GREEN)),
            Span::styled(format!("DN {:.0}  ", s.sess_vol_dn), Style::default().fg(BB_RED)),
            Span::styled(format!("tot {:.0}", total_vol), Style::default().fg(BB_WHITE)),
        ]),
        Line::from(vec![
            Span::styled("Ratio   ", Style::default().fg(BB_DIM)),
            Span::styled(format!("UP {:.0}% ", up_ratio), Style::default().fg(up_color).add_modifier(b)),
            Span::styled(format!("vol {:.0}% UP", vol_ratio), Style::default().fg(BB_DIM)),
        ]),
        Line::from(vec![
            Span::styled("Spread  ", Style::default().fg(BB_DIM)),
            Span::styled(format!("avg {:.4}  ", avg_spread), Style::default().fg(BB_WHITE)),
            Span::styled(format!("eff {:.2}%", eff_spread), Style::default().fg(BB_CYAN).add_modifier(b)),
        ]),
        Line::from(vec![
            Span::styled("Sesión  ", Style::default().fg(BB_DIM)),
            Span::styled(format!("{} ticks", s.sess_mids.len()), Style::default().fg(BB_AMBER)),
            Span::styled(format!("  imb {:.2}x", s.hft.imbalance), Style::default().fg(BB_WHITE).add_modifier(b)),
        ]),
    ];

    f.render_widget(Paragraph::new(lines).block(
        Block::default().borders(Borders::ALL).title("SESSION METRICS")
            .border_style(Style::default().fg(BB_BORDER))), area);
}


// ═══════════════════════════════════════════════════════════════════
// TAB 4: OFI (Order Flow Imbalance) + Micro-Price
// ═══════════════════════════════════════════════════════════════════

fn draw_ofi(f: &mut Frame, area: Rect, s: &State) {
    let vert = Layout::default().direction(Direction::Vertical)
        .constraints([Constraint::Ratio(2,5), Constraint::Ratio(1,5), Constraint::Ratio(2,5)])
        .split(area);

    let top = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)])
        .split(vert[0]);
    draw_ofi_bar(f, top[0], s, "UP", &s.ofi_up_history, BB_GREEN);
    draw_ofi_bar(f, top[1], s, "DN", &s.ofi_dn_history, BB_RED);

    draw_ofi_cross(f, vert[1], s);

    let bot = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)])
        .split(vert[2]);
    draw_micro_price_ofi(f, bot[0], s, "UP", &s.micro_up_history, BB_GREEN);
    draw_micro_price_ofi(f, bot[1], s, "DN", &s.micro_dn_history, BB_RED);
}

/// OFI bar chart: green above zero (buying), red below (selling)
fn draw_ofi_bar(f: &mut Frame, area: Rect, s: &State, label: &str,
                history: &VecDeque<f64>, color: Color) {
    let chart_h = (area.height as usize).saturating_sub(3).max(3);
    let chart_w = (area.width as usize).saturating_sub(10).max(8);
    let vals: Vec<f64> = history.iter().copied().collect();

    if vals.len() < 2 {
        let lines = vec![Line::from(Span::styled("  esperando...", Style::default().fg(BB_DIM)))];
        f.render_widget(Paragraph::new(lines).block(
            Block::default().borders(Borders::ALL).title(format!("OFI {label}"))
                .border_style(Style::default().fg(color))), area);
        return;
    }

    let step = if vals.len() > chart_w { (vals.len() as f64 / chart_w as f64).max(1.0) } else { 1.0 };
    let mut sampled: Vec<f64> = Vec::with_capacity(chart_w);
    let mut idx = 0.0;
    while idx < vals.len() as f64 && sampled.len() < chart_w {
        sampled.push(vals[idx as usize]);
        idx += step;
    }

    let abs_max = sampled.iter().map(|v| v.abs()).fold(0.0f64, f64::max).max(1.0);
    let range = abs_max * 2.0; // total range from -abs_max to +abs_max
    let zero_row = chart_h / 2;

    let mut lines: Vec<Line> = Vec::new();
    for row in 0..chart_h {
        let val_at = if row < zero_row {
            abs_max - (row as f64 / zero_row as f64) * abs_max
        } else if row > zero_row {
            -(row as f64 - zero_row as f64) / (chart_h - 1 - zero_row) as f64 * abs_max
        } else { 0.0 };

        let is_zero = row == zero_row;
        let mut spans = vec![
            Span::styled(if is_zero {
                format!("─0────── ")
            } else if row == 0 {
                format!("{:<+.0}      ", abs_max)
            } else if row == chart_h - 1 {
                format!("{:<+.0}      ", -abs_max)
            } else {
                format!("         ")
            }, Style::default().fg(if is_zero { BB_AMBER } else { BB_DIM })),
        ];

        for x in 0..sampled.len() {
            let v = sampled[x];
            let fills = if v >= 0.0 {
                let top = zero_row.saturating_sub(((v / abs_max) * zero_row as f64) as usize);
                row >= top && row <= zero_row
            } else {
                let bot = zero_row + (((-v) / abs_max) * (chart_h - 1 - zero_row) as f64) as usize;
                row >= zero_row && row <= bot
            };
            let ch = if fills { "█" } else if is_zero { "·" } else { " " };
            let c = if fills {
                if v >= 0.0 { BB_GREEN } else { BB_RED }
            } else if is_zero { BB_AMBER } else { Color::Reset };
            spans.push(Span::styled(ch, Style::default().fg(c)));
        }
        lines.push(Line::from(spans));
    }

    let cur = vals.last().copied().unwrap_or(0.0);
    let label_c = if cur > 0.0 { BB_GREEN } else if cur < 0.0 { BB_RED } else { BB_AMBER };
    let sum: f64 = vals.iter().sum();
    lines.push(Line::from(vec![
        Span::styled(format!("  OFI {label} "), Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{cur:+.0}  "), Style::default().fg(label_c).add_modifier(Modifier::BOLD)),
        Span::styled(format!("sum {sum:+.0}  N={}", vals.len()), Style::default().fg(BB_DIM)),
        Span::styled(format!("  |max|={:.0}", abs_max), Style::default().fg(BB_DIM)),
    ]));

    f.render_widget(Paragraph::new(lines).block(
        Block::default().borders(Borders::ALL).title(format!("OFI {label}  ■=buy ■=sell"))
            .border_style(Style::default().fg(color))), area);
}

/// Cross-book OFI: UP vs DN superimposed
fn draw_ofi_cross(f: &mut Frame, area: Rect, s: &State) {
    let chart_w = (area.width as usize).saturating_sub(12).max(10);
    let up_vals: Vec<f64> = s.ofi_up_history.iter().copied().collect();
    let dn_vals: Vec<f64> = s.ofi_dn_history.iter().copied().collect();
    let n = up_vals.len().min(dn_vals.len());

    if n < 2 {
        let lines = vec![Line::from(Span::styled("  esperando...", Style::default().fg(BB_DIM)))];
        f.render_widget(Paragraph::new(lines).block(
            Block::default().borders(Borders::ALL).title("OFI CROSS-BOOK")
                .border_style(Style::default().fg(BB_AMBER))), area);
        return;
    }

    let step = if n > chart_w { (n as f64 / chart_w as f64).max(1.0) } else { 1.0 };
    let mut up_s: Vec<f64> = Vec::with_capacity(chart_w);
    let mut dn_s: Vec<f64> = Vec::with_capacity(chart_w);
    let mut idx = 0.0;
    while idx < n as f64 && up_s.len() < chart_w {
        let i = idx as usize;
        up_s.push(up_vals[i]);
        dn_s.push(dn_vals[i]);
        idx += step;
    }

    let all: Vec<f64> = up_s.iter().chain(dn_s.iter()).copied().collect();
    let min_v = all.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_v = all.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let abs_max = min_v.abs().max(max_v.abs()).max(1.0);
    let chart_h = (area.height as usize).saturating_sub(3).max(3);
    let zero_row = chart_h / 2;

    // Cumulative flow (area between UP and DN OFI)
    let up_sum: f64 = up_vals.iter().sum();
    let dn_sum: f64 = dn_vals.iter().sum();
    let net_flow = up_sum - dn_sum;
    let net_label = if net_flow > 0.0 { "▲UP NET" } else if net_flow < 0.0 { "▼DN NET" } else { "—FLAT" };
    let net_c = if net_flow > 0.0 { BB_GREEN } else if net_flow < 0.0 { BB_RED } else { BB_AMBER };

    let mut lines: Vec<Line> = Vec::new();
    for row in 0..chart_h {
        let val_at = if row < zero_row {
            abs_max - (row as f64 / zero_row as f64) * abs_max
        } else if row > zero_row {
            -(row as f64 - zero_row as f64) / (chart_h - 1 - zero_row) as f64 * abs_max
        } else { 0.0 };

        let is_zero = row == zero_row;
        let mut spans = vec![
            Span::styled(if is_zero {
                format!("─0─────── ")
            } else if row % 3 == 0 {
                format!("{:<+.0}       ", val_at)
            } else { format!("          ") },
                Style::default().fg(if is_zero { BB_AMBER } else { BB_DIM })),
        ];

        for x in 0..up_s.len() {
            let up = up_s[x];
            let dn = dn_s[x];
            let up_y = if up >= 0.0 {
                zero_row.saturating_sub(((up / abs_max) * zero_row as f64) as usize)
            } else {
                zero_row + (((-up) / abs_max) * (chart_h - 1 - zero_row) as f64) as usize
            };
            let dn_y = if dn >= 0.0 {
                zero_row.saturating_sub(((dn / abs_max) * zero_row as f64) as usize)
            } else {
                zero_row + (((-dn) / abs_max) * (chart_h - 1 - zero_row) as f64) as usize
            };

            let up_fill = (up >= 0.0 && row <= zero_row && row >= up_y) || (up < 0.0 && row >= zero_row && row <= up_y);
            let dn_fill = (dn >= 0.0 && row <= zero_row && row >= dn_y) || (dn < 0.0 && row >= zero_row && row <= dn_y);

            let ch = if up_fill && dn_fill { "█" }
                else if up_fill { "▀" }
                else if dn_fill { "▄" }
                else if is_zero { "·" }
                else { " " };
            let c = if up_fill && dn_fill { BB_AMBER }
                else if up_fill { BB_GREEN }
                else if dn_fill { BB_RED }
                else if is_zero { BB_AMBER }
                else { Color::Reset };
            spans.push(Span::styled(ch, Style::default().fg(c)));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::from(vec![
        Span::styled(" ▀OFI UP ", Style::default().fg(BB_GREEN).add_modifier(Modifier::BOLD)),
        Span::styled(format!("sum {up_sum:+.0}  "), Style::default().fg(if up_sum > 0.0 { BB_GREEN } else { BB_RED })),
        Span::styled("▄OFI DN ", Style::default().fg(BB_RED).add_modifier(Modifier::BOLD)),
        Span::styled(format!("sum {dn_sum:+.0}  "), Style::default().fg(if dn_sum > 0.0 { BB_GREEN } else { BB_RED })),
        Span::styled(format!("{net_label} {net_flow:+.0}"), Style::default().fg(net_c).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  N={n}"), Style::default().fg(BB_DIM)),
    ]));

    f.render_widget(Paragraph::new(lines).block(
        Block::default().borders(Borders::ALL).title("OFI CROSS-BOOK  ▀UP ▄DN █BOTH")
            .border_style(Style::default().fg(BB_AMBER))), area);
}

/// Micro-price line chart: trade price vs mid vs micro-price
fn draw_micro_price_ofi(f: &mut Frame, area: Rect, _s: &State, label: &str,
                        micros: &VecDeque<f64>, color: Color) {
    let chart_h = (area.height as usize).saturating_sub(3).max(3);
    let chart_w = (area.width as usize).saturating_sub(10).max(8);
    let vals: Vec<f64> = micros.iter().copied().collect();

    if vals.len() < 2 {
        let lines = vec![Line::from(Span::styled("  esperando...", Style::default().fg(BB_DIM)))];
        f.render_widget(Paragraph::new(lines).block(
            Block::default().borders(Borders::ALL).title(format!("MICRO {label}"))
                .border_style(Style::default().fg(color))), area);
        return;
    }

    let step = if vals.len() > chart_w { (vals.len() as f64 / chart_w as f64).max(1.0) } else { 1.0 };
    let mut sampled: Vec<f64> = Vec::with_capacity(chart_w);
    let mut idx = 0.0;
    while idx < vals.len() as f64 && sampled.len() < chart_w {
        sampled.push(vals[idx as usize]);
        idx += step;
    }

    let min_v = sampled.iter().cloned().fold(f64::INFINITY, f64::min).max(0.0);
    let max_v = sampled.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let margin = (max_v - min_v) * 0.1;
    let min_v = (min_v - margin).max(0.0);
    let max_v = max_v + margin;
    let range = (max_v - min_v).max(0.0001);

    let mut lines: Vec<Line> = Vec::new();
    for row in 0..chart_h {
        let val_at = max_v - (row as f64 / (chart_h - 1) as f64) * range;
        let mut spans = vec![
            Span::styled(format!("{:<6.4} ", val_at),
                Style::default().fg(if row % 3 == 0 { BB_DIM } else { Color::Reset })),
        ];
        for x in 0..sampled.len() {
            let v = sampled[x];
            let v_y = ((max_v - v) / range * (chart_h - 1) as f64).round() as usize;
            let ch = if v_y == row { "█" } else { " " };
            spans.push(Span::styled(ch, Style::default().fg(if v_y == row { color } else { Color::Reset })));
        }
        lines.push(Line::from(spans));
    }

    let cur = vals.last().copied().unwrap_or(0.0);
    let first = *vals.first().unwrap_or(&cur);
    let delta = if first > 0.0 { (cur / first - 1.0) * 100.0 } else { 0.0 };
    lines.push(Line::from(vec![
        Span::styled(format!("  μ-price {:.4}", cur), Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  Δ{:+.2}%", delta),
            Style::default().fg(if delta >= 0.0 { BB_GREEN } else { BB_RED }).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  rango[{:.4},{:.4}]", min_v, max_v),
            Style::default().fg(BB_DIM)),
    ]));

    f.render_widget(Paragraph::new(lines).block(
        Block::default().borders(Borders::ALL).title(format!("MICRO-PRICE {label}"))
            .border_style(Style::default().fg(color))), area);
}


// ═══════════════════════════════════════════════════════════════════
// POSITION BAR
// ═══════════════════════════════════════════════════════════════════

fn draw_position_bar(f: &mut Frame, area: Rect, s: &State) {
    let bb = Modifier::BOLD;

    let (pos_text, pos_style) = if s.mt_state >= 2 {
        let current_px = if s.mt_outcome == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn };
        let entry = if s.mt_fill_avg > 0.0 { s.mt_fill_avg } else { s.mt_entry };
        let gain = (current_px - entry) >= 0.0;
        let bg = if gain { BB_GREEN } else { BB_RED };
        let side_sym = if s.mt_outcome == "up" { "▲" } else { "▼" };
        let tsl = if s.mt_tsl_pct > 0.0 { format!(" TSL:{:.0}%", s.mt_tsl_pct) } else { String::new() };
        let sl = if s.sl_pct > 0.0 { format!(" SL:{:.0}%", s.sl_pct) } else { String::new() };
        let txt = format!("{side_sym} POS {} sz={:.0} entry={:.4}→{:.4}{tsl}{sl}",
            s.mt_outcome.to_uppercase(), s.mt_size, entry, current_px);
        (txt, Style::default().fg(BB_WHITE).bg(bg).add_modifier(bb))
    } else if s.mt_state == 1 {
        let side_sym = if s.mt_outcome == "up" { "▲" } else { "▼" };
        let txt = format!("{side_sym} PENDING {} sz={:.0} @{:.4} ${:.2}  [{:.0}% filled]  /c=CANCELAR",
            s.mt_outcome.to_uppercase(), s.mt_size, s.mt_entry, s.mt_budget, s.mt_last_fill_pct);
        (txt, Style::default().fg(BB_WHITE).bg(BB_AMBER).add_modifier(bb))
    } else {
        (format!("0 POSICIONES  —  /4up65 para abrir"),
         Style::default().fg(BB_DIM))
    };

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

fn draw_footer(f: &mut Frame, area: Rect, s: &State) {
    let sl_info = if s.sl_pct > 0.0 {
        format!("{:.0}%", s.sl_pct)
    } else { "OFF".into() };
    let tsl_info = if s.mt_tsl_pct > 0.0 { format!("{}%", s.mt_tsl_pct) } else { "OFF".into() };

    let lines = vec![
        Line::from(vec![
            Span::styled("/l4up65 /l4d65 ", Style::default().fg(BB_GREEN).add_modifier(Modifier::BOLD)),
            Span::styled("BUY       ", Style::default().fg(BB_GRAY)),
            Span::styled("/xu70 /xd70  ", Style::default().fg(BB_AMBER).add_modifier(Modifier::BOLD)),
            Span::styled("SELL limit ", Style::default().fg(BB_GRAY)),
            Span::styled("/p", Style::default().fg(BB_RED).add_modifier(Modifier::BOLD)),
            Span::styled(" PANIC  ", Style::default().fg(BB_GRAY)),
            Span::styled("/c", Style::default().fg(BB_AMBER).add_modifier(Modifier::BOLD)),
            Span::styled(" cancel ", Style::default().fg(BB_GRAY)),
            Span::styled("/x", Style::default().fg(BB_MAGENTA).add_modifier(Modifier::BOLD)),
            Span::styled(" mkt sell", Style::default().fg(BB_GRAY)),
        ]),
        Line::from(vec![
            Span::styled("/l4up65e70  ", Style::default().fg(BB_GREEN)),
            Span::styled("BUY+exit  ", Style::default().fg(BB_DIM)),
            Span::styled("/sl5 /sl  ", Style::default().fg(BB_AMBER).add_modifier(Modifier::BOLD)),
            Span::styled("stop-loss ", Style::default().fg(BB_DIM)),
            Span::styled("/nsl ", Style::default().fg(BB_DIM)),
            Span::styled("off  ", Style::default().fg(BB_DIM)),
            Span::styled("/tsl2 ", Style::default().fg(BB_CYAN).add_modifier(Modifier::BOLD)),
            Span::styled("trail stop  ", Style::default().fg(BB_DIM)),
            Span::styled(format!("SL:{sl_info} TSL:{tsl_info}"), Style::default().fg(BB_DIM)),
        ]),
        Line::from(vec![
            Span::styled(" [/]comando  [Tab]vista  [Esc/q]salir", Style::default().fg(BB_BORDER)),
        ]),
    ];
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_command_bar(f: &mut Frame, area: Rect, s: &State) {
    let help = "/l10up65e70  BUY+EXIT   |  /clm  cancel+mkt  |  /xu70 /xd70  sell limit";
    let help2 = "/c  cancel  |  /x  mkt sell  |  /p  PANIC  |  /5g70  Gemini";
    let text = format!("▶ /{}_\n{help}\n{help2}", s.input_buf);
    f.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(BB_CYAN).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL)
                .border_style(Style::default().fg(BB_GREEN)).title("CMD")),
        area,
    );
}

fn draw_man_page(f: &mut Frame, area: Rect, _s: &State) {
    use crate::commands::REGISTRY;
    use std::collections::BTreeMap;
    let mut cats: BTreeMap<&str, Vec<&crate::commands::CmdDef>> = BTreeMap::new();
    for cmd in REGISTRY { cats.entry(cmd.category).or_default().push(cmd); }
    let mut lines: Vec<Line> = Vec::new();
    let bb = Modifier::BOLD;
    lines.push(Line::from(Span::styled(
        "╔══════════════════════════════════════╗", Style::default().fg(BB_AMBER).add_modifier(bb))));
    lines.push(Line::from(Span::styled(
        "║     ZZIGNAL MONITOR — COMANDOS      ║", Style::default().fg(BB_AMBER).add_modifier(bb))));
    lines.push(Line::from(Span::styled(
        "╚══════════════════════════════════════╝", Style::default().fg(BB_AMBER).add_modifier(bb))));
    lines.push(Line::from(""));
    for (cat, cmds) in &cats {
        lines.push(Line::from(Span::styled(format!("── {cat} ──"), Style::default().fg(BB_CYAN).add_modifier(bb))));
        for cmd in cmds {
            lines.push(Line::from(vec![
                Span::styled(cmd.syntax, Style::default().fg(BB_GREEN).add_modifier(bb)),
                Span::styled(cmd.desc, Style::default().fg(BB_GRAY)),
            ]));
        }
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled("/quit = salir  Esc/q = cerrar  [/] = nuevo comando",
        Style::default().fg(BB_DIM))));
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_gemini_card(f: &mut Frame, area: Rect, s: &State) {
    let bb = Modifier::BOLD;
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled("⚡ GEMINI: ", Style::default().fg(BB_MAGENTA).add_modifier(bb)));
    if s.gemini_active && !s.gemini_triggered {
        spans.push(Span::styled(format!("@{:.2}→{:.2} ${:.0}", s.gemini_trigger, s.gemini_target, s.gemini_budget),
            Style::default().fg(BB_WHITE).add_modifier(bb)));
        if s.gemini_exit > 0.0 {
            spans.push(Span::styled(format!("  EXIT @{:.2}", s.gemini_exit), Style::default().fg(BB_CYAN)));
        }
        spans.push(Span::styled("  /c=cancelar", Style::default().fg(BB_DIM)));
    } else if s.gemini_triggered || s.mt_state >= 1 {
        let out_s = if s.gemini_outcome.is_empty() { &s.mt_outcome } else { &s.gemini_outcome };
        let out_c = if out_s == "up" { BB_GREEN } else { BB_RED };
        spans.push(Span::styled(format!("{} ", out_s.to_uppercase()), Style::default().fg(out_c).add_modifier(bb)));
        spans.push(Span::styled(format!("@{:.4} sz={:.0} ${:.0}", s.mt_entry, s.mt_size, s.mt_budget),
            Style::default().fg(BB_WHITE)));
        match s.mt_state {
            1 => spans.push(Span::styled("  PENDING...", Style::default().fg(BB_AMBER).add_modifier(bb))),
            2 => {
                let cur = if out_s == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn };
                let upnl = s.mt_size * (cur - s.mt_entry);
                let pc = if upnl >= 0.0 { BB_GREEN } else { BB_RED };
                spans.push(Span::styled(format!("  PnL:{:+.2}", upnl), Style::default().fg(pc).add_modifier(bb)));
            }
            3 => spans.push(Span::styled("  EXITING...", Style::default().fg(BB_CYAN).add_modifier(bb))),
            _ => {}
        }
    }
    let border_c = if s.mt_state == 2 { BB_GREEN } else if s.mt_state == 1 || s.mt_state == 3 { BB_AMBER } else { BB_MAGENTA };
    f.render_widget(
        Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::ALL)
            .border_style(Style::default().fg(border_c))),
        area);
}

// ═══════════════════════════════════════════════════════════════════
// INDICATOR CALCS — pure functions for testing
// ═══════════════════════════════════════════════════════════════════

pub fn calc_clob_mom(up_px: f64, dn_px: f64, up_ref: f64, dn_ref: f64) -> (f64, f64, bool, bool, &'static str) {
    let up_d = if up_ref > 0.0 { (up_px / up_ref - 1.0) * 100.0 } else { 0.0 };
    let dn_d = if dn_ref > 0.0 { (dn_px / dn_ref - 1.0) * 100.0 } else { 0.0 };
    let clob_up = up_d >= 0.0;
    let dominant_up = up_d.abs() > dn_d.abs();
    let clob_dir = if dominant_up {
        if clob_up { "▲ UP" } else { "▼ UP" }
    } else {
        if dn_d >= 0.0 { "▲ DN" } else { "▼ DN" }
    };
    (up_d, dn_d, clob_up, dominant_up, clob_dir)
}

pub fn calc_btc_mom(btc: f64, btc_open: f64) -> (f64, bool, &'static str) {
    let btc_d = if btc_open > 0.0 { (btc / btc_open - 1.0) * 100.0 } else { 0.0 };
    let btc_up = btc_d >= 0.0;
    let btc_dir = if btc_up { "▲ BULL" } else { "▼ BEAR" };
    (btc_d, btc_up, btc_dir)
}

pub fn calc_aligned(clob_up: bool, btc_up: bool, up_d: f64, btc_d: f64) -> bool {
    clob_up == btc_up && up_d.abs() > 0.05 && btc_d.abs() > 0.01
}

pub fn calc_vol_ratio(bid_vol: f64, ask_vol: f64) -> (f64, &'static str) {
    let ratio = if ask_vol > 0.0 { bid_vol / ask_vol } else { 1.0 };
    let label = if ratio > 1.5 { "▲BUY" } else if ratio < 0.67 { "▼SELL" } else { "⚖NEUT" };
    (ratio, label)
}

#[cfg(test)]
mod indicator_tests {
    use super::*;

    #[test]
    fn clob_up_strong() {
        let (up_d, dn_d, clob_up, _dom, dir) = calc_clob_mom(0.55, 0.45, 0.50, 0.50);
        assert!((up_d - 10.0).abs() < 0.01);   // (0.55/0.50-1)*100 = +10%
        assert!((dn_d + 10.0).abs() < 0.01);   // (0.45/0.50-1)*100 = -10%
        assert!(clob_up);
        assert_eq!(dir, "▲ UP");
    }

    #[test]
    fn clob_dn_strong() {
        let (up_d, dn_d, clob_up, _dom, dir) = calc_clob_mom(0.45, 0.55, 0.50, 0.50);
        assert!((up_d + 10.0).abs() < 0.01);
        assert!((dn_d - 10.0).abs() < 0.01);
        assert!(!clob_up);
        assert_eq!(dir, "▲ DN"); // DN sube, clob_up sigue
    }

    #[test]
    fn clob_flat() {
        let (up_d, dn_d, clob_up, _dom, dir) = calc_clob_mom(0.50, 0.50, 0.50, 0.50);
        assert!((up_d - 0.0).abs() < 0.01);
        assert!((dn_d - 0.0).abs() < 0.01);
        assert!(clob_up);
        assert_eq!(dir, "▲ DN");  // flat: DN 0%, dominant_up=false → DN branch
    }

    #[test]
    fn btc_bull() {
        let (btc_d, btc_up, dir) = calc_btc_mom(88000.0, 85000.0);
        assert!((btc_d - 3.53).abs() < 0.1);
        assert!(btc_up);
        assert_eq!(dir, "▲ BULL");
    }

    #[test]
    fn btc_bear() {
        let (btc_d, btc_up, dir) = calc_btc_mom(82000.0, 85000.0);
        assert!((btc_d + 3.53).abs() < 0.1);
        assert!(!btc_up);
        assert_eq!(dir, "▼ BEAR");
    }

    #[test]
    fn btc_no_open() {
        let (btc_d, btc_up, dir) = calc_btc_mom(87000.0, 0.0);
        assert!((btc_d - 0.0).abs() < 0.01);
        assert!(btc_up);
        assert_eq!(dir, "▲ BULL");
    }

    #[test]
    fn aligned_true() {
        assert!(calc_aligned(true, true, 5.0, 2.0));
        assert!(calc_aligned(false, false, -5.0, -2.0));
    }

    #[test]
    fn aligned_false() {
        assert!(!calc_aligned(true, false, 5.0, 2.0));       // opposite direction
        assert!(!calc_aligned(true, true, 0.01, 2.0));        // CLOB too flat
        assert!(!calc_aligned(true, true, 5.0, 0.005));       // BTC too flat
        assert!(!calc_aligned(true, true, 0.0, 0.0));         // all zeros
    }

    #[test]
    fn volume_ratio_buy() {
        let (r, label) = calc_vol_ratio(2000.0, 1000.0);
        assert!((r - 2.0).abs() < 0.01);
        assert_eq!(label, "▲BUY");
    }

    #[test]
    fn volume_ratio_sell() {
        let (r, label) = calc_vol_ratio(500.0, 1000.0);
        assert!((r - 0.5).abs() < 0.01);
        assert_eq!(label, "▼SELL");
    }

    #[test]
    fn volume_ratio_neutral() {
        let (r, label) = calc_vol_ratio(1000.0, 1000.0);
        assert!((r - 1.0).abs() < 0.01);
        assert_eq!(label, "⚖NEUT");
    }

    #[test]
    fn volume_ratio_zero_ask() {
        let (r, label) = calc_vol_ratio(500.0, 0.0);
        assert!((r - 1.0).abs() < 0.01);
        assert_eq!(label, "⚖NEUT");
    }
}
