# ZZignal — Setup & Migration Guide (Mayo 2026)

> Guía completa del proceso de migración a CLOB V2, swap USDC nativo → pUSD, y configuración del bot.
> Verificado y funcional al **6 de Mayo de 2026**.

---

## 1. Resumen del Problema

Polymarket migró a **CLOB V2** el 28 de Abril de 2026. Cambios clave:

| Concepto | V1 (antes) | V2 (ahora) |
|---|---|---|
| Colateral | USDC.e | **pUSD** (Polymarket USD) |
| SDK Rust | `polymarket_client_sdk` | `polymarket_client_sdk_v2 = "0.6.0-canary.1"` |
| URL CLOB | `clob.polymarket.com` | `clob.polymarket.com` (misma, backend nuevo) |
| EIP-712 Exchange | versión `"1"` | versión `"2"` |
| Endpoint producción | `clob.polymarket.com` | `clob.polymarket.com` (NO `clob-v2`) |
| Approvals | USDC.e → CTF Exchange | pUSD → CTF Exchange V2 + Neg Risk |

---

## 2. Wallets

### EOA (signing + trading)
```
Dirección:  0xC9131fE7Ec4dBa6e606dC6e4a6b057A37bBdeB13
Private key: POLYMARKET_PRIVATE_KEY en .env
Propósito:  Firmar órdenes, pagar gas, almacenar pUSD
```

### Proxy Wallet (cuenta Magic/email en Polymarket)
```
Dirección:  0x0000000000000000000000000000000000000000
Propósito:  Cuenta de Polymarket creada con email (Magic Link)
            El pUSD estaba aquí originalmente (wrapeado desde la UI)
Vinculación: Gamma API confirma EOA → proxy (WalletDeployed on-chain)
```

### Nota importante sobre el proxy
La UI de Polymarket (web) muestra el balance del proxy. La API CLOB con L2 keys
**NO reconoce el proxy** como funder cuando se autentica con API keys.
Por eso el bot usa `SignatureType::Eoa` y opera directamente desde la EOA.

---

## 3. Estado Actual (funcionando)

```
✅ SDK: polymarket_client_sdk_v2 = "0.6.0-canary.1"
✅ SignatureType::Eoa (EOA directa, sin proxy)
✅ Balance CLOB: ~$7.96 pUSD
✅ Allowances: MAX_U256 para CTF Exchange V2, Neg Risk Exchange, Neg Risk Adapter
✅ URL CLOB: https://clob.polymarket.com
✅ Auto-wrap: al arrancar, si balance=0, detecta USDC.e en EOA y wrappea
✅ Wrap endpoint: POST /api/wrap — convierte USDC.e → pUSD
✅ RPC Polygon: https://polygon-bor-rpc.publicnode.com
```

---

## 4. Contratos Relevantes (Polygon Mainnet)

### Polymarket (CLOB V2)
| Contrato | Dirección |
|---|---|
| CTF Exchange V2 | `0xE111180000d2663C0091e4f400237545B87B996B` |
| Neg Risk CTF Exchange | `0xe2222d279d744050d28e00520010520000310F59` |
| Neg Risk Adapter | `0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296` |
| Conditional Tokens (CTF) | `0x4D97DCd97eC945f40cF65F87097ACe5EA0476045` |
| CollateralOnramp (wrap) | `0x93070a847efEf7F70739046A929D47a521F5B8ee` |
| CollateralOfframp (unwrap) | `0x2957922Eb93258b93368531d39fAcCA3B4dC5854` |
| pUSD token | `0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB` |
| pUSD implementation | `0x6bBCef9f7ef3B6C592c99e0f206a0DE94Ad0925f` |

### Tokens
| Token | Dirección | Decimales |
|---|---|---|
| USDC nativo (Circle) | `0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359` | 6 |
| USDC.e (bridged) | `0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174` | 6 |
| pUSD | `0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB` | 6 |

### Uniswap V3 (Polygon)
| Contrato | Dirección |
|---|---|
| SwapRouter | `0xE592427A0AEce92De3Edee1F18E0157C05861564` |
| **NO usar Router02** | `0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45` (revierte) |

---

## 5. Flujos de Trading

### 5.1 Depósito de fondos (fondeo)

```
1. Comprar USDC nativo en exchange (Binance, Coinbase)
2. Withdraw a Polygon → EOA (0xc9131fE7...)
3. Ejecutar swap_wrap_check.rs:
   cargo run --example swap_wrap_check --release
```

**O manualmente paso a paso:**

```bash
# Opción A: Usar el script completo (swap + wrap + verificar)
cargo run --example swap_wrap_check --release

# Opción B: Si ya tienes USDC.e en la EOA
curl -X POST http://localhost:8080/api/wrap
```

### 5.2 Withdraw (retiro de fondos)

