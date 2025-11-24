use std::{sync::OnceLock, time::Duration};

use anyhow::Result;
use hyper_util::rt::TokioIo;
use tokio::{net::TcpListener, spawn, time::timeout};
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, info, warn};
use tracing_subscriber::{
    EnvFilter, fmt::layer, layer::SubscriberExt, registry, util::SubscriberInitExt,
};

use crate::{
    config::Config,
    server::{handle_connection, handle_plaintext_connection, shutdown_signal},
    tls::load_tls_config,
};

mod config;
mod proxy;
mod server;
mod tls;

static GLOBAL_CONFIG: OnceLock<Config> = OnceLock::new();

#[tokio::main]
async fn main() -> Result<()> {
    registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,hyper_reverse_proxy=debug".into()),
        )
        .with(layer())
        .init();

    let config = Config::from_env()?;

    info!("Starting reverse proxy server");
    info!("Backend URL: {}", config.backend_url);
    info!("Bind address: {}", config.bind_addr);
    info!("TLS enabled: {}", config.use_tls);

    let listener = TcpListener::bind(config.bind_addr).await?;

    let tls_acceptor = if config.use_tls {
        let tls_config = load_tls_config(&config.cert_path, &config.key_path)?;
        Some(TlsAcceptor::from(tls_config))
    } else {
        warn!("Running in plaintext HTTP mode (TLS disabled)");
        None
    };

    if config.use_tls {
        info!("✓ Server listening on https://{}", config.bind_addr);
    } else {
        info!("✓ Server listening on http://{}", config.bind_addr);
    }
    info!("Press Ctrl+C to shutdown gracefully");

    let config_ref = GLOBAL_CONFIG.get_or_init(|| config);

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, remote_addr)) => {
                        let client_ip = remote_addr.ip();
                        debug!("Accepted connection from {}", client_ip);

                        if let Some(ref acceptor) = tls_acceptor {
                            let acceptor = acceptor.clone();
                            spawn(async move {
                                match timeout(
                                    Duration::from_secs(10),
                                    acceptor.accept(stream)
                                ).await {
                                    Ok(Ok(tls_stream)) => {
                                        let io = TokioIo::new(tls_stream);
                                        handle_connection(io, client_ip, config_ref).await;
                                    }
                                    Ok(Err(e)) => {
                                        warn!("TLS handshake error from {}: {:?}", client_ip, e);
                                    }
                                    Err(_) => {
                                        warn!("TLS handshake timeout from {}", client_ip);
                                    }
                                }
                            });
                        } else {
                            spawn(async move {
                                let io = TokioIo::new(stream);
                                handle_plaintext_connection(io, client_ip, config_ref).await;
                            });
                        }
                    }
                    Err(e) => {
                        error!("Failed to accept connection: {:?}", e);
                    }
                }
            }
            _ = shutdown_signal() => {
                info!("Shutting down gracefully...");
                break;
            }
        }
    }

    info!("Server shutdown complete");
    Ok(())
}
