//! Base64, for the one thing that needs it.
//!
//! A worksheet's figures arrive already base64 — they are text in the `.nomo`
//! file and `resource.rs` hands them to a `data:` URI unchanged, which is why
//! this crate has never needed an encoder. An embedded font is the other
//! direction: the caller reads a `.woff2` and the document has to carry it as
//! text.
//!
//! Written here rather than taken as a dependency because it is twenty lines of
//! integer arithmetic with no edge cases beyond the padding, and because a
//! crate in `nomo-core` has to satisfy the determinism guard — no host math, no
//! I/O — which is a stronger claim than most crates make about themselves.

/// The standard alphabet, RFC 4648 §4: the one a `data:` URI expects.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes as base64 with padding.
pub fn encode(bytes: &[u8]) -> String {
    // Four output characters for every three input bytes, rounded up.
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        // The chunk as one 24-bit number, short chunks padded with zero bits.
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;

        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        // A two-byte chunk encodes to three characters and one `=`; a one-byte
        // chunk to two characters and two. The padding is what tells a decoder
        // how many of the trailing zero bits were never data.
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn the_rfc_4648_test_vectors() {
        // Section 10, which exists precisely because the padding is where an
        // encoder written from the description goes wrong.
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(input.as_bytes()), expected, "for {input:?}");
        }
    }

    #[test]
    fn every_byte_value_round_trips_through_the_alphabet() {
        // A font is arbitrary binary, so the high bytes matter as much as the
        // printable ones.
        let all: Vec<u8> = (0..=255u8).collect();
        let encoded = encode(&all);
        assert_eq!(encoded.len(), 344, "256 bytes is 86 chunks: {encoded}");
        assert!(
            encoded
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'/' || c == b'='),
            "encoded to something a data: URI cannot carry: {encoded}"
        );
        assert!(encoded.ends_with('='), "256 is not a multiple of three");
    }
}
