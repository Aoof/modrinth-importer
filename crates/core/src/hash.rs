use crate::Result;
use sha1::Sha1;
use sha2::{Digest, Sha512};
use std::io::Read;
use std::path::Path;

pub fn sha1_hex(data: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(data);
    hex::encode(h.finalize())
}

pub fn sha512_hex(data: &[u8]) -> String {
    let mut h = Sha512::new();
    h.update(data);
    hex::encode(h.finalize())
}

pub fn sha1_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha1::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// CurseForge fingerprint: MurmurHash2 (seed 1) over the file bytes with
/// all whitespace bytes (9, 10, 13, 32) removed.
pub fn curseforge_fingerprint(data: &[u8]) -> u32 {
    let filtered: Vec<u8> = data
        .iter()
        .copied()
        .filter(|b| !matches!(b, 9 | 10 | 13 | 32))
        .collect();
    murmur2(&filtered, 1)
}

fn murmur2(data: &[u8], seed: u32) -> u32 {
    const M: u32 = 0x5bd1_e995;
    let len = data.len() as u32;
    let mut h: u32 = seed ^ len;
    let (chunks, rem) = data.as_chunks::<4>();
    for c in chunks {
        let mut k = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        k = k.wrapping_mul(M);
        k ^= k >> 24;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M) ^ k;
    }
    if !rem.is_empty() {
        if rem.len() >= 3 {
            h ^= (rem[2] as u32) << 16;
        }
        if rem.len() >= 2 {
            h ^= (rem[1] as u32) << 8;
        }
        h ^= rem[0] as u32;
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sha1_known() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }
    #[test]
    fn fingerprint_ignores_whitespace() {
        assert_eq!(
            curseforge_fingerprint(b"a b\nc"),
            curseforge_fingerprint(b"abc")
        );
    }
}
