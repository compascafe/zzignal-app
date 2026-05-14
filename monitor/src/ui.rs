use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::InputMode;
use crate::State;

const TAB_NAMES: &[&str] = &["DINERO REAL", "PAPER MONEY", "GRAFICOS"];

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
    let filler_w = total_w.saturating_sub(commit.len() + 19);
    let filler = " ".repeat(filler_w.min(80));
    f.render_widget(
        Paragraph::new(format!("|ZZIGNAL{filler}{commit}|"))
            .style(Style::default().fg(BB_AMBER).add_modifier(Modifier::BOLD)),
        chunks[ci],
    );
    ci += 1;

    // ─── TAB BAR ──────────────────────────────────────────────────────
    let tab_spans: Vec<Span> = TAB_NAMES.iter().enumerate().flat_map(|(i, name)| {
        let (fg, bg) = if i == s.tab {
            match i {
                0 => (BB_WHITE, BB_RED),
                1 => (BB_WHITE, BB_GREEN),
                _ => (BB_WHITE, BB_CYAN),
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
    let s1_fg = if aligned { BB_WHITE } else { if btc_up { BB_GREEN } else { BB_RED } };
    let clob_dir = if up_d.abs() > dn_d.abs() {
        if clob_up { "▲UP" } else { "▼UP" }
    } else {
        if dn_d >= 0.0 { "▲DN" } else { "▼DN" }
    };
    let up_30s_c = if up_30s > 0.0 { BB_GREEN } else if up_30s < 0.0 { BB_RED } else { BB_DIM };
    let dn_30s_c = if dn_30s > 0.0 { BB_GREEN } else if dn_30s < 0.0 { BB_RED } else { BB_DIM };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("CLOB MOM", Style::default().fg(BB_AMBER).add_modifier(b))),
            Line::from(vec![
                Span::styled(format!("{clob_dir} "), Style::default().fg(s1_border).add_modifier(b)),
                Span::styled(format!("ses UP{up_d:+.1}/DN{dn_d:+.1}%"), Style::default().fg(BB_WHITE)),
            ]),
            Line::from(vec![
                Span::styled("30s ", Style::default().fg(BB_DIM)),
                Span::styled(format!("UP{up_30s:+.1}"), Style::default().fg(up_30s_c)),
                Span::styled(format!("/DN{dn_30s:+.1}% "), Style::default().fg(dn_30s_c)),
                Span::styled(if aligned && pulse_on {"✓ BTC ✓"}else{"—"},
                    Style::default().fg(if aligned&&pulse_on{BB_GREEN}else{BB_DIM}).add_modifier(b)),
            ]),
        ]).block(Block::default().borders(Borders::ALL).title("S1 CLOB")
            .border_style(Style::default().fg(s1_border))
            .style(Style::default().bg(BB_CARD))),
        cols[0]);

    // ── S2: BTC MOM ──
    let s2_border = if aligned { if btc_up { BB_GREEN } else { BB_RED } } else { BB_BORDER };
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
            Line::from(Span::styled("BTC MOM", Style::default().fg(BB_AMBER).add_modifier(b))),
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
            Line::from(Span::styled("BTC Δ OPEN", Style::default().fg(BB_AMBER).add_modifier(b))),
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
            Line::from(Span::styled("IMBALANCE", Style::default().fg(BB_AMBER).add_modifier(b))),
            Line::from(Span::styled(format!("UP {up_imb:.2}x"),
                Style::default().fg(if up_imb>1.1{BB_GREEN}else if up_imb<0.9{BB_RED}else{BB_AMBER}).add_modifier(b))),
            Line::from(Span::styled(format!("{imb_label}  DN {dn_imb:.2}x"),
                Style::default().fg(BB_WHITE).add_modifier(b))),
        ]).block(Block::default().borders(Borders::ALL).title("S5 IMB")
            .border_style(Style::default().fg(imb_border))
            .style(Style::default().bg(BB_CARD))),
        cols[4]);

    // ── S6: DEPTH ABSORPTION ──
    let abs_up_c = if s.abs_up > 2.0 { BB_GREEN } else if s.abs_up < -2.0 { BB_RED } else { BB_AMBER };
    let abs_dn_c = if s.abs_dn > 2.0 { BB_GREEN } else if s.abs_dn < -2.0 { BB_RED } else { BB_AMBER };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("ABSORPTION", Style::default().fg(BB_CYAN).add_modifier(b))),
            Line::from(Span::styled(format!("UP {:+.3}%", s.abs_up),
                Style::default().fg(abs_up_c).add_modifier(b))),
            Line::from(Span::styled(format!("DN {:+.3}%", s.abs_dn),
                Style::default().fg(abs_dn_c).add_modifier(b))),
        ]).block(Block::default().borders(Borders::ALL).title("S6 ABS")
            .border_style(Style::default().fg(BB_BORDER))
            .style(Style::default().bg(BB_CARD))),
        cols[5]);
}

// ─── INDICATORS — 6 cards row 2 ──────────────────────────────

