use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use ratatui::Frame;

use crate::InputMode;
use crate::State;

const TAB_NAMES: &[&str] = &["Dashboard", "Trading", "Sessions", "Signals"];

pub fn draw(f: &mut Frame, s: &State) {
    let area = f.area();

    let pos_h = if s.pos_h65_up || s.pos_h65_dn || s.pos_odi_up || s.pos_odi_dn { 1 } else { 1 };
    let budget_h = if s.input_mode == InputMode::Budget { 3 } else { 0 };
    let cmd_h = if s.input_mode == InputMode::Command { 3 } else { 0 };

    let mut constraints = vec![
        Constraint::Length(1),     // tab bar
        Constraint::Length(pos_h), // position bar
        Constraint::Min(1),        // main content
    ];
    if cmd_h > 0 { constraints.push(Constraint::Length(cmd_h)); }
    if budget_h > 0 { constraints.push(Constraint::Length(budget_h)); }
    constraints.push(Constraint::Length(2)); // footer

    let chunks = Layout::default().direction(Direction::Vertical).constraints(constraints).split(area);
    let mut ci = 0;

    // ─── TAB BAR ──────────────────────────────────────────────────────
    let tab_titles: Vec<Line> = TAB_NAMES.iter().enumerate().map(|(i, name)| {
        let style = if i == s.tab {
            Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        Line::from(Span::styled(format!(" {name} "), style))
    }).collect();
    f.render_widget(Tabs::new(tab_titles).block(Block::default().borders(Borders::BOTTOM)), chunks[ci]);
    ci += 1;

    // ─── POSITION BAR (always visible) ─────────────────────────────────
    draw_position_bar(f, chunks[ci], s); ci += 1;

    // ─── MAIN CONTENT ─────────────────────────────────────────────────
    match s.tab {
        0 => draw_dashboard(f, chunks[ci], s),
        1 => draw_trading(f, chunks[ci], s),
        2 => draw_sessions(f, chunks[ci], s),
        3 => draw_signals(f, chunks[ci], s),
        _ => {}
    }
    ci += 1;

    // ─── INPUT MODAL (command / budget) ────────────────────────────
    if cmd_h > 0 || budget_h > 0 {
        draw_budget_input(f, chunks[ci], s);
        ci += 1;
    }

    // ─── FOOTER ───────────────────────────────────────────────────────
    draw_footer(f, chunks[ci], s);
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

    // MODE — fixed, no toggle
    let mode_style = if s.live {
        Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
    };
    f.render_widget(
        Paragraph::new(if s.live {"DINERO REAL"}else{"PAPER MONEY"}).style(mode_style).block(Block::default().borders(Borders::ALL)),
        h[2]);

    // HEALTH — combined WS + API latency
    let latency_ms = s.last_api_ok.elapsed().as_millis() as u64;
    let health_txt: String;
    let health_c: Color;
    if !s.connected {
        health_txt = "NO CONEXION".into(); health_c = Color::Red;
    } else if latency_ms > 10_000 {
        health_txt = "SIN DATOS".into(); health_c = Color::Red;
    } else if latency_ms > 2_000 {
        health_txt = format!("LAG {}ms", latency_ms); health_c = Color::Yellow;
    } else {
        health_txt = format!("OK {}ms", latency_ms); health_c = Color::Green;
    }
    f.render_widget(
        Paragraph::new(health_txt)
            .style(Style::default().fg(health_c).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL).title("HEALTH")),
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
// TAB 1: TRADING — Strategias + Filtros + Leyenda
// ═══════════════════════════════════════════════════════════════════

const FILTER_NAMES: &[(&str, &str)] = &[
    ("1","frozen_market"), ("2","spread_health"), ("3","flash_dump"),
    ("4","min_volume"), ("5","btc_trend_confirm"), ("6","reversal_risk"),
    ("7","spoof_protection"), ("8","ask_wall"), ("9","mid_price_sanity"),
    ("0","imbalance_sanity"), ("-","session_age"), ("=","reentry_cooldown"),
    ("[","liquidity_depth"), ("]","depth_balance"),
];

fn draw_trading(f: &mut Frame, area: Rect, s: &State) {
    let chunks = Layout::default().direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10),  // strategy status + filter grid
            Constraint::Length(10),  // legend + budget
            Constraint::Min(0),
        ]).split(area);

    draw_filter_grid(f, chunks[0], s);
    draw_trading_legend(f, chunks[1], s);
}

