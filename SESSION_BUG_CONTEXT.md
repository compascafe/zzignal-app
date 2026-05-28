# Session Bug — Contexto para continuar

## Problema
El TUI (`zz-monitor`) no muestra correctamente la sesión de 15 minutos. Tras restart, muestra sesiones de 30/60 min o countdown incorrecto (20 min en vez de 15).

## Commits aplicados (todos en main)

| Commit | Descripción |
|--------|-------------|
| `694641f` | BTC 15-min HARD LOCK + anti-duplicate session guards |
| `08ed0f4` | deploy.sh ahora compila el monitor (build.rs sin rerun-if-changed) |
| `be73471` | deploy.sh +x perm fix |
| `a8cf0d2` | BTC delta neutral at zero (─ FLAT amber) |
| `c334b5a` | Recovery validation + scheduler hard-lock |
| `94607cd` | `get_sessions_to_start` filter duration_min=15 + cancel_non_15min_scheduled cleanup |
| `20092a5` | Touch build.rs antes de build para forzar hash |
| `afe8176` | Countdown always 15-min from NOW (sobre-agresivo, causó bug inverso) |
| `18e72ba` | Countdown usa scheduled_end real, cappeado a now+15min |

## Archivos modificados

- `backend_rust/src/db/api.rs` — start_session hard lock, anti-duplicate guard, /api/sessions filter
- `backend_rust/src/db/repository.rs` — list_sessions_with_status, cancel_non_15min_scheduled, get_sessions_to_start filter
- `backend_rust/src/db/scheduler.rs` — auto_generate_child + recover_orphaned_parents force chunk_min=15
- `backend_rust/src/main.rs` — recovery validation, cleanup, t5/t3 state restore
- `monitor/src/main.rs` — 's' key guard (check existing session)
- `monitor/src/ui.rs` — BTC delta neutral display
- `monitor/build.rs` — rerun-if-changed + touch en deploy
- `deploy.sh` — build monitor step + touch build.rs

## Estado actual del bug
- **Sesiones de 30/60 min**: fixed (hard lock en creación, scheduler, recovery, startup cleanup)
- **Countdown incorrecto**: el fix `18e72ba` usa `scheduled_end.min(now+15min)` para auto-start y recovery. Si la sesión ya empezó y le quedan 7 min, debe mostrar 7 min.
- **PENDIENTE**: verificar que el countdown funciona correctamente tras deploy en servidor.

## Para deployar en servidor
```bash
git fetch origin && git reset --hard origin/main && chmod +x deploy.sh && ./deploy.sh
```

## Para debuggear en servidor
```sql
-- Ver sesiones recording
SELECT id, name, status, duration_min, scheduled_start, scheduled_end, started_at 
FROM recording_sessions WHERE status IN ('recording','scheduled') ORDER BY id DESC LIMIT 10;

-- Ver sesiones no-15min
SELECT id, name, status, duration_min FROM recording_sessions WHERE duration_min != 15;
```

## Posibles bugs restantes
1. `btc_open` solo se actualiza al arrancar el backend (Gamma API). Tras cambio de ronda (cada 15min), queda stale hasta el siguiente restart.
2. El countdown podría no estar sincronizado con la ronda real de Polymarket si la sesión recuperada tiene `scheduled_end` incorrecto.
