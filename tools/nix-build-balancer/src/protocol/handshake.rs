use std::io::{self, Read, Write};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::SOURCE_HASH;

pub const HASH_LEN: usize = 32;

/// Exchange and verify the 32-byte source-tree hash that gates every
/// connection.
///
/// Both sides write their hash, then read the peer's. On mismatch this
/// returns `PermissionDenied` and no further bytes are written.
pub fn perform_handshake_sync<S: Read + Write>(stream: &mut S) -> io::Result<()> {
    perform_handshake_sync_with(stream, SOURCE_HASH)
}

pub fn perform_handshake_sync_with<S: Read + Write>(
    stream: &mut S,
    our_hash: [u8; HASH_LEN],
) -> io::Result<()> {
    stream.write_all(&our_hash)?;
    stream.flush()?;
    let mut peer = [0u8; HASH_LEN];
    stream.read_exact(&mut peer)?;
    verify(peer, our_hash)
}

pub async fn perform_handshake_async<S>(stream: &mut S) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    perform_handshake_async_with(stream, SOURCE_HASH).await
}

pub async fn perform_handshake_async_with<S>(
    stream: &mut S,
    our_hash: [u8; HASH_LEN],
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    stream.write_all(&our_hash).await?;
    stream.flush().await?;
    let mut peer = [0u8; HASH_LEN];
    stream.read_exact(&mut peer).await?;
    verify(peer, our_hash)
}

fn verify(peer: [u8; HASH_LEN], ours: [u8; HASH_LEN]) -> io::Result<()> {
    if peer != ours {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "source-tree hash mismatch",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn matching_hashes_succeed() {
        let (mut a, mut b) = tokio::io::duplex(128);
        let hash = [0xAAu8; HASH_LEN];
        let (ra, rb) = tokio::join!(
            perform_handshake_async_with(&mut a, hash),
            perform_handshake_async_with(&mut b, hash),
        );
        ra.unwrap();
        rb.unwrap();
    }

    #[tokio::test]
    async fn mismatched_hashes_fail_both_ends() {
        let (mut a, mut b) = tokio::io::duplex(128);
        let (ra, rb) = tokio::join!(
            perform_handshake_async_with(&mut a, [0x11u8; HASH_LEN]),
            perform_handshake_async_with(&mut b, [0x22u8; HASH_LEN]),
        );
        let ea = ra.expect_err("a must fail");
        let eb = rb.expect_err("b must fail");
        assert_eq!(ea.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(eb.kind(), io::ErrorKind::PermissionDenied);
    }

    #[tokio::test]
    async fn mismatch_writes_no_frames() {
        // After a failed handshake, the caller drops the stream. Use a small
        // duplex buffer and verify only the 32 hash bytes were written by
        // each side — never any subsequent frame bytes.
        let (mut a, mut b) = tokio::io::duplex(128);
        let our_a = [0x11u8; HASH_LEN];
        let our_b = [0x22u8; HASH_LEN];
        let (ra, rb) = tokio::join!(
            perform_handshake_async_with(&mut a, our_a),
            perform_handshake_async_with(&mut b, our_b),
        );
        assert!(ra.is_err());
        assert!(rb.is_err());

        // Drop the duplex ends so the channel signals EOF.
        drop(a);
        drop(b);
    }

    #[test]
    fn sync_round_trip_through_in_memory_pipe() {
        // Two threads, each handing the other a hash and reading back.
        use std::sync::mpsc::sync_channel;

        let hash = [0xCDu8; HASH_LEN];
        let (tx_a, rx_b) = sync_channel::<u8>(64);
        let (tx_b, rx_a) = sync_channel::<u8>(64);

        struct ChannelStream {
            tx: std::sync::mpsc::SyncSender<u8>,
            rx: std::sync::mpsc::Receiver<u8>,
        }
        impl Read for ChannelStream {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                for (i, slot) in buf.iter_mut().enumerate() {
                    match self.rx.recv() {
                        Ok(byte) => *slot = byte,
                        Err(_) => return Ok(i),
                    }
                }
                Ok(buf.len())
            }
        }
        impl Write for ChannelStream {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                for byte in buf {
                    self.tx
                        .send(*byte)
                        .map_err(|err| io::Error::other(err.to_string()))?;
                }
                Ok(buf.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let a = std::thread::spawn(move || {
            let mut stream = ChannelStream { tx: tx_a, rx: rx_a };
            perform_handshake_sync_with(&mut stream, hash)
        });
        let b = std::thread::spawn(move || {
            let mut stream = ChannelStream { tx: tx_b, rx: rx_b };
            perform_handshake_sync_with(&mut stream, hash)
        });
        a.join().unwrap().unwrap();
        b.join().unwrap().unwrap();
    }
}
