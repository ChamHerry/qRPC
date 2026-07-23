//! Helpers invoked by the Python asyncio bridge.

use qrpc::{Client, Server, Status};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

static NEXT_PORT: AtomicUsize = AtomicUsize::new(19_000);

pub fn unary_roundtrip(payload: Vec<u8>) -> Result<Vec<u8>, Status> {
    let addr = next_addr();
    let mut server = block_on(Server::bind_inproc(addr))?;
    server.register_unary("demo.Echo", |_method, request| async move {
        let mut response = b"echo:".to_vec();
        response.extend_from_slice(&request);
        Ok(response)
    });

    let server_thread = std::thread::spawn(move || block_on(server.serve_one()));

    let client = block_on(Client::connect_inproc(addr))?;
    let response = block_on(client.unary("/demo.Echo/Unary", payload))?;
    server_thread
        .join()
        .map_err(|_| qrpc::Status::new(qrpc::ErrorCode::Internal, "server thread panicked"))??;
    Ok(response)
}

fn next_addr() -> SocketAddr {
    let port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);
    format!("127.0.0.1:{port}").parse().expect("in-proc addr")
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
