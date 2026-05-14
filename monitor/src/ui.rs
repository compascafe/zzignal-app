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

    // ── BTC CARD: Price to beat + Current + Delta ──
    // btc_open from backend = real session BTC open (Gamma/Pyth)
    // session_open_btc = fallback (live HFT BTC price at monitor session start)
    let btc_ref = if s.btc_open > 0.0 { s.btc_open }
        else if s.session_open_btc > 0.0 { s.session_open_btc }
        else { s.btc };
    let btc_delta = s.btc - btc_ref;
    let btc_delta_pct = if btc_ref > 0.0 { (s.btc / btc_ref - 1.0) * 100.0 } else { 0.0 };
    let btc_up = btc_delta >= 0.0;
    let btc_c = if btc_up { Color::Green } else { Color::Red };
    let arrow = if btc_up { "▲" } else { "▼" };
    // ── Card 1: BTC price NOW ──
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(format!("${:.0}", s.btc), Style::default().fg(Color::White).add_modifier(big)),
            ]),
            Line::from(Span::styled(
                format!("abrio ${:.0}", btc_ref), Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled(
                s.btc_provider.to_uppercase(), Style::default().fg(Color::DarkGray))),
        ]).block(Block::default().borders(Borders::ALL).title("BTC").border_style(Style::default().fg(btc_c))),
        cols[0]);

    // ── Card 2: BTC delta vs open (pulsing) ──
    let delta_pulse = s.pulse_tick % 10 < 7; // 7/10 on, 3/10 off
    let delta_bg = if delta_pulse {
        if btc_up { Color::Green } else { Color::Red }
    } else {
        Color::Reset
    };
    let delta_fg = if delta_pulse { Color::Black } else { btc_c };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(format!("{arrow} "), Style::default().fg(delta_fg).add_modifier(big)),
                Span::styled(format!("${:+.0}", btc_delta), Style::default().fg(delta_fg).add_modifier(big)),
            ]),
            Line::from(Span::styled(
                format!("{:+.1}%", btc_delta_pct),
                Style::default().fg(delta_fg).add_modifier(big))),
            Line::from(Span::styled(
                if btc_up {"▲ UP"} else {"▼ DN"}, Style::default().fg(delta_fg))),
        ]).block(Block::default().borders(Borders::ALL).title("BTC Δ")
            .border_style(Style::default().fg(btc_c))
            .style(Style::default().bg(delta_bg))),
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

// ─── INDICATORS — 6 cards ────────────────────────────────────