fn draw_filter_grid(f: &mut Frame, area: Rect, s: &State) {
    let half = (area.width as usize / 2).min(50);
    let grid = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Length(half as u16), Constraint::Length(half as u16)]).split(area);

    // Left column: strategies + filters 1-7
    let mut left: Vec<Line> = Vec::new();
    let ts = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);

    // Strategy status
    let o83_st = if s.odi_enabled { Color::Green } else { Color::DarkGray };
    let h65_st = if s.h65_enabled { Color::Green } else { Color::DarkGray };
    let o83_act = s.hft.od83_up.max(s.hft.od83_dn);
    let h65_act = s.hft.hd65_up.max(s.hft.hd65_dn);
    let o83_state = match o83_act { 2 => "ACTIVO", 1 => "espera", _ => "inactivo" };
    let h65_state = match h65_act { 2 => "ACTIVO", 1 => "espera", _ => "inactivo" };
    let o83_state_c = match o83_act { 2 => Color::Green, 1 => Color::Yellow, _ => Color::DarkGray };
    let h65_state_c = match h65_act { 2 => Color::Green, 1 => Color::Yellow, _ => Color::DarkGray };

    left.push(Line::from(Span::styled("── ESTRATEGIAS ──", ts)));
    left.push(Line::from(vec![
        Span::styled("[o] Odiseo 83 ", Style::default().fg(o83_st).add_modifier(Modifier::BOLD)),
        Span::styled(if s.odi_enabled {"ON"}else{"OFF"}, Style::default().fg(o83_st)),
        Span::styled(format!("  state:{}", o83_state), Style::default().fg(o83_state_c)),
        Span::styled(format!("  ${:.0}", s.odi_budget), Style::default().fg(Color::Gray)),
    ]));
    left.push(Line::from(vec![
        Span::styled("[h] Houdini 65", Style::default().fg(h65_st).add_modifier(Modifier::BOLD)),
        Span::styled(if s.h65_enabled {"ON"}else{"OFF"}, Style::default().fg(h65_st)),
        Span::styled(format!("  state:{}", h65_state), Style::default().fg(h65_state_c)),
        Span::styled(format!("  ${:.0}", s.h65_budget), Style::default().fg(Color::Gray)),
    ]));
    let mode_st = if s.live { Style::default().fg(Color::Red) } else { Style::default().fg(Color::Cyan) };
    left.push(Line::from(vec![
        Span::styled("[l] Modo: ", Style::default().fg(Color::White)),
        Span::styled(if s.live {"LIVE ⚡"}else{"PAPER"}, mode_st.add_modifier(Modifier::BOLD)),
        Span::styled("  [r]Reinv:", Style::default().fg(Color::White)),
        Span::styled(if s.reinvest {"ON"}else{"OFF"}, if s.reinvest { Color::Green } else { Color::DarkGray }),
    ]));
    left.push(Line::from(""));
    left.push(Line::from(Span::styled("── FILTROS (1-9,0) ──", ts)));

    // Filters 1-7
    for &(key, name) in &FILTER_NAMES[..7] {
        let bit = 1u16 << (FILTER_NAMES.iter().position(|&(k,_)| k==key).unwrap());
        let active = s.odi_filters & bit != 0;
        let st = if active { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::DarkGray) };
        let marker = if active { "▣" } else { "□" };
        left.push(Line::from(vec![
            Span::styled(format!(" [{key}] "), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{marker} {name}"), st),
        ]));
    }

    // Right column: filters 8-14 + quick actions
    let mut right: Vec<Line> = Vec::new();
    right.push(Line::from(""));
    right.push(Line::from(""));
    right.push(Line::from(""));
    right.push(Line::from(""));
    right.push(Line::from(""));
    right.push(Line::from(""));
    right.push(Line::from(Span::styled("── FILTROS (cont) ──", ts)));

    for &(key, name) in &FILTER_NAMES[7..] {
        let bit = 1u16 << (FILTER_NAMES.iter().position(|&(k,_)| k==key).unwrap());
        let active = s.odi_filters & bit != 0;
        let st = if active { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::DarkGray) };
        let marker = if active { "▣" } else { "□" };
        right.push(Line::from(vec![
            Span::styled(format!(" [{key}] "), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{marker} {name}"), st),
        ]));
    }
    right.push(Line::from(""));
    right.push(Line::from(Span::styled("── ACCIONES ──", ts)));
    right.push(Line::from(Span::styled(" [a] ALL filters ON", Color::Green)));
    right.push(Line::from(Span::styled(" [z] ALL filters OFF", Color::Red)));
    right.push(Line::from(Span::styled(" [t] Only THIS variant", Color::Yellow)));
    right.push(Line::from(Span::styled(" [p] PANIC SELL", Color::Red)));

    f.render_widget(
        Paragraph::new(left).block(Block::default().borders(Borders::ALL).title("Control Estrategias + Filtros")),
        grid[0]);
    f.render_widget(
        Paragraph::new(right).block(Block::default().borders(Borders::ALL)),
        grid[1]);
}

