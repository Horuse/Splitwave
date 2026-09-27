use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use flate2::{read::DeflateDecoder, write::DeflateEncoder, Compression};
use std::io::{Read, Write};

use crate::error::{AppError, AppResult};

// Raw deflate (no zlib/gzip header) at best compression -- SDP is ~70% compressible text.
// Base64url without padding: shorter than standard base64, URL-safe for copy/paste.
// Typical SDP for DataChannel-only: ~950 bytes raw -> ~350 compressed -> ~470 chars.

pub fn encode_sdp(sdp: &str) -> AppResult<String> {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
    enc.write_all(sdp.as_bytes())
        .map_err(|e| AppError::Stream(format!("sdp compress: {e}")))?;
    let compressed = enc
        .finish()
        .map_err(|e| AppError::Stream(format!("sdp compress finish: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(&compressed))
}

pub fn decode_sdp(code: &str) -> AppResult<String> {
    let compressed = URL_SAFE_NO_PAD
        .decode(code.trim())
        .map_err(|e| AppError::Stream(format!("sdp decode base64: {e}")))?;
    let mut dec = DeflateDecoder::new(&compressed[..]);
    let mut sdp = String::new();
    dec.read_to_string(&mut sdp)
        .map_err(|e| AppError::Stream(format!("sdp decompress: {e}")))?;
    Ok(sdp)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SDP: &str = "v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=candidate:host 1 udp 2130706431 127.0.0.1 50000 typ host\r\n";

    #[test]
    fn sdp_code_roundtrip_is_lossless() {
        let code = encode_sdp(SAMPLE_SDP).expect("encode");
        assert!(!code.contains('+'), "base64url alphabet");
        assert!(!code.contains('/'));
        assert!(!code.contains('='), "no padding");
        let back = decode_sdp(&code).expect("decode");
        assert_eq!(back, SAMPLE_SDP);
    }

    #[test]
    fn repeated_encoding_is_deterministic() {
        let a = encode_sdp(SAMPLE_SDP).unwrap();
        let b = encode_sdp(SAMPLE_SDP).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn sdp_compresses_substantially() {
        // A realistic SDP is ~950 bytes; deflate should roughly halve it
        // even for repetitive text.
        let long = SAMPLE_SDP.repeat(10);
        let code = encode_sdp(&long).unwrap();
        assert!(
            code.len() < long.len() / 2,
            "compressed {} vs raw {}",
            code.len(),
            long.len()
        );
        assert_eq!(decode_sdp(&code).unwrap(), long);
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode_sdp("!!!not-base64!!!").is_err());
        assert!(decode_sdp("aGVsbG8").is_err(), "invalid deflate stream");
        // Whitespace around the code is tolerated (copy-paste).
        let code = encode_sdp(SAMPLE_SDP).unwrap();
        assert_eq!(decode_sdp(&format!("  {code} ")).unwrap(), SAMPLE_SDP);
    }

    #[test]
    fn empty_sdp_roundtrips() {
        let code = encode_sdp("").unwrap();
        assert_eq!(decode_sdp(&code).unwrap(), "");
    }
}
