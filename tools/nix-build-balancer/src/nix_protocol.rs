use std::io::{self, Read, Write};

/// Read a little-endian 64-bit integer from the Nix hook protocol.
pub fn read_nix_u64<R: Read>(reader: &mut R) -> io::Result<u64> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

/// Write a little-endian 64-bit integer to the Nix hook protocol.
pub fn write_nix_u64<W: Write>(writer: &mut W, value: u64) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

/// Read a Nix protocol string and consume its 8-byte alignment padding.
pub fn read_nix_string<R: Read>(reader: &mut R) -> io::Result<String> {
    let len = read_nix_u64(reader)? as usize;
    let mut bytes = vec![0u8; len];
    reader.read_exact(&mut bytes)?;
    read_nix_padding(reader, len)?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

/// Write a Nix protocol string with 8-byte alignment padding.
pub fn write_nix_string<W: Write>(writer: &mut W, value: &str) -> io::Result<()> {
    let bytes = value.as_bytes();
    write_nix_u64(writer, bytes.len() as u64)?;
    writer.write_all(bytes)?;
    write_nix_padding(writer, bytes.len())
}

/// Read a counted list of Nix protocol strings.
pub fn read_nix_strings<R: Read>(reader: &mut R) -> io::Result<Vec<String>> {
    let count = read_nix_u64(reader)?;
    let mut values = Vec::new();
    for _ in 0..count {
        values.push(read_nix_string(reader)?);
    }
    Ok(values)
}

/// Write a counted list of Nix protocol strings.
pub fn write_nix_strings<W: Write>(writer: &mut W, values: &[String]) -> io::Result<()> {
    write_nix_u64(writer, values.len() as u64)?;
    for value in values {
        write_nix_string(writer, value)?;
    }
    Ok(())
}

fn read_nix_padding<R: Read>(reader: &mut R, len: usize) -> io::Result<()> {
    let padding = padding_len(len);
    if padding > 0 {
        let mut buf = vec![0u8; padding];
        reader.read_exact(&mut buf)?;
        if buf.iter().any(|byte| *byte != 0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "non-zero nix protocol padding",
            ));
        }
    }
    Ok(())
}

fn write_nix_padding<W: Write>(writer: &mut W, len: usize) -> io::Result<()> {
    let padding = padding_len(len);
    if padding > 0 {
        writer.write_all(&vec![0u8; padding])?;
    }
    Ok(())
}

fn padding_len(len: usize) -> usize {
    if len.is_multiple_of(8) {
        0
    } else {
        8 - (len % 8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn roundtrip_string(value: &str) {
        let mut buf = Vec::new();
        write_nix_string(&mut buf, value).unwrap();
        // 8 bytes for the length, then bytes, then padding to 8-byte boundary.
        let expected_total = 8 + value.len().div_ceil(8) * 8;
        assert_eq!(buf.len(), expected_total, "padding wrong for {value:?}");

        let mut cursor = Cursor::new(&buf);
        let decoded = read_nix_string(&mut cursor).unwrap();
        assert_eq!(decoded, value);
        assert_eq!(cursor.position() as usize, buf.len(), "trailing bytes");
    }

    #[test]
    fn string_roundtrip_padding_edges() {
        // Spec calls out 0, 7, 8, 9 byte strings specifically.
        roundtrip_string("");
        roundtrip_string("a");
        roundtrip_string("abcdefg");
        roundtrip_string("abcdefgh");
        roundtrip_string("abcdefghi");
    }

    #[test]
    fn strings_list_roundtrip() {
        let values = vec![
            String::new(),
            "x".to_string(),
            "longer string with padding".to_string(),
        ];
        let mut buf = Vec::new();
        write_nix_strings(&mut buf, &values).unwrap();
        let mut cursor = Cursor::new(&buf);
        let decoded = read_nix_strings(&mut cursor).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn rejects_non_zero_padding() {
        // len=1, byte='x', then padding that is non-zero.
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u64.to_le_bytes());
        buf.push(b'x');
        buf.extend_from_slice(&[1u8; 7]);
        let mut cursor = Cursor::new(&buf);
        let err = read_nix_string(&mut cursor).expect_err("non-zero padding must fail");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn u64_roundtrip() {
        let mut buf = Vec::new();
        write_nix_u64(&mut buf, 0x0123_4567_89ab_cdef).unwrap();
        let mut cursor = Cursor::new(&buf);
        assert_eq!(read_nix_u64(&mut cursor).unwrap(), 0x0123_4567_89ab_cdef);
    }
}
