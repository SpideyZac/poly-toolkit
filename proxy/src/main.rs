use std::{
    convert::Infallible,
    io,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Duration,
};

use http_body_util::{BodyExt, Empty, combinators::UnsyncBoxBody};
use hyper::{
    Request, Response, StatusCode,
    body::{Bytes, Incoming},
    server::conn::{http1, http2},
    service::service_fn,
};
use hyper_reverse_proxy::ReverseProxy;
use hyper_rustls::{ConfigBuilderExt, HttpsConnector};
use hyper_util::{
    client::legacy::connect::HttpConnector,
    rt::{TokioExecutor, TokioIo, TokioTimer},
};
use tokio::{net::TcpListener, signal};
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

type Connector = HttpsConnector<HttpConnector>;
type ResponseBody = UnsyncBoxBody<Bytes, std::io::Error>;

fn proxy_client() -> &'static ReverseProxy<Connector> {
    static PROXY_CLIENT: OnceLock<ReverseProxy<Connector>> = OnceLock::new();
    PROXY_CLIENT.get_or_init(|| {
        let connector: Connector = Connector::builder()
            .with_tls_config(
                rustls::ClientConfig::builder()
                    .with_native_roots()
                    .expect("with_native_roots")
                    .with_no_client_auth(),
            )
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();
        ReverseProxy::new(
            hyper_util::client::legacy::Builder::new(TokioExecutor::new())
                .pool_idle_timeout(Duration::from_secs(90))
                .pool_max_idle_per_host(32)
                .pool_timer(TokioTimer::new())
                .build::<_, Incoming>(connector),
        )
    })
}

async fn handle(
    client_ip: IpAddr,
    req: Request<Incoming>,
) -> Result<Response<ResponseBody>, Infallible> {
    let method = req.method().clone();
    let uri = req.uri().clone();

    debug!("Proxying request: {} {} from {}", method, uri, client_ip);

    match proxy_client()
        .call(client_ip, "https://vps.kodub.com", req)
        .await
    {
        Ok(response) => {
            debug!(
                "Proxy response: {} for {} {}",
                response.status(),
                method,
                uri
            );
            Ok(response)
        }
        Err(error) => {
            error!("Proxy error for {} {}: {:?}", method, uri, error);
            Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(UnsyncBoxBody::new(
                    Empty::<Bytes>::new().map_err(io::Error::other),
                ))
                .unwrap())
        }
    }
}

// Configuration structure
#[derive(Clone)]
struct Config {
    bind_addr: SocketAddr,
    backend_url: String,
    cert_path: PathBuf,
    key_path: PathBuf,
    use_tls: bool,
}

impl Config {
    fn from_env() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let bind_addr = std::env::var("BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8000".to_string())
            .parse()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

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

// Load TLS configuration with proper error handling
fn load_tls_config(
    cert_path: &PathBuf,
    key_path: &PathBuf,
) -> Result<Arc<rustls::ServerConfig>, Box<dyn std::error::Error + Send + Sync>> {
    info!("Loading TLS certificate from {:?}", cert_path);
    info!("Loading TLS private key from {:?}", key_path);

    let cert_file = std::fs::File::open(cert_path)
        .map_err(|e| format!("Failed to open cert file {:?}: {}", cert_path, e))?;
    let certs = rustls_pemfile::certs(&mut io::BufReader::new(cert_file))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to parse certificates: {}", e))?;

    if certs.is_empty() {
        return Err("No certificates found in cert file".into());
    }

    let key_file = std::fs::File::open(key_path)
        .map_err(|e| format!("Failed to open key file {:?}: {}", key_path, e))?;
    let key = rustls_pemfile::private_key(&mut io::BufReader::new(key_file))
        .map_err(|e| format!("Failed to parse private key: {}", e))?
        .ok_or("No private key found in key file")?;

    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("Failed to build TLS config: {}", e))?;

    // Enable HTTP/2 and HTTP/1.1 via ALPN
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    info!("TLS configuration loaded successfully");
    Ok(Arc::new(config))
}

// Graceful shutdown handler
async fn shutdown_signal() {
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
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C signal");
        },
        _ = terminate => {
            info!("Received terminate signal");
        },
    }
}

// Handle individual connection with protocol detection
async fn handle_connection(
    io: TokioIo<tokio_rustls::server::TlsStream<tokio::net::TcpStream>>,
    client_ip: IpAddr,
) {
    let service = service_fn(move |req| handle(client_ip, req));

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

// Handle plaintext HTTP connections (for development)
async fn handle_plaintext_connection(io: TokioIo<tokio::net::TcpStream>, client_ip: IpAddr) {
    let service = service_fn(move |req| handle(client_ip, req));

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,hyper_reverse_proxy=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
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

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, remote_addr)) => {
                        let client_ip = remote_addr.ip();
                        debug!("Accepted connection from {}", client_ip);

                        if let Some(ref acceptor) = tls_acceptor {
                            let acceptor = acceptor.clone();
                            tokio::task::spawn(async move {
                                match tokio::time::timeout(
                                    Duration::from_secs(10),
                                    acceptor.accept(stream)
                                ).await {
                                    Ok(Ok(tls_stream)) => {
                                        let io = TokioIo::new(tls_stream);
                                        handle_connection(io, client_ip).await;
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
                            // Plaintext HTTP mode
                            tokio::task::spawn(async move {
                                let io = TokioIo::new(stream);
                                handle_plaintext_connection(io, client_ip).await;
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
