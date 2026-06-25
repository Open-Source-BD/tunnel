use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tunnel_proto::codec::Codec;
use tunnel_proto::types::*;

pub struct TlsStream(tokio_rustls::client::TlsStream<TcpStream>);

impl AsyncRead for TlsStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for TlsStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

pub async fn start_http_tunnel(
    server: &str,
    token: &str,
    subdomain: &str,
    local_port: u16,
) -> anyhow::Result<()> {
    let stream = connect(server).await?;
    let (reader, writer) = tokio::io::split(stream);
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Frame>(1024);
    let outbound = tx.clone();

    tracing::info!("connecting to tunnel server at {server}...");

    let reg = RegisterPayload {
        subdomain: subdomain.to_string(),
        local_port,
        token: token.to_string(),
    };

    tx.send(Frame {
        stream_id: 0,
        msg_type: MessageType::Register,
        payload: bytes::Bytes::from(serde_json::to_string(&reg)?),
    })
    .await?;

    let writer_task = tokio::spawn(async move {
        let mut writer = tokio::io::BufWriter::new(writer);
        while let Some(frame) = rx.recv().await {
            Codec::encode_and_write(&mut writer, &frame)
                .await
                .map_err(|e| anyhow::anyhow!("write error: {e}"))?;
        }
        Ok::<_, anyhow::Error>(())
    });

    let reader_task = tokio::spawn(handle_http_frames(reader, local_port, outbound));

    tokio::select! {
        r = writer_task => r??,
        r = reader_task => r??,
    }

    Ok(())
}

pub async fn start_tcp_tunnel(
    server: &str,
    token: &str,
    local_port: u16,
) -> anyhow::Result<()> {
    let subdomain = format!(
        "tcp-{}",
        uuid::Uuid::new_v4()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>()
    );
    start_http_tunnel(server, token, &subdomain, local_port).await
}

async fn connect(server: &str) -> anyhow::Result<TlsStream> {
    let root_certs = rustls::RootCertStore::from_iter(
        webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
    );

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_certs)
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(config));
    let tcp = TcpStream::connect(server).await?;

    let domain_str = server.split(':').next().unwrap_or("localhost").to_string();
    let dns = rustls::pki_types::ServerName::try_from(domain_str.clone())
        .map_err(|_| anyhow::anyhow!("invalid domain: {domain_str}"))?;
    let tls = connector.connect(dns, tcp).await?;

    Ok(TlsStream(tls))
}

async fn handle_http_frames<R>(
    mut reader: R,
    local_port: u16,
    outbound: tokio::sync::mpsc::Sender<Frame>,
) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    loop {
        match Codec::decode(&mut reader).await {
            Ok(Some(frame)) => match frame.msg_type {
                MessageType::HttpRequest => {
                    let tx = outbound.clone();
                    tokio::spawn(async move {
                        if let Err(e) = forward_http_request(frame, local_port, tx).await {
                            tracing::error!("forward error: {e}");
                        }
                    });
                }
                MessageType::Heartbeat => {}
                MessageType::CloseStream => {}
                MessageType::Registered => {
                    if let Ok(reg) =
                        serde_json::from_slice::<RegisteredPayload>(&frame.payload)
                    {
                        tracing::info!("tunnel registered at {}", reg.assigned_url);
                        println!("Tunnel URL: {}", reg.assigned_url);
                    }
                }
                MessageType::Error => {
                    if let Ok(err) = serde_json::from_slice::<ErrorPayload>(&frame.payload) {
                        tracing::error!("server error: {}", err.message);
                        eprintln!("Error: {}", err.message);
                    }
                }
                _ => tracing::warn!("unexpected frame: {:?}", frame.msg_type),
            },
            Ok(None) => {
                tracing::info!("server disconnected");
                break;
            }
            Err(e) => {
                tracing::error!("decode error: {e}");
                break;
            }
        }
    }
    Ok(())
}

async fn forward_http_request(
    frame: Frame,
    local_port: u16,
    outbound: tokio::sync::mpsc::Sender<Frame>,
) -> anyhow::Result<()> {
    let req_data: serde_json::Value = serde_json::from_slice(&frame.payload)?;
    let method = req_data["method"].as_str().unwrap_or("GET");
    let uri = req_data["uri"].as_str().unwrap_or("/");
    let headers = req_data["headers"].as_object();
    let stream_id = frame.stream_id;

    let mut local = match TcpStream::connect(format!("127.0.0.1:{local_port}")).await {
        Ok(s) => s,
        Err(e) => {
            let err_resp = serde_json::json!({
                "status": 502,
                "headers": {},
                "body": format!("Connection refused: {e}"),
            });
            let _ = outbound
                .send(Frame {
                    stream_id,
                    msg_type: MessageType::HttpResponse,
                    payload: bytes::Bytes::from(serde_json::to_string(&err_resp)?),
                })
                .await;
            return Ok(());
        }
    };

    let mut req_str = format!("{method} {uri} HTTP/1.1\r\n");
    if let Some(headers) = headers {
        for (k, v) in headers {
            if let Some(v) = v.as_str() {
                if k != "host" && k != "content-length" {
                    req_str.push_str(&format!("{k}: {v}\r\n"));
                }
            }
        }
    }
    req_str.push_str("Host: localhost\r\n");
    req_str.push_str("Connection: close\r\n");
    req_str.push_str("\r\n");

    local.write_all(req_str.as_bytes()).await?;

    let mut resp_buf = Vec::new();
    local.read_to_end(&mut resp_buf).await?;
    let resp_str = String::from_utf8_lossy(&resp_buf).to_string();

    let status = resp_str
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(502);

    let body = resp_str.splitn(2, "\r\n\r\n").nth(1).unwrap_or("");

    let resp_data = serde_json::json!({
        "status": status,
        "headers": {},
        "body": body,
    });

    let _ = outbound
        .send(Frame {
            stream_id,
            msg_type: MessageType::HttpResponse,
            payload: bytes::Bytes::from(serde_json::to_string(&resp_data)?),
        })
        .await;

    tracing::info!("{method} {uri} -> {status}");
    Ok(())
}
