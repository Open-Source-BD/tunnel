use crate::tls::{TunnelAcceptor, WrappedStream};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use tunnel_proto::codec::Codec;
use tunnel_proto::types::*;

pub struct TunnelManager {
    token: String,
    tunnels: RwLock<HashMap<String, TunnelHandle>>,
}

struct TunnelHandleInner {
    subdomain: String,
    client_tx: mpsc::Sender<Frame>,
    pending: Arc<Mutex<HashMap<u32, oneshot::Sender<Frame>>>>,
    next_stream: AtomicU32,
    local_port: u16,
    connected_at: String,
}

#[derive(Clone)]
pub struct TunnelHandle {
    inner: Arc<TunnelHandleInner>,
}

pub struct TunnelInfo {
    pub subdomain: String,
    pub local_port: u16,
    pub connected_at: String,
}

impl TunnelHandle {
    pub async fn proxy_request(
        &self,
        req: axum::extract::Request,
        _path: &str,
        visitor_addr: std::net::SocketAddr,
    ) -> anyhow::Result<axum::response::Response> {
        let stream_id = self.inner.next_stream.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.inner.pending.lock().await;
            pending.insert(stream_id, tx);
        }

        let method = req.method().to_string();
        let uri = req.uri().to_string();
        let headers: serde_json::Value = req
            .headers()
            .iter()
            .map(|(k, v)| {
                (k.to_string(), serde_json::Value::String(v.to_str().unwrap_or("").to_string()))
            })
            .collect();

        let req_payload = serde_json::json!({
            "method": method,
            "uri": uri,
            "headers": headers,
            "visitor": visitor_addr.to_string(),
        });

        let frame = Frame {
            stream_id,
            msg_type: MessageType::HttpRequest,
            payload: bytes::Bytes::from(serde_json::to_string(&req_payload)?),
        };

        self.inner
            .client_tx
            .send(frame)
            .await
            .map_err(|_| anyhow::anyhow!("tunnel client disconnected"))?;

        let resp_frame = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            rx,
        )
        .await
        .map_err(|_| anyhow::anyhow!("request timeout (120s)"))?
        .map_err(|_| anyhow::anyhow!("tunnel client disconnected"))?;

        let resp_data: serde_json::Value =
            serde_json::from_slice(&resp_frame.payload)?;

        let status = resp_data["status"].as_u64().unwrap_or(502) as u16;
        let body = resp_data["body"].as_str().unwrap_or("");

        let mut response_builder = axum::response::Response::builder().status(status);
        if let Some(headers) = resp_data["headers"].as_object() {
            for (k, v) in headers {
                if let Some(val) = v.as_str() {
                    response_builder = response_builder.header(k.as_str(), val);
                }
            }
        }

        Ok(response_builder
            .body(axum::body::Body::from(body.to_string()))
            .unwrap())
    }
}

impl TunnelManager {
    pub fn new(token: String) -> Self {
        Self {
            token,
            tunnels: RwLock::new(HashMap::new()),
        }
    }

    pub async fn route(&self, subdomain: &str) -> Option<TunnelHandle> {
        let tunnels = self.tunnels.read().await;
        tunnels.get(subdomain).cloned()
    }

    pub async fn list(&self) -> Vec<TunnelInfo> {
        let tunnels = self.tunnels.read().await;
        tunnels
            .values()
            .map(|h| TunnelInfo {
                subdomain: h.inner.subdomain.clone(),
                local_port: h.inner.local_port,
                connected_at: h.inner.connected_at.clone(),
            })
            .collect()
    }

    async fn register(
        &self,
        subdomain: String,
        local_port: u16,
        client_tx: mpsc::Sender<Frame>,
    ) -> TunnelHandle {
        let pending: Arc<Mutex<HashMap<u32, oneshot::Sender<Frame>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let handle = TunnelHandle {
            inner: Arc::new(TunnelHandleInner {
                subdomain: subdomain.clone(),
                client_tx,
                pending,
                next_stream: AtomicU32::new(1),
                local_port,
                connected_at: chrono::Utc::now().to_rfc3339(),
            }),
        };

        let mut tunnels = self.tunnels.write().await;
        tunnels.insert(subdomain, handle.clone());
        handle
    }

