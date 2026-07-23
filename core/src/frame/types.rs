//! 帧类型、常量与错误定义。

use std::fmt;

/// qRPC 固定帧头长度。
pub const HEADER_LEN: usize = 8;

/// 0.1 默认单帧 payload 上限，对齐协议默认 4 MiB 消息上限。
pub const MAX_DEFAULT_PAYLOAD_LEN: usize = 4 * 1024 * 1024;

/// 帧标志位定义。
pub mod flags {
    /// 本帧是 stream 的最后一帧。
    pub const END_STREAM: u8 = 0x01;
    /// 压缩预留位。0.1 固定不支持。
    pub const COMPRESSED: u8 = 0x02;
    /// METADATA 帧集合结束。
    pub const END_METADATA: u8 = 0x04;
}

/// qRPC 帧类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameType {
    Metadata = 0x01,
    Data = 0x02,
    Status = 0x03,
    Settings = 0x04,
    Rst = 0x05,
    GoAway = 0x06,
}

impl FrameType {
    /// 从 wire 值解析帧类型。
    pub fn from_wire(value: u8) -> Result<Self, FrameError> {
        match value {
            0x01 => Ok(Self::Metadata),
            0x02 => Ok(Self::Data),
            0x03 => Ok(Self::Status),
            0x04 => Ok(Self::Settings),
            0x05 => Ok(Self::Rst),
            0x06 => Ok(Self::GoAway),
            value if value & 0x80 != 0 => Err(FrameError::IgnorableUnknownType(value)),
            value => Err(FrameError::UnknownFrameType(value)),
        }
    }

    /// 转为 wire 值。
    pub const fn as_wire(self) -> u8 {
        self as u8
    }
}

/// 一个 qRPC wire frame。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub ty: FrameType,
    pub flags: u8,
    pub payload: Vec<u8>,
}

impl Frame {
    /// 创建一个帧。
    pub fn new(ty: FrameType, flags: u8, payload: impl Into<Vec<u8>>) -> Self {
        Self {
            ty,
            flags,
            payload: payload.into(),
        }
    }

    /// 编码为 wire 字节。
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        super::codec::encode_frame(self)
    }

    /// 从缓冲区解码一个完整帧；数据不足时返回 `Ok(None)`。
    pub fn decode(buf: &mut Vec<u8>) -> Result<Option<Self>, FrameError> {
        super::codec::decode_frame(buf)
    }
}

/// 帧编解码错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    PayloadTooLarge { len: usize, max: usize },
    UnsupportedCompressedFlag,
    UnknownFrameType(u8),
    IgnorableUnknownType(u8),
    LengthOverflow(u32),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PayloadTooLarge { len, max } => {
                write!(f, "frame payload too large: {len} > {max}")
            }
            Self::UnsupportedCompressedFlag => {
                write!(f, "COMPRESSED flag is reserved and unsupported in qRPC 0.1")
            }
            Self::UnknownFrameType(value) => write!(f, "unknown critical frame type: 0x{value:02x}"),
            Self::IgnorableUnknownType(value) => {
                write!(f, "unknown ignorable frame type skipped: 0x{value:02x}")
            }
            Self::LengthOverflow(len) => write!(f, "frame length cannot fit usize: {len}"),
        }
    }
}

impl std::error::Error for FrameError {}