fn draw_indicators(f: &mut Frame, area: Rect, s: &State) {
    let cols = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,6); 6]).split(area);
    let b = Modifier::BOLD;

    // ── Aligned signal ──
    let up_ref = if s.session_open_up > 0.0 { s.session_open_up } else { s.hft.clob_trade_up };
    let dn_ref = if s.session_open_dn > 0.0 { s.session_open_dn } else { s.hft.clob_trade_dn };
    let up_d = if up_ref > 0.0 { (s.hft.clob_trade_up / up_ref - 1.0) * 100.0 } else { 0.0 };
    let dn_d = if dn_ref > 0.0 { (s.hft.clob_trade_dn / dn_ref - 1.0) * 100.0 } else { 0.0 };
    let clob_up = up_d >= 0.0;
    let btc_o = if s.btc_open > 0.0 { s.btc_open } else { s.session_open_btc };
    let btc_d = if btc_o > 0.0 { (s.btc / btc_o - 1.0) * 100.0 } else { 0.0 };
    let btc_up = btc_d >= 0.0;
    // 30-second momentum
    let up_30s = if s.clob_up_30s > 0.0 { (s.hft.clob_trade_up / s.clob_up_30s - 1.0) * 100.0 } else { 0.0 };
    let dn_30s = if s.clob_dn_30s > 0.0 { (s.hft.clob_trade_dn / s.clob_dn_30s - 1.0) * 100.0 } else { 0.0 };
    let btc_30s = if s.btc_price_30s > 0.0 { (s.btc / s.btc_price_30s - 1.0) * 100.0 } else { 0.0 };
    // Updated aligned: uses max(|up_d|,|dn_d|) instead of only up_d
    let clob_moved = up_d.abs().max(dn_d.abs()) > 0.05;
    let aligned = (clob_up == btc_up) && clob_moved && btc_d.abs() > 0.01;
    let pulse_on = aligned && (s.pulse_tick % 12) < 8; // blink: 8/12 on, 4/12 off

    // ── S1: CLOB MOM (aligned to BTC direction) ──
    let s1_bg = if pulse_on {
        if btc_up { Color::Green } else { Color::Red }
    } else if aligned {
        Color::Rgb(15, 25, 20)
    } else {
        Color::Reset
    };
    let s1_border = if aligned { if btc_up { Color::Green } else { Color::Red } } else { Color::Rgb(20, 30, 45) };
    let s1_fg = if aligned { Color::Black } else { Color::White };
    let clob_dir = if up_d.abs() > dn_d.abs() {
        if clob_up { "▲UP" } else { "▼UP" }
    } else {
        if dn_d >= 0.0 { "▲DN" } else { "▼DN" }
    };
    let up_30s_c = if up_30s > 0.0 { Color::Green } else if up_30s < 0.0 { Color::Red } else { Color::DarkGray };
    let dn_30s_c = if dn_30s > 0.0 { Color::Green } else if dn_30s < 0.0 { Color::Red } else { Color::DarkGray };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("CLOB MOM", Style::default().fg(if aligned { Color::White } else { Color::Cyan }).add_modifier(b))),
            Line::from(vec![
                Span::styled(format!("{clob_dir} "), Style::default().fg(s1_border).add_modifier(b)),
                Span::styled(format!("ses UP{up_d:+.1}/DN{dn_d:+.1}%"), Style::default().fg(s1_fg)),
            ]),
            Line::from(vec![
                Span::styled("30s ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("UP{up_30s:+.1}"), Style::default().fg(up_30s_c)),
                Span::styled(format!("/DN{dn_30s:+.1}% "), Style::default().fg(dn_30s_c)),
                Span::styled(if aligned {"✓ BTC ✓"}else{"—"}, Style::default().fg(if aligned { Color::Black } else { Color::DarkGray }).add_modifier(b)),
            ]),
        ]).block(Block::default().borders(Borders::ALL).title("S1 CLOB")
            .border_style(Style::default().fg(s1_border))
            .style(Style::default().bg(s1_bg))),
        cols[0]);

    // ── S2: BTC MOM (session + 30s + CLOB lead) ──
    let s2_bg = if pulse_on {
        if btc_up { Color::Green } else { Color::Red }
    } else if aligned {
        Color::Rgb(15, 25, 20)
    } else {
        Color::Reset
    };
    let s2_border = if aligned { if btc_up { Color::Green } else { Color::Red } } else { Color::Rgb(20, 30, 45) };
    let s2_fg = if aligned { Color::Black } else { Color::White };
    let btc_dir = if btc_d >= 0.0 { "▲BULL" } else { "▼BEAR" };
    let btc_30s_c = if btc_30s > 0.0 { Color::Green } else if btc_30s < 0.0 { Color::Red } else { Color::DarkGray };
    // BTC→CLOB lead: how much CLOB lags behind BTC
    let clob_max_d = up_d.abs().max(dn_d.abs());
    let btc_lead = if btc_d.abs() > clob_max_d && btc_d.abs() > 0.05 {
        let lead = btc_d.abs() - clob_max_d;
        (lead > 0.0, lead)
    } else {
        (false, 0.0)
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("BTC MOM", Style::default().fg(if aligned { Color::White } else { Color::Yellow }).add_modifier(b))),
            Line::from(vec![
                Span::styled(format!("${:.0} ", s.btc), Style::default().fg(s2_fg)),
                Span::styled(format!("ses {btc_d:+.1}%"), Style::default().fg(if btc_up { Color::Green } else { Color::Red })),
            ]),
            Line::from(vec![
                Span::styled(format!("{btc_dir} 30s"), Style::default().fg(s2_fg).add_modifier(b)),
                Span::styled(format!("{btc_30s:+.1}%"), Style::default().fg(btc_30s_c)),
                if btc_lead.0 {
                    Span::styled(format!(" →CLOB+{:.1}%", btc_lead.1), Style::default().fg(Color::Cyan).add_modifier(b))
                } else {
                    Span::styled("", Style::default())
                },
            ]),
        ]).block(Block::default().borders(Borders::ALL).title("S2 BTC")
            .border_style(Style::default().fg(s2_border))
            .style(Style::default().bg(s2_bg))),
        cols[1]);

    // ── S3: BTC Δ vs SESSION OPEN (Binance) ──
    // Reglas: [0,7]min → amarillo | (7,14]min → verde +$50, amarillo [-50,+50], rojo -$50 | (14,15]min → amarillo
    let btc_ref = if s.btc_open > 0.0 { s.btc_open }
        else if s.session_open_btc > 0.0 { s.session_open_btc }
        else { s.btc };
    let delta = s.btc - btc_ref;
    let delta_pct = if btc_ref > 0.0 { (s.btc / btc_ref - 1.0) * 100.0 } else { 0.0 };

    let total_secs = 900; // 15-min session
    let elapsed = if s.hft.secs_left >= 0 { (total_secs - s.hft.secs_left).max(0) } else { 0 };
    let elapsed_min = elapsed as f64 / 60.0;

    let (s3_bg, s3_fg, s3_border) = if elapsed_min <= 7.0 {
        (Color::Yellow, Color::Black, Color::Yellow)
    } else if elapsed_min <= 14.0 {
        if delta > 50.0 {
            (Color::Green, Color::Black, Color::Green)
        } else if delta >= -50.0 {
            (Color::Yellow, Color::Black, Color::Yellow)
        } else {
            (Color::Red, Color::Black, Color::Red)
        }
    } else {
        (Color::Yellow, Color::Black, Color::Yellow)
    };

    let s3_title = format!("S3 BTC Δ  [{:.0}m]", elapsed_min);

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("BTC Δ OPEN", Style::default().fg(s3_fg).add_modifier(b))),
            Line::from(Span::styled(
                format!("${:+.0}  {:+.1}%", delta, delta_pct),
                Style::default().fg(s3_fg).add_modifier(b))),
            Line::from(Span::styled(
                format!("min {:.0}/15  ref ${:.0}", elapsed_min, btc_ref),
                Style::default().fg(s3_fg))),
        ]).block(Block::default().borders(Borders::ALL).title(s3_title)
            .border_style(Style::default().fg(s3_border))
            .style(Style::default().bg(s3_bg))),
        cols[2]);

    // ── S4: VELOCIDAD + VOLUMEN BTC (Binance) ──
    let vel = s.btc_velocity;
    let acel = s.btc_acceleration;
    let vel_dir = if vel >= 0.0 { "▲" } else { "▼" };
    let acel_dir = if acel >= 0.0 { "▲" } else { "▼" };
    let vel_color = if vel > 0.5 { Color::Green } else if vel < -0.5 { Color::Red } else { Color::Yellow };

    let vol_1m = s.btc_vol_1m;
    let vol_ses = s.hft.btc_vol_ses;

    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(format!("{vel_dir} "), Style::default().fg(vel_color).add_modifier(b)),
                Span::styled(format!("${vel:+.2}/s  "), Style::default().fg(vel_color)),
                Span::styled(format!("{acel_dir} ${acel:+.2}/s²"), Style::default().fg(Color::Magenta)),
            ]),
            Line::from(Span::styled(
                format!("1m {vol_1m:.0} BTC"),
                Style::default().fg(if vol_1m > 25.0 { Color::Green } else if vol_1m > 10.0 { Color::Yellow } else { Color::DarkGray }))),
            Line::from(Span::styled(
                format!("ses {vol_ses:.0} BTC"),
                Style::default().fg(if vol_ses > 200.0 { Color::Green } else if vol_ses > 50.0 { Color::Cyan } else { Color::DarkGray }))),
        ]).block(Block::default().borders(Borders::ALL).title("S4 VEL+VOL").border_style(Style::default().fg(Color::Rgb(20, 30, 45)))),
        cols[3]);

    // ── S5: ORDER BOOK IMBALANCE (per-side B/A from depth) ──
    let up_bid_v: f64 = s.hft.depth_up_bids.iter().map(|(_,s)| s).sum();
    let up_ask_v: f64 = s.hft.depth_up_asks.iter().map(|(_,s)| s).sum();
    let dn_bid_v: f64 = s.hft.depth_dn_bids.iter().map(|(_,s)| s).sum();
    let dn_ask_v: f64 = s.hft.depth_dn_asks.iter().map(|(_,s)| s).sum();
    let up_imb = if up_ask_v > 0.0 { up_bid_v / up_ask_v } else { 1.0 };
    let dn_imb = if dn_ask_v > 0.0 { dn_bid_v / dn_ask_v } else { 1.0 };
    // Combined: more UP bids + DN asks = bullish, more UP asks + DN bids = bearish
    let total_bull = up_bid_v + dn_ask_v;  // bids on UP + asks on DN = bullish pressure
    let total_bear = up_ask_v + dn_bid_v;  // asks on UP + bids on DN = bearish pressure
    let comb_imb = if total_bear > 0.0 { total_bull / total_bear } else { 1.0 };
    let (imb_bg, imb_fg, imb_border, imb_label) = if comb_imb > 1.15 {
        (Color::Green, Color::White, Color::Green, "▲ UP")
    } else if comb_imb < 0.85 {
        (Color::Red, Color::White, Color::Red, "▼ DN")
    } else {
        (Color::Yellow, Color::Black, Color::Yellow, "—")
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("IMBALANCE", Style::default().fg(imb_fg).add_modifier(b))),
            Line::from(Span::styled(
                format!("UP {up_imb:.2}x"), Style::default().fg(imb_fg).add_modifier(b))),
            Line::from(Span::styled(
                format!("{imb_label}  DN {dn_imb:.2}x"),
                Style::default().fg(imb_fg).add_modifier(b))),
        ]).block(Block::default().borders(Borders::ALL).title("S5 IMB")
            .border_style(Style::default().fg(imb_border))
            .style(Style::default().bg(imb_bg))),
        cols[4]);

    // ── S6: DEPTH ABSORPTION (bid/ask size delta %) ──
    let abs_up_c = if s.abs_up > 2.0 { Color::Green } else if s.abs_up < -2.0 { Color::Red } else { Color::Yellow };
    let abs_dn_c = if s.abs_dn > 2.0 { Color::Green } else if s.abs_dn < -2.0 { Color::Red } else { Color::Yellow };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("ABSORPTION", Style::default().fg(Color::Cyan).add_modifier(b))),
            Line::from(Span::styled(
                format!("UP {:+.3}%", s.abs_up), Style::default().fg(abs_up_c).add_modifier(b))),
            Line::from(Span::styled(
                format!("DN {:+.3}%", s.abs_dn), Style::default().fg(abs_dn_c).add_modifier(b))),
        ]).block(Block::default().borders(Borders::ALL).title("S6 ABS").border_style(Style::default().fg(Color::Rgb(20, 30, 45)))),
        cols[5]);
}

