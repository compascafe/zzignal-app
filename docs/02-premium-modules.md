# Módulos Premium — Cómo Activarlos

Los módulos Premium son propietarios. Se venden por separado. Cada módulo se activa con una **feature flag** en tiempo de compilación y una **license key** en tiempo de ejecución.

---

## Módulos Disponibles

### 1. Collector — Recolección Multi-Timeframe

**Feature flag**: `premium-collector`

Captura el order book (UP y DOWN) en **6 timeframes** y lo agrega en velas OHLC.

| Timeframe | Intervalo | Uso típico |
|---|---|---|
| 1m | 60 segundos | Scalping |
| 5m | 300 segundos | Day trading |
| 15m | 900 segundos | Swing trading (sesiones Polymarket) |
| 1h | 3600 segundos | Análisis de sesión |
| 4h | 14400 segundos | Macro tendencia |
| 1d | 86400 segundos | Backtesting / datasets |

**Qué captura para cada timeframe y cada side (UP/DOWN):**
- Best Bid: open, high, low, close
- Best Ask: open, high, low, close
- Spread: open, high, low, close
- Mid Price: open, high, low, close
- Bid Volume, Ask Volume
- Tick Count

**Tablas en PostgreSQL**: `ob_timeframes`

**API Endpoints**:
- `GET /api/premium/collector/candles` — Query candles
- `GET /api/premium/collector/stats` — Estadísticas (total candles por intervalo)

---

### 2. Patterns — Detector de Patrones

**Feature flag**: `premium-patterns`

Escanea el order book en tiempo real (cada 200ms) y detecta patrones estadísticos.

| Patrón | Qué detecta | Severidad |
|---|---|---|
| **Wall** | Orden grande (>1000 shares) en top 5 del book | medium |
| **Spread Anomaly** | Spread > 2σ de la media reciente | high |
| **Depth Imbalance** | Bid volume 3x > ask volume (o viceversa) | medium |
| **Spoofing** | Orden grande que aparece y desaparece en <5s | high |
| **Momentum Shift** | Mid price cambia >0.5% en ventana de 10 ticks | medium |
| **Liquidity Vacuum** | Un lado del book sin liquidez | high |

**Las señales se:**
- Persisten en la tabla `pattern_signals`
- Se envían al frontend vía WebSocket broadcast (`{"type":"pattern_alert",...}`)
- El cooldown es de 800ms (máximo 1 alerta/segundo por side)

**API Endpoints**:
- `GET /api/premium/patterns/signals` — Listar señales
- `GET /api/premium/patterns/config` — Ver configuración del detector
- `PUT /api/premium/patterns/config` — Ajustar sensibilidad

---

### 3. Executor — Auto-Ejecución (Bot)

**Feature flag**: `premium-executor`

Motor de reglas: **IF** condiciones del order book **THEN** ejecutar orden.

**Cómo crear una estrategia:**

```json
POST /api/premium/strategies
{
  "name": "Scalp spread alto UP",
  "description": "Compra UP cuando el spread es >0.05",
  "conditions": [
    {
      "metric": "spread",
      "operator": "gt",
      "value": 0.05,
      "side": "up"
    }
  ],
  "action": {
    "order_type": "limit",
    "outcome": "up",
    "price": 0.52,
    "size": 10
  },
  "cooldown_secs": 30,
  "max_positions": 3,
  "stop_loss_pct": 5.0,
  "take_profit_pct": 10.0
}
```

**Métricas disponibles**:
- `spread`, `mid_price`, `best_bid`, `best_ask`
- `bid_volume`, `ask_volume`, `imbalance`
- `btc_price`

**Operadores**: `gt` (>), `lt` (<), `gte` (>=), `lte` (<=), `eq` (==)

**Tipos de órdenes**: `limit`, `market`, `scalp`

**Controles de riesgo**:
- `cooldown_secs` — Espera mínima entre ejecuciones
- `max_positions` — Máximo de posiciones abiertas simultáneas
- `max_size_total` — Tamaño máximo total en USDC
- `stop_loss_pct` — Cerrar si pérdida > X%
- `take_profit_pct` — Cerrar si ganancia > X%

**API Endpoints**:
- `GET /api/premium/strategies` — Listar estrategias
- `POST /api/premium/strategies` — Crear estrategia
- `POST /api/premium/strategies/{id}/toggle` — Activar/pausar
- `DELETE /api/premium/strategies/{id}` — Eliminar
- `GET /api/premium/executions` — Log de ejecuciones

---

## Cómo Comprar y Activar un Módulo

### Paso 1: Elegir el módulo

Contactar al vendedor indicando qué módulo(s) quieres:
- `collector` — Recolección multi-timeframe
- `patterns` — Detección de patrones
- `executor` — Auto-ejecución
- `all` — Los tres módulos

### Paso 2: Recibir la license key

El vendedor ejecuta:
```bash
cargo run --bin license-gen -- --module collector --customer tu-empresa --days 365
```

Esto genera un token firmado.

### Paso 3: Configurar la license key

Agregar al `.env` del servidor:
```env
ZZIGNAL_LICENSE_KEY=eyJtb2R1bGUiOiJjb2xsZWN0b3IiLCJjdXN0b21lciI6...
```

### Paso 4: Compilar con la feature flag

```bash
# Solo Collector
cargo build --release --features premium-collector

# Collector + Patterns
cargo build --release --features premium-collector,premium-patterns

# Todos
cargo build --release --features premium-all
```

### Paso 5: Verificar que funciona

```bash
# El módulo debería aparecer en los logs al iniciar:
#  INFO Collector: migración OK, iniciando captura multi-timeframe...

# Verificar endpoints:
curl http://localhost:8080/api/premium/collector/stats
```

---

## Dependencias entre módulos

```
premium-executor → premium-patterns → premium-collector
```

- Para usar **Patterns**, necesitas **Collector** (los patrones se detectan sobre datos en tiempo real)
- Para usar **Executor**, necesitas **Patterns** (el bot evalúa condiciones del order book que ya monitorea el detector)

---

## Dashboard Ejecutivo (Frontend)

En el frontend, la pestaña **"Ejecutivo"** aparece automáticamente. Tiene 4 sub-tabs:

| Tab | Módulo requerido | Funcionalidad |
|---|---|---|
| **Estrategias** | Executor | Crear, activar/pausar, eliminar estrategias |
| **Patrones** | Patterns | Feed en tiempo real de señales detectadas |
| **Ejecuciones** | Executor | Historial de órdenes ejecutadas por el bot |
| **Datos** | Collector | Estadísticas de velas capturadas por timeframe |

> Si un módulo no está activo, su tab muestra "no disponible".
