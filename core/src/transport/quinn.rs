//! quinn 传输后端。

use super::traits::{
    BiStream, BoxFuture, Connection, Listener, StreamError, TlsConfig, Transport, TransportError,
};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

/// 基于 quinn 的 QUIC 传输后端。
#[derive(Clone)]
pub struct QuinnTransport {
    client_config: ::quinn::ClientConfig,
    server_config: ::quinn::ServerConfig,
    default_server_name: String,
}

impl QuinnTransport {
    pub fn with_configs(
        client_config: ::quinn::ClientConfig,
        server_config: ::quinn::ServerConfig,
        default_server_name: impl Into<String>,
    ) -> Self {
        Self {
            client_config,
            server_config,
            default_server_name: default_server_name.into(),
        }
    }
}

impl Transport for QuinnTransport {
    fn connect<'a>(
        &'a self,
        addr: &'a SocketAddr,
        cfg: &'a TlsConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn Connection>, TransportError>> {
        Box::pin(async move {
            let bind_addr = wildcard_for(addr);
            let mut endpoint =
                ::quinn::Endpoint::client(bind_addr).map_err(map_transport_error)?;
            endpoint.set_default_client_config(self.client_config.clone());

            let server_name = cfg
                .server_name
                .as_deref()
                .unwrap_or(self.default_server_name.as_str());
            let connection = endpoint
                .connect(*addr, server_name)
                .map_err(map_transport_error)?
                .await
                .map_err(map_transport_error)?;

            Ok(Arc::new(QuinnConnection {
                _endpoint: endpoint,
                inner: connection,
            }) as Arc<dyn Connection>)
        })
    }

    fn listen<'a>(
        &'a self,
        addr: &'a SocketAddr,
        _cfg: &'a TlsConfig,
    ) -> BoxFuture<'a, Result<Box<dyn Listener>, TransportError>> {
        Box::pin(async move {
            let endpoint = ::quinn::Endpoint::server(self.server_config.clone(), *addr)
                .map_err(map_transport_error)?;
            Ok(Box::new(QuinnListener { endpoint }) as Box<dyn Listener>)
        })
    }
}

struct QuinnListener {
    endpoint: ::quinn::Endpoint,
}

impl Listener for QuinnListener {
    fn accept<'a>(&'a mut self) -> BoxFuture<'a, Result<Arc<dyn Connection>, TransportError>> {
        Box::pin(async move {
            let incoming = self
                .endpoint
                .accept()
                .await
                .ok_or(TransportError::ListenerClosed)?;
            let connection = incoming
                .accept()
                .map_err(map_transport_error)?
                .await
                .map_err(map_transport_error)?;
            Ok(Arc::new(QuinnConnection {
                _endpoint: self.endpoint.clone(),
                inner: connection,
            }) as Arc<dyn Connection>)
        })
    }
}

struct QuinnConnection {
    _endpoint: ::quinn::Endpoint,
    inner: ::quinn::Connection,
}

impl Connection for QuinnConnection {
    fn open_bi_stream<'a>(&'a self) -> BoxFuture<'a, Result<Box<dyn BiStream>, TransportError>> {
        Box::pin(async move {
            let (send, recv) = self.inner.open_bi().await.map_err(map_transport_error)?;
            Ok(Box::new(QuinnBiStream {
                send,
                recv,
                send_finished: false,
                recv_finished: false,
                reset_code: None,
            }) as Box<dyn BiStream>)
        })
    }

    fn accept_bi_stream<'a>(&'a self) -> BoxFuture<'a, Result<Box<dyn BiStream>, TransportError>> {
        Box::pin(async move {
            let (send, recv) = self.inner.accept_bi().await.map_err(map_transport_error)?;
            Ok(Box::new(QuinnBiStream {
                send,
                recv,
                send_finished: false,
                recv_finished: false,
                reset_code: None,
            }) as Box<dyn BiStream>)
        })
    }

    fn close<'a>(&'a self, reason: u32) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.inner
                .close(::quinn::VarInt::from_u32(reason), b"qrpc connection close");
        })
    }
}

