# Sistema de Licencias

Cada módulo Premium requiere una **license key** para funcionar. El sistema usa tokens firmados con HMAC-SHA256.

---

## Cómo funciona

1. **El vendedor** genera un token con `license-gen`
2. **El cliente** lo agrega a su `.env` como `ZZIGNAL_LICENSE_KEY`
3. **El backend** valida el token al iniciar cada módulo
4. Si el token es inválido o expiró, el módulo no se inicia

---

## Generar Licencias

```bash
# Licencia perpetua para Collector
cargo run --bin license-gen -- --module collector --customer acme-corp

# Licencia por 365 días para todos los módulos
cargo run --bin license-gen -- --module all --customer trader123 --days 365

# Licencia por 30 días para Patterns
cargo run --bin license-gen -- --module patterns --customer algofund --days 30
```

**Salida del generador:**

```
╔══════════════════════════════════════════════════════╗
║         ZZIGNAL — License Key Generator             ║
╠══════════════════════════════════════════════════════╣
║  Módulo:   collector                                ║
║  Cliente:  acme-corp                                ║
║  Emitido:  2026-04-25 14:30 UTC                     ║
║  Expira:   2027-04-25                                ║
║  Firma:    8f3a2b1c9d4e... ║
╠══════════════════════════════════════════════════════╣
║  LICENSE KEY:                                       ║
║  eyJtb2R1bGUiOiJjb2xsZWN0b3IiLCJjdXN0b21lci...     ║
╚══════════════════════════════════════════════════════╝

El cliente debe agregar esto en su .env:
  ZZIGNAL_LICENSE_KEY=eyJtb2R1bGUiOiJjb2xsZWN0b3Ii...
```

---

## Configuración del Cliente

El cliente agrega la license key a su `.env`:

```env
ZZIGNAL_LICENSE_KEY=eyJtb2R1bGUiOiJjb2xsZWN0b3IiLCJjdXN0b21lciI6ImFjbWUtY29ycCIsImlzc3VlZF9hdCI6MTcx...
```

Luego compila con la feature flag correspondiente:

```bash
cargo build --release --features premium-collector
```

---

## Validación

Al iniciar, cada módulo premium llama a `license::check_and_log()`:

```rust
// En collector/scheduler.rs
if !license::check_and_log(
    PremiumModule::Collector,
    std::env::var("ZZIGNAL_LICENSE_KEY").ok().as_deref()
).await {
    return; // módulo no se inicia
}
```

El token se decodifica y se verifica:
1. Firma HMAC-SHA256 válida (el token no fue manipulado)
2. No expiró (si tiene fecha de expiración)

---

## Seguridad

- Los tokens usan HMAC-SHA256 con una clave secreta (`SECRET_KEY`)
- La clave secreta se cambia en producción (actualmente es un placeholder en `license-gen.rs`)
- En producción: la validación puede hacerse contra un servidor de licencias externo
- Cada token está ligado a un cliente (`customer`)

---

## Personalización para Producción

### Cambiar la clave secreta

En `backend_rust/src/bin/license-gen.rs`:
```rust
const SECRET_KEY: &[u8] = b"tu-clave-secreta-de-64-bytes-o-mas";
```

En `backend_rust/src/modules/premium/license.rs`, usar la misma clave para validación.

### Validación contra servidor externo

Modificar `license.rs::validate_license()` para llamar a una API externa:
```rust
pub async fn validate_license(module: PremiumModule, license_key: &str) -> LicenseStatus {
    let client = reqwest::Client::new();
    let resp = client.post("https://licenses.example.com/validate")
        .json(&json!({"module": module.id(), "key": license_key}))
        .send()
        .await;
    // ...
}
```

---

## Archivos del Sistema de Licencias

```
backend_rust/src/
├── bin/license-gen.rs                    ← CLI generador de tokens
└── modules/premium/
    └── license.rs                        ← Validación de tokens en runtime
```