fn draw_trading_legend(f: &mut Frame, area: Rect, s: &State) {
    let chunks = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)]).split(area);

    let ts = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let g = Style::default().fg(Color::Gray);

    let mut legend: Vec<Line> = Vec::new();
    legend.push(Line::from(Span::styled("── TECLAS ──", ts)));
    legend.push(Line::from(Span::styled("", g)));
    legend.push(Line::from(Span::styled(" ESTRATEGIAS", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))));
    legend.push(Line::from(vec![
        Span::styled("  o", Style::default().fg(Color::Yellow)), Span::styled(" = toggle Odiseo 83", g),
    ]));
    legend.push(Line::from(vec![
        Span::styled("  h", Style::default().fg(Color::Yellow)), Span::styled(" = toggle Houdini 65", g),
    ]));
    legend.push(Line::from(vec![
        Span::styled("  t", Style::default().fg(Color::Yellow)), Span::styled(" = solo variante seleccionada", g),
    ]));
    legend.push(Line::from(Span::styled("", g)));
    legend.push(Line::from(Span::styled(" FILTROS", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))));
    legend.push(Line::from(vec![
        Span::styled("  1-9,0", Style::default().fg(Color::Yellow)), Span::styled(" = toggle filtro 1-10", g),
    ]));
    legend.push(Line::from(vec![
        Span::styled("  a", Style::default().fg(Color::Yellow)), Span::styled(" = todos ON", g),
        Span::styled("    z", Style::default().fg(Color::Yellow)), Span::styled(" = todos OFF", g),
    ]));
    legend.push(Line::from(Span::styled("  F11-14 sin hotkey — usa a/z", g)));
    legend.push(Line::from(Span::styled("", g)));
    legend.push(Line::from(Span::styled(" MODO / DINERO", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))));
    legend.push(Line::from(vec![
        Span::styled("  l", Style::default().fg(Color::Yellow)), Span::styled(" = LIVE/PAPER", g),
        Span::styled("    r", Style::default().fg(Color::Yellow)), Span::styled(" = Reinvest", g),
        Span::styled("    p", Style::default().fg(Color::Yellow)), Span::styled(" = PANIC", g),
    ]));
    legend.push(Line::from(Span::styled("", g)));
    legend.push(Line::from(Span::styled(" PRESUPUESTO", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))));
    legend.push(Line::from(vec![
        Span::styled("  ↑↓", Style::default().fg(Color::Yellow)), Span::styled(" = seleccionar variante", g),
    ]));
    legend.push(Line::from(vec![
        Span::styled("  [", Style::default().fg(Color::Yellow)), Span::styled(" / ", g),
        Span::styled("]", Style::default().fg(Color::Yellow)), Span::styled(" = bajar/subir budget", g),
    ]));
    legend.push(Line::from(Span::styled("", g)));
    legend.push(Line::from(Span::styled(" NAVEGACIÓN", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))));
    legend.push(Line::from(vec![
        Span::styled("  ←→", Style::default().fg(Color::Yellow)), Span::styled(" o Tab = cambiar pestaña", g),
    ]));
    legend.push(Line::from(vec![
        Span::styled("  q", Style::default().fg(Color::Yellow)), Span::styled(" o Esc = salir", g),
    ]));

    f.render_widget(
        Paragraph::new(legend).block(Block::default().borders(Borders::ALL).title("Leyenda Completa")),
        chunks[0]);

    // Right: budget quick set + PANIC
    let mut quick: Vec<Line> = Vec::new();
    quick.push(Line::from(Span::styled("── BUDGET RÁPIDO ──", ts)));
    quick.push(Line::from(""));
    let sel = if s.selected_variant == 0 { "Odiseo 83" } else { "Houdini 65" };
    let cur = if s.selected_variant == 0 { s.odi_budget } else { s.h65_budget };
    quick.push(Line::from(Span::styled(format!("Variante: {sel}  Budget: ${cur:.0}"), Style::default().fg(Color::White).add_modifier(Modifier::BOLD))));
    quick.push(Line::from(""));
    quick.push(Line::from(Span::styled(" [ or ]  =  ajustar budget", g)));
    quick.push(Line::from(Span::styled("", g)));
    quick.push(Line::from(Span::styled("── PANIC ──", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))));
    quick.push(Line::from(""));
    quick.push(Line::from(Span::styled("   [p] = PANIC SELL", Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD))));
    quick.push(Line::from(""));
    quick.push(Line::from(Span::styled("Cancela todas las órdenes", g)));
    quick.push(Line::from(Span::styled("y vende a mercado.", g)));

    f.render_widget(
        Paragraph::new(quick).block(Block::default().borders(Borders::ALL).title("Acciones Rápidas")),
        chunks[1]);
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
    let chunks = Layout::default().direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // BTC velocity metrics — BIG numbers
            Constraint::Min(1),     // trigger columns below
        ]).split(area);

    // ─── BTC VELOCITY METRICS — large, prominent ───────────────────────
    draw_btc_metrics(f, chunks[0], s);

    // ─── Trigger columns ──────────────────────────────────────────────
    let cols = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,3); 3]).split(chunks[1]);

    draw_trigger_column(f, cols[0], s, "UP", s.hft.clob_trade_up, 0.83, s.hft.od83_up, s.hft.hd65_up);
    draw_market_state(f, cols[1], s);
    draw_trigger_column(f, cols[2], s, "DOWN", s.hft.clob_trade_dn, 0.83, s.hft.od83_dn, s.hft.hd65_dn);
}

