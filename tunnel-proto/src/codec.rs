use crate::types::*;
use bytes::{BufMut, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct Codec;

impl Codec {
    pub fn encode(frame: &Frame) -> Result<BytesMut> {
        let payload_len = frame.payload.len() as u32;
        if payload_len > MAX_FRAME_PAYLOAD {
            return Err(ProtocolError::PayloadTooLarge(payload_len));
        }

        let mut buf = BytesMut::with_capacity(10 + frame.payload.len());
        buf.put_u8(PROTOCOL_VERSION);
        buf.put_u32(frame.stream_id);
        buf.put_u8(frame.msg_type as u8);
        buf.put_u32(payload_len);
        buf.extend_from_slice(&frame.payload);
        Ok(buf)
    }

    pub async fn decode<R: AsyncReadExt + Unpin>(
        reader: &mut R,
    ) -> Result<Option<Frame>> {
        let mut header = [0u8; 10];
        match reader.read_exact(&mut header).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(ProtocolError::Io(e)),
        }

        let version = header[0];
        if version != PROTOCOL_VERSION {
            return Err(ProtocolError::VersionMismatch {
                expected: PROTOCOL_VERSION,
                got: version,
            });
        }

        let stream_id = u32::from_be_bytes([header[1], header[2], header[3], header[4]]);
        let msg_type = MessageType::try_from(header[5])?;
        let payload_len = u32::from_be_bytes([header[6], header[7], header[8], header[9]]);

        if payload_len > MAX_FRAME_PAYLOAD {
            return Err(ProtocolError::PayloadTooLarge(payload_len));
        }

        let mut payload = BytesMut::with_capacity(payload_len as usize);
        payload.resize(payload_len as usize, 0);
        reader.read_exact(&mut payload).await?;

        Ok(Some(Frame {
            stream_id,
            msg_type,
            payload: payload.freeze(),
        }))
    }

    pub async fn encode_and_write<W: AsyncWriteExt + Unpin>(
        writer: &mut W,
        frame: &Frame,
    ) -> Result<()> {
        let buf = Self::encode(frame)?;
        writer.write_all(&buf).await?;
        Ok(())
    }
}
