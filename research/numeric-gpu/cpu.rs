//! CPU reference and common framing for the GPU experiment. The original codec stays frozen.
use anyhow::{ensure, Result};
use rayon::prelude::*;
use xxhash_rust::xxh64::xxh64;

pub const CHUNK: usize = 1048576;
pub const STRIDES: [usize; 7] = [1, 2, 4, 8, 16, 32, 64];
fn residual(data: &[u8], i: usize, stride: usize) -> u16 {
    let word = |j| u16::from_le_bytes([data[j * 2], data[j * 2 + 1]]);
    let d = word(i).wrapping_sub(if i >= stride { word(i - stride) } else { 0 }) as i16;
    ((d as u16) << 1) ^ ((d >> 15) as u16)
}
pub fn pack(data: &[u8], stride: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for start in (0..data.len() / 2).step_by(32) {
        let n = (data.len() / 2 - start).min(32);
        let mut values = [0u16; 32];
        let mut mask = 0;
        for j in 0..n { values[j] = residual(data, start + j, stride); mask |= values[j]; }
        let bits = 16 - mask.leading_zeros();
        out.push(bits as u8);
        let (mut value, mut used) = (0u64, 0u32);
        for v in &values[..n] {
            value |= u64::from(*v) << used; used += bits;
            while used >= 8 { out.push(value as u8); value >>= 8; used -= 8; }
        }
        if used != 0 { out.push(value as u8); }
    }
    if data.len() % 2 != 0 { out.push(*data.last().unwrap()); }
    out
}
/// Best sampled numeric stride, even when Zstd would be preferable. This is a forced-codec benchmark.
pub fn select(data: &[u8]) -> usize {
    if data.len() < 2 { return 1; }
    let width = data.len().min(4096);
    let offsets = [0, ((data.len() - width) / 2) & !1, (data.len() - width) & !1];
    *STRIDES.iter().min_by_key(|&&stride| offsets.iter().map(|&at| pack(&data[at..at + width], stride).len()).sum::<usize>()).unwrap()
}
pub fn frame(data: &[u8], stride: usize, body: Vec<u8>) -> Vec<u8> {
    let raw = body.len() >= data.len();
    let mut out = b"LTEX0001".to_vec();
    out.extend_from_slice(&[if raw { 0 } else { 2 }, if raw { 0 } else { stride as u8 }]);
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(&xxh64(data, 0).to_le_bytes());
    out.extend_from_slice(if raw { data } else { &body }); out
}
pub fn encode(data: &[u8], stride: usize) -> Result<Vec<u8>> {
    ensure!(data.len() <= 512 * CHUNK && STRIDES.contains(&stride), "Research limits");
    let chunks: Vec<_> = data.par_chunks(CHUNK).map(|part| {
        let packed = pack(part, stride);
        if packed.len() < part.len() { (packed, false) } else { (part.to_vec(), true) }
    }).collect();
    let mut body = Vec::new();
    for (part, raw) in chunks {
        body.extend_from_slice(&((part.len() as u32) | if raw { 1 << 31 } else { 0 }).to_le_bytes());
        body.extend(part);
    }
    Ok(frame(data, stride, body))
}
