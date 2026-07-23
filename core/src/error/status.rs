//! STATUS 负载编解码。

use super::codes::ErrorCode;
use crate::frame::{flags, Frame, FrameType};
use std::fmt;

/// 调用结束状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub code: ErrorCode,
    pub message: String,
    pub details: Option<Vec<u8>>,
}

impl Status {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn ok() -> Self {
        Self::new(ErrorCode::Ok, "")
    }

    pub fn with_details(
        code: ErrorCode,
        message: impl Into<String>,
        details: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            details: Some(details.into()),
        }
    }

    pub fn to_payload(&self) -> Result<Vec<u8>, StatusError> {
        let mut out = Vec::new();
        encode_varint(self.code.as_u8() as u64, &mut out);

        let message = self.message.as_bytes();
        let message_len =
            u32::try_from(message.len()).map_err(|_| StatusError::LengthOverflow)?;
        out.extend_from_slice(&message_len.to_be_bytes());
        out.extend_from_slice(message);

        let details = self.details.as_deref().unwrap_or(&[]);
        let details_len =
            u32::try_from(details.len()).map_err(|_| StatusError::LengthOverflow)?;
        out.extend_from_slice(&details_len.to_be_bytes());
        out.extend_from_slice(details);
        Ok(out)
    }

    pub fn from_payload(payload: &[u8]) -> Result<Self, StatusError> {
        let (code, mut offset) = decode_varint(payload)?;
        let code = ErrorCode::try_from(code).map_err(StatusError::UnknownCode)?;

        let message_len = read_u32(payload, &mut offset)? as usize;
        if payload.len() - offset < message_len {
            return Err(StatusError::Truncated);
        }
        let message = String::from_utf8(payload[offset..offset + message_len].to_vec())
            .map_err(|_| StatusError::InvalidUtf8)?;
        offset += message_len;

        let details_len = read_u32(payload, &mut offset)? as usize;
        if payload.len() - offset < details_len {
            return Err(StatusError::Truncated);
        }
        let details = if details_len == 0 {
            None
        } else {
            Some(payload[offset..offset + details_len].to_vec())
        };

        Ok(Self {
            code,
            message,
            details,
        })
    }

    pub fn to_frame(&self) -> Result<Frame, StatusError> {
        Ok(Frame::new(
            FrameType::Status,
            flags::END_STREAM,
            self.to_payload()?,
        ))
    }
}

fn encode_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn decode_varint(payload: &[u8]) -> Result<(u64, usize), StatusError> {
    let mut value = 0_u64;
    for (idx, byte) in payload.iter().copied().enumerate() {
        let shift = idx * 7;
        if shift >= 64 {
            return Err(StatusError::VarintOverflow);
        }
        value |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok((value, idx + 1));
        }
    }
    Err(StatusError::Truncated)
}

fn read_u32(payload: &[u8], offset: &mut usize) -> Result<u32, StatusError> {
    if payload.len() - *offset < 4 {
        return Err(StatusError::Truncated);
    }
    let value = u32::from_be_bytes([
        payload[*offset],
        payload[*offset + 1],
        payload[*offset + 2],
        payload[*offset + 3],
    ]);
    *offset += 4;
    Ok(value)
}

/// STATUS 编解码错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusError {
    UnknownCode(u64),
    Truncated,
    VarintOverflow,
    InvalidUtf8,
    LengthOverflow,
}

impl fmt::Display for StatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCode(code) => write!(f, "unknown status code: {code}"),
            Self::Truncated => write!(f, "truncated status payload"),
            Self::VarintOverflow => write!(f, "status varint overflow"),
            Self::InvalidUtf8 => write!(f, "status message is not valid UTF-8"),
            Self::LengthOverflow => write!(f, "status payload field too large"),
        }
    }
}

impl std::error::Error for StatusError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ALL_ERROR_CODES;

    #[test]
    fn all_grpc_codes_roundtrip_by_value() {
        for code in ALL_ERROR_CODES {
            assert_eq!(ErrorCode::try_from(code.as_u8() as u64), Ok(code));
        }
        assert_eq!(ALL_ERROR_CODES.len(), 17);
    }

    #[test]
    fn status_payload_roundtrips_with_details() {
        let status = Status::with_details(ErrorCode::Internal, "boom", b"detail".to_vec());

        let decoded = Status::from_payload(&status.to_payload().expect("payload")).expect("decode");

        assert_eq!(decoded, status);
    }

    #[test]
    fn ok_status_frame_uses_status_type_and_end_stream() {
        let frame = Status::ok().to_frame().expect("frame");

        assert_eq!(frame.ty, FrameType::Status);
        assert_eq!(frame.flags, flags::END_STREAM);
        assert_eq!(Status::from_payload(&frame.payload).expect("status"), Status::ok());
    }

    #[test]
    fn unknown_status_code_is_rejected() {
        let mut payload = Vec::new();
        encode_varint(99, &mut payload);
        payload.extend_from_slice(&0_u32.to_be_bytes());
        payload.extend_from_slice(&0_u32.to_be_bytes());

        assert_eq!(
            Status::from_payload(&payload),
            Err(StatusError::UnknownCode(99))
        );
    }

    #[test]
    fn truncated_payload_is_rejected() {
        assert_eq!(Status::from_payload(&[0, 0, 0]), Err(StatusError::Truncated));
    }
}
