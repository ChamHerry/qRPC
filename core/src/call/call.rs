//! 调用状态机实现。

use super::state::{CallState, Role};
use crate::frame::{flags, Frame, FrameError, FrameType};
use crate::transport::{BiStream, StreamError};
use std::fmt;

/// 调用元数据。
pub type Metadata = Vec<(String, Vec<u8>)>;

/// 调用级事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallEvent {
    Metadata(Metadata),
    Data { payload: Vec<u8>, end_stream: bool },
    Status { payload: Vec<u8>, end_stream: bool },
}

/// 一个 RPC 调用，对应一条双向 stream。
pub struct Call {
    stream: Box<dyn BiStream>,
    role: Role,
    state: CallState,
    recv_buf: Vec<u8>,
}

impl Call {
    pub fn new(stream: Box<dyn BiStream>, role: Role) -> Self {
        Self {
            stream,
            role,
            state: CallState::Init,
            recv_buf: Vec::new(),
        }
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn state(&self) -> CallState {
        self.state
    }

    pub async fn send_metadata(&mut self, meta: Metadata) -> Result<(), CallError> {
        let payload = encode_metadata(&meta)?;
        self.send_frame(Frame::new(FrameType::Metadata, flags::END_METADATA, payload))
            .await?;
        if matches!(self.state, CallState::Init) {
            self.state = CallState::MetadataSent;
        }
        Ok(())
    }

    pub async fn send_message(&mut self, data: Vec<u8>, end: bool) -> Result<(), CallError> {
        let frame_flags = if end { flags::END_STREAM } else { 0 };
        self.send_frame(Frame::new(FrameType::Data, frame_flags, data))
            .await?;
        if end {
            self.stream.finish_send().await?;
            self.mark_local_closed();
        } else if matches!(self.state, CallState::Init | CallState::MetadataSent) {
            self.state = CallState::Open;
        }
        Ok(())
    }

    pub async fn finish_send(&mut self) -> Result<(), CallError> {
        self.stream.finish_send().await?;
        self.mark_local_closed();
        Ok(())
    }

    pub async fn recv_event(&mut self) -> Result<Option<CallEvent>, CallError> {
        loop {
            match Frame::decode(&mut self.recv_buf)? {
                Some(frame) => return self.frame_to_event(frame).map(Some),
                None => match self.stream.recv().await? {
                    Some(chunk) => self.recv_buf.extend_from_slice(&chunk),
                    None => {
                        self.mark_remote_closed();
                        return Ok(None);
                    }
                },
            }
        }
    }

    pub fn cancel(&mut self, code: u32) {
        self.stream.reset(code);
        self.state = CallState::Cancelled;
    }

    async fn send_frame(&mut self, frame: Frame) -> Result<(), CallError> {
        let encoded = frame.encode()?;
        self.stream.send(&encoded).await?;
        Ok(())
    }

    fn frame_to_event(&mut self, frame: Frame) -> Result<CallEvent, CallError> {
        let end_stream = frame.flags & flags::END_STREAM != 0;
        let event = match frame.ty {
            FrameType::Metadata => CallEvent::Metadata(decode_metadata(&frame.payload)?),
            FrameType::Data => CallEvent::Data {
                payload: frame.payload,
                end_stream,
            },
            FrameType::Status => CallEvent::Status {
                payload: frame.payload,
                end_stream,
            },
            FrameType::Settings | FrameType::Rst | FrameType::GoAway => {
                return Err(CallError::Protocol("unexpected control frame in call stream"));
            }
        };
        if end_stream {
            self.mark_remote_closed();
        } else if matches!(self.state, CallState::Init | CallState::MetadataSent) {
            self.state = CallState::Open;
        }
        Ok(event)
    }

    fn mark_local_closed(&mut self) {
        self.state = match self.state {
            CallState::HalfRemote | CallState::Closed => CallState::Closed,
            CallState::Cancelled => CallState::Cancelled,
            _ => CallState::HalfLocal,
        };
    }

    fn mark_remote_closed(&mut self) {
        self.state = match self.state {
            CallState::HalfLocal | CallState::Closed => CallState::Closed,
            CallState::Cancelled => CallState::Cancelled,
            _ => CallState::HalfRemote,
        };
    }
}

fn encode_metadata(meta: &Metadata) -> Result<Vec<u8>, CallError> {
    let mut out = Vec::new();
    for (key, value) in meta {
        let key = key.to_ascii_lowercase();
        let key_bytes = key.as_bytes();
        let key_len =
            u16::try_from(key_bytes.len()).map_err(|_| CallError::InvalidMetadata)?;
        let value_len =
            u32::try_from(value.len()).map_err(|_| CallError::InvalidMetadata)?;
        out.extend_from_slice(&key_len.to_be_bytes());
        out.extend_from_slice(key_bytes);
        out.extend_from_slice(&value_len.to_be_bytes());
        out.extend_from_slice(value);
    }
    Ok(out)
}

fn decode_metadata(payload: &[u8]) -> Result<Metadata, CallError> {
    let mut offset = 0;
    let mut out = Vec::new();
    while offset < payload.len() {
        if payload.len() - offset < 2 {
            return Err(CallError::InvalidMetadata);
        }
        let key_len = u16::from_be_bytes([payload[offset], payload[offset + 1]]) as usize;
        offset += 2;
        if payload.len() - offset < key_len + 4 {
            return Err(CallError::InvalidMetadata);
        }
        let key = String::from_utf8(payload[offset..offset + key_len].to_vec())
            .map_err(|_| CallError::InvalidMetadata)?;
        offset += key_len;
        let value_len = u32::from_be_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ]) as usize;
        offset += 4;
        if payload.len() - offset < value_len {
            return Err(CallError::InvalidMetadata);
        }
        let value = payload[offset..offset + value_len].to_vec();
        offset += value_len;
        out.push((key, value));
    }
    Ok(out)
}

/// 调用层错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    Frame(FrameError),
    Stream(StreamError),
    InvalidMetadata,
    Protocol(&'static str),
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frame(err) => write!(f, "{err}"),
            Self::Stream(err) => write!(f, "{err}"),
            Self::InvalidMetadata => write!(f, "invalid metadata payload"),
            Self::Protocol(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for CallError {}

impl From<FrameError> for CallError {
    fn from(value: FrameError) -> Self {
        Self::Frame(value)
    }
}

impl From<StreamError> for CallError {
    fn from(value: StreamError) -> Self {
        Self::Stream(value)
    }
}
