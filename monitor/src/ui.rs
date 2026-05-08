use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use ratatui::Frame;

use super::State;

const TAB_NAMES: &[&str] = &["Dashboard", "Trading", "Sessions", "Signals"];

pub fn draw(f: &mut Frame, s: &State) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(2)])
        .split(area);

    // ─── TAB BAR ──────────────────────────────────────────────────────
    let tab_titles: Vec<Line> = TAB_NAMES.iter().enumerate().map(|(i, name)| {
        let style = if i == s.tab {
            Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        Line::from(Span::styled(format!(" {name} "), style))
    }).collect();
    f.render_widget(Tabs::new(tab_titles).block(Block::default().borders(Borders::BOTTOM)), chunks[0]);

    // ─── MAIN CONTENT ─────────────────────────────────────────────────
    match s.tab {
        0 => draw_dashboard(f, chunks[1], s),
        1 => draw_trading(f, chunks[1], s),
        2 => draw_sessions(f, chunks[1], s),
        3 => draw_signals(f, chunks[1], s),
        _ => {}
    }

    // ─── FOOTER ───────────────────────────────────────────────────────
    let footer_text = match s.tab {
        0 => "[←→]tab [l]LIVE [p]PANIC [r]Reinv [o]Odi83 [h]H65 [q]quit",
        1 => "[←→]tab [1-4]Odi-budget [5-8]H65-budget [a/z]all-ON/OFF [f]filters [t]only-this [q]quit",
        2 => "[←→]tab [s]start-session [S]stop-session [e]export [↑↓]select [q]quit",
        3 => "[←→]tab [space]pause [q]quit",
        _ => "[q]quit",
    };
    f.render_widget(
        Paragraph::new(footer_text).style(Style::default().fg(Color::DarkGray)),
        chunks[2],
    );
}

// ═══════════════════════════════════════════════════════════════════
// TAB 0: DASHBOARD
// ═══════════════════════════════════════════════════════════════════

fn draw_dashboard(f: &mut Frame, area: Rect, s: &State) {
    let warn_h = if s.warnings.is_empty() { 0 } else { (s.warnings.len().min(3) as u16).max(1) };
    let mut constraints = vec![
        Constraint::Length(3),     // header
        Constraint::Length(1),     // banner
        Constraint::Length(7),     // Odiseo
        Constraint::Length(7),     // Houdini
    ];
    if warn_h > 0 { constraints.push(Constraint::Length(warn_h)); }
    constraints.push(Constraint::Min(2)); // log

    let m = Layout::default().direction(Direction::Vertical).constraints(constraints).split(area);
    let mut idx: usize = 0;

    // Header
    draw_header(f, m[idx], s); idx += 1;

    // Banner
    draw_banner(f, m[idx], s); idx += 1;

    // Odiseo panel
    draw_odiseo_panel(f, m[idx], s); idx += 1;

    // Houdini panel
    draw_houdini_panel(f, m[idx], s); idx += 1;

    // Warnings
    if warn_h > 0 {
        let warn_lines: Vec<Line> = s.warnings.iter().take(3).map(|w|
            Line::from(Span::styled(w, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)))
        ).collect();
        f.render_widget(Paragraph::new(warn_lines).block(Block::default().borders(Borders::ALL).title("Alertas").border_style(Style::default().fg(Color::Red))), m[idx]);
        idx += 1;
    }

    // Log
    let lines: Vec<Line> = s.log.iter().map(|e| Line::from(vec![
        Span::styled(format!("{} ", e.ts), Style::default().fg(Color::DarkGray)),
        Span::styled(&e.text, Style::default().fg(e.color)),
    ])).collect();
    f.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Eventos")), m[idx]);
}

