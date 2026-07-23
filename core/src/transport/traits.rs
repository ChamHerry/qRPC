//! 传输抽象层 trait。

use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

/// TAL 使用的 boxed future 类型，避免在 0.1 骨架期引入 async-trait。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 传输层 TLS 配置占位。
///
/// 具体证书/CA/rustls 配置由 T-009 落地；T-003 只保留 trait 入口。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TlsConfig {
    pub server_name: Option<String>,
}

/// 传输后端 trait。
pub trait Transport: Send + Sync {
    fn connect<'a>(
        &'a self,
        addr: &'a SocketAddr,
        cfg: &'a TlsConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn Connection>, TransportError>>;

    fn listen<'a>(
        &'a self,
        addr: &'a SocketAddr,
        cfg: &'a TlsConfig,
    ) -> BoxFuture<'a, Result<Box<dyn Listener>, TransportError>>;
}

/// 监听器：接受入站连接。
pub trait Listener: Send + Sync {
    fn accept<'a>(&'a mut self) -> BoxFuture<'a, Result<Arc<dyn Connection>, TransportError>>;
}

/// 一条传输连接：承载多个并发 RPC stream。
pub trait Connection: Send + Sync {
    fn open_bi_stream<'a>(&'a self) -> BoxFuture<'a, Result<Box<dyn BiStream>, TransportError>>;

    fn accept_bi_stream<'a>(&'a self) -> BoxFuture<'a, Result<Box<dyn BiStream>, TransportError>>;

    fn send_datagram<'a>(
        &'a self,
        _data: &'a [u8],
    ) -> BoxFuture<'a, Result<(), TransportError>> {
        Box::pin(async { Err(TransportError::UnsupportedDatagram) })
    }

    fn close<'a>(&'a self, reason: u32) -> BoxFuture<'a, ()>;
}

/// 双向可靠字节流。
pub trait BiStream: Send + Sync {
    fn send<'a>(&'a mut self, data: &'a [u8]) -> BoxFuture<'a, Result<(), StreamError>>;

    fn recv<'a>(&'a mut self) -> BoxFuture<'a, Result<Option<Vec<u8>>, StreamError>>;

    /// 结束本端发送方向；对端 `recv` 应返回 `Ok(None)`。
    fn finish_send<'a>(&'a mut self) -> BoxFuture<'a, Result<(), StreamError>>;

    fn reset(&mut self, code: u32);

    fn is_finished(&self) -> bool;
}

/// 连接/监听层错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    AddrInUse(SocketAddr),
    NotListening(SocketAddr),
    ListenerClosed,
    ConnectionClosed,
    UnsupportedDatagram,
    Other(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AddrInUse(addr) => write!(f, "address already in use: {addr}"),
            Self::NotListening(addr) => write!(f, "no listener for address: {addr}"),
            Self::ListenerClosed => write!(f, "listener closed"),
            Self::ConnectionClosed => write!(f, "connection closed"),
            Self::UnsupportedDatagram => write!(f, "datagram is reserved and unsupported in qRPC 0.1"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for TransportError {}

/// stream 层错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamError {
    Closed,
    Reset(u32),
    PeerClosed,
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => write!(f, "stream closed"),
            Self::Reset(code) => write!(f, "stream reset with code {code}"),
            Self::PeerClosed => write!(f, "stream peer closed"),
        }
    }
}

impl std::error::Error for StreamError {}