fn draw_btc_metrics(f: &mut Frame, area: Rect, s: &State) {
    let cols = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,4); 4]).split(area);

    let b = Modifier::BOLD;

    // Velocidad (slope)
    let vel_sign = s.hft.btc_vel;
    let vel_c = if vel_sign > 5.0 { Color::Green } else if vel_sign > 0.0 { Color::LightGreen } else if vel_sign > -5.0 { Color::LightRed } else { Color::Red };
    let vel_label = if vel_sign > 20.0 { "FUERTE ⬆" } else if vel_sign > 5.0 { "subiendo" } else if vel_sign > 0.0 { "leve ⬆" } else if vel_sign > -5.0 { "leve ⬇" } else if vel_sign > -20.0 { "bajando" } else { "FUERTE ⬇" };

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("VELOCIDAD", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled(format!("{:+.1}", vel_sign), Style::default().fg(vel_c).add_modifier(b))),
            Line::from(Span::styled(format!("USD/s  {vel_label}"), Style::default().fg(vel_c))),
        ]).block(Block::default().borders(Borders::ALL).title("Pendiente BTC")),
        cols[0]);

    // Aceleración (curvature)
    let acel = s.hft.btc_acel;
    let acel_c = if acel > 2.0 { Color::Green } else if acel > 0.0 { Color::LightGreen } else if acel > -2.0 { Color::LightRed } else { Color::Red };
    let acel_label = if acel > 5.0 { "acelerando ⬆" } else if acel > 0.5 { "empujando" } else if acel > -0.5 { "plano" } else if acel > -5.0 { "frenando" } else { "frenazo ⬇" };

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("ACELERACIÓN", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled(format!("{:+.2}", acel), Style::default().fg(acel_c).add_modifier(b))),
            Line::from(Span::styled(format!("USD/s²  {acel_label}"), Style::default().fg(acel_c))),
        ]).block(Block::default().borders(Borders::ALL).title("Curvatura")),
        cols[1]);

    // Volatilidad (micro)
    let vol = s.hft.btc_volatility;
    let vol_c = if vol > 15.0 { Color::Red } else if vol > 8.0 { Color::Yellow } else if vol > 3.0 { Color::LightGreen } else { Color::Green };
    let vol_label = if vol > 20.0 { "CAÓTICO" } else if vol > 10.0 { "turbulento" } else if vol > 5.0 { "nervioso" } else if vol > 2.0 { "normal" } else { "tranquilo" };

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("VOLATILIDAD", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled(format!("{:.1}", vol), Style::default().fg(vol_c).add_modifier(b))),
            Line::from(Span::styled(format!("EMA|vel|  {vol_label}"), Style::default().fg(vol_c))),
        ]).block(Block::default().borders(Borders::ALL).title("Micro-Vol")),
        cols[2]);

    // BTC price
    let btc_delta = if s.btc_entry > 0.0 { s.btc - s.btc_entry }
        else if s.btc_open > 0.0 { s.btc - s.btc_open }
        else { 0.0 };
    let btc_c = if btc_delta > 0.0 { Color::Green } else if btc_delta < 0.0 { Color::Red } else { Color::Yellow };

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("BTC PRICE", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled(format!("${:.0}", s.btc), Style::default().fg(Color::White).add_modifier(b))),
            Line::from(Span::styled(format!("{:+.0}", btc_delta), Style::default().fg(btc_c))),
        ]).block(Block::default().borders(Borders::ALL).title("Precio")),
        cols[3]);
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