struct QuinnBiStream {
    send: ::quinn::SendStream,
    recv: ::quinn::RecvStream,
    send_finished: bool,
    recv_finished: bool,
    reset_code: Option<u32>,
}

impl BiStream for QuinnBiStream {
    fn send<'a>(&'a mut self, data: &'a [u8]) -> BoxFuture<'a, Result<(), StreamError>> {
        Box::pin(async move {
            if self.send_finished || self.reset_code.is_some() {
                return Err(StreamError::Closed);
            }
            self.send
                .write_all(data)
                .await
                .map_err(|_| StreamError::PeerClosed)
        })
    }

    fn recv<'a>(&'a mut self) -> BoxFuture<'a, Result<Option<Vec<u8>>, StreamError>> {
        Box::pin(async move {
            if let Some(code) = self.reset_code {
                return Err(StreamError::Reset(code));
            }
            if self.recv_finished {
                return Ok(None);
            }

            let mut buf = vec![0; 16 * 1024];
            match self.recv.read(&mut buf).await {
                Ok(Some(n)) => {
                    buf.truncate(n);
                    Ok(Some(buf))
                }
                Ok(None) => {
                    self.recv_finished = true;
                    Ok(None)
                }
                Err(_) => Err(StreamError::PeerClosed),
            }
        })
    }

    fn finish_send<'a>(&'a mut self) -> BoxFuture<'a, Result<(), StreamError>> {
        Box::pin(async move {
            if self.send_finished || self.reset_code.is_some() {
                return Err(StreamError::Closed);
            }
            self.send.finish().map_err(|_| StreamError::Closed)?;
            self.send_finished = true;
            Ok(())
        })
    }

    fn reset(&mut self, code: u32) {
        self.reset_code = Some(code);
        let _ = self.send.reset(::quinn::VarInt::from_u32(code));
    }

    fn is_finished(&self) -> bool {
        self.send_finished && self.recv_finished
    }
}

fn wildcard_for(addr: &SocketAddr) -> SocketAddr {
    match addr {
        SocketAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        SocketAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    }
}

fn map_transport_error(error: impl std::fmt::Display) -> TransportError {
    TransportError::Other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::quinn::rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::atomic::{AtomicU16, Ordering};

    static NEXT_PORT: AtomicU16 = AtomicU16::new(21_000);

    fn next_addr() -> SocketAddr {
        SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            NEXT_PORT.fetch_add(1, Ordering::SeqCst),
        )
    }

    fn test_transport() -> QuinnTransport {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("cert");
        let cert_der = CertificateDer::from(cert.cert);
        let key_der = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());

        let mut roots = ::quinn::rustls::RootCertStore::empty();
        roots.add(cert_der.clone()).expect("root");

        let client_config =
            ::quinn::ClientConfig::with_root_certificates(Arc::new(roots)).expect("client config");
        let server_config =
            ::quinn::ServerConfig::with_single_cert(vec![cert_der], key_der.into())
                .expect("server config");

        QuinnTransport::with_configs(client_config, server_config, "localhost")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn quinn_backend_connects_and_exchanges_bytes() {
        let transport = test_transport();
        let addr = next_addr();
        let cfg = TlsConfig::default();
        let mut listener = transport.listen(&addr, &cfg).await.expect("listen");

        let client_fut = transport.connect(&addr, &cfg);
        let server_fut = listener.accept();
        let (client, server) = tokio::try_join!(client_fut, server_fut).expect("connect pair");

        let mut client_stream = client.open_bi_stream().await.expect("open stream");
        client_stream.send(b"ping").await.expect("client send");

        let mut server_stream = server.accept_bi_stream().await.expect("accept stream");
        let request = server_stream.recv().await.expect("server recv").expect("data");
        assert_eq!(request, b"ping");

        server_stream.send(b"pong").await.expect("server send");
        let response = client_stream.recv().await.expect("client recv").expect("data");
        assert_eq!(response, b"pong");

        client_stream.finish_send().await.expect("finish");
        assert_eq!(server_stream.recv().await.expect("server half close"), None);
    }
}
