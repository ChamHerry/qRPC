use qrpc::{Client, Server};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

fn main() {
    let addr = "127.0.0.1:37101".parse().expect("addr");
    let mut server = block_on(Server::bind_inproc(addr)).expect("server");
    server.register_unary("demo.Echo", |_method, request| async move {
        let mut response = b"rust echo: ".to_vec();
        response.extend_from_slice(&request);
        Ok(response)
    });

    let server_thread = std::thread::spawn(move || block_on(server.serve_one()));
    let client = block_on(Client::connect_inproc(addr)).expect("client");
    let response = block_on(client.unary("/demo.Echo/Unary", b"hello".to_vec())).expect("unary");
    server_thread.join().expect("server thread").expect("serve");
    println!("{}", String::from_utf8(response).expect("utf8"));
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
