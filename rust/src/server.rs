//! Rust 服务端绑定。

use crate::client::{status_from_call, status_from_transport};
use qrpc_core::call::{Call, CallEvent, Metadata, Role};
use qrpc_core::dispatch::{DispatchStatus, Dispatcher, ServiceHandler, StatusCode};
use qrpc_core::error::{ErrorCode, Status};
use qrpc_core::transport::{BoxFuture, InprocTransport, Listener, TlsConfig, Transport};
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

type UnaryHandlerFn =
    dyn Fn(String, Vec<u8>) -> BoxFuture<'static, Result<Vec<u8>, Status>> + Send + Sync;

/// qRPC Rust 服务端。
pub struct Server {
    dispatcher: Dispatcher,
    listener: Box<dyn Listener>,
}

impl Server {
    /// 绑定 in-proc listener，用于本机 Rust↔Rust 验证和生成代码单测。
    pub async fn bind_inproc(addr: SocketAddr) -> Result<Self, Status> {
        let transport = InprocTransport::new();
        let listener = transport
            .listen(&addr, &TlsConfig::default())
            .await
            .map_err(status_from_transport)?;
        Ok(Self {
            dispatcher: Dispatcher::new(),
            listener,
        })
    }

    /// 注册 bytes 级 unary handler。生成的 server stub 可在此处完成 protobuf 编解码。
    pub fn register_unary<F, Fut>(&mut self, service: impl Into<String>, handler: F)
    where
        F: Fn(String, Vec<u8>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<u8>, Status>> + Send + 'static,
    {
        let handler = Arc::new(move |method: String, request: Vec<u8>| {
            Box::pin(handler(method, request)) as BoxFuture<'static, Result<Vec<u8>, Status>>
        });
        self.dispatcher
            .register_service(service, Arc::new(UnaryServiceHandler { handler }));
    }

    /// 接收并处理一个入站 call。0.1 绑定测试用该最小循环验证 Rust↔Rust 互通。
    pub async fn serve_one(&mut self) -> Result<(), Status> {
        let conn = self
            .listener
            .accept()
            .await
            .map_err(status_from_transport)?;
        let stream = conn
            .accept_bi_stream()
            .await
            .map_err(status_from_transport)?;
        let mut call = Call::new(stream, Role::Server);
        let meta = match call.recv_event().await.map_err(status_from_call)? {
            Some(CallEvent::Metadata(meta)) => meta,
            Some(_) => {
                return Err(Status::new(
                    ErrorCode::InvalidArgument,
                    "first call event is not metadata",
                ));
            }
            None => return Err(Status::new(ErrorCode::Cancelled, "call closed")),
        };

        dispatch_status_to_result(self.dispatcher.dispatch(call, meta).await)
    }
}

struct UnaryServiceHandler {
    handler: Arc<UnaryHandlerFn>,
}

impl ServiceHandler for UnaryServiceHandler {
    fn handle<'a>(
        &'a self,
        call: Call,
        method: String,
        _meta: Metadata,
    ) -> BoxFuture<'a, DispatchStatus> {
        let handler = Arc::clone(&self.handler);
        Box::pin(async move {
            let mut call = call;
            let request = match next_data(&mut call).await {
                Ok(request) => request,
                Err(status) => return dispatch_status_from_status(status),
            };

            match handler(method, request).await {
                Ok(response) => match call.send_message(response, true).await {
                    Ok(()) => DispatchStatus::ok(),
                    Err(err) => DispatchStatus::internal(err.to_string()),
                },
                Err(status) => dispatch_status_from_status(status),
            }
        })
    }
}

async fn next_data(call: &mut Call) -> Result<Vec<u8>, Status> {
    loop {
        match call.recv_event().await.map_err(status_from_call)? {
            Some(CallEvent::Metadata(_)) => continue,
            Some(CallEvent::Data { payload, .. }) => return Ok(payload),
            Some(CallEvent::Status { .. }) => {
                return Err(Status::new(
                    ErrorCode::Internal,
                    "unexpected status before request data",
                ));
            }
            None => return Err(Status::new(ErrorCode::Cancelled, "call closed")),
        }
    }
}

fn dispatch_status_to_result(status: DispatchStatus) -> Result<(), Status> {
    match status.code {
        StatusCode::Ok => Ok(()),
        StatusCode::Unimplemented => Err(Status::new(ErrorCode::Unimplemented, status.message)),
        StatusCode::Internal => Err(Status::new(ErrorCode::Internal, status.message)),
    }
}

fn dispatch_status_from_status(status: Status) -> DispatchStatus {
    match status.code {
        ErrorCode::Ok => DispatchStatus::ok(),
        ErrorCode::Unimplemented => DispatchStatus::unimplemented(status.message),
        _ => DispatchStatus::internal(status.message),
    }
}