// ─── INDICATORS — 6 cards row 2 ──────────────────────────────

fn draw_indicators_row2(f: &mut Frame, area: Rect, s: &State) {
    let cols = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,6); 6]).split(area);
    let b = Modifier::BOLD;

    // ── S7: SPREAD ──
    let spread_val = s.hft.spread;
    let (s7_bg, s7_fg, s7_border, s7_label) = if spread_val < 0.003 {
        (Color::Green, Color::Black, Color::Green, "TIGHT ▲")
    } else if spread_val < 0.008 {
        (Color::Yellow, Color::Black, Color::Yellow, "MED —")
    } else {
        (Color::Red, Color::Black, Color::Red, "WIDE ▼")
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("SPREAD", Style::default().fg(s7_fg).add_modifier(b))),
            Line::from(Span::styled(format!("{:.4}", spread_val), Style::default().fg(s7_fg).add_modifier(b))),
            Line::from(Span::styled(s7_label, Style::default().fg(s7_fg))),
        ]).block(Block::default().borders(Borders::ALL).title("S7 B/A")
            .border_style(Style::default().fg(s7_border))
            .style(Style::default().bg(s7_bg))),
        cols[0]);

    // ── S8: DUMP SCORE ──
    let dump = s.hft.dump_score;
    let (s8_bg, s8_fg, s8_border, s8_label) = if dump == 0 {
        (Color::Green, Color::Black, Color::Green, "SAFE")
    } else if dump <= 2 {
        (Color::Yellow, Color::Black, Color::Yellow, "WARN")
    } else {
        (Color::Red, Color::Black, Color::Red, "DUMP!")
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("DUMP", Style::default().fg(s8_fg).add_modifier(b))),
            Line::from(Span::styled(format!("{}/3", dump), Style::default().fg(s8_fg).add_modifier(b))),
            Line::from(Span::styled(s8_label, Style::default().fg(s8_fg))),
        ]).block(Block::default().borders(Borders::ALL).title("S8 DC")
            .border_style(Style::default().fg(s8_border))
            .style(Style::default().bg(s8_bg))),
        cols[1]);

    // ── S9: TICK GAP ──
    let gap = s.hft.tick_gap_ms;
    let (s9_bg, s9_fg, s9_border, s9_label) = if gap < 500 {
        (Color::Green, Color::Black, Color::Green, "FAST ▲")
    } else if gap < 2000 {
        (Color::Yellow, Color::Black, Color::Yellow, "SLOW —")
    } else {
        (Color::Red, Color::Black, Color::Red, "FROZEN ▼")
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("TICK", Style::default().fg(s9_fg).add_modifier(b))),
            Line::from(Span::styled(format!("{}ms", gap), Style::default().fg(s9_fg).add_modifier(b))),
            Line::from(Span::styled(s9_label, Style::default().fg(s9_fg))),
        ]).block(Block::default().borders(Borders::ALL).title("S9 MS")
            .border_style(Style::default().fg(s9_border))
            .style(Style::default().bg(s9_bg))),
        cols[2]);

    // ── S10: SPOOF + ASK WALL ──
    let spoof_val = s.hft.spoof;
    let wall_val = s.hft.ask_wall;
    let spoof_risk = spoof_val + wall_val; // 0=clean, 1=warning, 2=danger
    let (s10_bg, s10_fg, s10_border, s10_label) = if spoof_risk == 0 {
        (Color::Green, Color::Black, Color::Green, "CLEAN")
    } else if spoof_risk == 1 {
        (Color::Yellow, Color::Black, Color::Yellow, "FLAG ⚠")
    } else {
        (Color::Red, Color::Black, Color::Red, "TRAP!")
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("SPOOF+WALL", Style::default().fg(s10_fg).add_modifier(b))),
            Line::from(Span::styled(
                format!("S:{} W:{}", spoof_val, wall_val),
                Style::default().fg(s10_fg).add_modifier(b))),
            Line::from(Span::styled(s10_label, Style::default().fg(s10_fg))),
        ]).block(Block::default().borders(Borders::ALL).title("S10 SW")
            .border_style(Style::default().fg(s10_border))
            .style(Style::default().bg(s10_bg))),
        cols[3]);

    // ── S11: LIQUIDITY CONCENTRATION ──
    // How much of total depth is in top 3 levels → whale walls
    let up_total: f64 = s.hft.depth_up_bids.iter().map(|(_,s)| s).sum::<f64>()
        + s.hft.depth_up_asks.iter().map(|(_,s)| s).sum::<f64>();
    let up_top3: f64 = s.hft.depth_up_bids.iter().take(3).map(|(_,s)| s).sum::<f64>()
        + s.hft.depth_up_asks.iter().take(3).map(|(_,s)| s).sum::<f64>();
    let dn_total: f64 = s.hft.depth_dn_bids.iter().map(|(_,s)| s).sum::<f64>()
        + s.hft.depth_dn_asks.iter().map(|(_,s)| s).sum::<f64>();
    let dn_top3: f64 = s.hft.depth_dn_bids.iter().take(3).map(|(_,s)| s).sum::<f64>()
        + s.hft.depth_dn_asks.iter().take(3).map(|(_,s)| s).sum::<f64>();
    let conc_up = if up_total > 0.0 { up_top3 / up_total } else { 0.0 };
    let conc_dn = if dn_total > 0.0 { dn_top3 / dn_total } else { 0.0 };
    let conc_max = conc_up.max(conc_dn);
    let (s11_bg, s11_fg, s11_border, s11_label) = if conc_max < 0.5 {
        (Color::Green, Color::Black, Color::Green, "SPREAD ▲")
    } else if conc_max < 0.75 {
        (Color::Yellow, Color::Black, Color::Yellow, "WHALE? —")
    } else {
        (Color::Red, Color::Black, Color::Red, "WALL ▼")
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("CONC", Style::default().fg(s11_fg).add_modifier(b))),
            Line::from(Span::styled(
                format!("U{:.0}% D{:.0}%", conc_up*100.0, conc_dn*100.0),
                Style::default().fg(s11_fg).add_modifier(b))),
            Line::from(Span::styled(s11_label, Style::default().fg(s11_fg))),
        ]).block(Block::default().borders(Borders::ALL).title("S11 LIQ")
            .border_style(Style::default().fg(s11_border))
            .style(Style::default().bg(s11_bg))),
        cols[4]);

    // ── S12: CLOB-BTC DIVERGENCE ──
    // Compare CLOB direction vs BTC direction since session start
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
    let (s12_bg, s12_fg, s12_border, s12_label) = if aligned {
        (Color::Green, Color::Black, Color::Green, "LOCKED ▲")
    } else if clob_dom < 0.1 || btc_d.abs() < 0.02 {
        (Color::Yellow, Color::Black, Color::Yellow, "FLAT —")
    } else {
        (Color::Red, Color::Black, Color::Red, "DIVERGE ▼")
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("C↔B DIV", Style::default().fg(s12_fg).add_modifier(b))),
            Line::from(Span::styled(
                format!("C{clob_dom:+.1}% B{btc_d:+.1}%"),
                Style::default().fg(s12_fg).add_modifier(b))),
            Line::from(Span::styled(s12_label, Style::default().fg(s12_fg))),
        ]).block(Block::default().borders(Borders::ALL).title("S12 DIV")
            .border_style(Style::default().fg(s12_border))
            .style(Style::default().bg(s12_bg))),
        cols[5]);
}

