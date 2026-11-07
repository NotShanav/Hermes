//! Encrypted record envelope. Everything that identifies *what* happened lives in here, inside
//! the HPKE ciphertext: event kind, session id, body — and padding that hides the body length.
//!
//! plaintext = env_ver(1)=1 | kind(u16 LE) | session_id(u64 LE) | body_len(u32 LE) | body | 0x00 padding
//! total length is rounded up to a multiple of PAD_BLOCK (256) bytes.

use crate::error::{CryptoError, Result};
use zeroize::Zeroizing;

pub const ENVELOPE_VERSION: u8 = 1;
pub const HEADER_LEN: usize = 1 + 2 + 8 + 4;
pub const PAD_BLOCK: usize = 256;
pub const MAX_BODY: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub kind: u16,
    pub session_id: u64,
    pub body: Vec<u8>,
}

pub fn padded_len(body_len: usize) -> usize {
    (HEADER_LEN + body_len).div_ceil(PAD_BLOCK) * PAD_BLOCK
}

pub fn encode(kind: u16, session_id: u64, body: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if body.len() > MAX_BODY {
        return Err(CryptoError::RecordTooLarge);
    }
    let mut v = Zeroizing::new(Vec::with_capacity(padded_len(body.len())));
    v.push(ENVELOPE_VERSION);
    v.extend_from_slice(&kind.to_le_bytes());
    v.extend_from_slice(&session_id.to_le_bytes());
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(body);
    v.resize(padded_len(body.len()), 0);
    Ok(v)
}

pub fn decode(plain: &[u8]) -> Result<Record> {
    if plain.len() < HEADER_LEN || plain.len() % PAD_BLOCK != 0 {
        return Err(CryptoError::MalformedRecord);
    }
    if plain[0] != ENVELOPE_VERSION {
        return Err(CryptoError::UnsupportedVersion(plain[0]));
    }
    let kind = u16::from_le_bytes(plain[1..3].try_into().unwrap());
    let session_id = u64::from_le_bytes(plain[3..11].try_into().unwrap());
    let len = u32::from_le_bytes(plain[11..15].try_into().unwrap()) as usize;
    if len > MAX_BODY || HEADER_LEN + len > plain.len() || padded_len(len) != plain.len() {
        return Err(CryptoError::MalformedRecord);
    }
    if plain[HEADER_LEN + len..].iter().any(|b| *b != 0) {
        return Err(CryptoError::MalformedRecord);
    }
    Ok(Record {
        kind,
        session_id,
        body: plain[HEADER_LEN..HEADER_LEN + len].to_vec(),
    })
}
