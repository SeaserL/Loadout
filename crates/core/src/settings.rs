// SPDX-License-Identifier: Apache-2.0

//! Registry of user-configurable settings. Every setting has a default, a validator and a
//! description; only registered keys can be set.

use loadout_ipc::api::Setting;
use loadout_storage::{Database, StorageError};

pub struct SettingDef {
    pub key: &'static str,
    pub default: &'static str,
    pub description: &'static str,
    pub restart_required: bool,
    validate: fn(&str) -> Result<(), String>,
}

impl std::fmt::Debug for SettingDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingDef")
            .field("key", &self.key)
            .finish()
    }
}

impl SettingDef {
    pub fn validate(&self, value: &str) -> Result<(), String> {
        (self.validate)(value)
    }
}

pub const GATEWAY_PORT: &str = "gateway.port";
pub const GATEWAY_ALLOWED_ORIGINS: &str = "gateway.allowed_origins";
pub const FIRST_TOKEN_TIMEOUT: &str = "upstream.first_token_timeout_secs";
pub const MAX_ATTEMPTS: &str = "routing.max_attempts";
pub const QUOTA_RESERVE: &str = "routing.quota_reserve_percent";
pub const ULTRA_FAST_MS: &str = "latency.ultra_fast_ms";
pub const FAST_MS: &str = "latency.fast_ms";
pub const AUTO_PROBE: &str = "benchmark.auto_probe";
pub const PROBE_INTERVAL: &str = "benchmark.probe_interval_hours";
pub const PROBE_MAX_PRICE: &str = "benchmark.max_probe_output_price";

pub const SETTINGS: &[SettingDef] = &[
    SettingDef {
        key: GATEWAY_PORT,
        default: "8317",
        description: "TCP port of the local gateway on 127.0.0.1",
        restart_required: true,
        validate: validate_port,
    },
    SettingDef {
        key: GATEWAY_ALLOWED_ORIGINS,
        default: "",
        description: "Comma-separated browser origins allowed to call the gateway (e.g. \
                      http://localhost:3000); empty blocks all browser calls",
        restart_required: true,
        validate: validate_origins,
    },
    SettingDef {
        key: FIRST_TOKEN_TIMEOUT,
        default: "120",
        description: "Seconds to wait for an upstream's first output token before failing over",
        restart_required: true,
        validate: validate_timeout,
    },
    SettingDef {
        key: MAX_ATTEMPTS,
        default: "3",
        description: "Routes tried per request before giving up (ADR-003)",
        restart_required: true,
        validate: validate_attempts,
    },
    SettingDef {
        key: QUOTA_RESERVE,
        default: "0",
        description: "Stop routing to an account when any quota window has less than this \
                      percent left (0 disables; ADR-003)",
        restart_required: false,
        validate: validate_reserve,
    },
    SettingDef {
        key: ULTRA_FAST_MS,
        default: "800",
        description: "P90 time to first token (ms) below which a route is ULTRA_FAST (ADR-003)",
        restart_required: false,
        validate: validate_ttft_ms,
    },
    SettingDef {
        key: FAST_MS,
        default: "2000",
        description: "P90 time to first token (ms) below which a route is FAST",
        restart_required: false,
        validate: validate_ttft_ms,
    },
    SettingDef {
        key: AUTO_PROBE,
        default: "true",
        description: "Send tiny background probes to measure routes without recent traffic",
        restart_required: false,
        validate: validate_bool,
    },
    SettingDef {
        key: PROBE_INTERVAL,
        default: "6",
        description: "Hours between automatic probes of the same route",
        restart_required: false,
        validate: validate_probe_interval,
    },
    SettingDef {
        key: PROBE_MAX_PRICE,
        default: "30",
        description: "Skip automatic probes of models whose output price exceeds this (USD per                       million tokens; unknown prices are probed)",
        restart_required: false,
        validate: validate_price,
    },
];

pub fn find(key: &str) -> Option<&'static SettingDef> {
    SETTINGS.iter().find(|d| d.key == key)
}

fn validate_port(value: &str) -> Result<(), String> {
    match value.parse::<u16>() {
        Ok(0) | Err(_) => Err("expected a port number between 1 and 65535".into()),
        Ok(_) => Ok(()),
    }
}

