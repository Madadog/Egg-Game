//! Minimal standard base64 (RFC 4648, with padding) — the binary leg of the
//! web persistence route. `localStorage` holds JS strings, so binary asset
//! bytes (image-layer PNGs, or any future binary write) ride through
//! [`encode`]/[`decode`] under their own key prefix (see
//! `fantasy_console::ASSET_OVERRIDE_B64_PREFIX`). Hand-written rather than a
//! crate: ~60 lines, no dependency, and it compiles (and is unit-tested) on
//! every target even though only the wasm build calls it.

// Only the web persistence path calls these; keep the native build warning-free
// without cfg-gating the module itself (so `cargo test` covers it natively).
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode `bytes` as standard padded base64.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let sextet = |shift: u32| ALPHABET[((n >> shift) & 0x3f) as usize] as char;
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if chunk.len() > 1 { sextet(6) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(0) } else { '=' });
    }
    out
}

/// Decode standard padded base64. `None` on any malformed input — a corrupt
/// persisted entry must read as "no override", never as garbage bytes.
pub fn decode(s: &str) -> Option<Vec<u8>> {
    let s = s.as_bytes();
    if !s.len().is_multiple_of(4) {
        return None;
    }
    let sextet = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    for (i, chunk) in s.chunks(4).enumerate() {
        let last = (i + 1) * 4 == s.len();
        // Padding is only legal as the last one or two characters of the input.
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        if pad > 0 && (!last || chunk[..4 - pad].contains(&b'=')) {
            return None;
        }
        let n = chunk[..4 - pad]
            .iter()
            .try_fold(0u32, |n, &c| Some((n << 6) | sextet(c)?))?
            << (6 * pad as u32);
        match pad {
            0 => out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]),
            1 => out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8]),
            2 => out.push((n >> 16) as u8),
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The RFC 4648 test vectors, both directions.
    #[test]
    fn rfc4648_vectors() {
        let vectors: &[(&str, &str)] = &[
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (plain, encoded) in vectors {
            assert_eq!(encode(plain.as_bytes()), *encoded, "encode {plain:?}");
            assert_eq!(
                decode(encoded).as_deref(),
                Some(plain.as_bytes()),
                "decode {encoded:?}"
            );
        }
    }

    /// Every byte value round-trips — the property the PNG route depends on
    /// (image bytes are arbitrary, including NUL and invalid UTF-8).
    #[test]
    fn all_bytes_round_trip() {
        let all: Vec<u8> = (0..=255u8).collect();
        assert_eq!(decode(&encode(&all)).as_deref(), Some(all.as_slice()));
        // And at every remainder-length alignment.
        for len in 0..7 {
            let bytes = &all[..len];
            assert_eq!(decode(&encode(bytes)).as_deref(), Some(bytes));
        }
    }

    /// Malformed input reads as `None`, not garbage: bad length, bad character,
    /// interior or excess padding.
    #[test]
    fn malformed_input_is_rejected() {
        for bad in ["Zg=", "Zg", "Zm9v!A==", "Z===", "Zg==Zg==x", "=Zg=", "Z=g="] {
            assert_eq!(decode(bad), None, "{bad:?} rejected");
        }
        // Padding mid-stream (a valid-looking chunk before more data) is refused.
        assert_eq!(decode("Zg==Zm9v"), None);
    }
}
