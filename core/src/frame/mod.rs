//! RPC 帧协议模块。
//!
//! 负责 8 字节帧头、帧类型、flags 与 payload 的编解码边界。

mod codec;
mod types;

pub use codec::{decode_frame, encode_frame};
pub use types::{
    flags, Frame, FrameError, FrameType, HEADER_LEN, MAX_DEFAULT_PAYLOAD_LEN,
};
