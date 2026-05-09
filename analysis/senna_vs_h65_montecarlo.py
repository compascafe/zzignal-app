#!/usr/bin/env python3
"""Monte Carlo backtest: Senna vs Houdini 65 — same CSV, real price logic."""

import sys, random, statistics, math, csv

CSV_FILES = sys.argv[1:] if len(sys.argv) > 1 else [
    "/Volumes/DMAC/ZZIGNAL_PROJECT/csv_zzignal/session_1195_BTC1520260509_1945UTC_hft.csv",
    "/Volumes/DMAC/ZZIGNAL_PROJECT/csv_zzignal/session_1194_BTC1520260509_1930UTC_hft.csv",
]
N_SIMS = 50
JITTER_TICKS = 2  # skip 0..2 random ticks for Monte Carlo variation

def load_csv(path):
    rows = []
    with open(path) as f:
        for line in f:
            if line.startswith('#'): continue
            parts = line.strip().split(',')
            if len(parts) < 69: continue
            try:
                rows.append({
                    'ts': parts[0], 'ev': parts[2],
                    'vel': float(parts[8]) if parts[8] else 0.0,
                    'mid': float(parts[14]) if parts[14] else 0.0,
                    'sl': int(parts[23]) if parts[23] else 0,
                    'up': float(parts[27]) if parts[27] else 0.0,
                    'dn': float(parts[28]) if parts[28] else 0.0,
                    'bid_vol': float(parts[16]) if len(parts)>16 and parts[16] else 0.0,
                    'ask_vol': float(parts[17]) if len(parts)>17 and parts[17] else 0.0,
                })
            except: pass
    return rows

def fresh_price(px_trade, mid):
    """Mimics bot: lt > raw_trade > mid(alive) > last_px."""
    if px_trade > 0: return px_trade, True
    if abs(mid - 0.5) > 0.01 and mid > 0: return mid, True
    return 0.0, False

def run_strategy(rows, cfg, jitter=False):
    """Run strategy on tick data. Returns trades list."""
    trades = []
    pos = None          # {side, entry, size, max_price}
    last_px = 0.0
    confirm = 0
    prices_up, prices_dn = [], []  # scalp mode price windows
    raw_up, raw_dn = 0.0, 0.0     # raw trade tracking
    skip = 0

    for r in rows:
        if jitter and skip > 0:
            skip -= 1
            continue
        if jitter:
            skip = random.randint(0, JITTER_TICKS)

        vel = r['vel']; mid = r['mid']; sl = r['sl']
        px_up, px_dn = r['up'], r['dn']

        # Track raw trade prices
        if px_up > 0: raw_up = px_up
        if px_dn > 0: raw_dn = px_dn

        # ─── Manage open position ───
        if pos:
            side = pos['side']
            px_trade = px_up if side == 'UP' else px_dn
            raw = raw_up if side == 'UP' else raw_dn
            px, _ = fresh_price(px_trade, mid)
            if px == 0: px = raw
            if px == 0: px = last_px if last_px > 0 else pos['entry']
            if px > 0: last_px = px
            pos['max_price'] = max(pos['max_price'], px)

            # Boundary: last 60s force exit
            if sl <= 60 and sl >= 0:
                trades.append({'side': side, 'entry': pos['entry'], 'exit': px, 'pnl': (px-pos['entry'])*pos['size'], 'reason': 'boundary'})
                pos = None; continue

            tp = pos['entry'] + 0.03 if cfg.get('scalp') else cfg['tp']
            sl_hard = pos['entry'] - 0.02 if cfg.get('scalp') else cfg['sl_hard']
            trail_d = 0.01 if cfg.get('scalp') else cfg['trail']

            if px >= tp:
                trades.append({'side': side, 'entry': pos['entry'], 'exit': tp, 'pnl': (tp-pos['entry'])*pos['size'], 'reason': 'TP'})
                pos = None; continue
            if px <= sl_hard:
                trades.append({'side': side, 'entry': pos['entry'], 'exit': px, 'pnl': (px-pos['entry'])*pos['size'], 'reason': 'SL'})
                pos = None; continue
            if pos['max_price'] > 0 and px <= pos['max_price'] - trail_d:
                trades.append({'side': side, 'entry': pos['entry'], 'exit': px, 'pnl': (px-pos['entry'])*pos['size'], 'reason': 'trail'})
                pos = None; continue

        # ─── Check entry ───
        for side, px_trade, raw in [('UP', px_up, raw_up), ('DN', px_dn, raw_dn)]:
            if pos: break
            px, fresh = fresh_price(px_trade, mid)
            if px == 0: px = raw
            if px == 0: continue

            if cfg.get('scalp'):
                # Scalp: momentum entry
                if px < cfg['entry_threshold'] or px > cfg['tp']: continue
                pw = prices_up if side == 'UP' else prices_dn
                pw.append(px)
                if len(pw) > 4: pw.pop(0)
                if len(pw) >= 2 and (px - pw[0]) >= cfg['momentum_delta']:
                    sz = max(1, int(cfg['budget'] / px))
                    pos = {'side': side, 'entry': px, 'size': sz, 'max_price': px}
                    pw.clear()
            else:
                # Threshold entry
                if not (cfg['entry_threshold'] <= px <= cfg['tp']): continue
                # Momentum: only on fresh data
                if fresh and last_px > 0 and px <= last_px: continue
                # BTC momentum
                btc_ok = (side == 'UP' and vel > -5.0) or (side == 'DN' and vel < 5.0)
                if not btc_ok: continue
                confirm += 1
                if confirm >= cfg.get('confirm_ticks', 1):
                    sz = max(1, int(cfg['budget'] / px))
                    pos = {'side': side, 'entry': px, 'size': sz, 'max_price': px}
                    confirm = 0

    # Force close at end
    if pos:
        px = last_px if last_px > 0 else pos['entry']
        trades.append({'side': pos['side'], 'entry': pos['entry'], 'exit': px, 'pnl': (px-pos['entry'])*pos['size'], 'reason': 'EOS'})
    return trades