// ═══════════════════════════════════════════════════════════════════
// POSITION BAR — visible in ALL tabs
// ═══════════════════════════════════════════════════════════════════

fn draw_position_bar(f: &mut Frame, area: Rect, s: &State) {
    let tab_name = TAB_NAMES[s.tab];

    let h65_pos = if s.pos_h65_up {
        let entry = s.pos_h65_entry_up;
        let current = s.hft.clob_trade_up;
        let invested = s.h65_budget;
        let current_val = if entry > 0.0 { invested * (current / entry) } else { 0.0 };
        let pnl = current_val - invested;
        format!("▲ UP  inv:${:.0}  PnL:{:+.2}  @{:.4}→{:.4}  [{:.1}%]  CONFIRMADO",
            invested, pnl, entry, current, if entry>0.0{(current/entry-1.0)*100.0}else{0.0})
    } else if s.pos_h65_dn {
        let entry = s.pos_h65_entry_dn;
        let current = s.hft.clob_trade_dn;
        let invested = s.h65_budget;
        let current_val = if entry > 0.0 { invested * (current / entry) } else { 0.0 };
        let pnl = current_val - invested;
        format!("▼ DN  inv:${:.0}  PnL:{:+.2}  @{:.4}→{:.4}  [{:.1}%]  CONFIRMADO",
            invested, pnl, entry, current, if entry>0.0{(current/entry-1.0)*100.0}else{0.0})
    } else if s.h65_enabled {
        format!("◆ esperando ${:.0}", s.h65_budget)
    } else {
        "○ OFF".to_string()
    };

    let h65_c = if s.pos_h65_up || s.pos_h65_dn {
        let delta = if s.pos_h65_up { s.hft.clob_trade_up - s.pos_h65_entry_up }
                   else { s.hft.clob_trade_dn - s.pos_h65_entry_dn };
        if delta > 0.0 { Color::Green } else { Color::Red }
    } else if s.h65_enabled { Color::Yellow } else { Color::DarkGray };

    let odi_pos = if s.pos_odi_up {
        let entry = s.pos_odi_entry_up;
        let current = s.hft.clob_trade_up;
        let invested = s.odi_budget;
        let current_val = if entry > 0.0 { invested * (current / entry) } else { 0.0 };
        let pnl = current_val - invested;
        format!("▲ UP  inv:${:.0}  PnL:{:+.2}  @{:.4}→{:.4}  CONFIRMADO", invested, pnl, entry, current)
    } else if s.pos_odi_dn {
        let entry = s.pos_odi_entry_dn;
        let current = s.hft.clob_trade_dn;
        let invested = s.odi_budget;
        let current_val = if entry > 0.0 { invested * (current / entry) } else { 0.0 };
        let pnl = current_val - invested;
        format!("▼ DN  inv:${:.0}  PnL:{:+.2}  @{:.4}→{:.4}  CONFIRMADO", invested, pnl, entry, current)
    } else if s.odi_enabled {
        format!("◆ esperando ${:.0}", s.odi_budget)
    } else {
        "○ OFF".to_string()
    };

    let odi_c = if s.pos_odi_up || s.pos_odi_dn { Color::Green } else if s.odi_enabled { Color::Yellow } else { Color::DarkGray };

    let h = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 2); 2]).split(area);

    f.render_widget(
        Paragraph::new(h65_pos).style(Style::default().fg(h65_c).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL).title(format!("Houdini 65 | {tab_name}"))),
        h[0]);

    f.render_widget(
        Paragraph::new(odi_pos).style(Style::default().fg(odi_c).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL).title(format!("Odiseo 83 | {tab_name}"))),
        h[1]);
}

