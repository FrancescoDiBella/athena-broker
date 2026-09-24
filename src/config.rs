use anyhow::{anyhow, ensure, Context};
use athena_storage::{maintenance::RetentionConfig, DatabaseConfig};
use athena_subscription::EngineConfig;
use serde::Deserialize;
use std::{net::IpAddr, path::Path, str::FromStr};

#[derive(Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct BrokerConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub subscriptions: EngineConfig,
    pub limits: athena_api::state::ApiLimits,
    pub jsonld: JsonLdConfig,
    pub retention: RetentionConfig,
    pub security: SecurityConfig,
}
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub host: IpAddr,
    pub port: u16,
    pub log_level: String,
    pub shutdown_grace_sec: u64,
}
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: IpAddr::from([0, 0, 0, 0]),
            port: 8080,
            log_level: "info".into(),
            shutdown_grace_sec: 30,
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JsonLdConfig {
    pub context_cache_size: usize,
    pub max_concurrency: usize,
}
impl Default for JsonLdConfig {
    fn default() -> Self {
        Self {
            context_cache_size: 128,
            max_concurrency: 16,
        }
    }
}
#[derive(Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SecurityConfig {
    pub allow_internal_endpoints: bool,
}

impl BrokerConfig {
    pub fn load(path: Option<&Path>) -> anyhow::Result<Self> {
        let source = path
            .map(std::fs::read_to_string)
            .transpose()
            .context("Cannot read configuration file")?;
        Self::parse(source.as_deref(), |key| std::env::var(key).ok())
    }
    fn parse(source: Option<&str>, env: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let mut config: Self = match source {
            // TOML errors can include credentials from source lines. Never echo their contents.
            Some(source) => toml::from_str(source).map_err(|e: toml::de::Error| {
                let line = e
                    .span()
                    .map(|s| {
                        source[..s.start.min(source.len())]
                            .bytes()
                            .filter(|b| *b == b'\n')
                            .count()
                            + 1
                    })
                    .unwrap_or(1);
                anyhow!("Invalid TOML configuration near line {line}; check field names and types")
            })?,
            None => Self::default(),
        };
        fn override_with<T: FromStr>(
            target: &mut T,
            env: &impl Fn(&str) -> Option<String>,
            key: &str,
        ) -> anyhow::Result<()> {
            if let Some(value) = env(key) {
                *target = value
                    .parse()
                    .map_err(|_| anyhow!("Invalid value for {key}"))?;
            }
            Ok(())
        }
        macro_rules! set {
            ($field:expr,$name:literal) => {
                override_with(&mut $field, &env, $name)?
            };
        }
        set!(config.server.port, "PORT");
        set!(config.server.host, "HOST");
        set!(config.server.log_level, "RUST_LOG");
        set!(config.server.shutdown_grace_sec, "SHUTDOWN_GRACE_SECONDS");
        set!(config.database.url, "DATABASE_URL");
        set!(config.database.max_connections, "DB_MAX_CONNECTIONS");
        set!(config.database.min_connections, "DB_MIN_CONNECTIONS");
        set!(
            config.database.connect_timeout_sec,
            "DB_CONNECT_TIMEOUT_SECONDS"
        );
        set!(
            config.database.statement_timeout_ms,
            "DB_STATEMENT_TIMEOUT_MS"
        );
        set!(config.database.lock_timeout_ms, "DB_LOCK_TIMEOUT_MS");
        set!(config.subscriptions.worker_threads, "NOTIFICATION_WORKERS");
        set!(
            config.subscriptions.request_timeout_sec,
            "NOTIFICATION_TIMEOUT_SECONDS"
        );
        set!(
            config.subscriptions.lease_duration_sec,
            "NOTIFICATION_LEASE_SECONDS"
        );
        set!(
            config.subscriptions.max_attempts,
            "NOTIFICATION_MAX_ATTEMPTS"
        );
        set!(config.limits.max_in_flight_writes, "MAX_IN_FLIGHT_WRITES");
        set!(config.limits.max_pending_events, "MAX_PENDING_EVENTS");
        if let Some(value) = env("ALLOW_INTERNAL_ENDPOINTS") {
            config.security.allow_internal_endpoints = match value.as_str() {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => return Err(anyhow!("Invalid value for ALLOW_INTERNAL_ENDPOINTS")),
            };
        }
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        let db = &self.database;
        db.validate_url().map_err(anyhow::Error::msg)?;
        ensure!(self.server.port > 0, "server.port must be > 0");
        ensure!(
            (1..=300).contains(&self.server.shutdown_grace_sec),
            "server.shutdown_grace_sec must be 1..300"
        );
        ensure!(
            tracing_subscriber::EnvFilter::try_new(&self.server.log_level).is_ok(),
            "Invalid logging filter"
        );
        ensure!(
            (db.url.starts_with("postgres://") || db.url.starts_with("postgresql://"))
                && !db.url.contains(['\n', '\r']),
            "database.url must be a PostgreSQL URL"
        );
        ensure!(
            (3..=1000).contains(&db.max_connections) && db.min_connections <= db.max_connections,
            "database pool requires 3..1000 connections and min <= max"
        );
        ensure!(
            (1..=300).contains(&db.connect_timeout_sec)
                && (1..=86400).contains(&db.idle_timeout_sec),
            "Invalid database connection/idle timeout"
        );
        ensure!(
            (1..=300000).contains(&db.statement_timeout_ms)
                && (1..=db.statement_timeout_ms).contains(&db.lock_timeout_ms),
            "Database timeouts require 0 < lock <= statement <= 300000 ms"
        );
        self.subscriptions.validate().map_err(anyhow::Error::msg)?;
        if self.subscriptions.mqtt_ca_file.is_some() {
            athena_subscription::mqtt::MqttClient::from_ca_file(
                self.subscriptions.mqtt_ca_file.as_deref(),
            )
            .map_err(anyhow::Error::msg)?;
        }
        ensure!(self.subscriptions.lease_duration_sec * 1000 > self.subscriptions.request_timeout_sec * 1000 + 2 * (db.statement_timeout_ms + db.connect_timeout_sec * 1000), "Notification lease must cover HTTP timeout and two database operations (acquisition + statement timeout)");
        self.limits.validate().map_err(anyhow::Error::msg)?;
        ensure!(
            (1..=1024).contains(&self.jsonld.context_cache_size)
                && (1..=256).contains(&self.jsonld.max_concurrency),
            "Invalid JSON-LD cache size or concurrency"
        );
        self.retention.validate().map_err(anyhow::Error::msg)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_example_and_environment_precedence() {
        // The shipped example must stay executable, not aspirational configuration.
        BrokerConfig::parse(Some(include_str!("../config/default.toml")), |_| None).unwrap();
        let c = BrokerConfig::parse(Some("[server]\nport=9090"), |k| {
            (k == "PORT").then(|| "9091".into())
        })
        .unwrap();
        assert_eq!(c.server.port, 9091);
        assert_eq!(c.database.max_connections, 50);
    }
    #[test]
    fn rejects_silent_fallbacks_unknown_fields_and_unsafe_cross_settings() {
        for source in [
            "[server]\nprort=9090",
            "[database]\nmin_connections=51",
            "[subscriptions]\nlease_duration_sec=11",
            "[limits]\nmax_in_flight_writes=0",
        ] {
            assert!(
                BrokerConfig::parse(Some(source), |_| None).is_err(),
                "{source}"
            );
        }
        assert!(BrokerConfig::parse(None, |k| (k == "PORT").then(|| "not-a-port".into())).is_err());
        let err = BrokerConfig::parse(
            Some("[database]\nmax_connections='secret-password'"),
            |_| None,
        )
        .err()
        .unwrap()
        .to_string();
        assert!(!err.contains("secret-password"));
    }
}
