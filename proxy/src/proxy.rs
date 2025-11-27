use std::{convert::Infallible, io::Error, net::IpAddr, sync::OnceLock, time::Duration};

use http_body_util::{BodyExt, Empty, combinators::UnsyncBoxBody};
use hyper::{
    Request, Response, StatusCode,
    body::{Bytes, Incoming},
};
use hyper_reverse_proxy::ReverseProxy;
use hyper_rustls::{ConfigBuilderExt, HttpsConnector};
use hyper_util::{
    client::legacy::{Builder, connect::HttpConnector},
    rt::{TokioExecutor, TokioTimer},
};
use rustls::ClientConfig;
use tracing::{debug, error};

/// Type alias for the HTTPS connector and response body
type Connector = HttpsConnector<HttpConnector>;
/// Type alias for the response body
pub type ResponseBody = UnsyncBoxBody<Bytes, Error>;

/// Get a singleton reverse proxy client
fn proxy_client() -> &'static ReverseProxy<Connector> {
    static PROXY_CLIENT: OnceLock<ReverseProxy<Connector>> = OnceLock::new();
    PROXY_CLIENT.get_or_init(|| {
        let connector: Connector = Connector::builder()
            .with_tls_config(
                ClientConfig::builder()
                    .with_native_roots()
                    .expect("with_native_roots")
                    .with_no_client_auth(),
            )
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();

        ReverseProxy::new(
            Builder::new(TokioExecutor::new())
                .pool_idle_timeout(Duration::from_secs(90))
                .pool_max_idle_per_host(32)
                .pool_timer(TokioTimer::new())
                .build::<_, Incoming>(connector),
        )
    })
}

/// Handle proxying the incoming request to the backend URL
pub async fn handle(
    client_ip: IpAddr,
    backend_url: &str,
    req: Request<Incoming>,
) -> Result<Response<ResponseBody>, Infallible> {
    let method = req.method().clone();
    let uri = req.uri().clone();

    debug!("Proxying request: {} {} from {}", method, uri, client_ip);

    match proxy_client().call(client_ip, backend_url, req).await {
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
                    Empty::<Bytes>::new().map_err(Error::other),
                ))
                .unwrap())
        }
    }
}
