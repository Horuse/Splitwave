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