fn validate_timeout(value: &str) -> Result<(), String> {
    match value.parse::<u64>() {
        Ok(5..=1800) => Ok(()),
        _ => Err("expected 5-1800 seconds".into()),
    }
}

fn validate_attempts(value: &str) -> Result<(), String> {
    match value.parse::<usize>() {
        Ok(1..=10) => Ok(()),
        _ => Err("expected 1-10".into()),
    }
}

fn validate_reserve(value: &str) -> Result<(), String> {
    match value.parse::<f64>() {
        Ok(v) if (0.0..=50.0).contains(&v) => Ok(()),
        _ => Err("expected a percentage between 0 and 50".into()),
    }
}

fn validate_ttft_ms(value: &str) -> Result<(), String> {
    match value.parse::<u32>() {
        Ok(50..=60_000) => Ok(()),
        _ => Err("expected 50-60000 milliseconds".into()),
    }
}

fn validate_bool(value: &str) -> Result<(), String> {
    parse_bool(value)
        .map(|_| ())
        .ok_or_else(|| "expected true or false".into())
}

pub fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn validate_probe_interval(value: &str) -> Result<(), String> {
    match value.parse::<u32>() {
        Ok(1..=168) => Ok(()),
        _ => Err("expected 1-168 hours".into()),
    }
}

fn validate_price(value: &str) -> Result<(), String> {
    match value.parse::<f64>() {
        Ok(v) if (0.0..=100_000.0).contains(&v) => Ok(()),
        _ => Err("expected a non-negative price".into()),
    }
}

fn validate_origins(value: &str) -> Result<(), String> {
    for origin in split_list(value) {
        let rest = origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
            .ok_or_else(|| format!("origin {origin:?} must start with http:// or https://"))?;
        if rest.is_empty() || rest.contains('/') && !rest.ends_with('/') {
            return Err(format!("origin {origin:?} must be scheme://host[:port]"));
        }
    }
    Ok(())
}

fn split_list(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').map(str::trim).filter(|s| !s.is_empty())
}

/// The effective value of a registered setting: the stored value when it validates,
/// otherwise the default.
pub fn value(db: &Database, key: &str) -> Result<String, StorageError> {
    let Some(def) = find(key) else {
        return Err(StorageError::NotFound(format!("setting {key}")));
    };
    Ok(match db.settings().get(key)? {
        Some(v) if def.validate(&v).is_ok() => v,
        Some(v) => {
            tracing::warn!(key, value = %v, "ignoring invalid stored setting");
            def.default.to_owned()
        }
        None => def.default.to_owned(),
    })
}

/// Latency and probe settings, read by the control plane each cycle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LatencySettings {
    pub thresholds: loadout_router::TierThresholds,
    pub auto_probe: bool,
    pub probe_interval_ms: i64,
    pub max_probe_output_price: f64,
}

impl LatencySettings {
    pub fn load(db: &Database) -> Result<Self, StorageError> {
        let ultra: u32 = value(db, ULTRA_FAST_MS)?.parse().unwrap_or(800);
        let fast: u32 = value(db, FAST_MS)?.parse().unwrap_or(2000);
        Ok(Self {
            thresholds: loadout_router::TierThresholds {
                ultra_fast_ms: ultra,
                fast_ms: fast.max(ultra + 1),
            },
            auto_probe: parse_bool(&value(db, AUTO_PROBE)?).unwrap_or(true),
            probe_interval_ms: i64::from(value(db, PROBE_INTERVAL)?.parse::<u32>().unwrap_or(6))
                * 3_600_000,
            max_probe_output_price: value(db, PROBE_MAX_PRICE)?.parse().unwrap_or(30.0),
        })
    }
}

/// Effective values with defaults applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub gateway_port: u16,
    pub allowed_origins: Vec<String>,
    pub first_token_timeout_secs: u64,
    pub max_attempts: usize,
}

