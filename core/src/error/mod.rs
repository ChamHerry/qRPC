//! 错误与状态模块。
//!
//! 负责 gRPC 兼容状态码、STATUS 帧映射以及跨语言一致的错误表示。

mod codes;
mod status;

pub use codes::{ErrorCode, ALL_ERROR_CODES};
pub use status::{Status, StatusError};
