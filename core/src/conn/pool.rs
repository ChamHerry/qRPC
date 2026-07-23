//! 连接池与断连事件。

use crate::transport::{Connection, TlsConfig, Transport, TransportError};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// keepalive 策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepaliveConfig {
    pub interval: Duration,
    pub timeout: Duration,
}

/// 连接级事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionEvent {
    Unavailable(SocketAddr),
}

struct PoolEntry {
    connection: Arc<dyn Connection>,
    available: bool,
}

/// 到远端地址的连接复用池。
pub struct ConnectionPool {
    transport: Arc<dyn Transport>,
    entries: Mutex<HashMap<SocketAddr, PoolEntry>>,
    keepalive: Mutex<Option<KeepaliveConfig>>,
    subscribers: Mutex<Vec<Sender<ConnectionEvent>>>,
}

impl ConnectionPool {
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self {
            transport,
            entries: Mutex::new(HashMap::new()),
            keepalive: Mutex::new(None),
            subscribers: Mutex::new(Vec::new()),
        }
    }

    pub async fn dial(
        &self,
        addr: &SocketAddr,
        cfg: &TlsConfig,
    ) -> Result<Arc<dyn Connection>, TransportError> {
        if let Some(connection) = self.available_connection(addr)? {
            return Ok(connection);
        }

        let connection = self.transport.connect(addr, cfg).await?;
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| TransportError::Other("connection pool poisoned".to_string()))?;
        entries.insert(
            *addr,
            PoolEntry {
                connection: connection.clone(),
                available: true,
            },
        );
        Ok(connection)
    }

    pub fn enable_keepalive(&self, interval: Duration, timeout: Duration) {
        if let Ok(mut keepalive) = self.keepalive.lock() {
            *keepalive = Some(KeepaliveConfig { interval, timeout });
        }
    }

    pub fn keepalive_config(&self) -> Option<KeepaliveConfig> {
        self.keepalive.lock().ok().and_then(|cfg| *cfg)
    }

    pub fn subscribe_events(&self) -> Receiver<ConnectionEvent> {
        let (tx, rx) = mpsc::channel();
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.push(tx);
        }
        rx
    }

    pub fn mark_unavailable(&self, addr: &SocketAddr) {
        if let Ok(mut entries) = self.entries.lock() {
            if let Some(entry) = entries.get_mut(addr) {
                entry.available = false;
            }
        }
        self.publish(ConnectionEvent::Unavailable(*addr));
    }

    fn available_connection(
        &self,
        addr: &SocketAddr,
    ) -> Result<Option<Arc<dyn Connection>>, TransportError> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| TransportError::Other("connection pool poisoned".to_string()))?;
        Ok(entries
            .get(addr)
            .filter(|entry| entry.available)
            .map(|entry| entry.connection.clone()))
    }

    fn publish(&self, event: ConnectionEvent) {
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.retain(|subscriber| subscriber.send(event.clone()).is_ok());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::InprocTransport;
    use std::future::Future;
    use std::net::{IpAddr, Ipv4Addr};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    static NEXT_PORT: AtomicU16 = AtomicU16::new(23_000);

    fn next_addr() -> SocketAddr {
        SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            NEXT_PORT.fetch_add(1, Ordering::SeqCst),
        )
    }

    #[test]
    fn dial_reuses_available_connection() {
        let transport = Arc::new(InprocTransport::new());
        let pool = ConnectionPool::new(transport.clone());
        let addr = next_addr();
        let cfg = TlsConfig::default();
        let mut listener = block_on(transport.listen(&addr, &cfg)).expect("listen");

        let first = block_on(pool.dial(&addr, &cfg)).expect("dial first");
        let _server = block_on(listener.accept()).expect("accept first");
        let second = block_on(pool.dial(&addr, &cfg)).expect("dial second");

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn unavailable_connection_notifies_and_redials() {
        let transport = Arc::new(InprocTransport::new());
        let pool = ConnectionPool::new(transport.clone());
        let addr = next_addr();
        let cfg = TlsConfig::default();
        let mut listener = block_on(transport.listen(&addr, &cfg)).expect("listen");
        let events = pool.subscribe_events();

        let first = block_on(pool.dial(&addr, &cfg)).expect("dial first");
        let _server = block_on(listener.accept()).expect("accept first");
        pool.mark_unavailable(&addr);

        assert_eq!(events.recv().expect("event"), ConnectionEvent::Unavailable(addr));

        let second = block_on(pool.dial(&addr, &cfg)).expect("dial second");
        let _server = block_on(listener.accept()).expect("accept second");

        assert!(!Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn keepalive_config_is_stored() {
        let pool = ConnectionPool::new(Arc::new(InprocTransport::new()));

        pool.enable_keepalive(Duration::from_secs(5), Duration::from_secs(2));

        assert_eq!(
            pool.keepalive_config(),
            Some(KeepaliveConfig {
                interval: Duration::from_secs(5),
                timeout: Duration::from_secs(2),
            })
        );
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
