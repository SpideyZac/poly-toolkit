use std::{io::BufReader, path::Path, sync::Arc};

use anyhow::{Context, Result, ensure};
use tracing::info;

pub fn load_tls_config(cert_path: &Path, key_path: &Path) -> Result<Arc<rustls::ServerConfig>> {
    info!("Loading TLS certificate from {:?}", cert_path);
    info!("Loading TLS private key from {:?}", key_path);

    let cert_file = std::fs::File::open(cert_path)
        .with_context(|| format!("Failed to open cert file {:?}", cert_path))?;

    let certs = rustls_pemfile::certs(&mut BufReader::new(cert_file))
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to parse certificates")?;

    ensure!(!certs.is_empty(), "No certificates found in cert file");

    let key_file = std::fs::File::open(key_path)
        .with_context(|| format!("Failed to open key file {:?}", key_path))?;

    let key = rustls_pemfile::private_key(&mut BufReader::new(key_file))
        .context("Failed to parse private key")?
        .context("No private key found in key file")?;

    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("Failed to build TLS config")?;

    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    info!("TLS configuration loaded successfully");
    Ok(Arc::new(config))
}
