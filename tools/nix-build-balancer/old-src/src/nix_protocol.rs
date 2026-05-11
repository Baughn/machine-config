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