fn draw_header(f: &mut Frame, area: Rect, s: &State) {
    let h = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,8); 8]).split(area);

    // BTC
    let btc_ref = if s.btc_entry > 0.0 { s.btc_entry } else { s.btc_open };
    let btc_delta = if btc_ref > 0.0 { s.btc - btc_ref } else { 0.0 };
    let btc_c = if btc_delta > 0.0 { Color::Green } else if btc_delta < 0.0 { Color::Red } else { Color::Yellow };
    f.render_widget(
        Paragraph::new(format!("BTC ${:.0} ({:+.0})", s.btc, btc_delta))
            .style(Style::default().fg(btc_c).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL)),
        h[0]);

    // USD
    let bal_c = if s.bal > 100.0 { Color::Green } else if s.bal > 50.0 { Color::Yellow } else { Color::Red };
    f.render_widget(
        Paragraph::new(format!("USD ${:.2}", s.bal))
            .style(Style::default().fg(bal_c).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL)),
        h[1]);

    // MODE
    let mode_style = if s.live {
        Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
    };
    f.render_widget(
        Paragraph::new(if s.live {" LIVE "}else{"PAPER"}).style(mode_style).block(Block::default().borders(Borders::ALL)),
        h[2]);

    // WS
    let wsc = if s.connected { Color::Green } else { Color::Red };
    f.render_widget(
        Paragraph::new(if s.connected {"WS OK"}else{"WS OFF"})
            .style(Style::default().fg(wsc).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL)),
        h[3]);

    // Reinv
    let rc = if s.reinvest { Color::Green } else { Color::DarkGray };
    f.render_widget(
        Paragraph::new(if s.reinvest {"Reinv ON"}else{"Reinv OFF"})
            .style(Style::default().fg(rc).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL)),
        h[4]);

    // Odiseo status
    let o_st = if s.odi_enabled { if s.live { Color::Red } else { Color::Green } } else { Color::DarkGray };
    f.render_widget(
        Paragraph::new(if s.odi_enabled { format!("{} ON", s.odi_label) } else { format!("{} OFF", s.odi_label) })
            .style(Style::default().fg(o_st).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL)),
        h[5]);

    // Houdini status
    let h_st = if s.h65_enabled { if s.live { Color::Red } else { Color::Green } } else { Color::DarkGray };
    f.render_widget(
        Paragraph::new(if s.h65_enabled {"H65 ON"}else{"H65 OFF"})
            .style(Style::default().fg(h_st).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL)),
        h[6]);

    // Odiseo filters
    let f_on = s.odi_filters.count_ones();
    let f_style = if f_on > 0 { Color::Yellow } else { Color::DarkGray };
    f.render_widget(
        Paragraph::new(format!("F:{}", f_on))
            .style(Style::default().fg(f_style).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL)),
        h[7]);
}

fn draw_banner(f: &mut Frame, area: Rect, s: &State) {
    if s.live {
        let b = Paragraph::new(format!(" DINERO REAL ACTIVO — {} + HOUDINI 65 EN VIVO ", s.odi_label))
            .style(Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD));
        f.render_widget(b, area);
    } else {
        let b = Paragraph::new(" PAPER MONEY — GRABANDO — Sin dinero real ")
            .style(Style::default().fg(Color::White).bg(Color::Blue).add_modifier(Modifier::BOLD));
        f.render_widget(b, area);
    }
}

fn draw_odiseo_panel(f: &mut Frame, area: Rect, s: &State) {
    let panels = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)]).split(area);

    let t = s.odi_t_up + s.odi_t_dn;
    let w = s.odi_w_up + s.odi_w_dn;
    let wr = if t > 0 { format!("{:.0}%", w as f64 / t as f64 * 100.0) } else { "-".into() };
    let pnl_pct = if s.odi_budget > 0.0 { s.odi_pnl / s.odi_budget * 100.0 } else { 0.0 };
    let pc = if s.odi_pnl > 0.001 { Color::Green } else if s.odi_pnl < -0.001 { Color::Red } else { Color::Gray };
    let bc = if s.live && s.odi_enabled { Color::Red } else { Color::DarkGray };

    let stats = format!(
        "Budget: ${:.0}   Balance: ${:.2}   PnL: {:+.4} ({:+.1}%)\n\
         Win Rate: {} ({}/{} trades)   Accuracy: {:.0}%\n\
         Sessions: {}   Avg PnL: {:+.4}   Best: {:+.4}   Worst: {:+.4}",
        s.odi_budget, s.odi_bal, s.odi_pnl, pnl_pct,
        wr, w, t, s.odi_accuracy * 100.0,
        s.odi_sessions, s.odi_avg_pnl, s.odi_best, s.odi_worst,
    );
    f.render_widget(
        Paragraph::new(stats).style(Style::default().fg(pc))
            .block(Block::default().borders(Borders::ALL)
                .title(format!("{} — Estrategia Principal", s.odi_label))
                .border_style(Style::default().fg(bc))),
        panels[0]);

    let exit_info = format!(
        "ENTRADAS / SALIDAS\n\
         UP:   {}/{} trades   won {}/{}   TP {}   SL {}\n\
         DOWN: {}/{} trades   won {}/{}   TP {}   SL {}\n\
         TOTAL: {} trades   {} TP   {} SL",
        s.odi_w_up, s.odi_t_up, s.odi_w_up.max(0), s.odi_t_up, s.odi_tp_up, s.odi_sl_up,
        s.odi_w_dn, s.odi_t_dn, s.odi_w_dn.max(0), s.odi_t_dn, s.odi_tp_dn, s.odi_sl_dn,
        t, s.odi_tp_up + s.odi_tp_dn, s.odi_sl_up + s.odi_sl_dn,
    );
    f.render_widget(
        Paragraph::new(exit_info).style(Style::default().fg(Color::Gray))
            .block(Block::default().borders(Borders::ALL).title("Detalle UP/DOWN")),
        panels[1]);
}

