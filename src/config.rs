use {serde::Deserialize, std::str::FromStr};

#[derive(Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct Configuration {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    pub database_url: String,
    pub keypair_seed: String,
    #[serde(default = "default_is_test", skip)]
    /// This is an internal flag to disable logging, cannot be defined by user
    pub is_test: bool,

    // TELEMETRY
    pub otel_exporter_otlp_endpoint: Option<String>,
    pub telemetry_prometheus_port: Option<u16>,
}

impl Configuration {
    pub fn new() -> crate::Result<Configuration> {
        let config = envy::from_env::<Configuration>()?;
        Ok(config)
    }

    pub fn log_level(&self) -> tracing::Level {
        tracing::Level::from_str(self.log_level.as_str()).expect("Invalid log level")
    }
}

fn default_port() -> u16 {
    3000
}

fn default_log_level() -> String {
    "DEBUG".to_string()
}

fn default_is_test() -> bool {
    false
}
