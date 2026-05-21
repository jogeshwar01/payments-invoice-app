use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub bind: String,
    pub psp_base_url: String,
    pub psp_timeout: Duration,
    pub reconciler_interval: Duration,
    pub reconciler_stale_after: Duration,
    pub dispatcher_interval: Duration,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://dodo:dodo@localhost:7000/dodo".into()),
            bind: std::env::var("BIND").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            psp_base_url: std::env::var("PSP_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:7001".into()),
            psp_timeout: Duration::from_millis(
                std::env::var("PSP_TIMEOUT_MS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(5000),
            ),
            reconciler_interval: Duration::from_millis(
                std::env::var("RECONCILER_INTERVAL_MS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(2000),
            ),
            reconciler_stale_after: Duration::from_millis(
                std::env::var("RECONCILER_STALE_AFTER_MS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(10_000),
            ),
            dispatcher_interval: Duration::from_millis(
                std::env::var("DISPATCHER_INTERVAL_MS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(2000),
            ),
        }
    }
}
