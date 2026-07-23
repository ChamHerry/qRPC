//! qRPC Rust 绑定 crate。
//!
//! Rust 绑定直连 `qrpc-core`，不经过 C ABI。当前公开 bytes 级 async API，
//! 由后续 `protoc-gen-qrpc` 生成层负责把 protobuf 类型映射到 bytes。

mod client;
mod server;

pub use client::{Client, ClientCall};
pub use qrpc_core::error::{ErrorCode, Status};
pub use server::Server;

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    #[test]
    fn rust_unary_roundtrip_uses_inproc_core() {
        let addr = "127.0.0.1:17011".parse().expect("addr");
        let mut server = block_on(Server::bind_inproc(addr)).expect("server");
        server.register_unary("demo.Echo", |_method, request| async move {
            let mut response = b"echo:".to_vec();
            response.extend_from_slice(&request);
            Ok(response)
        });

        let server_thread = std::thread::spawn(move || block_on(server.serve_one()));

        let client = block_on(Client::connect_inproc(addr)).expect("client");
        let response = block_on(client.unary("/demo.Echo/Unary", b"ping".to_vec())).expect("unary");

        assert_eq!(response, b"echo:ping");
        server_thread.join().expect("server thread").expect("serve");
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