SENNA = {'scalp': True, 'momentum_delta': 0.015, 'budget': 20, 'entry_threshold': 0.30, 'tp': 0.99, 'sl_hard': 0.0, 'trail': 0.0, 'confirm_ticks': 1}
H65   = {'scalp': False, 'momentum_delta': 0.0, 'budget': 20, 'entry_threshold': 0.65, 'tp': 0.75, 'sl_hard': 0.60, 'trail': 0.04, 'confirm_ticks': 1}

for fpath in CSV_FILES:
    rows = load_csv(fpath)
    if not rows:
        print(f"\nSKIP {fpath}: no data")
        continue
    print(f"\n{'='*60}\n{fpath}\nTicks: {len(rows)} | {rows[0]['ts']} → {rows[-1]['ts']}\n")

    for cfg_name, cfg in [("SENNA", SENNA), ("H65", H65)]:
        # Deterministic run
        trades = run_strategy(rows, cfg, jitter=False)
        wins = sum(1 for t in trades if t['pnl'] > 0)
        total = len(trades)
        pnl = sum(t['pnl'] for t in trades)
        wr = wins/total*100 if total else 0
        reasons = {}
        for t in trades: reasons[t['reason']] = reasons.get(t['reason'], 0) + 1

        # Monte Carlo
        pnls_mc = []
        for _ in range(N_SIMS):
            t2 = run_strategy(rows, cfg, jitter=True)
            pnls_mc.append(sum(t['pnl'] for t in t2))

        print(f"  {cfg_name}: {total} trades | WR {wr:.0f}% | PnL ${pnl:+.2f} | MC avg ${statistics.mean(pnls_mc):+.2f}±${statistics.stdev(pnls_mc) if len(pnls_mc)>1 else 0:.2f}")
        print(f"    Exits: {reasons}")
        if trades:
            # Best/worst
            best = max(trades, key=lambda t: t['pnl'])
            worst = min(trades, key=lambda t: t['pnl'])
            print(f"    Best: {best['side']} e={best['entry']:.3f} x={best['exit']:.3f} pnl={best['pnl']:+.2f} [{best['reason']}]")
            print(f"    Worst: {worst['side']} e={worst['entry']:.3f} x={worst['exit']:.3f} pnl={worst['pnl']:+.2f} [{worst['reason']}]")
