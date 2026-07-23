//! qRPC 8 字节帧头编解码。

use super::types::{flags, Frame, FrameError, FrameType, HEADER_LEN, MAX_DEFAULT_PAYLOAD_LEN};

/// 编码一个完整帧。
pub fn encode_frame(frame: &Frame) -> Result<Vec<u8>, FrameError> {
    validate_flags(frame.flags)?;

    let len = frame.payload.len();
    if len > MAX_DEFAULT_PAYLOAD_LEN {
        return Err(FrameError::PayloadTooLarge {
            len,
            max: MAX_DEFAULT_PAYLOAD_LEN,
        });
    }

    let mut out = Vec::with_capacity(HEADER_LEN + len);
    out.push(frame.ty.as_wire());
    out.push(frame.flags);
    out.extend_from_slice(&(len as u32).to_be_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&frame.payload);
    Ok(out)
}

/// 从缓冲区解码一个完整帧。
///
/// 数据不足时不会消耗缓冲区。可忽略未知帧会被消费并返回 `Ok(None)`。
pub fn decode_frame(buf: &mut Vec<u8>) -> Result<Option<Frame>, FrameError> {
    if buf.len() < HEADER_LEN {
        return Ok(None);
    }

    let ty_raw = buf[0];
    let flags = buf[1];
    let len = u32::from_be_bytes([buf[2], buf[3], buf[4], buf[5]]);
    let len = usize::try_from(len).map_err(|_| FrameError::LengthOverflow(len))?;

    if len > MAX_DEFAULT_PAYLOAD_LEN {
        return Err(FrameError::PayloadTooLarge {
            len,
            max: MAX_DEFAULT_PAYLOAD_LEN,
        });
    }

    let frame_len = HEADER_LEN + len;
    if buf.len() < frame_len {
        return Ok(None);
    }

    let ty = match FrameType::from_wire(ty_raw) {
        Ok(ty) => ty,
        Err(FrameError::IgnorableUnknownType(_)) => {
            buf.drain(..frame_len);
            return Ok(None);
        }
        Err(err) => return Err(err),
    };

    validate_flags(flags)?;

    let payload = buf[HEADER_LEN..frame_len].to_vec();
    buf.drain(..frame_len);
    Ok(Some(Frame { ty, flags, payload }))
}

fn validate_flags(flags: u8) -> Result<(), FrameError> {
    if flags & flags::COMPRESSED != 0 {
        return Err(FrameError::UnsupportedCompressedFlag);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::flags::{END_METADATA, END_STREAM};

    #[test]
    fn encode_metadata_frame_matches_golden_sample() {
        let frame = Frame::new(FrameType::Metadata, END_METADATA, b"hello".to_vec());

        let encoded = encode_frame(&frame).expect("encode");

        assert_eq!(
            encoded,
            vec![0x01, 0x04, 0, 0, 0, 5, 0, 0, b'h', b'e', b'l', b'l', b'o']
        );
    }

    #[test]
    fn decode_metadata_frame_matches_golden_sample() {
        let mut buf = vec![0x01, 0x04, 0, 0, 0, 5, 0, 0, b'h', b'e', b'l', b'l', b'o'];

        let frame = decode_frame(&mut buf).expect("decode").expect("frame");

        assert!(buf.is_empty());
        assert_eq!(frame.ty, FrameType::Metadata);
        assert_eq!(frame.flags, END_METADATA);
        assert_eq!(frame.payload, b"hello");
    }

    #[test]
    fn empty_payload_roundtrips() {
        let frame = Frame::new(FrameType::Status, END_STREAM, Vec::new());
        let mut encoded = frame.encode().expect("encode");

        let decoded = Frame::decode(&mut encoded).expect("decode").expect("frame");

        assert_eq!(decoded, frame);
        assert!(encoded.is_empty());
    }

    #[test]
    fn decode_waits_for_full_header() {
        let mut buf = vec![0x01, 0x04, 0, 0];

        assert_eq!(decode_frame(&mut buf).expect("decode"), None);
        assert_eq!(buf, vec![0x01, 0x04, 0, 0]);
    }

    #[test]
    fn decode_waits_for_full_payload() {
        let mut buf = vec![0x02, 0, 0, 0, 0, 3, 0, 0, b'a'];

        assert_eq!(decode_frame(&mut buf).expect("decode"), None);
        assert_eq!(buf, vec![0x02, 0, 0, 0, 0, 3, 0, 0, b'a']);
    }

    #[test]
    fn oversize_payload_is_rejected_on_encode() {
        let frame = Frame::new(FrameType::Data, 0, vec![0; MAX_DEFAULT_PAYLOAD_LEN + 1]);

        assert!(matches!(
            encode_frame(&frame),
            Err(FrameError::PayloadTooLarge { len, max })
                if len == MAX_DEFAULT_PAYLOAD_LEN + 1 && max == MAX_DEFAULT_PAYLOAD_LEN
        ));
    }

    #[test]
    fn oversize_payload_is_rejected_on_decode_before_allocation() {
        let mut buf = vec![0x02, 0, 0, 0x40, 0, 1, 0, 0];

        assert!(matches!(
            decode_frame(&mut buf),
            Err(FrameError::PayloadTooLarge { len, max })
                if len == MAX_DEFAULT_PAYLOAD_LEN + 1 && max == MAX_DEFAULT_PAYLOAD_LEN
        ));
    }

    #[test]
    fn unknown_critical_type_is_rejected() {
        let mut buf = vec![0x07, 0, 0, 0, 0, 0, 0, 0];

        assert_eq!(decode_frame(&mut buf), Err(FrameError::UnknownFrameType(0x07)));
    }

    #[test]
    fn unknown_ignorable_type_is_consumed() {
        let mut buf = vec![0x80, 0, 0, 0, 0, 1, 0, 0, b'x'];

        assert_eq!(decode_frame(&mut buf).expect("decode"), None);
        assert!(buf.is_empty());
    }

    #[test]
    fn compressed_flag_is_rejected_in_0_1() {
        let frame = Frame::new(FrameType::Data, flags::COMPRESSED, b"x".to_vec());

        assert_eq!(
            encode_frame(&frame),
            Err(FrameError::UnsupportedCompressedFlag)
        );
    }
}
