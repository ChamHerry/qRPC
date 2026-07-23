//! Rust 客户端绑定。

use qrpc_core::call::{Call, CallError, CallEvent, Metadata, Role};
use qrpc_core::error::{ErrorCode, Status, StatusError};
use qrpc_core::transport::{Connection, InprocTransport, TlsConfig, Transport, TransportError};
use std::net::SocketAddr;
use std::sync::Arc;

/// qRPC Rust 客户端。
///
/// Rust 绑定直连 core，不经过 C ABI。0.1 先暴露 bytes 级 API，protobuf 类型由
/// `protoc-gen-qrpc` 生成层负责序列化/反序列化。
#[derive(Clone)]
pub struct Client {
    conn: Arc<dyn Connection>,
}

impl Client {
    /// 连接到 in-proc server，用于本机 Rust↔Rust 验证和生成代码单测。
    pub async fn connect_inproc(addr: SocketAddr) -> Result<Self, Status> {
        let transport = InprocTransport::new();
        let conn = transport
            .connect(&addr, &TlsConfig::default())
            .await
            .map_err(status_from_transport)?;
        Ok(Self { conn })
    }

    /// 发起 raw call。调用方可用返回的 `ClientCall` 组合四种 RPC 模式。
    pub async fn start_call(&self, method: impl Into<String>) -> Result<ClientCall, Status> {
        let stream = self
            .conn
            .open_bi_stream()
            .await
            .map_err(status_from_transport)?;
        let mut call = Call::new(stream, Role::Client);
        call.send_metadata(method_metadata(method.into()))
            .await
            .map_err(status_from_call)?;
        Ok(ClientCall { inner: call })
    }

    /// unary 便捷 API：发送一个 bytes 请求并等待一个 bytes 响应。
    pub async fn unary(
        &self,
        method: impl Into<String>,
        request: impl Into<Vec<u8>>,
    ) -> Result<Vec<u8>, Status> {
        let mut call = self.start_call(method).await?;
        call.send(request.into(), true).await?;
        match call.recv().await? {
            Some(response) => Ok(response),
            None => Err(Status::new(
                ErrorCode::Internal,
                "call closed before response",
            )),
        }
    }
}

/// 客户端 raw call。生成代码可基于它实现 unary/streaming 签名。
pub struct ClientCall {
    inner: Call,
}

impl ClientCall {
    pub async fn send(&mut self, data: Vec<u8>, end_stream: bool) -> Result<(), Status> {
        self.inner
            .send_message(data, end_stream)
            .await
            .map_err(status_from_call)
    }

    pub async fn finish_send(&mut self) -> Result<(), Status> {
        self.inner.finish_send().await.map_err(status_from_call)
    }

    pub async fn recv(&mut self) -> Result<Option<Vec<u8>>, Status> {
        loop {
            match self.inner.recv_event().await.map_err(status_from_call)? {
                Some(CallEvent::Metadata(_)) => continue,
                Some(CallEvent::Data { payload, .. }) => return Ok(Some(payload)),
                Some(CallEvent::Status { payload, .. }) => {
                    let status = Status::from_payload(&payload).map_err(status_from_status)?;
                    if status.code == ErrorCode::Ok {
                        return Ok(None);
                    }
                    return Err(status);
                }
                None => return Ok(None),
            }
        }
    }

    pub fn cancel(&mut self, code: u32) {
        self.inner.cancel(code);
    }
}

fn method_metadata(method: String) -> Metadata {
    vec![(":method".to_string(), method.into_bytes())]
}

pub(crate) fn status_from_call(err: CallError) -> Status {
    Status::new(ErrorCode::Internal, err.to_string())
}

pub(crate) fn status_from_transport(err: TransportError) -> Status {
    let code = match err {
        TransportError::NotListening(_)
        | TransportError::ListenerClosed
        | TransportError::ConnectionClosed => ErrorCode::Unavailable,
        TransportError::AddrInUse(_) => ErrorCode::AlreadyExists,
        TransportError::UnsupportedDatagram | TransportError::Other(_) => ErrorCode::Internal,
    };
    Status::new(code, err.to_string())
}

fn status_from_status(err: StatusError) -> Status {
    Status::new(ErrorCode::Internal, err.to_string())
}
