#!/usr/bin/env python3
"""Monte Carlo backtest: Senna vs Houdini 65 — same CSV data, randomized timing."""

import csv, sys, random, statistics, math
from collections import defaultdict

CSV_FILE = sys.argv[1] if len(sys.argv) > 1 else "/Volumes/DMAC/ZZIGNAL_PROJECT/csv_zzignal/session_1154_BTC1520260509_0930UTC_hft.csv"
N_SIMS = 50   # Monte Carlo iterations
DELAY_JITTER_MS = 200  # randomize entry timing by ±200ms

# ─── Read CSV data ───────────────────────────────────────────────────
rows = []
with open(CSV_FILE) as f:
    # Skip comments, find header
    for line in f:
        if line.startswith('#'):
            continue
        parts = line.strip().split(',')
        if len(parts) < 69:
            continue
        # Header: col 0=time, 2=event, 4=binance_price, 8=btc_vel, 14=mid,
        # 23=secs_left, 27=clob_trade_up, 28=clob_trade_dn
        try:
            ts = parts[0]
            ev = parts[2]
            vel = float(parts[8]) if parts[8] else 0.0
            mid = float(parts[14]) if parts[14] else 0.0
            sl = int(parts[23]) if parts[23] else 0
            up = float(parts[27]) if parts[27] else 0.0
            dn = float(parts[28]) if parts[28] else 0.0
            rows.append({'ts': ts, 'ev': ev, 'vel': vel, 'mid': mid, 'sl': sl, 'up': up, 'dn': dn})
        except:
            pass

if not rows:
    print("ERROR: No data rows found")
    sys.exit(1)

print(f"Loaded {len(rows)} ticks from {CSV_FILE}")
print(f"Time range: {rows[0]['ts']} → {rows[-1]['ts']}")

# ─── Strategy execution ──────────────────────────────────────────────

def run_strategy(name, rows, config, jitter=False):
    """Run a strategy on the tick data. Returns (trades, pnl_total, wins, losses)."""
    pos = None  # {side:'UP'|'DN', entry:float, size:int, max_price:float, trail_high:float}
    trades = []
    pnl = 0.0
    wins = 0
    losses = 0
    prices_up = []
    prices_dn = []
    confirm = 0
    last_px = 0.0

    for r in rows:
        px_up = r['up']
        px_dn = r['dn']
        vel = r['vel']
        sl = r['sl']
        mid = r['mid']

        # Price source: prefer trade data, fallback to mid if alive
        def fresh_px(px_trade, px_mid):
            if px_trade > 0: return (px_trade, True)
            if abs(px_mid - 0.5) > 0.01 and px_mid > 0: return (px_mid, True)
            return (0.0, False)

        # Skip tick with jitter (Monte Carlo)
        if jitter and random.randint(0, DELAY_JITTER_MS * 2) < DELAY_JITTER_MS:
            continue

        # ─── Manage open position ───
        if pos:
            side = pos['side']
            px, _ = fresh_px(px_up if side == 'UP' else px_dn, mid)
            if px == 0: px = last_px if last_px > 0 else pos['entry']
            if px > 0: last_px = px
            pos['max_price'] = max(pos['max_price'], px)

            # Enforce boundaries
            if sl <= 60 and pos['entry'] > 0:
                # Last 60s: force exit at market
                fill = px
                trade_pnl = (fill - pos['entry']) * pos['size'] if side == 'UP' else (fill - pos['entry']) * pos['size']
                pnl += trade_pnl
                trades.append({'side': side, 'entry': pos['entry'], 'exit': fill, 'pnl': trade_pnl, 'reason': 'boundary'})
                if trade_pnl > 0: wins += 1
                else: losses += 1
                pos = None
                continue

            exit_px = None
            exit_reason = None
            tp, sl_hard, trail = config['tp'], config['sl_hard'], config['trail']

            # Scalp mode uses relative TP/SL
            if config.get('scalp'):
                tp = pos['entry'] + 0.03
                sl_hard = pos['entry'] - 0.02
                trail = 0.01

            if px >= tp:
                exit_px = tp; exit_reason = 'TP'
            elif px <= sl_hard:
                exit_px = px; exit_reason = 'SL'
            elif pos['max_price'] > 0 and px <= pos['max_price'] - trail:
                exit_px = px; exit_reason = 'trail'

            if exit_px:
                trade_pnl = (exit_px - pos['entry']) * pos['size'] if side == 'UP' else (exit_px - pos['entry']) * pos['size']
                pnl += trade_pnl
                trades.append({'side': side, 'entry': pos['entry'], 'exit': exit_px, 'pnl': trade_pnl, 'reason': exit_reason})
                if trade_pnl > 0: wins += 1
                else: losses += 1
                pos = None
                last_px = 0
                continue

        # ─── Check for new entry ───
        for direction, px_trade in [('UP', px_up), ('DN', px_dn)]:
            if pos: break  # already have a position
            px, fresh = fresh_px(px_trade, mid)
            if px <= 0: continue

        if config.get('scalp'):
            # Scalp mode: momentum entry (px must be >= entry_threshold)
            if px < config['entry_threshold'] or px > config['tp']:
                continue
            prices_up.append(px) if direction == 'UP' else prices_dn.append(px)
            prices = prices_up if direction == 'UP' else prices_dn
            if len(prices) > 4: prices.pop(0)
            if len(prices) >= 2:
                gap = px - prices[0]
                if gap >= config['momentum_delta']:
                    btc_ok = (direction == 'UP' and vel > -5.0) or (direction == 'DN' and vel < 5.0)
                    if btc_ok:
                        sz = math.floor(config['budget'] / px)
                        if sz < 1: sz = 1
                        pos = {'side': direction, 'entry': px, 'size': sz, 'max_price': px}
        else:
                # Threshold mode
                if px >= config['entry_threshold'] and px <= config['tp']:
                    # BTC momentum check
                    btc_ok = (direction == 'UP' and vel > -5.0) or (direction == 'DN' and vel < 5.0)
                    if btc_ok:
                        confirm += 1
                        if confirm >= config['confirm_ticks']:
                            pos = {'side': direction, 'entry': px, 'size': config['budget'] / px, 'max_price': px}
                            confirm = 0
                    elif fresh:
                        confirm = 0
                elif fresh:
                    confirm = 0

    # Force close any open position at end
    if pos:
        last_px = last_px if last_px > 0 else pos['entry']
        trade_pnl = (last_px - pos['entry']) * pos['size']
        pnl += trade_pnl
        trades.append({'side': pos['side'], 'entry': pos['entry'], 'exit': last_px, 'pnl': trade_pnl, 'reason': 'EOS'})
        if trade_pnl > 0: wins += 1
        else: losses += 1

    return trades, pnl, wins, losses

