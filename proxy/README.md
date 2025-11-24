# Poly Proxy

Poly Proxy is a lightweight HTTP reverse proxy server designed to allow the usage of the PolyTrack leaderboard without
being affected by CORS (Cross-Origin Resource Sharing) restrictions.

## Acceptable Use Policy

By using Poly Proxy, you agree to the following terms:

1. **Legitimate Use**: Poly Proxy is intended for legitimate use cases only. You agree not to use the proxy for any illegal, harmful, malicious, cheating, or unethical activities.
2. **Prohibited Content**: You agree not to use Poly Proxy to access, distribute, or facilitate content that is illegal, harmful, or violates the rights of others.
3. **No Liability**: The developers and maintainers of Poly Proxy are not responsible for any misuse of the proxy or any consequences arising from its use.

## Configuration

Configure via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `BIND_ADDR` | `127.0.0.1:8000` | Address and port to bind the proxy server. |
| `BACKEND_URL` | `https://vps.kodub.com` | Backend service URL. |
| `CERT_PATH` | `cert.pem` | Path to TLS certificate |
| `KEY_PATH` | `key.pem` | Path to TLS private key |
| `USE_TLS` | `true` | Enable/disable TLS |
| `RUST_LOG` | `info` | Logging level (trace, debug, info, warn, error) |

## Formatting

```bash
cargo +nightly fmt
```
