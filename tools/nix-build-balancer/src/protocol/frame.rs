use std::io::{self, Read, Write};

use bincode::config::Configuration;
use bincode::{Decode, Encode};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Maximum body length permitted in a single frame.
///
/// Decoders error out and close the connection before reading the body when
/// the declared length exceeds this cap.
pub const MAX_BODY_LEN: u32 = 1 << 20;

pub fn bincode_config() -> Configuration {
    bincode::config::standard()
}

/// One length-prefixed binary frame: `u16 op_id` + `u32 length` + body.
///
/// Encodings on the wire are little-endian. The body is the raw bytes of a
/// bincode-encoded value; see [`Frame::with_body`] / [`Frame::decode_body`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub op_id: u16,
    pub body: Vec<u8>,
}

impl Frame {
    pub fn empty(op_id: u16) -> Self {
        Self {
            op_id,
            body: Vec::new(),
        }
    }

    pub fn with_body<T: Encode>(op_id: u16, value: &T) -> io::Result<Self> {
        let body = bincode::encode_to_vec(value, bincode_config()).map_err(io::Error::other)?;
        if body.len() as u64 > MAX_BODY_LEN as u64 {
            return Err(too_large(body.len() as u64));
        }
        Ok(Self { op_id, body })
    }

    pub fn decode_body<T: Decode<()>>(&self) -> io::Result<T> {
        bincode::decode_from_slice(&self.body, bincode_config())
            .map(|(value, _)| value)
            .map_err(io::Error::other)
    }
}

fn too_large(len: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "frame body {len} bytes exceeds {} MiB cap",
            MAX_BODY_LEN >> 20
        ),
    )
}

pub fn write_frame_sync<W: Write>(writer: &mut W, frame: &Frame) -> io::Result<()> {
    if frame.body.len() as u64 > MAX_BODY_LEN as u64 {
        return Err(too_large(frame.body.len() as u64));
    }
    writer.write_all(&frame.op_id.to_le_bytes())?;
    writer.write_all(&(frame.body.len() as u32).to_le_bytes())?;
    writer.write_all(&frame.body)?;
    Ok(())
}

pub fn read_frame_sync<R: Read>(reader: &mut R) -> io::Result<Frame> {
    let mut header = [0u8; 6];
    reader.read_exact(&mut header)?;
    let op_id = u16::from_le_bytes([header[0], header[1]]);
    let length = u32::from_le_bytes([header[2], header[3], header[4], header[5]]);
    if length > MAX_BODY_LEN {
        return Err(too_large(length as u64));
    }
    let mut body = vec![0u8; length as usize];
    reader.read_exact(&mut body)?;
    Ok(Frame { op_id, body })
}

pub async fn write_frame_async<W>(writer: &mut W, frame: &Frame) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    if frame.body.len() as u64 > MAX_BODY_LEN as u64 {
        return Err(too_large(frame.body.len() as u64));
    }
    writer.write_all(&frame.op_id.to_le_bytes()).await?;
    writer
        .write_all(&(frame.body.len() as u32).to_le_bytes())
        .await?;
    writer.write_all(&frame.body).await?;
    Ok(())
}

pub async fn read_frame_async<R>(reader: &mut R) -> io::Result<Frame>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0u8; 6];
    reader.read_exact(&mut header).await?;
    let op_id = u16::from_le_bytes([header[0], header[1]]);
    let length = u32::from_le_bytes([header[2], header[3], header[4], header[5]]);
    if length > MAX_BODY_LEN {
        return Err(too_large(length as u64));
    }
    let mut body = vec![0u8; length as usize];
    reader.read_exact(&mut body).await?;
    Ok(Frame { op_id, body })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn empty_body_round_trip() {
        let mut buf = Vec::new();
        write_frame_sync(&mut buf, &Frame::empty(7)).unwrap();
        assert_eq!(buf, vec![7, 0, 0, 0, 0, 0]);
        let decoded = read_frame_sync(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(decoded, Frame::empty(7));
    }

    #[test]
    fn body_round_trip_with_op_id_alignment() {
        let mut original = Frame {
            op_id: 0xBEEF,
            body: vec![1, 2, 3, 4, 5],
        };
        // Encoded form:
        //   op_id LE: ef be
        //   length LE: 05 00 00 00
        //   body: 01 02 03 04 05
        let mut buf = Vec::new();
        write_frame_sync(&mut buf, &original).unwrap();
        assert_eq!(buf, vec![0xef, 0xbe, 0x05, 0x00, 0x00, 0x00, 1, 2, 3, 4, 5]);
        let decoded = read_frame_sync(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(decoded, original);
        original.body.clear();
    }

    #[test]
    fn rejects_body_exceeding_cap_on_encode() {
        let frame = Frame {
            op_id: 1,
            body: vec![0u8; MAX_BODY_LEN as usize + 1],
        };
        let mut buf = Vec::new();
        let err = write_frame_sync(&mut buf, &frame).expect_err("must reject");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(
            buf.is_empty(),
            "no bytes should be written on cap violation"
        );
    }

    #[test]
    fn rejects_length_exceeding_cap_on_decode_without_reading_body() {
        // Header declares 2 MiB body; we provide no body bytes. Decoder must
        // error after the 6-byte header read, never attempting to read the
        // body.
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&(2 * 1024 * 1024u32).to_le_bytes());
        let mut cursor = Cursor::new(&buf);
        let err = read_frame_sync(&mut cursor).expect_err("must reject");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(cursor.position(), 6, "header read but body never attempted");
    }

    #[test]
    fn truncated_body_is_unexpected_eof() {
        // Declare 4-byte body but only provide 2.
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&4u32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 2]);
        let err = read_frame_sync(&mut Cursor::new(&buf)).expect_err("must fail");
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn async_round_trip_over_duplex() {
        let (mut a, mut b) = tokio::io::duplex(64);
        let outgoing = Frame {
            op_id: 42,
            body: b"hello".to_vec(),
        };
        write_frame_async(&mut a, &outgoing).await.unwrap();
        let received = read_frame_async(&mut b).await.unwrap();
        assert_eq!(outgoing, received);
    }

    #[tokio::test]
    async fn async_rejects_oversize_length_without_body_read() {
        let (mut a, mut b) = tokio::io::duplex(64);
        let mut header = Vec::new();
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&(2 * 1024 * 1024u32).to_le_bytes());
        a.write_all(&header).await.unwrap();
        drop(a);
        let err = read_frame_async(&mut b).await.expect_err("must reject");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn with_body_encodes_bincode_and_decode_body_recovers() {
        #[derive(bincode::Encode, bincode::Decode, PartialEq, Debug)]
        struct Sample {
            n: u64,
            label: String,
        }

        let sample = Sample {
            n: 42,
            label: "x".to_string(),
        };
        let frame = Frame::with_body(99, &sample).unwrap();
        assert_eq!(frame.op_id, 99);
        let decoded: Sample = frame.decode_body().unwrap();
        assert_eq!(decoded, sample);
    }
}