// ═══════════════════════════════════════════════════════════════════
// BUDGET INPUT MODAL
// ═══════════════════════════════════════════════════════════════════

fn draw_budget_input(f: &mut Frame, area: Rect, s: &State) {
    // Command mode
    if s.input_mode == InputMode::Command {
        let text = format!(
            "▶ /{}_   [/h10 Houdini $10] [/o20 Odiseo $20] [/p PANIC] [/b30 budget] [/r reinv] [Enter]ok [Esc]cancel",
            s.input_buf
        );
        f.render_widget(
            Paragraph::new(text)
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Green))),
            area,
        );
        return;
    }
    // Budget mode
    let variant = if s.selected_variant == 0 { "Odiseo 83" } else { "Houdini 65" };
    let text = format!(
        "{} | Budget: ${}_   [Enter]confirm [Esc]cancel",
        variant, s.input_buf
    );
    f.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan))),
        area,
    );
}

// ═══════════════════════════════════════════════════════════════════
// FOOTER — hotkeys permanentes
// ═══════════════════════════════════════════════════════════════════

fn draw_footer(f: &mut Frame, area: Rect, s: &State) {
    let variant = if s.selected_variant == 0 { "O83" } else { "H65" };

    let line1 = format!(
        "[←→]tab  [h]H65:{}/{}  [o]O83:{}/{}  [p]PANIC  [/]cmd  [b]budget  [-/+]±$5  [1-0]preset  [q]quit",
        if s.h65_enabled {"ON"}else{"OFF"},
        s.h65_budget as i32,
        if s.odi_enabled {"ON"}else{"OFF"},
        s.odi_budget as i32,
    );
    let line2 = format!(
        "▲ {} seleccionado  [j/k]cambiar variante  [r]reinv:{}",
        variant,
        if s.reinvest {"ON"}else{"OFF"}
    );

    f.render_widget(
        Paragraph::new(format!("{}\n{}", line1, line2))
            .style(Style::default().fg(Color::DarkGray)),
        area,
    );
}
