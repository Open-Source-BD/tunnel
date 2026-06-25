use std::io::BufReader;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

pub struct TlsConfig {
    pub server_config: rustls::ServerConfig,
}

pub struct WrappedStream(pub tokio_rustls::server::TlsStream<TcpAcceptStream>);

pub struct TcpAcceptStream(pub tokio::net::TcpStream);

impl AsyncRead for TcpAcceptStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for TcpAcceptStream {
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

impl AsyncRead for WrappedStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for WrappedStream {
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

pub struct TunnelAcceptor {
    listener: TcpListener,
    acceptor: TlsAcceptor,
}

impl TunnelAcceptor {
    pub async fn accept(&self) -> std::io::Result<(WrappedStream, std::net::SocketAddr)> {
        let (stream, addr) = self.listener.accept().await?;
        let tls_stream = self.acceptor.accept(TcpAcceptStream(stream)).await?;
        Ok((WrappedStream(tls_stream), addr))
    }
}

impl TlsConfig {
    pub fn self_signed(domain: &str) -> anyhow::Result<Self> {
        let cert = rcgen::generate_simple_self_signed(vec![domain.into()])?;
        let cert_der = cert.cert.der().to_vec();
        let key_der = cert.key_pair.serialize_der();

        let cert = rustls::pki_types::CertificateDer::from(cert_der);
        let key = rustls::pki_types::PrivateKeyDer::try_from(key_der)
            .map_err(|e| anyhow::anyhow!("invalid key: {e}"))?;

        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)?;

        Ok(Self { server_config: config })
    }

    pub fn from_files(cert_path: &str, key_path: &str) -> anyhow::Result<Self> {
        let certs = rustls_pemfile::certs(&mut BufReader::new(std::fs::File::open(cert_path)?))
            .collect::<Result<Vec<_>, _>>()?;
        let key = rustls_pemfile::private_key(&mut BufReader::new(std::fs::File::open(key_path)?))?
            .ok_or_else(|| anyhow::anyhow!("no private key found in {key_path}"))?;

        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        Ok(Self { server_config: config })
    }

    pub async fn bind(&self, addr: std::net::SocketAddr) -> std::io::Result<TunnelAcceptor> {
        let listener = TcpListener::bind(addr).await?;
        let acceptor = TlsAcceptor::from(Arc::new(self.server_config.clone()));
        Ok(TunnelAcceptor { listener, acceptor })
    }
}