**Paso 1: Unwrap pUSD → USDC.e**
```rust
// CollateralOfframp.unwrap(asset, to, amount)
// asset = USDC.e address (0x2791Bca...)
// to = EOA (recibe USDC.e)
// amount = pUSD a convertir (6 decimales)

use alloy::sol;

sol! {
    #[sol(rpc)]
    interface ICollateralOfframp {
        function unwrap(address _asset, address _to, uint256 _amount) external;
    }
}

let offramp = ICollateralOfframp::new(
    address!("0x2957922Eb93258b93368531d39fAcCA3B4dC5854"),
    provider.clone()
);

// Primero: approve pUSD → Offramp
// pUSD token: 0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB

// Luego: unwrap
offramp.unwrap(
    address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"), // USDC.e
    eoa,
    amount_pusd
).send().await?.watch().await?;
```

**Paso 2: Swap USDC.e → USDC nativo (opcional)**
```bash
# Usar el mismo swap pero inverso:
cargo run --example swap_wrap_check --release
# (modificar el script para hacer USDC.e → USDC nativo)
```

**Paso 3: Enviar a exchange**
- Transfer normal de ERC-20 desde MetaMask o vía código con alloy

**Alternativa vía Polymarket UI:**
1. Importar EOA `0xc91...` en MetaMask
2. Conectar en polymarket.com
3. La UI muestra el balance de la EOA
4. Usar el botón "Withdraw" de la UI

---

## 6. Comandos Útiles del Servidor

```bash
# Verificar estado
curl -s http://localhost:8080/api/status

# Ver balance CLOB
curl -s http://localhost:8080/api/balance

# Verificar billetera
curl -s http://localhost:8080/api/btc

# Wrap USDC.e → pUSD (si hay USDC.e en EOA)
curl -X POST http://localhost:8080/api/wrap

# Ver Odiseo
curl -s http://localhost:8080/api/odiseo/status

# Logs
sudo journalctl -u zzignal-app -f
sudo journalctl -u zzignal-app -f | grep -i balance

# Reiniciar
sudo systemctl restart zzignal-app
```

---

## 7. Ejemplos del Proyecto

| Archivo | Propósito |
|---|---|
| `examples/swap_wrap_check.rs` | Swap USDC nat→USDC.e + Wrap→pUSD + Verificar CLOB |
| `examples/check_balance.rs` | Debug de balance con todos los SignatureType |
| `examples/regen_keys.rs` | Regenerar CLOB API keys |
| `examples/debug_balance.rs` | HTTP directo al CLOB con L2 auth |
| `examples/gamma_check.rs` | Verificar vínculo EOA→proxy en Gamma API |
| `examples/test_auth_order.rs` | Probar orden de autenticación |
| `examples/wrap_and_check.rs` | Solo wrap USDC→pUSD + verificar CLOB |
| `examples/approve_proxy.rs` | Aprobar contratos vía relayer (WIP) |

---

## 8. Configuración del .env

```env
# Obligatorio
POLYMARKET_PRIVATE_KEY=0x...   # Private key de la EOA (64 hex chars, con o sin 0x)
CLOB_API_KEY=uuid              # API key L2 del CLOB
CLOB_API_SECRET=...            # API secret L2
CLOB_API_PASSPHRASE=...        # API passphrase L2

# PostgreSQL (opcional)
DATABASE_URL=postgres://user:pass@host:port/db

# Relayer (opcional, para gasless)
RELAYER_API_KEY=...
RELAYER_API_KEY_ADDRESS=0x...
```

---

## 9. Lecciones Aprendidas

1. **CLOB V2 requiere pUSD**: USDC.e ya no es el colateral. Hay que wrappear vía CollateralOnramp.

2. **SignatureType::Proxy + API keys ≠ UI**: La UI usa Magic auth que sí vincula el proxy. Las L2 API keys no reconocen el proxy como funder. Usar `SignatureType::Eoa` con pUSD en la EOA.

3. **SwapRouter02 revierte**: Uniswap V3 Router02 (`0x68b3...`) revierte en Polygon para USDC. Usar Router (`0xE592...`).

4. **USDC nativo ≠ USDC.e**: CollateralOnramp solo acepta USDC.e. Si tienes USDC nativo (Circle), hay que swappear primero.

5. **El SDK deriva mal el proxy**: `derive_proxy_wallet(0xc91..., POLYGON)` devuelve `0xc38e...` pero el proxy real es `0x059...`. No confiar en el auto-derive.

6. **Las API keys son idempotentes**: `create_or_derive_api_key` devuelve siempre la misma key. Para regenerar, usar `delete_api_key()` + `create_api_key()`.

7. **allowances en CLOB**: Las allowances que ves en `balance_allowance` son las del CLOB (no on-chain). Deben estar en MAX_U256 para poder tradear. El approve on-chain (vía alloy) configura las allowances del CLOB.

---

## 10. Versiones

| Componente | Versión |
|---|---|
| Rust SDK | `polymarket_client_sdk_v2 = "0.6.0-canary.1"` |
| Alloy | `1.8` |
| Tokio | `1` |
| Rust MSRV | `1.91` |

---

*Documento generado el 6 de Mayo de 2026. Última verificación: balance CLOB $7.96 confirmado.*
