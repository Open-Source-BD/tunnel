mod tls;
mod tunnel;

use crate::tls::TlsConfig;
use crate::tunnel::TunnelManager;
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use clap::Parser;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "tunnel-server", about = "ngrok alternative server")]
struct Args {
    #[arg(long, default_value = "0.0.0.0")]
    bind: String,

    #[arg(long, default_value = "9000")]
    tunnel_port: u16,

    #[arg(long, default_value = "443")]
    http_port: u16,

    #[arg(long, default_value = "80")]
    http_redirect_port: u16,

    #[arg(long, env = "TUNNEL_DOMAIN", default_value = "localhost")]
    domain: String,

    #[arg(long, env = "TUNNEL_TOKEN")]
    token: String,

    #[arg(long)]
    cert_file: Option<String>,

    #[arg(long)]
    key_file: Option<String>,
}

#[derive(Clone)]
struct AppState {
    tunnels: Arc<TunnelManager>,
    domain: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let tunnels = Arc::new(TunnelManager::new(args.token.clone(), args.domain.clone()));

    // Set up TLS
    let tls_config = if let (Some(cert), Some(key)) = (&args.cert_file, &args.key_file) {
        TlsConfig::from_files(cert, key)?
    } else {
        tracing::warn!("using self-signed cert (set --cert-file and --key-file for production)");
        TlsConfig::self_signed(&args.domain)?
    };

    // Start tunnel listener (TLS on tunnel_port)
    let tunnel_addr: SocketAddr = format!("{}:{}", args.bind, args.tunnel_port).parse()?;
    let tunnel_acceptor = tls_config.bind(tunnel_addr).await?;
    let tunnels_clone = tunnels.clone();
    tokio::spawn(async move {
        tunnel::run_tunnel_listener(tunnel_acceptor, tunnels_clone).await;
    });

    // Start HTTP redirect (80 -> 443)
    if args.http_redirect_port > 0 {
        let redirect_addr: SocketAddr = format!("{}:{}", args.bind, args.http_redirect_port).parse()?;
        tokio::spawn(async move {
            let app = Router::new().fallback(redirect_to_https);
            let listener = tokio::net::TcpListener::bind(redirect_addr).await.unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    }

    // Start visitor HTTP server (443)
    let state = AppState {
        tunnels,
        domain: args.domain.clone(),
    };

    let app = Router::new()
        .route("/_tunnel/health", axum::routing::get(health))
        .route("/_tunnel/dashboard", axum::routing::get(dashboard))
        .route("/{*path}", any(proxy_handler))
        .fallback(any(proxy_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let https_addr: SocketAddr = format!("{}:{}", args.bind, args.http_port).parse()?;
    tracing::info!("visitor HTTPS server listening on {https_addr}");
    tracing::info!("tunnel server listening on {tunnel_addr}");
    tracing::info!("domain: {}", args.domain);

    let listener = tokio::net::TcpListener::bind(https_addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

async fn redirect_to_https() -> Response {
    (StatusCode::PERMANENT_REDIRECT, "HTTPS required").into_response()
}

async fn health() -> &'static str {
    "ok"
}

async fn dashboard(State(state): State<AppState>) -> impl IntoResponse {
    let tunnels = state.tunnels.list().await;
    let rows: String = tunnels
        .iter()
        .map(|t| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"status-online\">online</td></tr>",
                t.subdomain, t.local_port, t.connected_at
            )
        })
        .collect();

    let html = format!(
        r#"<!DOCTYPE html>
<html><head><title>Tunnel Dashboard</title>
<meta name="viewport" content="width=device-width,initial-scale=1">
<style>
body{{font-family:system-ui,sans-serif;max-width:800px;margin:40px auto;padding:0 20px}}
table{{width:100%;border-collapse:collapse}}
th,td{{padding:8px 12px;text-align:left;border-bottom:1px solid #ddd}}
th{{background:#f5f5f5}}
.status-online{{color:#090}}
</style></head><body>
<h1>Tunnel Dashboard</h1>
<table><tr><th>Subdomain</th><th>Local Port</th><th>Connected</th><th>Status</th></tr>
{rows}</table>
<p><a href=\"/_tunnel/health\">Health check</a></p></body></html>"#
    );
    (StatusCode::OK, [("content-type", "text/html; charset=utf-8")], html)
}

async fn proxy_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: axum::extract::Request,
) -> Response {
    let host = req
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let subdomain = host.split('.').next().unwrap_or("").to_string();

    if subdomain.is_empty() || subdomain == state.domain.split('.').next().unwrap_or("") {
        return (StatusCode::NOT_FOUND, "no tunnel").into_response();
    }

    let path = req.uri().path().to_string();

    match state.tunnels.route(&subdomain).await {
        Some(handle) => match handle.proxy_request(req, &path, addr).await {
            Ok(resp) => resp,
            Err(e) => {
                tracing::error!("proxy error for {subdomain}: {e}");
                (StatusCode::BAD_GATEWAY, format!("tunnel error: {e}")).into_response()
            }
        },
        None => (StatusCode::NOT_FOUND, format!("no tunnel for '{subdomain}'")).into_response(),
    }
}
