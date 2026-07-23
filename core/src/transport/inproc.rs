//! in-proc 传输后端。
//!
//! 该后端只用于单元测试和内核集成验证：不走网络、不做序列化，通过内存通道
//! 模拟连接、双向流、复位和半关闭。

use super::traits::{
    BiStream, BoxFuture, Connection, Listener, StreamError, TlsConfig, Transport, TransportError,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};

type ConnSender = Sender<Arc<dyn Connection>>;
type StreamBox = Box<dyn BiStream>;
type StreamSender = Sender<StreamBox>;

fn registry() -> &'static Mutex<HashMap<SocketAddr, ConnSender>> {
    static REGISTRY: OnceLock<Mutex<HashMap<SocketAddr, ConnSender>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 内存传输后端。
#[derive(Debug, Default, Clone)]
pub struct InprocTransport;

impl InprocTransport {
    pub fn new() -> Self {
        Self
    }
}

impl Transport for InprocTransport {
    fn connect<'a>(
        &'a self,
        addr: &'a SocketAddr,
        _cfg: &'a TlsConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn Connection>, TransportError>> {
        Box::pin(async move {
            let sender = {
                let guard = registry()
                    .lock()
                    .map_err(|_| TransportError::Other("in-proc registry poisoned".to_string()))?;
                guard
                    .get(addr)
                    .cloned()
                    .ok_or(TransportError::NotListening(*addr))?
            };

            let (client, server) = InprocConnection::pair();
            sender
                .send(server)
                .map_err(|_| TransportError::ListenerClosed)?;
            Ok(client)
        })
    }

    fn listen<'a>(
        &'a self,
        addr: &'a SocketAddr,
        _cfg: &'a TlsConfig,
    ) -> BoxFuture<'a, Result<Box<dyn Listener>, TransportError>> {
        Box::pin(async move {
            let (tx, rx) = mpsc::channel::<Arc<dyn Connection>>();
            let mut guard = registry()
                .lock()
                .map_err(|_| TransportError::Other("in-proc registry poisoned".to_string()))?;
            if guard.contains_key(addr) {
                return Err(TransportError::AddrInUse(*addr));
            }
            guard.insert(*addr, tx);
            Ok(Box::new(InprocListener {
                addr: *addr,
                rx: Mutex::new(rx),
            }) as Box<dyn Listener>)
        })
    }
}

struct InprocListener {
    addr: SocketAddr,
    rx: Mutex<Receiver<Arc<dyn Connection>>>,
}

impl Listener for InprocListener {
    fn accept<'a>(&'a mut self) -> BoxFuture<'a, Result<Arc<dyn Connection>, TransportError>> {
        Box::pin(async move {
            let rx = self
                .rx
                .lock()
                .map_err(|_| TransportError::Other("in-proc listener receiver poisoned".to_string()))?;
            rx.recv().map_err(|_| TransportError::ListenerClosed)
        })
    }
}

impl Drop for InprocListener {
    fn drop(&mut self) {
        if let Ok(mut guard) = registry().lock() {
            guard.remove(&self.addr);
        }
    }
}

struct InprocConnection {
    incoming_streams: Mutex<Receiver<StreamBox>>,
    peer_streams: StreamSender,
    closed: AtomicBool,
}

impl InprocConnection {
    fn pair() -> (Arc<dyn Connection>, Arc<dyn Connection>) {
        let (a_tx, a_rx) = mpsc::channel::<StreamBox>();
        let (b_tx, b_rx) = mpsc::channel::<StreamBox>();

        let a = Arc::new(Self {
            incoming_streams: Mutex::new(a_rx),
            peer_streams: b_tx,
            closed: AtomicBool::new(false),
        });
        let b = Arc::new(Self {
            incoming_streams: Mutex::new(b_rx),
            peer_streams: a_tx,
            closed: AtomicBool::new(false),
        });

        (a, b)
    }
}

impl Connection for InprocConnection {
    fn open_bi_stream<'a>(&'a self) -> BoxFuture<'a, Result<StreamBox, TransportError>> {
        Box::pin(async move {
            if self.closed.load(Ordering::SeqCst) {
                return Err(TransportError::ConnectionClosed);
            }

            let (local, remote) = InprocBiStream::pair();
            self.peer_streams
                .send(remote)
                .map_err(|_| TransportError::ConnectionClosed)?;
            Ok(local)
        })
    }

    fn accept_bi_stream<'a>(&'a self) -> BoxFuture<'a, Result<StreamBox, TransportError>> {
        Box::pin(async move {
            if self.closed.load(Ordering::SeqCst) {
                return Err(TransportError::ConnectionClosed);
            }
            let rx = self
                .incoming_streams
                .lock()
                .map_err(|_| TransportError::Other("in-proc stream receiver poisoned".to_string()))?;
            rx.recv().map_err(|_| TransportError::ConnectionClosed)
        })
    }

    fn close<'a>(&'a self, _reason: u32) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.closed.store(true, Ordering::SeqCst);
        })
    }
}