impl Resolved {
    pub fn load(db: &Database) -> Result<Self, StorageError> {
        let get = |key: &str| -> Result<String, StorageError> {
            let def = find(key).expect("registered setting");
            Ok(match db.settings().get(key)? {
                // A value that no longer validates (e.g. edited by hand) falls back to default.
                Some(v) if def.validate(&v).is_ok() => v,
                Some(v) => {
                    tracing::warn!(key, value = %v, "ignoring invalid stored setting");
                    def.default.to_owned()
                }
                None => def.default.to_owned(),
            })
        };
        Ok(Self {
            gateway_port: get(GATEWAY_PORT)?.parse().unwrap_or(8317),
            allowed_origins: split_list(&get(GATEWAY_ALLOWED_ORIGINS)?)
                .map(str::to_owned)
                .collect(),
            first_token_timeout_secs: get(FIRST_TOKEN_TIMEOUT)?.parse().unwrap_or(120),
            max_attempts: get(MAX_ATTEMPTS)?.parse().unwrap_or(3),
        })
    }
}

/// All registered settings with their stored values.
pub fn list(db: &Database) -> Result<Vec<Setting>, StorageError> {
    SETTINGS.iter().map(|def| view(db, def)).collect()
}

pub fn view(db: &Database, def: &SettingDef) -> Result<Setting, StorageError> {
    let stored = db.settings().get(def.key)?;
    Ok(Setting {
        key: def.key.to_owned(),
        value: stored.clone().unwrap_or_else(|| def.default.to_owned()),
        default: def.default.to_owned(),
        is_default: stored.is_none(),
        description: def.description.to_owned(),
        restart_required: def.restart_required,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_validation() {
        let def = find(GATEWAY_PORT).expect("def");
        assert!(def.validate("8317").is_ok());
        assert!(def.validate("65535").is_ok());
        for bad in ["0", "65536", "-1", "abc", ""] {
            assert!(def.validate(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn origin_validation() {
        let def = find(GATEWAY_ALLOWED_ORIGINS).expect("def");
        assert!(def.validate("").is_ok());
        assert!(
            def.validate("http://localhost:3000, https://app.test")
                .is_ok()
        );
        assert!(def.validate("localhost:3000").is_err());
        assert!(def.validate("http://a.test/path").is_err());
    }

    #[test]
    fn resolve_defaults_and_overrides() {
        let db = Database::open_in_memory().expect("db");
        assert_eq!(
            Resolved::load(&db).expect("load"),
            Resolved {
                gateway_port: 8317,
                allowed_origins: vec![],
                first_token_timeout_secs: 120,
                max_attempts: 3,
            }
        );

        db.settings().set(GATEWAY_PORT, "9001").expect("set");
        db.settings()
            .set(GATEWAY_ALLOWED_ORIGINS, " http://a.test ,http://b.test")
            .expect("set");
        let r = Resolved::load(&db).expect("load");
        assert_eq!(r.gateway_port, 9001);
        assert_eq!(r.allowed_origins, vec!["http://a.test", "http://b.test"]);

        db.settings().set(GATEWAY_PORT, "not-a-port").expect("set");
        assert_eq!(Resolved::load(&db).expect("load").gateway_port, 8317);
    }

    #[test]
    fn latency_settings_keep_thresholds_ordered() {
        let db = Database::open_in_memory().expect("db");
        let s = LatencySettings::load(&db).expect("load");
        assert_eq!(s.thresholds, loadout_router::TierThresholds::default());
        assert!(s.auto_probe);
        assert_eq!(s.probe_interval_ms, 6 * 3_600_000);
        db.settings().set(ULTRA_FAST_MS, "3000").expect("set");
        db.settings().set(AUTO_PROBE, "off").expect("set");
        let s = LatencySettings::load(&db).expect("load");
        assert_eq!(s.thresholds.fast_ms, 3001);
        assert!(!s.auto_probe);
    }

    #[test]
    fn list_marks_defaults() {
        let db = Database::open_in_memory().expect("db");
        db.settings().set(GATEWAY_PORT, "9001").expect("set");
        let all = list(&db).expect("list");
        let port = all.iter().find(|s| s.key == GATEWAY_PORT).expect("port");
        assert_eq!(port.value, "9001");
        assert!(!port.is_default);
        let origins = all
            .iter()
            .find(|s| s.key == GATEWAY_ALLOWED_ORIGINS)
            .expect("origins");
        assert!(origins.is_default);
    }
}