fn draw_houdini_panel(f: &mut Frame, area: Rect, s: &State) {
    let panels = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)]).split(area);

    let t = s.h65_t_up + s.h65_t_dn;
    let w = s.h65_w_up + s.h65_w_dn;
    let wr = if t > 0 { format!("{:.0}%", w as f64 / t as f64 * 100.0) } else { "-".into() };
    let pnl_pct = if s.h65_budget > 0.0 { s.h65_pnl / s.h65_budget * 100.0 } else { 0.0 };
    let pc = if s.h65_pnl > 0.001 { Color::Green } else if s.h65_pnl < -0.001 { Color::Red } else { Color::Gray };
    let bc = if s.live && s.h65_enabled { Color::Red } else { Color::DarkGray };

    let stats = format!(
        "Budget: ${:.0}   Balance: ${:.2}   PnL: {:+.4} ({:+.1}%)\n\
         Win Rate: {} ({}/{} trades)   Accuracy: {:.0}%\n\
         Entry: >=0.65   TP:0.75   SL:0.60\n\
         Sessions: {}   Avg PnL: {:+.4}   Best: {:+.4}   Worst: {:+.4}",
        s.h65_budget, s.h65_bal, s.h65_pnl, pnl_pct,
        wr, w, t, s.h65_accuracy * 100.0,
        s.h65_sessions, s.h65_avg_pnl, s.h65_best, s.h65_worst,
    );
    f.render_widget(
        Paragraph::new(stats).style(Style::default().fg(pc))
            .block(Block::default().borders(Borders::ALL)
                .title("Houdini 65 — Scalp")
                .border_style(Style::default().fg(bc))),
        panels[0]);

    let exit_info = format!(
        "ENTRADAS / SALIDAS\n\
         UP:   {}/{} trades   won {}/{}   TP {}   SL {}\n\
         DOWN: {}/{} trades   won {}/{}   TP {}   SL {}\n\
         TOTAL: {} trades   {} TP   {} SL",
        s.h65_w_up, s.h65_t_up, s.h65_w_up.max(0), s.h65_t_up, s.h65_tp_up, s.h65_sl_up,
        s.h65_w_dn, s.h65_t_dn, s.h65_w_dn.max(0), s.h65_t_dn, s.h65_tp_dn, s.h65_sl_dn,
        t, s.h65_tp_up + s.h65_tp_dn, s.h65_sl_up + s.h65_sl_dn,
    );
    f.render_widget(
        Paragraph::new(exit_info).style(Style::default().fg(Color::Gray))
            .block(Block::default().borders(Borders::ALL).title("Detalle UP/DOWN H65")),
        panels[1]);
}

// ═══════════════════════════════════════════════════════════════════
// TAB 1: TRADING
// ═══════════════════════════════════════════════════════════════════

fn draw_trading(f: &mut Frame, area: Rect, s: &State) {
    let chunks = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)]).split(area);

    // Left: Variant control
    draw_variant_control(f, chunks[0], s);

    // Right: Quick commands + filter hint
    draw_quick_commands(f, chunks[1], s);
}

