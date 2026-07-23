//! 服务端方法分发器。

use crate::call::{Call, Metadata};
use crate::transport::BoxFuture;
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::panic::{self, AssertUnwindSafe};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// T-006 临时状态码；T-008 会迁移到统一 error/status 模块。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum StatusCode {
    Ok = 0,
    Unimplemented = 12,
    Internal = 13,
}

/// 分发结果状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchStatus {
    pub code: StatusCode,
    pub message: String,
}

impl DispatchStatus {
    pub fn ok() -> Self {
        Self {
            code: StatusCode::Ok,
            message: String::new(),
        }
    }

    pub fn unimplemented(message: impl Into<String>) -> Self {
        Self {
            code: StatusCode::Unimplemented,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: StatusCode::Internal,
            message: message.into(),
        }
    }
}

impl fmt::Display for DispatchStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            write!(f, "{:?}", self.code)
        } else {
            write!(f, "{:?}: {}", self.code, self.message)
        }
    }
}

/// 服务处理器 trait。
pub trait ServiceHandler: Send + Sync {
    fn handle<'a>(
        &'a self,
        call: Call,
        method: String,
        meta: Metadata,
    ) -> BoxFuture<'a, DispatchStatus>;
}

/// 按 service 注册的分发器。
#[derive(Default)]
pub struct Dispatcher {
    handlers: HashMap<String, Arc<dyn ServiceHandler>>,
}

impl Dispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_service(
        &mut self,
        service: impl Into<String>,
        handler: Arc<dyn ServiceHandler>,
    ) {
        self.handlers.insert(normalize_service(service.into()), handler);
    }

    pub async fn dispatch(&self, call: Call, meta: Metadata) -> DispatchStatus {
        let method = match find_method(&meta) {
            Some(method) => method,
            None => return DispatchStatus::unimplemented("missing :method metadata"),
        };
        let service = match service_from_method(&method) {
            Some(service) => service,
            None => return DispatchStatus::unimplemented(format!("invalid method: {method}")),
        };
        let Some(handler) = self.handlers.get(&service).cloned() else {
            return DispatchStatus::unimplemented(format!("unknown service: {service}"));
        };

        PanicToInternal {
            inner: handler.handle(call, method, meta),
        }
        .await
    }
}

fn find_method(meta: &Metadata) -> Option<String> {
    meta.iter()
        .find(|(key, _)| key == ":method")
        .and_then(|(_, value)| String::from_utf8(value.clone()).ok())
}

fn service_from_method(method: &str) -> Option<String> {
    let method = method.trim_start_matches('/');
    let (service, method_name) = method.rsplit_once('/')?;
    if service.is_empty() || method_name.is_empty() {
        return None;
    }
    Some(normalize_service(service.to_string()))
}

fn normalize_service(service: String) -> String {
    service.trim_start_matches('/').to_string()
}

struct PanicToInternal<'a> {
    inner: BoxFuture<'a, DispatchStatus>,
}

impl Future for PanicToInternal<'_> {
    type Output = DispatchStatus;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match panic::catch_unwind(AssertUnwindSafe(|| self.inner.as_mut().poll(cx))) {
            Ok(poll) => poll,
            Err(_) => Poll::Ready(DispatchStatus::internal("handler panicked")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::{Call, Role};
    use crate::transport::{BiStream, StreamError};
    use std::future::Future;
    use std::sync::Mutex;
    use std::task::{RawWaker, RawWakerVTable, Waker};

    #[test]
    fn dispatch_routes_by_service_name() {
        let handler = Arc::new(RecordingHandler::default());
        let mut dispatcher = Dispatcher::new();
        dispatcher.register_service("demo.Echo", handler.clone());

        let status = block_on(dispatcher.dispatch(
            dummy_call(),
            vec![(":method".to_string(), b"/demo.Echo/Unary".to_vec())],
        ));

        assert_eq!(status.code, StatusCode::Ok);
        assert_eq!(
            handler.methods.lock().expect("methods").as_slice(),
            &["/demo.Echo/Unary".to_string()]
        );
    }

    #[test]
    fn unknown_service_returns_unimplemented() {
        let dispatcher = Dispatcher::new();

        let status = block_on(dispatcher.dispatch(
            dummy_call(),
            vec![(":method".to_string(), b"/demo.Missing/Unary".to_vec())],
        ));

        assert_eq!(status.code, StatusCode::Unimplemented);
    }

    #[test]
    fn handler_panic_returns_internal() {
        let mut dispatcher = Dispatcher::new();
        dispatcher.register_service("demo.Echo", Arc::new(PanicHandler));

        let status = block_on(dispatcher.dispatch(
            dummy_call(),
            vec![(":method".to_string(), b"/demo.Echo/Unary".to_vec())],
        ));

        assert_eq!(status.code, StatusCode::Internal);
    }

    #[derive(Default)]
    struct RecordingHandler {
        methods: Mutex<Vec<String>>,
    }

    impl ServiceHandler for RecordingHandler {
        fn handle<'a>(
            &'a self,
            _call: Call,
            method: String,
            _meta: Metadata,
        ) -> BoxFuture<'a, DispatchStatus> {
            Box::pin(async move {
                self.methods.lock().expect("methods").push(method);
                DispatchStatus::ok()
            })
        }
    }

    struct PanicHandler;

    impl ServiceHandler for PanicHandler {
        fn handle<'a>(
            &'a self,
            _call: Call,
            _method: String,
            _meta: Metadata,
        ) -> BoxFuture<'a, DispatchStatus> {
            Box::pin(async { panic!("handler failed") })
        }
    }

    fn dummy_call() -> Call {
        Call::new(Box::new(NoopStream), Role::Server)
    }

    struct NoopStream;

    impl BiStream for NoopStream {
        fn send<'a>(&'a mut self, _data: &'a [u8]) -> BoxFuture<'a, Result<(), StreamError>> {
            Box::pin(async { Ok(()) })
        }

        fn recv<'a>(&'a mut self) -> BoxFuture<'a, Result<Option<Vec<u8>>, StreamError>> {
            Box::pin(async { Ok(None) })
        }

        fn finish_send<'a>(&'a mut self) -> BoxFuture<'a, Result<(), StreamError>> {
            Box::pin(async { Ok(()) })
        }

        fn reset(&mut self, _code: u32) {}

        fn is_finished(&self) -> bool {
            false
        }
    }

    fn block_on<F: Future>(mut future: F) -> F::Output {
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        let mut future = unsafe { Pin::new_unchecked(&mut future) };
        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    fn noop_waker() -> Waker {
        unsafe fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        unsafe fn wake(_: *const ()) {}
        unsafe fn wake_by_ref(_: *const ()) {}
        unsafe fn drop(_: *const ()) {}

        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
        unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
    }
}
