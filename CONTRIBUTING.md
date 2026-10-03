# Contributing

Thanks for your interest in ZZignal! This is a real-money trading tool, so
correctness and safety come first.

## Development setup

Requirements: **Rust 1.91+** (MSRV), Linux or macOS.

```bash
cp .env.example .env        # fill with your credentials; only needed to run
cd backend_rust && cargo run --release
cd TUI_monitor  && cargo run --release
```

You can develop the TUI against any backend instance:

```bash
ZZIGNAL_API_URL=http://192.168.1.10:8080 \
ZZIGNAL_WS_URL=ws://192.168.1.10:8080/ws cargo run
```

## Before opening a PR

Both crates must be clean and green:

```bash
cd backend_rust && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
cd TUI_monitor  && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
```

Guidelines:

- Keep the hot path allocation-free where practical; measure with
  `GET /api/perf` before/after latency-sensitive changes.
- Never log, print, or persist key material. `POLYMARKET_PRIVATE_KEY` must not
  appear in any output, fixture, or test file.
- Do not add hardcoded wallet addresses, RPC endpoints with credentials, or
  personal data of any kind.
- New API surface must be documented in `docs/API.md`.
- Commit messages: short imperative subject (`perf: …`, `fix: …`, `docs: …`).

## Reporting bugs

Open a GitHub issue with:

1. What you expected vs. what happened.
2. Backend log excerpt (`RUST_LOG` is not required; default logs are fine).
3. `GET /api/health` and `GET /api/perf` output when relevant.
4. Commit hash (`polymarket-backend` prints it at startup; the TUI shows it in
   the top bar).

For security issues, follow [`SECURITY.md`](SECURITY.md) instead of opening a
public issue.