fn draw_variant_control(f: &mut Frame, area: Rect, s: &State) {
    let mut lines: Vec<Line> = Vec::new();

    let title_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    lines.push(Line::from(Span::styled("── ODISEO 83 ──", title_style)));

    // Variant 0: Odiseo 83
    let o83_st = if s.odi_enabled { Color::Green } else { Color::DarkGray };
    let o83_sel = if s.selected_variant == 0 { "▸ " } else { "  " };
    let o83_onoff = if s.odi_enabled { "ON" } else { "OFF" };
    lines.push(Line::from(vec![
        Span::styled(format!("{o83_sel}[o] Odiseo 83 "), Style::default().fg(o83_st).add_modifier(Modifier::BOLD)),
        Span::styled(o83_onoff, Style::default().fg(o83_st)),
        Span::styled(format!(" budget=${:.0} PnL={:+.2}", s.odi_budget, s.odi_pnl), Style::default().fg(Color::Gray)),
    ]));

    // Variant 1: Houdini 65
    let h65_st = if s.h65_enabled { Color::Green } else { Color::DarkGray };
    let h65_sel = if s.selected_variant == 1 { "▸ " } else { "  " };
    let h65_onoff = if s.h65_enabled { "ON" } else { "OFF" };
    lines.push(Line::from(vec![
        Span::styled(format!("{h65_sel}[h] Houdini 65 "), Style::default().fg(h65_st).add_modifier(Modifier::BOLD)),
        Span::styled(h65_onoff, Style::default().fg(h65_st)),
        Span::styled(format!(" budget=${:.0} PnL={:+.2}", s.h65_budget, s.h65_pnl), Style::default().fg(Color::Gray)),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("── GENERAL ──", title_style)));

    let live_st = if s.live { Color::Red } else { Color::Cyan };
    lines.push(Line::from(vec![
        Span::styled("[l] LIVE: ", Style::default().fg(Color::White)),
        Span::styled(if s.live {"ON ⚡"}else{"OFF"}, Style::default().fg(live_st).add_modifier(Modifier::BOLD)),
    ]));

    let r_st = if s.reinvest { Color::Green } else { Color::DarkGray };
    lines.push(Line::from(vec![
        Span::styled("[r] Reinvest: ", Style::default().fg(Color::White)),
        Span::styled(if s.reinvest {"ON"}else{"OFF"}, Style::default().fg(r_st)),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("── TECLAS RÁPIDAS ──", title_style)));
    lines.push(Line::from(Span::styled(" [o] toggle Odiseo 83    [h] toggle Houdini 65", Color::Gray)));
    lines.push(Line::from(Span::styled(" [l] toggle LIVE/PAPER   [r] toggle Reinvest", Color::Gray)));
    lines.push(Line::from(Span::styled(" [p] PANIC sell          [a] All ON   [z] All OFF", Color::Gray)));
    lines.push(Line::from(Span::styled(" [1-4] Odi-budget $5/10/20/40   [5-8] H65-budget $5/10/20/40", Color::Gray)));
    lines.push(Line::from(Span::styled(" [f] Filter menu         [t] Only THIS variant", Color::Gray)));

    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Control de Variantes")),
        area,
    );
}

fn draw_quick_commands(f: &mut Frame, area: Rect, s: &State) {
    let chunks = Layout::default().direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)]).split(area);

    // PANIC button
    let panic_style = Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD);
    f.render_widget(
        Paragraph::new(" [p] = PANIC SELL ").style(panic_style).block(Block::default().borders(Borders::ALL)),
        chunks[0],
    );

    // Filter status
    let mut lines: Vec<Line> = Vec::new();
    let filter_names: &[&str] = &[
        "F1:dump_score", "F2:tick_gap", "F3:spread", "F4:imbalance",
        "F5:ask_wall", "F6:spoof", "F7:btc_vel", "F8:btc_acel",
        "F9:price_impact", "F10:depth_concentration", "F11:liquidity_depth", "F12:depth_balance",
        "F13:liq_depth", "F14:depth_bal",
    ];
    lines.push(Line::from(Span::styled("── FILTROS (bitmask) ──", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));

    let mask = s.odi_filters;
    for (i, name) in filter_names.iter().enumerate() {
        let bit = 1u16 << i;
        let active = mask & bit != 0;
        let st = if active { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::DarkGray) };
        let marker = if active { "▣" } else { "□" };
        let text = format!(" {marker} {name}");
        if i % 2 == 0 {
            lines.push(Line::from(Span::styled(text, st)));
        } else {
            if let Some(last) = lines.last_mut() {
                last.spans.push(Span::styled(format!("    {text}"), st));
            }
        }
    }

    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Filtros Activos [f]")),
        chunks[1],
    );
}

// ═══════════════════════════════════════════════════════════════════
// TAB 2: SESSIONS
// ═══════════════════════════════════════════════════════════════════

