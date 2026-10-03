use crate::api::*;
use crate::State;
use ratatui::style::Color;

pub struct CmdDef {
    pub syntax: &'static str,
    pub desc: &'static str,
    pub category: &'static str,
}

pub const REGISTRY: &[CmdDef] = &[
    CmdDef {
        syntax: "/b up <usd>          ",
        desc: "BUY UP market $X",
        category: "TRADE",
    },
    CmdDef {
        syntax: "/b down <usd>        ",
        desc: "BUY DOWN market $X",
        category: "TRADE",
    },
    CmdDef {
        syntax: "/s up <usd>          ",
        desc: "SELL UP market $X",
        category: "TRADE",
    },
    CmdDef {
        syntax: "/s down <usd>        ",
        desc: "SELL DOWN market $X",
        category: "TRADE",
    },
    CmdDef {
        syntax: "/l up <price> <size> ",
        desc: "Limit BUY UP",
        category: "TRADE",
    },
    CmdDef {
        syntax: "/l down <price> <size>",
        desc: "Limit BUY DOWN",
        category: "TRADE",
    },
    CmdDef {
        syntax: "/c                    ",
        desc: "Cancel ALL orders",
        category: "TRADE",
    },
    CmdDef {
        syntax: "/panic                ",
        desc: "PANIC: cancel + market sell all",
        category: "EMERGENCY",
    },
    CmdDef {
        syntax: "/man                  ",
        desc: "Help",
        category: "META",
    },
];

enum Parsed {
    BuyMarket { side: String, amount: f64 },
    SellMarket { side: String, amount: f64 },
    LimitBuy { side: String, price: f64, size: f64 },
    CancelAll,
    Panic,
    ShowMan,
    Unknown(String),
}

pub async fn dispatch(input: &str, s: &mut State) {
    let cmd = parse(input.trim());
    match cmd {
        Parsed::BuyMarket { side, amount } => exec_buy_market(&side, amount, s).await,
        Parsed::SellMarket { side, amount } => exec_sell_market(&side, amount, s).await,
        Parsed::LimitBuy { side, price, size } => exec_limit_buy(&side, price, size, s).await,
        Parsed::CancelAll => exec_cancel_all(s).await,
        Parsed::Panic => exec_panic(s).await,
        Parsed::ShowMan => show_man(s),
        Parsed::Unknown(input) => {
            s.add_log(format!("?: /{} — unknown", input), Color::Red);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// PARSER
// ═══════════════════════════════════════════════════════════════════

fn parse(input: &str) -> Parsed {
    if input == "panic" || input == "p" {
        return Parsed::Panic;
    }
    if input == "c" {
        return Parsed::CancelAll;
    }
    if input == "man" {
        return Parsed::ShowMan;
    }

    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.is_empty() {
        return Parsed::Unknown(input.to_string());
    }

    match parts[0] {
        "b" if parts.len() == 3 => {
            let amount = parts[2].parse::<f64>().unwrap_or(0.0);
            if amount > 0.0 {
                Parsed::BuyMarket {
                    side: parts[1].to_string(),
                    amount,
                }
            } else {
                Parsed::Unknown(input.to_string())
            }
        }
        "s" if parts.len() == 3 => {
            let amount = parts[2].parse::<f64>().unwrap_or(0.0);
            if amount > 0.0 {
                Parsed::SellMarket {
                    side: parts[1].to_string(),
                    amount,
                }
            } else {
                Parsed::Unknown(input.to_string())
            }
        }
        "l" if parts.len() == 4 => {
            let price = parts[2].parse::<f64>().unwrap_or(0.0);
            let size = parts[3].parse::<f64>().unwrap_or(0.0);
            if price > 0.0 && size > 0.0 {
                Parsed::LimitBuy {
                    side: parts[1].to_string(),
                    price,
                    size,
                }
            } else {
                Parsed::Unknown(input.to_string())
            }
        }
        _ => Parsed::Unknown(input.to_string()),
    }
}

// ═══════════════════════════════════════════════════════════════════
// EXECUTORS
// ═══════════════════════════════════════════════════════════════════

async fn exec_buy_market(side: &str, amount: f64, s: &mut State) {
    let body = format!(
        r#"{{"side":"buy","outcome":"{}","amount_usdc":{}}}"#,
        side, amount
    );
    match http_post("/api/orders/market", &body).await {
        Ok(()) => s.add_log(
            format!("MARKET BUY {} ${:.0}", side.to_uppercase(), amount),
            Color::Green,
        ),
        Err(e) => s.add_log(format!("BUY FAIL: {}", e), Color::Red),
    }
}

async fn exec_sell_market(side: &str, amount: f64, s: &mut State) {
    let body = format!(
        r#"{{"side":"sell","outcome":"{}","amount_usdc":{}}}"#,
        side, amount
    );
    match http_post("/api/orders/market", &body).await {
        Ok(()) => s.add_log(
            format!("MARKET SELL {} ${:.0}", side.to_uppercase(), amount),
            Color::Green,
        ),
        Err(e) => s.add_log(format!("SELL FAIL: {}", e), Color::Red),
    }
}

async fn exec_limit_buy(side: &str, price: f64, size: f64, s: &mut State) {
    let body = format!(
        r#"{{"side":"buy","outcome":"{}","price":{},"size":{}}}"#,
        side, price, size
    );
    match http_post("/api/orders/limit", &body).await {
        Ok(()) => s.add_log(
            format!(
                "LIMIT BUY {} @{:.4} x{}",
                side.to_uppercase(),
                price,
                size as u64
            ),
            Color::Green,
        ),
        Err(e) => s.add_log(format!("LIMIT FAIL: {}", e), Color::Red),
    }
}

async fn exec_cancel_all(s: &mut State) {
    match http_delete("/api/orders").await {
        Ok(()) => s.add_log("ALL CANCELLED".to_string(), Color::Yellow),
        Err(e) => s.add_log(format!("CANCEL FAIL: {}", e), Color::Red),
    }
}

async fn exec_panic(s: &mut State) {
    let body = r#"{}"#;
    match http_post("/api/panic", body).await {
        Ok(()) => s.add_log("🚨 PANIC executed".to_string(), Color::Red),
        Err(e) => s.add_log(format!("PANIC FAIL: {}", e), Color::Red),
    }
}

fn show_man(s: &mut State) {
    for cmd in REGISTRY {
        s.add_log(format!("{:30} {}", cmd.syntax, cmd.desc), Color::Cyan);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(input: &str) -> String {
        match parse(input) {
            Parsed::BuyMarket { side, amount } => format!("BUY {} ${}", side, amount),
            Parsed::SellMarket { side, amount } => format!("SELL {} ${}", side, amount),
            Parsed::LimitBuy { side, price, size } => {
                format!("LIMIT {} @{} x{}", side, price, size)
            }
            Parsed::CancelAll => "CANCEL_ALL".into(),
            Parsed::Panic => "PANIC".into(),
            Parsed::ShowMan => "MAN".into(),
            Parsed::Unknown(s) => format!("UNKNOWN:{s}"),
        }
    }

    #[test]
    fn test_commands() {
        assert_eq!(parsed("b up 50"), "BUY up $50");
        assert_eq!(parsed("b down 20"), "BUY down $20");
        assert_eq!(parsed("s up 30"), "SELL up $30");
        assert_eq!(parsed("l up 0.65 100"), "LIMIT up @0.65 x100");
        assert_eq!(parsed("c"), "CANCEL_ALL");
        assert_eq!(parsed("panic"), "PANIC");
        assert_eq!(parsed("man"), "MAN");
    }
}