// ─── MANUAL TRADING STATUS BAR ────────────────────────────────────

fn draw_manual_status(f: &mut Frame, area: Rect, s: &State) {
    let b = Modifier::BOLD;
    let mut spans: Vec<Span> = Vec::new();

    spans.push(Span::styled("MANUAL: ", Style::default().fg(Color::DarkGray).add_modifier(b)));

    match s.mt_state {
        0 => {
            spans.push(Span::styled("IDLE", Style::default().fg(Color::DarkGray)));
            spans.push(Span::styled("  0 posiciones", Style::default().fg(Color::DarkGray)));
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
            spans.push(Span::styled(format!("entry:{:.4}→{:.4} ", s.mt_entry, current_px),
                Style::default().fg(Color::White)));

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
            spans.push(Span::styled(format!("exit @{:.4}  sz={:.0}",
                exit_px, s.mt_size), Style::default().fg(Color::White)));
            spans.push(Span::styled("  /c=CANCELAR EXIT", Style::default().fg(Color::DarkGray)));
        }
        _ => {}
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

// ─── TRADE LOG INLINE ─────────────────────────────────────────────

fn draw_trade_log_inline(f: &mut Frame, area: Rect, s: &State) {
    let max_n = (area.height as usize).saturating_sub(2).min(30);
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

    draw_book_side(f, chunks[0], "UP", Color::Green, &s.book_up, &s.hft.depth_up_bids, &s.hft.depth_up_asks);
    draw_book_side(f, chunks[1], "DOWN", Color::Red, &s.book_dn, &s.hft.depth_dn_bids, &s.hft.depth_dn_asks);
}

fn draw_book_side(f: &mut Frame, area: Rect, label: &str, border_c: Color,
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

    let avail = (area.height as usize).saturating_sub(2);
    let half = (avail.saturating_sub(1) / 2).min(9).max(3);

    asks.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let top_asks: Vec<_> = asks.into_iter().take(half).rev().collect();

    bids.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let top_bids: Vec<_> = bids.into_iter().take(half).collect();

    let max_size = top_asks.iter().map(|&(_,s)| s)
        .chain(top_bids.iter().map(|&(_,s)| s))
        .fold(0.0f64, f64::max).max(1.0);
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
        let c = if is_ceiling { Color::Yellow } else { Color::Red };
        lines.push(Line::from(vec![
            Span::styled(format!("{:.4} ", price), Style::default().fg(c)),
            Span::styled(bar, Style::default().fg(Color::Red)),
            Span::styled(format!(" {:.0}", size), Style::default().fg(Color::DarkGray)),
        ]));
    }

    if best_bid > 0.0 && best_ask > 0.0 {
        let s_label = format!("── SPREAD {spread:.4} ── MID {mid:.4} ──");
        lines.push(Line::from(vec![
            Span::styled(s_label, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
    }

    for &(price, size) in top_bids.iter() {
        let log_sz = (size + 1.0).ln();
        let w = if log_max > 0.0 { (log_sz / log_max * bar_w as f64) as usize } else { 0 };
        let bar = "█".repeat(w.min(bar_w));
        let is_floor = (price - best_bid).abs() < 0.0001;
        let c = if is_floor { Color::Yellow } else { Color::Green };
        lines.push(Line::from(vec![
            Span::styled(format!("{:.4} ", price), Style::default().fg(c)),
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
    let b = Modifier::BOLD;

    let (pos_text, pos_style) = if s.mt_state >= 2 {
        let current_px = if s.mt_outcome == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn };
        let entry = if s.mt_fill_avg > 0.0 { s.mt_fill_avg } else { s.mt_entry };
        let gain = (current_px - entry) >= 0.0;
        let bg = if gain { Color::Green } else { Color::Red };
        let side_sym = if s.mt_outcome == "up" { "▲" } else { "▼" };
        let tsl = if s.mt_tsl_pct > 0.0 { format!(" TSL:{:.0}%", s.mt_tsl_pct) } else { String::new() };
        let sl = if s.sl_pct > 0.0 { format!(" SL:{:.0}%", s.sl_pct) } else { String::new() };
        let exit_info = if s.mt_exit_price > 0.0 { format!(" TP:{:.4}", s.mt_exit_price) } else { String::new() };
        let txt = format!("{side_sym} POS {} sz={:.0} entry={:.4}→{:.4}{tsl}{sl}{exit_info}",
            s.mt_outcome.to_uppercase(), s.mt_size, entry, current_px);
        (txt, Style::default().fg(Color::Black).bg(bg).add_modifier(b))
    } else if s.mt_state == 1 {
        let side_sym = if s.mt_outcome == "up" { "▲" } else { "▼" };
        let txt = format!("{side_sym} PENDING {} sz={:.0} @{:.4} ${:.2}  [{:.0}% filled]  /c=CANCELAR",
            s.mt_outcome.to_uppercase(), s.mt_size, s.mt_entry, s.mt_budget, s.mt_last_fill_pct);
        (txt, Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(b))
    } else {
        (format!("0 POSICIONES  —  /4up65 para abrir"),
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
// GEMINI CARD — footer inline
// ═══════════════════════════════════════════════════════════════════

fn draw_gemini_card(f: &mut Frame, area: Rect, s: &State) {
    let b = Modifier::BOLD;
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled("⚡ GEMINI: ", Style::default().fg(Color::Magenta).add_modifier(b)));

    if s.gemini_active && !s.gemini_triggered {
        spans.push(Span::styled(
            format!("@{:.2}→{:.2} ${:.0}", s.gemini_trigger, s.gemini_target, s.gemini_budget),
            Style::default().fg(Color::White).add_modifier(b)));
        if s.gemini_exit > 0.0 {
            spans.push(Span::styled(format!("  EXIT @{:.2}", s.gemini_exit), Style::default().fg(Color::Cyan)));
        }
        spans.push(Span::styled("  /c=cancelar", Style::default().fg(Color::DarkGray)));
    } else if s.gemini_triggered || s.mt_state >= 1 {
        let out_s = if s.gemini_outcome.is_empty() { &s.mt_outcome } else { &s.gemini_outcome };
        let out_c = if out_s == "up" { Color::Green } else { Color::Red };
        spans.push(Span::styled(format!("{} ", out_s.to_uppercase()), Style::default().fg(out_c).add_modifier(b)));
        spans.push(Span::styled(
            format!("@{:.4} sz={:.0} ${:.0}", s.mt_entry, s.mt_size, s.mt_budget),
            Style::default().fg(Color::White)));
        match s.mt_state {
            1 => spans.push(Span::styled("  PENDING...", Style::default().fg(Color::Yellow).add_modifier(b))),
            2 => {
                let cur = if out_s == "up" { s.hft.clob_trade_up } else { s.hft.clob_trade_dn };
                let upnl = s.mt_size * (cur - s.mt_entry);
                let pc = if upnl >= 0.0 { Color::Green } else { Color::Red };
                spans.push(Span::styled(format!("  PnL:{:+.2}", upnl), Style::default().fg(pc).add_modifier(b)));
            }
            3 => spans.push(Span::styled("  EXITING...", Style::default().fg(Color::Cyan).add_modifier(b))),
            _ => {}
        }
    }

    let border_c = if s.mt_state == 2 { Color::Green }
        else if s.mt_state == 1 || s.mt_state == 3 { Color::Yellow }
        else { Color::Magenta };
    f.render_widget(
        Paragraph::new(Line::from(spans))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(border_c))),
        area);
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
            Span::styled(" /4up65 /4d65  ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled("BUY        ", Style::default().fg(Color::Gray)),
            Span::styled("/lup70 /ld70  ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("SELL limit  ", Style::default().fg(Color::Gray)),
            Span::styled("/co", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled(" cash out  ", Style::default().fg(Color::Gray)),
            Span::styled("/c", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" cancel  ", Style::default().fg(Color::Gray)),
            Span::styled("/x", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::styled(" mkt sell", Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled("/4up65e70 ", Style::default().fg(Color::Green)),
            Span::styled("BUY+exit  ", Style::default().fg(Color::DarkGray)),
            Span::styled("/sl5 /sl ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("stop-loss ", Style::default().fg(Color::DarkGray)),
            Span::styled("/nsl ", Style::default().fg(Color::DarkGray)),
            Span::styled("off  ", Style::default().fg(Color::DarkGray)),
            Span::styled("/tsl2 ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::styled("trail stop  ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("SL:{sl_info} TSL:{tsl_info}", sl_info=sl_info, tsl_info=tsl_info), Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled(" [/]comando  [Tab]vista  [Esc/q]salir", Style::default().fg(Color::Rgb(30, 40, 55))),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), area);
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