enum StreamMessage {
    Data(Vec<u8>),
    Finish,
    Reset(u32),
}

struct InprocBiStream {
    incoming: Mutex<Receiver<StreamMessage>>,
    outgoing: Sender<StreamMessage>,
    send_finished: bool,
    recv_finished: bool,
    reset: Option<u32>,
}

impl InprocBiStream {
    fn pair() -> (StreamBox, StreamBox) {
        let (a_tx, a_rx) = mpsc::channel::<StreamMessage>();
        let (b_tx, b_rx) = mpsc::channel::<StreamMessage>();

        let a = Self {
            incoming: Mutex::new(a_rx),
            outgoing: b_tx,
            send_finished: false,
            recv_finished: false,
            reset: None,
        };
        let b = Self {
            incoming: Mutex::new(b_rx),
            outgoing: a_tx,
            send_finished: false,
            recv_finished: false,
            reset: None,
        };

        (Box::new(a), Box::new(b))
    }
}

impl BiStream for InprocBiStream {
    fn send<'a>(&'a mut self, data: &'a [u8]) -> BoxFuture<'a, Result<(), StreamError>> {
        Box::pin(async move {
            if self.reset.is_some() || self.recv_finished {
                return Err(StreamError::Closed);
            }
            if self.send_finished {
                return Err(StreamError::PeerClosed);
            }
            self.outgoing
                .send(StreamMessage::Data(data.to_vec()))
                .map_err(|_| StreamError::PeerClosed)
        })
    }

    fn recv<'a>(&'a mut self) -> BoxFuture<'a, Result<Option<Vec<u8>>, StreamError>> {
        Box::pin(async move {
            if let Some(code) = self.reset {
                return Err(StreamError::Reset(code));
            }
            if self.recv_finished {
                return Ok(None);
            }

            let msg = {
                let rx = self.incoming.lock().map_err(|_| StreamError::Closed)?;
                rx.recv().map_err(|_| StreamError::PeerClosed)?
            };

            match msg {
                StreamMessage::Data(data) => Ok(Some(data)),
                StreamMessage::Finish => {
                    self.recv_finished = true;
                    Ok(None)
                }
                StreamMessage::Reset(code) => {
                    self.reset = Some(code);
                    Err(StreamError::Reset(code))
                }
            }
        })
    }

    fn finish_send<'a>(&'a mut self) -> BoxFuture<'a, Result<(), StreamError>> {
        Box::pin(async move {
            if self.reset.is_some() || self.send_finished {
                return Err(StreamError::Closed);
            }
            self.send_finished = true;
            self.outgoing
                .send(StreamMessage::Finish)
                .map_err(|_| StreamError::PeerClosed)
        })
    }

    fn reset(&mut self, code: u32) {
        self.reset = Some(code);
        let _ = self.outgoing.send(StreamMessage::Reset(code));
    }

    fn is_finished(&self) -> bool {
        self.send_finished && self.recv_finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::net::{IpAddr, Ipv4Addr};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    static NEXT_PORT: AtomicU16 = AtomicU16::new(20_000);

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

    fn connected_pair() -> (Arc<dyn Connection>, Arc<dyn Connection>) {
        let transport = InprocTransport::new();
        let addr = next_addr();
        let cfg = TlsConfig::default();
        let mut listener = block_on(transport.listen(&addr, &cfg)).expect("listen");
        let client = block_on(transport.connect(&addr, &cfg)).expect("connect");
        let server = block_on(listener.accept()).expect("accept");
        (client, server)
    }

    #[test]
    fn listen_connect_accepts_connection() {
        let (_client, _server) = connected_pair();
    }

    #[test]
    fn bi_stream_sends_and_receives_bytes() {
        let (client, server) = connected_pair();
        let mut client_stream = block_on(client.open_bi_stream()).expect("open stream");
        let mut server_stream = block_on(server.accept_bi_stream()).expect("accept stream");

        block_on(client_stream.send(b"ping")).expect("send");
        let received = block_on(server_stream.recv()).expect("recv").expect("data");

        assert_eq!(received, b"ping");
    }

    #[test]
    fn reset_is_observed_by_peer() {
        let (client, server) = connected_pair();
        let mut client_stream = block_on(client.open_bi_stream()).expect("open stream");
        let mut server_stream = block_on(server.accept_bi_stream()).expect("accept stream");

        client_stream.reset(42);

        assert_eq!(block_on(server_stream.recv()), Err(StreamError::Reset(42)));
    }

    #[test]
    fn finish_send_half_closes_peer_recv() {
        let (client, server) = connected_pair();
        let mut client_stream = block_on(client.open_bi_stream()).expect("open stream");
        let mut server_stream = block_on(server.accept_bi_stream()).expect("accept stream");

        block_on(client_stream.finish_send()).expect("finish");

        assert_eq!(block_on(server_stream.recv()).expect("recv"), None);
    }
}