    async fn unregister(&self, subdomain: &str) {
        let mut tunnels = self.tunnels.write().await;
        tunnels.remove(subdomain);
    }
}

pub async fn run_tunnel_listener(
    acceptor: TunnelAcceptor,
    manager: Arc<TunnelManager>,
) {
    loop {
        match acceptor.accept().await {
            Ok((stream, addr)) => {
                tracing::info!("tunnel client connected from {addr}");
                let manager = manager.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_tunnel_client(stream, manager).await {
                        tracing::error!("tunnel client {addr} error: {e}");
                    }
                    tracing::info!("tunnel client {addr} disconnected");
                });
            }
            Err(e) => {
                tracing::error!("tunnel accept error: {e}");
            }
        }
    }
}

async fn handle_tunnel_client(
    mut stream: WrappedStream,
    manager: Arc<TunnelManager>,
) -> anyhow::Result<()> {
    let (tx, mut rx) = mpsc::channel::<Frame>(256);

    let frame = Codec::decode(&mut stream)
        .await?
        .ok_or_else(|| anyhow::anyhow!("client disconnected before register"))?;

    if frame.msg_type != MessageType::Register {
        return Err(anyhow::anyhow!("expected register, got {:?}", frame.msg_type));
    }

    let reg: RegisterPayload = serde_json::from_slice(&frame.payload)?;

    if reg.token != manager.token {
        let err = ErrorPayload {
            message: "invalid token".to_string(),
        };
        Codec::encode_and_write(
            &mut stream,
            &Frame {
                stream_id: 0,
                msg_type: MessageType::Error,
                payload: bytes::Bytes::from(serde_json::to_string(&err)?),
            },
        )
        .await?;
        return Err(anyhow::anyhow!("invalid token from client"));
    }

    let subdomain = reg.subdomain.clone();
    let handle = manager.register(subdomain.clone(), reg.local_port, tx).await;

    let assigned_url = format!("https://{subdomain}.{}", std::env::var("TUNNEL_DOMAIN").unwrap_or_default());
    let registered = RegisteredPayload {
        assigned_url,
        tunnel_id: uuid::Uuid::new_v4().to_string(),
    };

    Codec::encode_and_write(
        &mut stream,
        &Frame {
            stream_id: 0,
            msg_type: MessageType::Registered,
            payload: bytes::Bytes::from(serde_json::to_string(&registered)?),
        },
    )
    .await?;

    tracing::info!("tunnel registered: {subdomain}");

    let (mut reader, mut writer) = tokio::io::split(stream);

    let handle2 = handle.clone();
    let reader_task = tokio::spawn(async move {
        loop {
            match Codec::decode(&mut reader).await {
                Ok(Some(frame)) => {
                    match frame.msg_type {
                        MessageType::HttpResponse | MessageType::TcpData => {
                            let mut pending = handle2.inner.pending.lock().await;
                            if let Some(sender) = pending.remove(&frame.stream_id) {
                                let _ = sender.send(frame);
                            }
                        }
                        MessageType::Heartbeat => {}
                        MessageType::CloseStream => {
                            let mut pending = handle2.inner.pending.lock().await;
                            pending.remove(&frame.stream_id);
                        }
                        _ => {
                            tracing::warn!("unexpected frame from client: {:?}", frame.msg_type);
                        }
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    tracing::error!("decode error: {e}");
                    break;
                }
            }
        }
    });

    while let Some(frame) = rx.recv().await {
        if let Err(e) = Codec::encode_and_write(&mut writer, &frame).await {
            tracing::error!("write error: {e}");
            break;
        }
    }

    reader_task.abort();
    manager.unregister(&subdomain).await;
    tracing::info!("tunnel unregistered: {subdomain}");

    Ok(())
}
