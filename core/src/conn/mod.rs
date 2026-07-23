//! 连接管理模块。
//!
//! 负责连接复用、keepalive、断连检测以及进行中调用的错误通知。

mod pool;

pub use pool::{ConnectionEvent, ConnectionPool, KeepaliveConfig};
