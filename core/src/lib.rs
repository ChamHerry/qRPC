//! qRPC 内核 crate。
//!
//! 本 crate 按设计拆分为 frame/call/dispatch/conn/error/transport/abi/auth
//! 八个模块。T-001 只建立模块边界，具体协议和运行时行为由后续任务实现。

pub mod abi;
pub mod auth;
pub mod call;
pub mod conn;
pub mod dispatch;
pub mod error;
pub mod frame;
pub mod transport;
