//! Sistema de actualización modular para módulos premium.
//!
//! El Core (open source) se actualiza vía git pull + cargo build --release.
//! Los módulos premium se actualizan vía este sistema:
//!   - Si se compila con --features premium-xxx, el módulo se incluye en el binario
//!   - El sistema revisa si hay una nueva versión del módulo disponible
//!   - Los módulos están en `modules/premium/` y se actualizan independientemente
//!
//! Flujo para el desarrollador que vende módulos:
//!   1. Desarrolla el módulo en `modules/premium/collector/`
//!   2. Sube los cambios a un repo privado
//!   3. El cliente recibe los archivos actualizados del módulo
//!   4. Recompila con --features premium-collector
//!
//! Para automatizar, el módulo expone un endpoint `/api/premium/version`
//! que reporta la versión actual de cada módulo instalado.

use serde::Serialize;

/// Versión semántica de un módulo
#[derive(Debug, Clone, Serialize)]
pub struct ModuleVersion {
    pub name:    String,
    pub version: &'static str,
    pub build:   &'static str,
}

/// Versiones hardcodeadas de cada módulo.
/// Se actualizan manualmente en cada release.
pub fn collector_version() -> ModuleVersion {
    ModuleVersion {
        name:    "premium-collector".into(),
        version: "1.0.0",
        build:   env!("CARGO_PKG_VERSION"),
    }
}

pub fn patterns_version() -> ModuleVersion {
    ModuleVersion {
        name:    "premium-patterns".into(),
        version: "1.0.0",
        build:   env!("CARGO_PKG_VERSION"),
    }
}

pub fn executor_version() -> ModuleVersion {
    ModuleVersion {
        name:    "premium-executor".into(),
        version: "1.0.0",
        build:   env!("CARGO_PKG_VERSION"),
    }
}

/// Devuelve las versiones de todos los módulos premium compilados
#[allow(unused)]
pub fn all_versions() -> Vec<ModuleVersion> {
    let mut versions = vec![];

    #[cfg(feature = "premium-collector")]
    versions.push(collector_version());

    #[cfg(feature = "premium-patterns")]
    versions.push(patterns_version());

    #[cfg(feature = "premium-executor")]
    versions.push(executor_version());

    versions
}

/// API endpoint handler — GET /api/premium/version
#[allow(unused)]
pub async fn version_handler() -> axum::Json<serde_json::Value> {
    let versions: Vec<ModuleVersion> = all_versions();
    axum::Json(serde_json::json!({
        "core_version": env!("CARGO_PKG_VERSION"),
        "modules": versions,
        "compiled_features": {
            "collector": cfg!(feature = "premium-collector"),
            "patterns":  cfg!(feature = "premium-patterns"),
            "executor":  cfg!(feature = "premium-executor"),
        }
    }))
}
