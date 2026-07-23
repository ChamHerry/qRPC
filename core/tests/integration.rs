use qrpc_core::call::{Call, CallEvent, CallState, Role};
use qrpc_core::transport::{
    BiStream, BoxFuture, InprocTransport, StreamError, TlsConfig, Transport,
};
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

static NEXT_PORT: AtomicU16 = AtomicU16::new(22_000);

fn next_addr() -> SocketAddr {
    SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        NEXT_PORT.fetch_add(1, Ordering::SeqCst),
    )
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

fn call_pair() -> (Call, Call) {
    let transport = InprocTransport::new();
    let addr = next_addr();
    let cfg = TlsConfig::default();
    let mut listener = block_on(transport.listen(&addr, &cfg)).expect("listen");
    let client_conn = block_on(transport.connect(&addr, &cfg)).expect("connect");
    let server_conn = block_on(listener.accept()).expect("accept");

    let client_stream = block_on(client_conn.open_bi_stream()).expect("client stream");
    let server_stream = block_on(server_conn.accept_bi_stream()).expect("server stream");

    (
        Call::new(client_stream, Role::Client),
        Call::new(server_stream, Role::Server),
    )
}

fn expect_data(event: Option<CallEvent>) -> (Vec<u8>, bool) {
    match event.expect("event") {
        CallEvent::Data {
            payload,
            end_stream,
        } => (payload, end_stream),
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn unary_call_roundtrips_over_inproc() {
    let (mut client, mut server) = call_pair();

    block_on(client.send_metadata(vec![(
        ":method".to_string(),
        b"/demo.Echo/Unary".to_vec(),
    )]))
    .expect("client metadata");
    assert!(matches!(
        block_on(server.recv_event()).expect("server metadata"),
        Some(CallEvent::Metadata(_))
    ));

    block_on(client.send_message(b"request".to_vec(), true)).expect("request");
    let (request, request_end) = expect_data(block_on(server.recv_event()).expect("recv req"));
    assert_eq!(request, b"request");
    assert!(request_end);

    block_on(server.send_message(b"response".to_vec(), true)).expect("response");
    let (response, response_end) = expect_data(block_on(client.recv_event()).expect("recv resp"));
    assert_eq!(response, b"response");
    assert!(response_end);
    assert_eq!(client.state(), CallState::Closed);
}

#[test]
fn client_streaming_marks_last_message_with_end_stream() {
    let (mut client, mut server) = call_pair();

    block_on(client.send_message(b"part-1".to_vec(), false)).expect("part1");
    block_on(client.send_message(b"part-2".to_vec(), true)).expect("part2");

    let (part1, end1) = expect_data(block_on(server.recv_event()).expect("recv part1"));
    let (part2, end2) = expect_data(block_on(server.recv_event()).expect("recv part2"));

    assert_eq!(part1, b"part-1");
    assert!(!end1);
    assert_eq!(part2, b"part-2");
    assert!(end2);
    assert_eq!(server.state(), CallState::HalfRemote);
}

#[test]
fn server_streaming_sends_multiple_responses() {
    let (mut client, mut server) = call_pair();

    block_on(client.send_message(b"request".to_vec(), true)).expect("request");
    let (_request, request_end) = expect_data(block_on(server.recv_event()).expect("recv req"));
    assert!(request_end);

    block_on(server.send_message(b"resp-1".to_vec(), false)).expect("resp1");
    block_on(server.send_message(b"resp-2".to_vec(), true)).expect("resp2");

    let (resp1, end1) = expect_data(block_on(client.recv_event()).expect("recv resp1"));
    let (resp2, end2) = expect_data(block_on(client.recv_event()).expect("recv resp2"));

    assert_eq!(resp1, b"resp-1");
    assert!(!end1);
    assert_eq!(resp2, b"resp-2");
    assert!(end2);
}

#[test]
fn bidirectional_streaming_allows_interleaved_messages() {
    let (mut client, mut server) = call_pair();

    block_on(client.send_message(b"client-1".to_vec(), false)).expect("client1");
    let (client1, end1) = expect_data(block_on(server.recv_event()).expect("server recv"));
    assert_eq!(client1, b"client-1");
    assert!(!end1);

    block_on(server.send_message(b"server-1".to_vec(), false)).expect("server1");
    let (server1, end2) = expect_data(block_on(client.recv_event()).expect("client recv"));
    assert_eq!(server1, b"server-1");
    assert!(!end2);

    block_on(client.finish_send()).expect("client finish");
    assert_eq!(block_on(server.recv_event()).expect("server half close"), None);
}

#[test]
fn cancel_resets_peer_stream() {
    let (mut client, mut server) = call_pair();

    client.cancel(99);

    assert_eq!(
        block_on(server.recv_event()),
        Err(qrpc_core::call::CallError::Stream(StreamError::Reset(99)))
    );
    assert_eq!(client.state(), CallState::Cancelled);
}

#[test]
fn send_message_propagates_backpressure_pending() {
    let gate = Arc::new(AtomicBool::new(false));
    let stream = Box::new(GatedStream { gate: gate.clone() });
    let mut call = Call::new(stream, Role::Client);
    let mut future = Box::pin(call.send_message(b"blocked".to_vec(), false));
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);

    assert!(matches!(future.as_mut().poll(&mut cx), Poll::Pending));

    gate.store(true, Ordering::SeqCst);
    assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
}

struct GatedStream {
    gate: Arc<AtomicBool>,
}

impl BiStream for GatedStream {
    fn send<'a>(&'a mut self, _data: &'a [u8]) -> BoxFuture<'a, Result<(), StreamError>> {
        Box::pin(GatedSend {
            gate: self.gate.clone(),
        })
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

struct GatedSend {
    gate: Arc<AtomicBool>,
}

impl Future for GatedSend {
    type Output = Result<(), StreamError>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.gate.load(Ordering::SeqCst) {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}
