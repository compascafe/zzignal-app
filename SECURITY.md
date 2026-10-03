# Security Policy

ZZignal handles a private key that can move real funds and places real
orders. Treat it accordingly.

## Reporting a vulnerability

Please **do not open a public issue** for security problems. Use GitHub's
private vulnerability reporting ("Security" tab → "Report a vulnerability")
or contact the maintainer directly. You can expect an initial response
within a few days.

## Scope

In scope:

- Leakage of `POLYMARKET_PRIVATE_KEY`, CLOB API credentials, or the
  funder/proxy address through logs, API responses, CSV files, or panics.
- Order-placement or cancellation paths that can be triggered by an
  unauthenticated network peer beyond the intended local use.
- Remote code execution, memory unsafety, or denial of service in the
  backend or TUI.

Not in scope (by design):

- The REST API and WebSocket have **no authentication** and permissive CORS.
  They are intended to run on localhost or a trusted private network. Exposing
  port 8080 publicly is a misconfiguration, not a vulnerability.
- Losses caused by trading decisions or market conditions.

## Known advisories

`cargo audit` runs in CI against both lockfiles and currently passes. The
remaining RustSec entries are informational warnings on transitive
dependencies (unmaintained crates and advisories without a released fix),
tracked upstream:

- `derivative`, `paste`, `proc-macro-error2` — unmaintained proc-macro
  crates pulled in by the Polymarket SDK / `alloy` stack.
- `lru` — panic-safety advisory in `LruCache::pop()` (used internally by
  `ratatui`; unfixed upstream at the time of writing).

## Operational guidance

- Keep `.env` out of version control (it is gitignored) and out of backups
  you do not control.
- Prefer a dedicated wallet with limited funds for this bot.
- Do not expose the backend port to the internet. If remote access is needed,
  put it behind a VPN or an authenticating reverse proxy.
- Rotate CLOB API credentials immediately if you suspect they leaked.