# ─── Strategies ──────────────────────────────────────────────────────
SENNA = {
    'scalp': True,
    'momentum_delta': 0.015,
    'budget': 20,
    'tp': 0.99, 'sl_hard': 0.20, 'trail': 0.01,
    'confirm_ticks': 1, 'entry_threshold': 0.30,
}

H65 = {
    'scalp': False,
    'budget': 20,
    'entry_threshold': 0.65, 'tp': 0.75,
    'sl_hard': 0.60, 'trail': 0.04,
    'confirm_ticks': 1,
}

# ─── Run once (deterministic) ────────────────────────────────────────
print("\n=== DETERMINISTIC RUN ===")
for name, cfg in [("Senna", SENNA), ("Houdini 65", H65)]:
    trades, pnl, wins, losses = run_strategy(name, rows, cfg, jitter=False)
    total = wins + losses
    wr = wins / total * 100 if total > 0 else 0
    avg_pnl = pnl / total if total > 0 else 0
    print(f"\n{name}:")
    print(f"  Trades: {total}  Wins: {wins}  Losses: {losses}  WR: {wr:.0f}%")
    print(f"  PnL: ${pnl:+.2f}  Avg: ${avg_pnl:+.2f}/trade  Budget: ${cfg['budget']}")
    # Show exit reasons
    from collections import Counter
    reasons = Counter(t['reason'] for t in trades)
    for r, c in reasons.most_common():
        print(f"  {r}: {c}")
    # Show first 5 trades
    for t in trades[:5]:
        print(f"    {t['side']} entry={t['entry']:.3f} exit={t['exit']:.3f} pnl={t['pnl']:+.2f} [{t['reason']}]")

# ─── Monte Carlo (50 runs with timing jitter) ────────────────────────
print(f"\n\n=== MONTE CARLO ({N_SIMS} runs, ±{DELAY_JITTER_MS}ms jitter) ===")
for name, cfg in [("Senna", SENNA), ("H65", H65)]:
    pnls = []
    winrates = []
    trade_counts = []
    for _ in range(N_SIMS):
        trades, pnl, wins, losses = run_strategy(name, rows, cfg, jitter=True)
        pnls.append(pnl)
        total = wins + losses
        winrates.append(wins / total * 100 if total > 0 else 0)
        trade_counts.append(total)

    avg_pnl = statistics.mean(pnls)
    std_pnl = statistics.stdev(pnls) if len(pnls) > 1 else 0
    avg_wr = statistics.mean(winrates)
    avg_trades = statistics.mean(trade_counts)
    max_loss = min(pnls)
    max_gain = max(pnls)

    print(f"\n{name}:")
    print(f"  PnL: ${avg_pnl:+.2f} ± ${std_pnl:.2f}  [{max_loss:+.2f} … {max_gain:+.2f}]")
    print(f"  WinRate: {avg_wr:.0f}%  Trades: {avg_trades:.0f}/sim")
    print(f"  PnL range: {min(pnls):+.2f} to {max(pnls):+.2f}")