fn draw_indicators_row2(f: &mut Frame, area: Rect, s: &State) {
    let cols = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,6); 6]).split(area);
    let b = Modifier::BOLD;

    // ── S7: SPREAD ──
    let spread_val = s.hft.spread;
    let (s7_border, s7_label) = if spread_val < 0.003 {
        (BB_GREEN, "TIGHT ▲")
    } else if spread_val < 0.008 {
        (BB_AMBER, "MED —")
    } else {
        (BB_RED, "WIDE ▼")
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("SPREAD", Style::default().fg(BB_AMBER).add_modifier(b))),
            Line::from(Span::styled(format!("{:.4}", spread_val), Style::default().fg(BB_WHITE).add_modifier(b))),
            Line::from(Span::styled(s7_label, Style::default().fg(s7_border))),
        ]).block(Block::default().borders(Borders::ALL).title("S7 B/A")
            .border_style(Style::default().fg(s7_border))
            .style(Style::default().bg(BB_CARD))),
        cols[0]);

    // ── S8: DUMP SCORE ──
    let dump = s.hft.dump_score;
    let (s8_border, s8_label) = if dump == 0 {
        (BB_GREEN, "SAFE")
    } else if dump <= 2 {
        (BB_AMBER, "WARN")
    } else {
        (BB_RED, "DUMP!")
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("DUMP", Style::default().fg(BB_AMBER).add_modifier(b))),
            Line::from(Span::styled(format!("{}/3", dump), Style::default().fg(BB_WHITE).add_modifier(b))),
            Line::from(Span::styled(s8_label, Style::default().fg(s8_border))),
        ]).block(Block::default().borders(Borders::ALL).title("S8 DC")
            .border_style(Style::default().fg(s8_border))
            .style(Style::default().bg(BB_CARD))),
        cols[1]);

    // ── S9: TICK GAP ──
    let gap = s.hft.tick_gap_ms;
    let (s9_border, s9_label) = if gap < 500 {
        (BB_GREEN, "FAST ▲")
    } else if gap < 2000 {
        (BB_AMBER, "SLOW —")
    } else {
        (BB_RED, "FROZEN ▼")
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("TICK", Style::default().fg(BB_AMBER).add_modifier(b))),
            Line::from(Span::styled(format!("{}ms", gap), Style::default().fg(BB_WHITE).add_modifier(b))),
            Line::from(Span::styled(s9_label, Style::default().fg(s9_border))),
        ]).block(Block::default().borders(Borders::ALL).title("S9 MS")
            .border_style(Style::default().fg(s9_border))
            .style(Style::default().bg(BB_CARD))),
        cols[2]);

    // ── S10: SPOOF + ASK WALL ──
    let spoof_risk = s.hft.spoof + s.hft.ask_wall;
    let (s10_border, s10_label) = if spoof_risk == 0 {
        (BB_GREEN, "CLEAN")
    } else if spoof_risk == 1 {
        (BB_AMBER, "FLAG ⚠")
    } else {
        (BB_RED, "TRAP!")
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("SPOOF+WALL", Style::default().fg(BB_AMBER).add_modifier(b))),
            Line::from(Span::styled(format!("S:{} W:{}", s.hft.spoof, s.hft.ask_wall),
                Style::default().fg(BB_WHITE).add_modifier(b))),
            Line::from(Span::styled(s10_label, Style::default().fg(s10_border))),
        ]).block(Block::default().borders(Borders::ALL).title("S10 SW")
            .border_style(Style::default().fg(s10_border))
            .style(Style::default().bg(BB_CARD))),
        cols[3]);

    // ── S11: LIQUIDITY CONCENTRATION ──
    let up_total: f64 = s.hft.depth_up_bids.iter().map(|(_,s)| s).sum::<f64>()
        + s.hft.depth_up_asks.iter().map(|(_,s)| s).sum::<f64>();
    let up_top3: f64 = s.hft.depth_up_bids.iter().take(3).map(|(_,s)| s).sum::<f64>()
        + s.hft.depth_up_asks.iter().take(3).map(|(_,s)| s).sum::<f64>();
    let dn_total: f64 = s.hft.depth_dn_bids.iter().map(|(_,s)| s).sum::<f64>()
        + s.hft.depth_dn_asks.iter().map(|(_,s)| s).sum::<f64>();
    let dn_top3: f64 = s.hft.depth_dn_bids.iter().take(3).map(|(_,s)| s).sum::<f64>()
        + s.hft.depth_dn_asks.iter().take(3).map(|(_,s)| s).sum::<f64>();
    let conc_max = if up_total>0.0 {up_top3/up_total} else{0.0}
        .max(if dn_total>0.0 {dn_top3/dn_total} else{0.0});
    let (s11_border, s11_label) = if conc_max < 0.5 {
        (BB_GREEN, "SPREAD ▲")
    } else if conc_max < 0.75 {
        (BB_AMBER, "WHALE? —")
    } else {
        (BB_RED, "WALL ▼")
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("CONC", Style::default().fg(BB_AMBER).add_modifier(b))),
            Line::from(Span::styled(format!("max {:.0}%", conc_max*100.0),
                Style::default().fg(BB_WHITE).add_modifier(b))),
            Line::from(Span::styled(s11_label, Style::default().fg(s11_border))),
        ]).block(Block::default().borders(Borders::ALL).title("S11 LIQ")
            .border_style(Style::default().fg(s11_border))
            .style(Style::default().bg(BB_CARD))),
        cols[4]);

    // ── S12: CLOB-BTC DIVERGENCE ──
    let up_ref = if s.session_open_up > 0.0 { s.session_open_up } else { s.hft.clob_trade_up };
    let dn_ref = if s.session_open_dn > 0.0 { s.session_open_dn } else { s.hft.clob_trade_dn };
    let btc_open = if s.btc_open > 0.0 { s.btc_open } else if s.session_open_btc > 0.0 { s.session_open_btc } else { s.btc };
    let clob_up_d = if up_ref > 0.0 { (s.hft.clob_trade_up / up_ref - 1.0) * 100.0 } else { 0.0 };
    let clob_dn_d = if dn_ref > 0.0 { (s.hft.clob_trade_dn / dn_ref - 1.0) * 100.0 } else { 0.0 };
    let btc_d = if btc_open > 0.0 { (s.btc / btc_open - 1.0) * 100.0 } else { 0.0 };
    let clob_dom = clob_up_d.abs().max(clob_dn_d.abs());
    let clob_dir = if clob_up_d.abs() > clob_dn_d.abs() { clob_up_d >= 0.0 } else { clob_dn_d >= 0.0 };
    let btc_dir = btc_d >= 0.0;
    let aligned = clob_dir == btc_dir && clob_dom > 0.1 && btc_d.abs() > 0.02;
    let (s12_border, s12_label) = if aligned {
        (BB_GREEN, "LOCKED ▲")
    } else if clob_dom < 0.1 || btc_d.abs() < 0.02 {
        (BB_AMBER, "FLAT —")
    } else {
        (BB_RED, "DIVERGE ▼")
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("C↔B DIV", Style::default().fg(BB_AMBER).add_modifier(b))),
            Line::from(Span::styled(format!("C{clob_dom:+.1}% B{btc_d:+.1}%"),
                Style::default().fg(BB_WHITE).add_modifier(b))),
            Line::from(Span::styled(s12_label, Style::default().fg(s12_border))),
        ]).block(Block::default().borders(Borders::ALL).title("S12 DIV")
            .border_style(Style::default().fg(s12_border))
            .style(Style::default().bg(BB_CARD))),
        cols[5]);
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
            spans.push(Span::styled("| /lupXX /lm /c", Style::default().fg(BB_DIM)));
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
    let log_max = (max_size + 1.0).ln();

    let bar_w = area.width.saturating_sub(18) as usize;
    let mut lines: Vec<Line> = Vec::new();
    let bid_bg = if label == "UP" { Color::Green } else { Color::Red };

    for &(price, size) in asks.iter().take(n).rev() {
        let log_sz = (size + 1.0).ln();
        let w = if log_max > 0.0 { (log_sz / log_max * bar_w as f64) as usize } else { 0 };
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
        let log_sz = (size + 1.0).ln();
        let w = if log_max > 0.0 { (log_sz / log_max * bar_w as f64) as usize } else { 0 };
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
            Span::styled(" /4up65 /4d65  ", Style::default().fg(BB_GREEN).add_modifier(Modifier::BOLD)),
            Span::styled("BUY        ", Style::default().fg(BB_GRAY)),
            Span::styled("/lup70 /ld70  ", Style::default().fg(BB_AMBER).add_modifier(Modifier::BOLD)),
            Span::styled("SELL limit  ", Style::default().fg(BB_GRAY)),
            Span::styled("/co", Style::default().fg(BB_RED).add_modifier(Modifier::BOLD)),
            Span::styled(" cash out  ", Style::default().fg(BB_GRAY)),
            Span::styled("/c", Style::default().fg(BB_AMBER).add_modifier(Modifier::BOLD)),
            Span::styled(" cancel  ", Style::default().fg(BB_GRAY)),
            Span::styled("/x", Style::default().fg(BB_MAGENTA).add_modifier(Modifier::BOLD)),
            Span::styled(" mkt sell", Style::default().fg(BB_GRAY)),
        ]),
        Line::from(vec![
            Span::styled("/4up65e70 ", Style::default().fg(BB_GREEN)),
            Span::styled("BUY+exit  ", Style::default().fg(BB_DIM)),
            Span::styled("/sl5 /sl ", Style::default().fg(BB_AMBER).add_modifier(Modifier::BOLD)),
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
    let help = "/l10up65e70  BUY+EXIT  |  /clup65  cancel+liq  |  /lup70 /ld70  liq limit";
    let help2 = "/c  cancel  |  /lm  liq mercado  |  /clm  cancel+mkt  |  /p  PANIC";
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
