# ZZignal App — Documentación

> Dashboard de trading algorítmico para Polymarket (High Frequency Trading en mercados de predicción BTC).

---

## Estructura de la Documentación

| Archivo | Contenido |
|---|---|
| `README.md` | Este índice |
| `01-core-open-source.md` | Core: instalación, arquitectura, APIs, WebSocket |
| `02-premium-modules.md` | Módulos Premium: qué son, cómo comprarlos, cómo activarlos |
| `03-license-system.md` | Sistema de licencias: generación de tokens, validación |
| `04-api-reference.md` | API REST completa (Core + Premium) |
| `05-deployment.md` | Cómo desplegar en producción (EC2, systemd, nginx) |
| `06-architecture.md` | Arquitectura técnica completa (mismo que `../ARCHITECTURE.md`) |

---

## Primeros Pasos

### Requisitos
- Rust 1.91+
- Node.js 18+
- PostgreSQL 14+ (opcional para Core, requerido para módulos Premium)

### Instalación rápida

```bash
# 1. Clonar
git clone https://github.com/compascafe/zzignal-app.git
cd zzignal-app

# 2. Configurar credenciales
cp .env.example .env
# Editar .env con tus credenciales de Polymarket
#   POLYMARKET_PRIVATE_KEY=0x...
#   CLOB_API_KEY=...
#   CLOB_API_SECRET=...
#   CLOB_API_PASSPHRASE=...
#   DATABASE_URL=postgres://user:pass@host/db   (opcional)

# 3. Backend (solo Core Open Source)
cd backend_rust
cargo run --release

# 4. Frontend
cd frontend_react
npm install
npm run dev
```

Abre http://localhost:5173

### Compilar con módulos Premium

```bash
cd backend_rust

# Solo Collector
cargo build --release --features premium-collector

# Todos los módulos Premium
cargo build --release --features premium-all

# Generar licencia para un cliente
cargo run --bin license-gen -- --module collector --customer acme-corp --days 365
```

---

## Módulos del Sistema

```
┌─────────────────────────────────────────────────────────────┐
│                      ZZIGNAL APP                            │
├─────────────────────┬───────────────────────────────────────┤
│     OPEN SOURCE     │           PROPIETARIO (Premium)       │
│     (MIT License)   │           (Licencia de pago)          │
├─────────────────────┼──────────────┬────────────┬───────────┤
│       CORE          │  COLLECTOR   │  PATTERNS  │ EXECUTOR  │
│                     │              │            │           │
│ • Conexión CLOB     │ • Order book │ • Walls    │ • Bot de  │
│ • Auth Polymarket   │   multi-TF   │ • Anomalías│   trading │
│ • BTC multi-prov.   │ • 1m,5m,15m, │ • Imbalance│ • Reglas  │
│ • Órdenes limit/    │   1h,4h,1d   │ • Spoofing │   IF-THEN │
│   market/scalp      │ • OHLC bid/  │ • Momentum │ • Auto-    │
│ • Velas sintéticas  │   ask/spread │ • Alertas  │   ejecución│
│ • API REST + WS     │ • Export     │   realtime │ • Cooldown │
│ • UI de trading     │   CSV/JSON   │            │ • Stop loss│
├─────────────────────┼──────────────┼────────────┼───────────┤
│       GRATIS        │   $ POR       │  $ POR     │  $ POR    │
│                     │   MÓDULO      │  MÓDULO    │  MÓDULO   │
└─────────────────────┴──────────────┴────────────┴───────────┘
```