fn draw_sessions(f: &mut Frame, area: Rect, s: &State) {
    let chunks = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)]).split(area);

    // Left: Active sessions + controls
    let left = Layout::default().direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)]).split(chunks[0]);

    let active_count = s.sessions.iter().filter(|sess| sess.status == "recording").count();
    let summary = format!(
        "Active: {}   Total sessions: {}\n\
         [s] Start new 15-min session   [S] Stop active session",
        active_count, s.sessions.len()
    );
    f.render_widget(
        Paragraph::new(summary).style(Style::default().fg(Color::White))
            .block(Block::default().borders(Borders::ALL).title("Sesiones")),
        left[0]);

    // Session list
    let mut lines: Vec<Line> = Vec::new();
    for (i, sess) in s.sessions.iter().enumerate() {
        let st = match sess.status.as_str() {
            "recording" => Color::Green,
            "completed" => Color::DarkGray,
            "scheduled" => Color::Yellow,
            _ => Color::Gray,
        };
        let marker = if i == s.selected_session { "▸" } else { " " };
        let _dur = if sess.duration_min > 0 { format!("{}m", sess.duration_min) } else { "∞".into() };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker}"), Style::default().fg(Color::Cyan)),
            Span::styled(format!("#{} ", sess.id), Style::default().fg(st).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} ", sess.name), Style::default().fg(Color::White)),
            Span::styled(format!("[{}] ", sess.status), Style::default().fg(st)),
            Span::styled(format!("{} ticks", sess.tick_count), Style::default().fg(Color::DarkGray)),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled("No sessions loaded", Color::DarkGray)));
    }
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Lista [↑↓] [e]xport")),
        left[1]);

    // Right: Session detail
    if let Some(sess) = s.sessions.get(s.selected_session) {
        let start = if sess.scheduled_start.len() > 19 { &sess.scheduled_start[..19] } else { &sess.scheduled_start };
        let end = if sess.scheduled_end.len() > 19 { &sess.scheduled_end[..19] } else { &sess.scheduled_end };
        let info = format!(
            "Session #{}\n\
             Name: {}\n\
             Status: {}\n\
             Start: {}\n\
             End:   {}\n\
             Duration: {} min\n\
             Ticks: {}   Trades: {}\n\n\
             [e] Export CSV",
            sess.id, sess.name, sess.status,
            start, end,
            sess.duration_min, sess.tick_count, sess.trade_count,
        );
        f.render_widget(
            Paragraph::new(info).block(Block::default().borders(Borders::ALL).title("Detalle")),
            chunks[1]);
    } else {
        f.render_widget(
            Paragraph::new("Selecciona una sesión con ↑↓").block(Block::default().borders(Borders::ALL)),
            chunks[1]);
    }
}

// ═══════════════════════════════════════════════════════════════════
// TAB 3: SIGNALS (real-time triggers)
// ═══════════════════════════════════════════════════════════════════

fn draw_signals(f: &mut Frame, area: Rect, s: &State) {
    let chunks = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,3), Constraint::Ratio(1,3), Constraint::Ratio(1,3)]).split(area);

    // Column 1: CLOB triggers
    draw_trigger_column(f, chunks[0], s, "UP", s.hft.clob_trade_up, 0.83, s.hft.od83_up, s.hft.hd65_up);
    // Column 2: Market state
    draw_market_state(f, chunks[1], s);
    // Column 3: CLOB triggers DOWN
    draw_trigger_column(f, chunks[2], s, "DOWN", s.hft.clob_trade_dn, 0.83, s.hft.od83_dn, s.hft.hd65_dn);
}

