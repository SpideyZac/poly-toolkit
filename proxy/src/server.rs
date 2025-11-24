#[cfg(not(unix))]
use std::future::pending;
use std::{net::IpAddr, time::Duration};

use hyper::{
    server::conn::{http1, http2},
    service::service_fn,
};
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use tokio::{net::TcpStream, signal};
use tokio_rustls::server::TlsStream;
use tracing::{debug, info, warn};

use crate::{config::Config, proxy::handle};

pub async fn handle_connection(
    io: TokioIo<TlsStream<TcpStream>>,
    client_ip: IpAddr,
    config: &'static Config,
) {
    let service = service_fn(move |req| handle(client_ip, &config.backend_url, req));

    // Get the negotiated ALPN protocol
    let (_, tls_session) = io.inner().get_ref();
    let protocol = tls_session
        .alpn_protocol()
        .and_then(|p| std::str::from_utf8(p).ok());

    match protocol {
        Some("h2") => {
            debug!("Using HTTP/2 for connection from {}", client_ip);
            if let Err(e) = http2::Builder::new(TokioExecutor::new())
                .timer(TokioTimer::new())
                .keep_alive_interval(Some(Duration::from_secs(20)))
                .keep_alive_timeout(Duration::from_secs(10))
                .serve_connection(io, service)
                .await
            {
                warn!("HTTP/2 connection error from {}: {:?}", client_ip, e);
            }
        }
        _ => {
            debug!("Using HTTP/1.1 for connection from {}", client_ip);
            if let Err(e) = http1::Builder::new()
                .timer(TokioTimer::new())
                .keep_alive(true)
                .serve_connection(io, service)
                .await
            {
                warn!("HTTP/1.1 connection error from {}: {:?}", client_ip, e);
            }
        }
    }
}

pub async fn handle_plaintext_connection(
    io: TokioIo<TcpStream>,
    client_ip: IpAddr,
    config: &'static Config,
) {
    let service = service_fn(move |req| handle(client_ip, &config.backend_url, req));

    debug!(
        "Using HTTP/1.1 (plaintext) for connection from {}",
        client_ip
    );
    if let Err(e) = http1::Builder::new()
        .timer(TokioTimer::new())
        .keep_alive(true)
        .serve_connection(io, service)
        .await
    {
        warn!("HTTP/1.1 connection error from {}: {:?}", client_ip, e);
    }
}

pub async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C signal");
        },
        _ = terminate => {
            info!("Received terminate signal");
        },
    }
}
