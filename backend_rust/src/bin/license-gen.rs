//! ZZignal License Key Generator
//!
//! Genera tokens de activación para módulos premium firmados con HMAC-SHA256.
//! Uso:
//!   cargo run --bin license-gen -- --module collector --customer acme-corp
//!   cargo run --bin license-gen -- --module all --customer trader123 --days 365
//!
//! El token se imprime en consola. El cliente lo pega en el .env como:
//!   ZZIGNAL_LICENSE_KEY=eyJ...

use std::env;
use chrono::{Utc, Duration};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

type HmacSha256 = Hmac<Sha256>;

/// Secret key hardcodeada para firmar tokens.
/// En producción, esto viene de una variable de entorno o archivo seguro.
const SECRET_KEY: &[u8] = b"zzignal-premium-secret-v1-change-in-production";

#[derive(Debug, Serialize, Deserialize)]
struct LicenseToken {
    module:    String,           // "collector", "patterns", "executor", "all"
    customer:  String,
    issued_at: i64,              // Unix timestamp
    expires_at: i64,             // Unix timestamp, 0 = never
    signature: String,           // HMAC-SHA256 signature of the payload
}

impl LicenseToken {
    fn payload_to_sign(&self) -> String {
        format!("{}|{}|{}|{}", self.module, self.customer, self.issued_at, self.expires_at)
    }

    fn sign(&mut self) {
        let payload = self.payload_to_sign();
        let mut mac = HmacSha256::new_from_slice(SECRET_KEY).expect("HMAC key");
        mac.update(payload.as_bytes());
        let result = mac.finalize();
        self.signature = URL_SAFE_NO_PAD.encode(result.into_bytes());
    }

    fn verify(&self) -> bool {
        let payload = self.payload_to_sign();
        let mut mac = HmacSha256::new_from_slice(SECRET_KEY).expect("HMAC key");
        mac.update(payload.as_bytes());
        let expected = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        expected == self.signature
    }

    fn is_expired(&self) -> bool {
        if self.expires_at == 0 {
            return false; // never expires
        }
        Utc::now().timestamp() > self.expires_at
    }

    fn encode(&self) -> String {
        let json = serde_json::to_string(self).unwrap();
        URL_SAFE_NO_PAD.encode(json.as_bytes())
    }

    fn decode(encoded: &str) -> Option<Self> {
        let bytes = URL_SAFE_NO_PAD.decode(encoded).ok()?;
        let json = String::from_utf8(bytes).ok()?;
        serde_json::from_str(&json).ok()
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 || args.contains(&"--help".into()) {
        eprintln!("ZZignal License Key Generator v1.0");
        eprintln!();
        eprintln!("Uso:");
        eprintln!("  license-gen --module <MODULE> --customer <NAME> [--days <N>]");
        eprintln!();
        eprintln!("Módulos: collector | patterns | executor | all");
        eprintln!("Opciones:");
        eprintln!("  --days N    Duración en días (default: sin expiración)");
        eprintln!();
        eprintln!("Ejemplo:");
        eprintln!("  cargo run --bin license-gen -- --module collector --customer acme-corp --days 365");
        return;
    }

    let mut module = String::new();
    let mut customer = String::new();
    let mut days: Option<i64> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--module" => { i += 1; if i < args.len() { module = args[i].clone(); } }
            "--customer" => { i += 1; if i < args.len() { customer = args[i].clone(); } }
            "--days" => { i += 1; if i < args.len() { days = args[i].parse().ok(); } }
            _ => {}
        }
        i += 1;
    }

    if module.is_empty() || customer.is_empty() {
        eprintln!("Error: --module y --customer son obligatorios");
        return;
    }

    let valid_modules = ["collector", "patterns", "executor", "all"];
    if !valid_modules.contains(&module.as_str()) {
        eprintln!("Error: módulo inválido '{}'. Válidos: {:?}", module, valid_modules);
        return;
    }

    let now = Utc::now();
    let expires_at = match days {
        Some(d) if d > 0 => (now + Duration::days(d)).timestamp(),
        _ => 0, // never expires
    };

    let mut token = LicenseToken {
        module,
        customer,
        issued_at: now.timestamp(),
        expires_at,
        signature: String::new(),
    };
    token.sign();

    let encoded = token.encode();

    println!("╔══════════════════════════════════════════════════════╗");
    println!("║         ZZIGNAL — License Key Generator             ║");
    println!("╠══════════════════════════════════════════════════════╣");
    println!("║  Módulo:   {:<43}║", token.module);
    println!("║  Cliente:  {:<43}║", token.customer);
    println!("║  Emitido:  {:<43}║", Utc::now().format("%Y-%m-%d %H:%M UTC"));
    if expires_at > 0 {
        println!("║  Expira:   {:<43}║",
            chrono::DateTime::from_timestamp(expires_at, 0)
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default());
    } else {
        println!("║  Expira:   NUNCA                                     ║");
    }
    println!("║  Firma:    {}... ║", &token.signature[..token.signature.len().min(32)]);
    println!("╠══════════════════════════════════════════════════════╣");
    println!("║  LICENSE KEY:                                       ║");
    println!("║  {} ║", encoded);
    println!("╚══════════════════════════════════════════════════════╝");
    println!();
    println!("El cliente debe agregar esto en su .env:");
    println!("  ZZIGNAL_LICENSE_KEY={}", encoded);

    // Verify it works
    match LicenseToken::decode(&encoded) {
        Some(decoded) if decoded.verify() && !decoded.is_expired() => {
            println!("\n✅ Token verificado — válido.");
        }
        _other => {
            eprintln!("\n❌ Error: el token generado no pasa la verificación. Algo salió mal.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_and_verify() {
        let mut token = LicenseToken {
            module: "collector".into(),
            customer: "test".into(),
            issued_at: Utc::now().timestamp(),
            expires_at: 0,
            signature: String::new(),
        };
        token.sign();
        assert!(token.verify());
    }

    #[test]
    fn test_encode_decode() {
        let mut token = LicenseToken {
            module: "collector".into(),
            customer: "test".into(),
            issued_at: Utc::now().timestamp(),
            expires_at: 0,
            signature: String::new(),
        };
        token.sign();
        let encoded = token.encode();
        let decoded = LicenseToken::decode(&encoded).unwrap();
        assert!(decoded.verify());
        assert_eq!(decoded.customer, "test");
    }
}
