//! 传输抽象层模块。
//!
//! 负责定义可靠双向 stream 抽象，并承载 in-proc、quinn 等后端实现。

mod inproc;
mod quinn;
mod traits;

pub use inproc::InprocTransport;
pub use quinn::QuinnTransport;
pub use traits::{
    BiStream, BoxFuture, Connection, Listener, StreamError, TlsConfig, Transport, TransportError,
};
