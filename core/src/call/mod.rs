//! RPC 调用状态机模块。
//!
//! 负责四种调用模式、取消传播、半关闭语义以及背压向上传导。

mod call;
mod state;

pub use call::{Call, CallError, CallEvent, Metadata};
pub use state::{CallState, Role};
