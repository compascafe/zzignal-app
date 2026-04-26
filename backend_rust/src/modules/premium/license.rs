//! Sistema de licencias por módulo.
//!
//! Cada módulo premium valida su licencia al iniciar.
//! La licencia es un hash del module_id + una clave secreta que solo el
//! vendedor (tú) puede generar. En producción, la validación se hace contra
//! un servidor de licencias. En desarrollo, se mockea.

/// Identificador único de cada módulo premium
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PremiumModule {
    Collector,
    Patterns,
    Executor,
}

impl PremiumModule {
    pub fn id(&self) -> &'static str {
        match self {
            PremiumModule::Collector => "zzignal-collector-v1",
            PremiumModule::Patterns  => "zzignal-patterns-v1",
            PremiumModule::Executor  => "zzignal-executor-v1",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            PremiumModule::Collector => "Multi-Timeframe Collector",
            PremiumModule::Patterns  => "Pattern Detector",
            PremiumModule::Executor  => "Auto-Execution Engine",
        }
    }
}

/// Estado de licencia para un módulo
#[derive(Debug, Clone)]
pub struct LicenseStatus {
    pub module:      PremiumModule,
    pub licensed:    bool,
    pub expires_at:  Option<chrono::DateTime<chrono::Utc>>,
    pub customer_id: Option<String>,
}

/// Valida la licencia de un módulo.
///
/// En producción: llama a un endpoint de licencias con el `license_key` + `machine_id`.
/// En desarrollo: devuelve true siempre (mock).
pub async fn validate_license(_module: PremiumModule, _license_key: &str) -> LicenseStatus {
    // TODO: En producción, validar contra servidor de licencias.
    // Por ahora, en desarrollo, todos los módulos están licenciados.
    LicenseStatus {
        module:      _module,
        licensed:    true,
        expires_at:  None,
        customer_id: Some("dev".into()),
    }
}

/// Verifica si un módulo puede iniciar.
/// Si no está licenciado, loguea un warning y retorna false.
pub async fn check_and_log(module: PremiumModule, license_key: Option<&str>) -> bool {
    let key = match license_key {
        Some(k) => k,
        None => {
            tracing::warn!(
                "Módulo '{}' — sin license key. El módulo NO se iniciará.",
                module.name()
            );
            return false;
        }
    };

    let status = validate_license(module, key).await;

    if status.licensed {
        tracing::info!(
            "Módulo '{}' — licencia válida (customer: {:?})",
            module.name(),
            status.customer_id
        );
        true
    } else {
        tracing::warn!(
            "Módulo '{}' — licencia INVÁLIDA. El módulo NO se iniciará.",
            module.name()
        );
        false
    }
}