fn draw_trigger_column(f: &mut Frame, area: Rect, s: &State, label: &str,
                         trigger: f64, threshold: f64, od_active: u8, hd_active: u8) {
    let chunks = Layout::default().direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // trigger meter
            Constraint::Length(5),  // strategy state
            Constraint::Min(1),     // balance
        ]).split(area);

    // Trigger gauge
    let trigger_pct = (trigger / 1.0).min(1.0);
    let bar_w = (chunks[0].width as usize).saturating_sub(6).max(1);
    let filled = (trigger_pct * bar_w as f64) as usize;
    let bar: String = "█".repeat(filled) + &"░".repeat(bar_w.saturating_sub(filled));

    let trigger_c = if trigger >= threshold { Color::Green } else if trigger >= threshold * 0.8 { Color::Yellow } else { Color::DarkGray };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(format!("{label} TRIGGER: {trigger:.4}"), Style::default().fg(trigger_c).add_modifier(Modifier::BOLD))),
            Line::from(Span::styled(format!("[{bar}]"), Style::default().fg(trigger_c))),
            Line::from(Span::styled(format!("Threshold: {threshold:.2} (O83) / 0.65 (H65)"), Style::default().fg(Color::DarkGray))),
        ]).block(Block::default().borders(Borders::ALL).title("TRIGGER")),
        chunks[0]);

    // Strategy signals
    let _od_c = match od_active { 2 => Color::Green, 1 => Color::Yellow, _ => Color::DarkGray };
    let _hd_c = match hd_active { 2 => Color::Cyan, 1 => Color::Yellow, _ => Color::DarkGray };
    let od_status = match od_active { 2 => "ACTIVE", 1 => "WATCH", _ => "IDLE" };
    let hd_status = match hd_active { 2 => "ACTIVE", 1 => "WATCH", _ => "IDLE" };

    let bal = if label == "UP" { (s.hft.od83_up_bal, s.hft.hd65_up_bal) } else { (s.hft.od83_dn_bal, s.hft.hd65_dn_bal) };

    let signal_info = format!(
        "Odiseo {label}: {od_status}  Bal: ${:.2}\n\
         Houdini {label}: {hd_status}  Bal: ${:.2}\n\
         Event: {}",
        bal.0, bal.1,
        if label == "UP" { &s.hft.od83_event } else { "" },
    );
    f.render_widget(
        Paragraph::new(signal_info)
            .block(Block::default().borders(Borders::ALL).title("Señales")),
        chunks[1]);
}

fn draw_market_state(f: &mut Frame, area: Rect, s: &State) {
    let chunks = Layout::default().direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // mid + spread
            Constraint::Length(3),  // volumes
            Constraint::Length(4),  // risk flags
            Constraint::Length(2),  // secs_left
            Constraint::Min(1),     // velocity
        ]).split(area);

    // Mid + spread
    let _spread_c = if s.hft.spread > 0.0 { Color::Green } else if s.hft.spread < 0.0 { Color::Red } else { Color::Gray };
    f.render_widget(
        Paragraph::new(format!(
            "Mid: {:.4}\nSpread: {:+.4}\nImbalance: {:.3}",
            s.hft.mid, s.hft.spread, s.hft.imbalance
        )).block(Block::default().borders(Borders::ALL).title("Order Book")),
        chunks[0]);

    // Volumes
    f.render_widget(
        Paragraph::new(format!(
            "Bid Vol: {:.0}\nAsk Vol: {:.0}\nUP Vol: {:.0}  DN Vol: {:.0}",
            s.hft.bid_vol, s.hft.ask_vol, s.hft.clob_trade_up_vol, s.hft.clob_trade_dn_vol
        )).block(Block::default().borders(Borders::ALL).title("Volúmenes")),
        chunks[1]);

    // Risk flags
    let _spoof_c = if s.hft.spoof > 0 { Color::Red } else { Color::DarkGray };
    let _dump_c = match s.hft.dump_score { 0 => Color::DarkGray, 1 => Color::Yellow, 2|3 => Color::Red, _ => Color::DarkGray };
    let _wall_c = if s.hft.ask_wall > 0 { Color::Red } else { Color::DarkGray };
    f.render_widget(
        Paragraph::new(format!(
            "Spoof: {}   Dump: {}   AskWall: {}\n\
             Tick Gap: {}ms\n\
             BTC Vel: {:+.2}   Acel: {:+.4}",
            if s.hft.spoof > 0 { "⚠" } else { "✓" },
            s.hft.dump_score,
            if s.hft.ask_wall > 0 { "⚠" } else { "✓" },
            s.hft.tick_gap_ms, s.hft.btc_vel, s.hft.btc_acel,
        )).block(Block::default().borders(Borders::ALL).title("Riesgo")),
        chunks[2]);

    // Seconds left
    let sec_c = if s.hft.secs_left > 300 { Color::Green } else if s.hft.secs_left > 60 { Color::Yellow } else if s.hft.secs_left > 0 { Color::Red } else { Color::DarkGray };
    let sec_min = s.hft.secs_left / 60;
    let sec_rem = s.hft.secs_left % 60;
    f.render_widget(
        Paragraph::new(format!("{sec_min}:{sec_rem:02} remaining"))
            .style(Style::default().fg(sec_c).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL).title("Session Time")),
        chunks[3]);

    // Last tick
    f.render_widget(
        Paragraph::new(format!(
            "Last: {}\nEvent: {}",
            &s.hft.time[s.hft.time.len().saturating_sub(19)..],
            s.hft.event,
        )).block(Block::default().borders(Borders::ALL).title("Último Tick")),
        chunks[4]);
}
