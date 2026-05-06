# Wide 65 — Activación

> La variante ya está implementada en `odiseo_strategies.rs`.
> Solo falta activarla con curl.

---

## Estado actual

```
Índice 0: Odiseo 83  ✅ Activo  Budget $8.48
Índice 1: Wide 65    ❌ Inactivo
Índices 2-12:        ❌ Inactivos
```

---

## Plan de activación por fases

### Fase 1: Solo 83 (ahora)
Crecer de $8.48 hasta $12-15 (~4-8 horas).

### Fase 2: Split $6 + $6
```bash
# Odiseo 83 con $6
curl -X POST http://localhost:8080/api/odiseo/budget -H "Content-Type: application/json" -d '{"index": 0, "amount": 6}'

# Activar Wide 65 con $6
curl -X POST http://localhost:8080/api/odiseo/variant -H "Content-Type: application/json" -d '{"index": 1, "enable": true}'
curl -X POST http://localhost:8080/api/odiseo/budget -H "Content-Type: application/json" -d '{"index": 1, "amount": 6}'
```

### Fase 3: $10 + $10  
```bash
curl -X POST http://localhost:8080/api/odiseo/budget -H "Content-Type: application/json" -d '{"index": 0, "amount": 10}'
curl -X POST http://localhost:8080/api/odiseo/budget -H "Content-Type: application/json" -d '{"index": 1, "amount": 10}'
```

### Fase 4: $100 + $100 (cap)
Ambos en su máximo de 100. Ganancia semanal proyectada: ~$5,200.

---

## Wide 65 — Parámetros

| Parámetro | Valor | Con $10 |
|---|---|---|
| Entry | 0.65 | |
| TP | 0.95 | +$4.50 (15 × $0.30) |
| SL Hard | 0.63 | -$0.30 |
| Contratos | 15 | floor(10/0.65) |
| Profit Stop | 15% | $1.50 |
| Max SLs | 4 | máximo -$1.20 |

---

## Verificar estado

```bash
curl -s http://localhost:8080/api/odiseo/status | python3 -c "
import sys,json
d=json.load(sys.stdin)
for v in d['variants'][:2]:
    print(f'{v[\"name\"]}: enabled={v.get(\"enabled\")} budget=\${v.get(\"budget\",0)}')
"
```

---

## Rollback (volver solo a 83)

```bash
curl -X POST http://localhost:8080/api/odiseo/variant -H "Content-Type: application/json" -d '{"index": 1, "enable": false}'
curl -X POST http://localhost:8080/api/odiseo/budget -H "Content-Type: application/json" -d '{"index": 0, "amount": 8}'
```
