use std::{env, net::SocketAddr, time::Duration};

const DEFAULT_BIND: &str = "0.0.0.0:7070";
const DEFAULT_REQUEST_TIMEOUT_SECONDS: u64 = 60 * 60;
const DEFAULT_MAX_BODY_BYTES: usize = 5 * 1024 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub shared_token: String,
    pub request_timeout: Duration,
    pub max_body_bytes: usize,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let _ = dotenvy::dotenv();

        let bind = env::var("PCLOUD_RELAY_BIND")
            .unwrap_or_else(|_| DEFAULT_BIND.to_owned())
            .parse::<SocketAddr>()?;

        let shared_token = env::var("PCLOUD_RELAY_TOKEN")
            .map_err(|_| anyhow::anyhow!("PCLOUD_RELAY_TOKEN must be set"))?;

        let request_timeout_seconds = env::var("PCLOUD_RELAY_REQUEST_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_REQUEST_TIMEOUT_SECONDS);

        let max_body_bytes = env::var("PCLOUD_RELAY_MAX_BODY_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(DEFAULT_MAX_BODY_BYTES);

        Ok(Self {
            bind,
            shared_token,
            request_timeout: Duration::from_secs(request_timeout_seconds),
            max_body_bytes,
        })
    }
}
