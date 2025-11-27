use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};

/// Configuration for the application.
pub struct Config {
    /// The address to bind the server to.
    pub bind_addr: SocketAddr,
    /// The backend URL to connect to.
    pub backend_url: String,
    /// Path to the TLS certificate file.
    pub cert_path: PathBuf,
    /// Path to the TLS key file.
    pub key_path: PathBuf,
    /// Whether to use TLS.
    pub use_tls: bool,
}

impl Config {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        let bind_addr = std::env::var("BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8000".to_string())
            .parse()
            .context("Failed to parse BIND_ADDR")?;

        let backend_url =
            std::env::var("BACKEND_URL").unwrap_or_else(|_| "https://vps.kodub.com".to_string());

        let cert_path = std::env::var("CERT_PATH")
            .unwrap_or_else(|_| "cert.pem".to_string())
            .into();

        let key_path = std::env::var("KEY_PATH")
            .unwrap_or_else(|_| "key.pem".to_string())
            .into();

        let use_tls = std::env::var("USE_TLS")
            .unwrap_or_else(|_| "true".to_string())
            .parse()
            .unwrap_or(true);

        Ok(Config {
            bind_addr,
            backend_url,
            cert_path,
            key_path,
            use_tls,
        })
    }
}
