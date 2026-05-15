pub mod agent;
pub mod controller;
pub mod estimator;
pub mod hook;
pub mod inflight;
pub mod nix_protocol;
pub mod persistence;
pub mod protocol;
pub mod scheduler;
pub mod spool;
pub mod telemetry;
pub mod util;

/// SHA-256 of the crate's source tree, computed at build time by `build.rs`.
///
/// Used as the wire-protocol handshake: peers compare bytewise on every
/// connection and close on mismatch. Replaces version negotiation because
/// both ends ship together through the flake.
pub const SOURCE_HASH_HEX: &str = env!("NBB_SOURCE_HASH");

/// The 32-byte digest behind [`SOURCE_HASH_HEX`].
pub const SOURCE_HASH: [u8; 32] = decode_source_hash();

const fn decode_source_hash() -> [u8; 32] {
    let bytes = SOURCE_HASH_HEX.as_bytes();
    if bytes.len() != 64 {
        panic!("NBB_SOURCE_HASH must be exactly 64 hex characters");
    }
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = (hex_nibble(bytes[2 * i]) << 4) | hex_nibble(bytes[2 * i + 1]);
        i += 1;
    }
    out
}

const fn hex_nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => panic!("NBB_SOURCE_HASH contains a non-lowercase-hex character"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_hash_is_thirty_two_bytes() {
        assert_eq!(SOURCE_HASH.len(), 32);
    }

    #[test]
    fn source_hash_hex_is_sixty_four_chars() {
        assert_eq!(SOURCE_HASH_HEX.len(), 64);
        assert!(SOURCE_HASH_HEX
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    }

    #[test]
    fn source_hash_is_not_all_zero() {
        assert_ne!(SOURCE_HASH, [0u8; 32]);
    }
}
