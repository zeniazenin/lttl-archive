//! lttl numeric block codec. Derived from the frozen optimized Dense experiment.
use anyhow::{bail, ensure, Result};
use rayon::prelude::*;
use serde::Serialize;
use std::{io::{Read, Write}, time::Instant};
use xxhash_rust::xxh64::xxh64;
use std::cell::RefCell;

#[derive(Default, Clone, Serialize)]
pub struct Profile { pub chunks_wall_ms: f64, pub transform_worker_ms: f64,
    pub entropy_worker_ms: f64, pub framing_ms: f64 }
thread_local! {
    static PROFILE: RefCell<Profile> = RefCell::new(Profile::default());
    static COMPRESSOR: RefCell<Option<(i32, zstd::bulk::Compressor<'static>)>> = RefCell::new(None);
}
#[allow(dead_code)]
pub fn last_profile() -> Profile { PROFILE.with(|p| p.borrow().clone()) }

pub const MAX: usize = 512 * 1024 * 1024;
const CHUNK: usize = 1_048_560; // divisible by 3, 4, 8 and 16; packed-212 pairs never cross chunks
const MAGIC: &[u8; 8] = b"LTNV0002";
const STRIDES: [usize; 11] = [1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 64];

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct Layout {
    // 3 denotes two WFDB-212 samples in three bytes; others are bytes per sample.
    width: u8,
    big_endian: bool,
    xor: bool,
    stride: usize,
}
impl Layout {
    fn bits(self) -> u32 { if self.width == 3 { 12 } else { self.width as u32 * 8 } }
    fn mask(self) -> u64 { u64::MAX >> (64 - self.bits()) }
    fn count(self, n: usize) -> usize { if self.width == 3 { n / 3 * 2 } else { n / self.width as usize } }
    fn complete(self, n: usize) -> usize { n / self.width as usize * self.width as usize }
    fn lanes(self) -> usize { if self.width == 3 { 2 } else { self.width as usize } }
    fn valid(self) -> bool { [2, 3, 4, 8].contains(&self.width) && STRIDES.contains(&self.stride) && (self.width != 3 || !self.big_endian) }
    fn words(self, data: &[u8]) -> Vec<u64> {
        if self.width == 3 {
            return data.chunks_exact(3).flat_map(|b| [u64::from(b[0]) | (u64::from(b[1] & 15) << 8),
                u64::from(b[2]) | (u64::from(b[1] >> 4) << 8)]).collect();
        }
        data.chunks_exact(self.width as usize).map(|b| {
            let mut value = 0u64;
            if self.big_endian { for x in b { value = (value << 8) | u64::from(*x); } }
            else { for (i, x) in b.iter().enumerate() { value |= u64::from(*x) << (8 * i); } }
            value
        }).collect()
    }
    fn residuals(self, data: &[u8]) -> Vec<u64> {
        let mut words = self.words(data);
        let mask = self.mask();
        for i in (0..words.len()).rev() {
            let previous = if i >= self.stride { words[i - self.stride] } else { 0 };
            words[i] = if self.xor { words[i] ^ previous } else {
                let d = words[i].wrapping_sub(previous) & mask;
                ((d << 1) ^ 0u64.wrapping_sub(d >> (self.bits() - 1))) & mask
            };
        }
        words
    }
    fn restore(self, mut values: Vec<u64>, tail: &[u8]) -> Vec<u8> {
        let mask = self.mask();
        for i in 0..values.len() {
            let previous = if i >= self.stride { values[i - self.stride] } else { 0 };
            values[i] = if self.xor { values[i] ^ previous } else {
                let d = (values[i] >> 1) ^ 0u64.wrapping_sub(values[i] & 1);
                previous.wrapping_add(d) & mask
            };
        }
        let mut out = Vec::new();
        if self.width == 3 {
            for pair in values.chunks_exact(2) {
                out.extend_from_slice(&[pair[0] as u8, ((pair[0] >> 8) | ((pair[1] >> 8) << 4)) as u8, pair[1] as u8]);
            }
        } else {
            out.reserve(values.len() * self.width as usize + tail.len());
            for value in values {
                if self.big_endian { out.extend_from_slice(&value.to_be_bytes()[8 - self.width as usize..]); }
                else { out.extend_from_slice(&value.to_le_bytes()[..self.width as usize]); }
            }
        }
        out.extend_from_slice(tail); out
    }
}

fn packed_size(values: &[u64]) -> usize {
    values.chunks(32).map(|tile| {
        let mask = tile.iter().fold(0, |a, b| a | b);
        1 + (tile.len() * (64 - mask.leading_zeros()) as usize).div_ceil(8)
    }).sum()
}
fn pack(values: &[u64], tail: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(packed_size(values) + tail.len());
    for tile in values.chunks(32) {
        let bits = 64 - tile.iter().fold(0, |a, b| a | b).leading_zeros();
        out.push(bits as u8);
        // u128 avoids undefined/overflowing shifts for full-width 64-bit residuals.
        let (mut accumulator, mut used) = (0u128, 0u32);
        for value in tile {
            accumulator |= u128::from(*value) << used; used += bits;
            while used >= 8 { out.push(accumulator as u8); accumulator >>= 8; used -= 8; }
        }
        if used != 0 { out.push(accumulator as u8); }
    }
    out.extend_from_slice(tail); out
}
fn planes(values: &[u64], tail: &[u8], layout: Layout) -> Vec<u8> {
    let mut out = vec![0; values.len() * layout.lanes()];
    for lane in 0..layout.lanes() {
        for (i, value) in values.iter().enumerate() { out[lane * values.len() + i] = (value >> (lane * 8)) as u8; }
    }
    out.extend_from_slice(tail); out
}
// Specialize common 16-bit/packed-12 paths. Read directly from input rather than
// expanding every value into a u64 array and traversing that array repeatedly.
fn direct16<const KIND: u8, const XOR: bool, const PLANES: bool>(data: &[u8], stride: usize) -> Vec<u8> {
    let packed12 = KIND == 3;
    let count = if packed12 { data.len() / 3 * 2 } else { data.len() / 2 };
    let complete = if packed12 { data.len() / 3 * 3 } else { data.len() / 2 * 2 };
    let bits = if packed12 { 12 } else { 16 };
    let mask = if packed12 { 4095 } else { u16::MAX };
    let word = |i: usize| -> u16 {
        if packed12 {
            let pair = i / 2 * 3;
            if i % 2 == 0 { u16::from(data[pair]) | (u16::from(data[pair + 1] & 15) << 8) }
            else { u16::from(data[pair + 2]) | (u16::from(data[pair + 1] >> 4) << 8) }
        } else if KIND == 1 { u16::from_be_bytes([data[2 * i], data[2 * i + 1]]) }
        else { u16::from_le_bytes([data[2 * i], data[2 * i + 1]]) }
    };
    let residual = |i: usize| -> u16 {
        let previous = if i >= stride { word(i - stride) } else { 0 };
        if XOR { word(i) ^ previous } else {
            let d = word(i).wrapping_sub(previous) & mask;
            ((d << 1) ^ 0u16.wrapping_sub(d >> (bits - 1))) & mask
        }
    };
    let mut out = if PLANES { vec![0; count * 2] } else { Vec::with_capacity(data.len()) };
    if PLANES {
        for i in 0..count { let v = residual(i); out[i] = v as u8; out[count + i] = (v >> 8) as u8; }
    } else {
        for start in (0..count).step_by(32) {
            let n = (count - start).min(32); let mut values = [0u16; 32]; let mut mask = 0;
            for j in 0..n { values[j] = residual(start + j); mask |= values[j]; }
            let width = 16 - mask.leading_zeros(); out.push(width as u8);
            let (mut acc, mut used) = (0u64, 0u32);
            for v in &values[..n] {
                acc |= u64::from(*v) << used; used += width;
                while used >= 8 { out.push(acc as u8); acc >>= 8; used -= 8; }
            }
            if used != 0 { out.push(acc as u8); }
        }
    }
    out.extend_from_slice(&data[complete..]); out
}
fn transform16<const PLANES: bool>(data: &[u8], l: Layout) -> Vec<u8> {
    #[cfg(target_arch = "x86_64")]
    if PLANES && l.width == 2 && std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY: feature detection is required before calling the AVX2 implementation.
        return unsafe { planes16_avx2(data, l) };
    }
    match (l.width, l.big_endian, l.xor) {
        (3, _, false) => direct16::<3, false, PLANES>(data, l.stride),
        (3, _, true) => direct16::<3, true, PLANES>(data, l.stride),
        (_, false, false) => direct16::<0, false, PLANES>(data, l.stride),
        (_, false, true) => direct16::<0, true, PLANES>(data, l.stride),
        (_, true, false) => direct16::<1, false, PLANES>(data, l.stride),
        (_, true, true) => direct16::<1, true, PLANES>(data, l.stride),
    }
}
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn planes16_avx2(data: &[u8], l: Layout) -> Vec<u8> {
    use std::arch::x86_64::*;
    let n = data.len() / 2;
    let mut out = vec![0u8; data.len()];
    let word = |i: usize| if l.big_endian { u16::from_be_bytes([data[2*i],data[2*i+1]]) }
        else { u16::from_le_bytes([data[2*i],data[2*i+1]]) };
    let scalar = |i: usize| {
        let previous = if i >= l.stride { word(i-l.stride) } else { 0 };
        if l.xor { word(i)^previous } else { let d = word(i).wrapping_sub(previous) as i16; ((d as u16)<<1)^((d>>15) as u16) }
    };
    let mut i = 0;
    while i < l.stride.min(n) { let v=scalar(i); out[i]=v as u8; out[n+i]=(v>>8) as u8; i+=1; }
    // SAFETY: i >= stride, i+16 <= n. Loads cover 16 complete current and
    // previous words inside data. Stores cover 16 bytes inside each disjoint
    // n-byte output plane. All accesses are explicitly unaligned.
    unsafe {
        while i + 16 <= n {
            let mut current = _mm256_loadu_si256(data.as_ptr().add(2*i).cast());
            let mut previous = _mm256_loadu_si256(data.as_ptr().add(2*(i-l.stride)).cast());
            if l.big_endian {
                current = _mm256_or_si256(_mm256_srli_epi16(current,8),_mm256_slli_epi16(current,8));
                previous = _mm256_or_si256(_mm256_srli_epi16(previous,8),_mm256_slli_epi16(previous,8));
            }
            let v = if l.xor { _mm256_xor_si256(current,previous) } else {
                let d = _mm256_sub_epi16(current,previous);
                _mm256_xor_si256(_mm256_slli_epi16(d,1),_mm256_srai_epi16(d,15))
            };
            let low = _mm256_and_si256(v,_mm256_set1_epi16(255));
            let high = _mm256_srli_epi16(v,8);
            let packed = _mm256_permute4x64_epi64(_mm256_packus_epi16(low,high),0xd8);
            _mm_storeu_si128(out.as_mut_ptr().add(i).cast(),_mm256_castsi256_si128(packed));
            _mm_storeu_si128(out.as_mut_ptr().add(n+i).cast(),_mm256_extracti128_si256(packed,1));
            i += 16;
        }
    }
    while i < n { let v=scalar(i); out[i]=v as u8; out[n+i]=(v>>8) as u8; i+=1; }
    if data.len()%2 != 0 { out[data.len()-1]=data[data.len()-1]; }
    out
}
fn compress(data: &[u8], level: i32) -> Result<Vec<u8>> {
    COMPRESSOR.with(|cell| {
        let mut state = cell.borrow_mut();
        if state.is_none() { *state=Some((level,zstd::bulk::Compressor::new(level)?)); }
        let (previous, compressor)=state.as_mut().unwrap();
        if *previous != level { compressor.set_compression_level(level)?; *previous=level; }
        Ok(compressor.compress(data)?)
    })
}
fn decompress(data: &[u8], limit: usize) -> Result<Vec<u8>> {
    let frame = zstd::zstd_safe::find_frame_compressed_size(data).map_err(|_| anyhow::anyhow!("Invalid Zstd frame"))?;
    ensure!(frame == data.len(), "Trailing Zstd frame/data");
    let mut decoder = zstd::stream::read::Decoder::new(data)?;
    decoder.window_log_max(23)?;
    let mut out = Vec::new(); decoder.take(limit as u64 + 1).read_to_end(&mut out)?;
    ensure!(out.len() <= limit, "Inflated block limit"); Ok(out)
}

#[derive(Clone, Debug, Serialize)]
pub struct Plan { pub layout: Layout, pub stage: u8, pub level: i32, pub selection_ms: f64,
    pub sample_pack_bytes: usize, pub sample_zstd_bytes: usize, pub sample_input_bytes: usize }
// Stage 0 raw, 1 tile packed, 2 Zstd byte planes, 3 ordinary Zstd, 4 Zstd tile packed.
pub fn select(data: &[u8], dense: bool) -> Result<Plan> {
    let started = Instant::now();
    let default = Layout { width: 2, big_endian: false, xor: false, stride: 1 };
    let level = 3;
    if data.len() < 256 { return Ok(Plan { layout: default, stage: 3, level, selection_ms: 0.,
        sample_pack_bytes: 0, sample_zstd_bytes: 0, sample_input_bytes: 0 }); }
    let width = data.len().min(8192);
    // Interior regions reduce over-weighting file headers and trailers.
    let span = data.len() - width;
    let offsets = [(span / 8) / 24 * 24, (span / 2) / 24 * 24, (span * 7 / 8) / 24 * 24];
    let samples: Vec<_> = offsets.iter().map(|&at| &data[at..at + width]).collect();
    let raw_score = width * samples.len();
    let baseline: usize = samples.iter().map(|s| compress(s, level).map(|v| v.len())).collect::<Result<Vec<_>>>()?.iter().sum();
    let mut layouts = Vec::new();
    for bytes in [2, 3, 4, 8] { for be in [false, true] { for xor in [false, true] {
        if bytes == 3 && (be || xor) { continue; }
        for stride in STRIDES {
            let layout = Layout { width: bytes, big_endian: be, xor, stride };
            layouts.push(layout);
        }
    } } }
    let mut candidates: Vec<_> = layouts.par_iter().map(|&layout| {
        let score: usize = samples.iter().map(|s| packed_size(&layout.residuals(s)) + s.len() - layout.complete(s.len())).sum();
        (score, layout)
    }).collect();
    candidates.sort_by_key(|(size, _)| *size);
    // Evaluate the same candidates and modes, but reuse residuals for both
    // representations and run independent candidates on the existing pool.
    // Applying the scores below remains ordered, preserving tie/margin behavior.
    let dense_scores: Vec<(usize, usize)> = if dense {
        candidates[..4].par_iter().map(|(_, candidate)| -> Result<_> {
            let (mut plane_score, mut packed_score) = (0, 0);
            for s in &samples {
                let values = candidate.residuals(s); let tail = &s[candidate.complete(s.len())..];
                plane_score += compress(&planes(&values,tail,*candidate),level)?.len();
                packed_score += compress(&pack(&values,tail),level)?.len();
            }
            Ok((plane_score,packed_score))
        }).collect::<Result<Vec<_>>>()?
    } else { Vec::new() };
    let (mut best, mut layout, mut stage) = (baseline.min(raw_score), default, if baseline < raw_score { 3 } else { 0 });
    // Require 3% sample advantage over ordinary Zstd to justify a numeric transform.
    for (index, (pack_score, candidate)) in candidates.iter().take(if dense { 4 } else { 1 }).enumerate() {
        if pack_score * 100 < best * 97 { best = *pack_score; layout = *candidate; stage = 1; }
        if dense {
            for (mode, score) in [(2,dense_scores[index].0),(4,dense_scores[index].1)] {
                if score * 100 < best * 97 { best = score; layout = *candidate; stage = mode; }
            }
        }
    }
    // Fast favors cheap packing when the numeric signal is strong and its sample
    // is within 25% of ordinary Zstd. Dense instead minimizes sampled size.
    if !dense && candidates[0].0 * 100 < raw_score * 65 && candidates[0].0 * 100 < baseline * 125 {
        layout = candidates[0].1; stage = 1;
    }
    Ok(Plan { layout, stage, level, selection_ms: started.elapsed().as_secs_f64() * 1000.,
        sample_pack_bytes: candidates[0].0, sample_zstd_bytes: baseline, sample_input_bytes: raw_score })
}

#[cfg(test)]
pub fn encode(data: &[u8], dense: bool) -> Result<(Vec<u8>, Plan)> {
    ensure!(data.len() <= MAX, "Research input limit");
    let plan = select(data, dense)?;
    Ok((encode_plan(data, &plan)?, plan))
}
pub(crate) fn plain_plan(level: i32) -> Plan {
    Plan { layout: Layout { width: 2, big_endian: false, xor: false, stride: 1 }, stage: 3,
        level, selection_ms: 0., sample_pack_bytes: 0, sample_zstd_bytes: 0, sample_input_bytes: 0 }
}
pub(crate) fn encode_plan(data: &[u8], plan: &Plan) -> Result<Vec<u8>> {
    ensure!(data.len() <= MAX && plan.layout.valid() && plan.stage <= 4, "Invalid encode plan");
    PROFILE.with(|p| *p.borrow_mut() = Profile::default());
    if plan.stage == 0 { return Ok(frame(data, plan.layout, 1, &[])); }
    if plan.stage == 3 {
        // Keep the ordinary fallback's dictionary across chunk boundaries.
        let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), plan.level)?;
        if data.len() > CHUNK { encoder.multithread(rayon::current_num_threads().min(256) as u32)?; }
        encoder.include_checksum(true)?; encoder.write_all(data)?;
        let body = encoder.finish()?;
        return Ok(frame(data, plan.layout, if body.len() >= data.len() { 1 } else { 2 }, &body));
    }
    let chunk_start = Instant::now();
    let chunks: Result<Vec<_>> = data.par_chunks(CHUNK).map(|chunk| {
        let mut transform_ms = 0.; let mut entropy_ms = 0.;
        let encoded = match plan.stage {
            0 => chunk.to_vec(),
            3 => compress(chunk, plan.level)?,
            mode => {
                let start = Instant::now();
                let transform = if plan.layout.width <= 3 {
                    if mode == 2 { transform16::<true>(chunk, plan.layout) } else { transform16::<false>(chunk, plan.layout) }
                } else {
                    let values = plan.layout.residuals(chunk); let tail = &chunk[plan.layout.complete(chunk.len())..];
                    if mode == 2 { planes(&values, tail, plan.layout) } else { pack(&values, tail) }
                };
                transform_ms = start.elapsed().as_secs_f64() * 1000.;
                if mode == 1 { transform } else {
                    let start = Instant::now(); let compressed = compress(&transform, plan.level)?;
                    entropy_ms = start.elapsed().as_secs_f64() * 1000.; compressed
                }
            }
        };
        if encoded.len() >= chunk.len() { Ok((0u8, chunk.to_vec(), transform_ms, entropy_ms)) }
        else { Ok((plan.stage, encoded, transform_ms, entropy_ms)) }
    }).collect();
    let chunks = chunks?;
    let mut profile = Profile { chunks_wall_ms: chunk_start.elapsed().as_secs_f64() * 1000., ..Profile::default() };
    let framing = Instant::now();
    let body_len: usize = chunks.iter().map(|(_,c,_,_)| c.len()+5).sum();
    let stored = body_len >= data.len();
    let mut out = header(data, plan.layout, stored as u8, if stored { data.len() } else { body_len });
    if stored { out.extend_from_slice(data); }
    for (stage, chunk, transform_ms, entropy_ms) in chunks {
        profile.transform_worker_ms += transform_ms; profile.entropy_worker_ms += entropy_ms;
        if !stored { out.push(stage); out.extend_from_slice(&(chunk.len() as u32).to_le_bytes()); out.extend(chunk); }
    }
    profile.framing_ms = framing.elapsed().as_secs_f64() * 1000.;
    PROFILE.with(|p| *p.borrow_mut() = profile); Ok(out)
}
fn frame(data: &[u8], l: Layout, mode: u8, body: &[u8]) -> Vec<u8> {
    let payload = if mode == 1 { data } else { body };
    let mut out = header(data,l,mode,payload.len()); out.extend_from_slice(payload); out
}
fn header(data: &[u8], l: Layout, mode: u8, payload_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(29+payload_len); out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(data.len() as u64).to_le_bytes()); out.extend_from_slice(&xxh64(data, 0).to_le_bytes());
    out.extend_from_slice(&[l.width, l.big_endian as u8, l.xor as u8, l.stride as u8, mode]);
    out
}

struct Reader<'a> { data: &'a [u8], pos: usize }
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> { ensure!(n <= self.data.len() - self.pos, "Truncated stream"); let s = &self.data[self.pos..self.pos+n]; self.pos += n; Ok(s) }
    fn byte(&mut self) -> Result<u8> { Ok(self.take(1)?[0]) }
    fn u32(&mut self) -> Result<u32> { Ok(u32::from_le_bytes(self.take(4)?.try_into()?)) }
    fn u64(&mut self) -> Result<u64> { Ok(u64::from_le_bytes(self.take(8)?.try_into()?)) }
    fn end(&self) -> Result<()> { ensure!(self.pos == self.data.len(), "Trailing data"); Ok(()) }
}
fn unpack(data: &[u8], size: usize, layout: Layout, mode: u8) -> Result<Vec<u8>> {
    let count = layout.count(size); let tail_len = size - layout.complete(size);
    let mut reader = Reader { data, pos: 0 }; let mut values = Vec::with_capacity(count);
    if mode == 2 {
        let planes = reader.take(count * layout.lanes())?;
        for i in 0..count {
            let mut value = 0;
            for lane in 0..layout.lanes() { value |= u64::from(planes[lane * count + i]) << (lane * 8); }
            ensure!(value <= layout.mask(), "Noncanonical high bits"); values.push(value);
        }
    } else {
        for start in (0..count).step_by(32) {
            let n = (count - start).min(32); let bits = reader.byte()? as u32;
            ensure!(bits <= layout.bits(), "Invalid packed width");
            let bytes = reader.take((n * bits as usize).div_ceil(8))?;
            let (mut acc, mut used, mut index) = (0u128, 0u32, 0usize);
            for _ in 0..n {
                while used < bits { acc |= u128::from(bytes[index]) << used; used += 8; index += 1; }
                values.push((acc & ((1u128 << bits) - 1)) as u64); acc >>= bits; used -= bits;
            }
            ensure!(acc == 0, "Noncanonical padding");
        }
    }
    let out = layout.restore(values, reader.take(tail_len)?); reader.end()?; Ok(out)
}
pub fn decode(data: &[u8]) -> Result<Vec<u8>> {
    ensure!(data.len() <= MAX + 29, "Research stream limit");
    let mut r = Reader { data, pos: 0 }; ensure!(r.take(8)? == MAGIC, "Invalid magic");
    let size = r.u64()?; ensure!(size <= MAX as u64, "Decoded size limit"); let size = size as usize; let hash = r.u64()?;
    let width = r.byte()?; let endian = r.byte()?; let xor = r.byte()?; let stride = r.byte()? as usize; let stored = r.byte()?;
    let layout = Layout { width, big_endian: endian == 1, xor: xor == 1, stride };
    ensure!(layout.valid() && endian < 2 && xor < 2 && stored < 3, "Invalid layout/flags");
    let out = if stored == 1 { r.take(size)?.to_vec() } else if stored == 2 {
        decompress(r.take(data.len() - r.pos)?, size)?
    } else {
        let mut blocks = Vec::new(); let mut remaining = size;
        while remaining != 0 {
            let count = remaining.min(CHUNK); let mode = r.byte()?; let n = r.u32()? as usize;
            ensure!(mode <= 4 && n > 0 && n <= count && (mode != 0 || n == count), "Invalid chunk");
            blocks.push((mode, count, r.take(n)?)); remaining -= count;
        }
        let decoded: Result<Vec<Vec<u8>>> = blocks.into_par_iter().map(|(mode, n, block)| {
            match mode {
                0 => Ok(block.to_vec()),
                1 => unpack(block, n, layout, 1),
                2 | 4 => {
                    let words = layout.count(n);
                    let bound = words * layout.lanes() + words.div_ceil(32) + layout.width as usize;
                    unpack(&decompress(block, bound)?, n, layout, mode)
                }
                3 => { let out = decompress(block, n)?; ensure!(out.len() == n, "Block size mismatch"); Ok(out) },
                _ => bail!("Invalid block mode"),
            }
        }).collect(); decoded?.concat()
    };
    r.end()?; ensure!(out.len() == size && xxh64(&out, 0) == hash, "Content checksum/length mismatch"); Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(n: usize) -> Vec<u8> { let mut x = 19u64; (0..n).map(|_| { x ^= x << 13; x ^= x >> 7; x ^= x << 17; x as u8 }).collect() }
    #[test]
    fn all_layouts_widths_predictors_and_tails() -> Result<()> {
        for width in [2, 3, 4, 8] { for big_endian in [false, true] { for xor in [false, true] { for stride in STRIDES {
            let l = Layout { width, big_endian, xor, stride }; if !l.valid() { continue; }
            for n in [0, 1, 2, 3, 7, 8, 63, 64, 65, 127, 129, 257, 2049] {
                let data = fixture(n); let residuals = l.residuals(&data); let tail = &data[l.complete(n)..];
                assert_eq!(l.restore(residuals.clone(), tail), data);
                assert_eq!(unpack(&pack(&residuals, tail), n, l, 1)?, data);
                assert_eq!(unpack(&planes(&residuals, tail, l), n, l, 2)?, data);
                if width <= 3 {
                    assert_eq!(transform16::<false>(&data, l), pack(&residuals, tail));
                    assert_eq!(transform16::<true>(&data, l), planes(&residuals, tail, l));
                }
            }
        } } } } Ok(())
    }
    #[test]
    fn packed_212_known_bytes_and_float_bit_patterns() -> Result<()> {
        let l = Layout { width: 3, big_endian: false, xor: false, stride: 2 };
        assert_eq!(l.words(&[0x23, 0xa1, 0xbc]), [0x123, 0xabc]);
        for width in [4, 8] {
            let l = Layout { width, big_endian: false, xor: true, stride: 1 };
            let data: Vec<u8> = [0u64, 1, u64::MAX, 0x7ff8000000000001, 0x8000000000000000, 0xfff0000000000000].iter().flat_map(|x| x.to_le_bytes()).collect();
            assert_eq!(l.restore(l.residuals(&data), &[]), data);
        } Ok(())
    }
    #[test]
    fn frame_stages_mixed_blocks_and_boundaries() -> Result<()> {
        let mut mixed = vec![0u8; CHUNK]; mixed.extend(fixture(CHUNK)); mixed.extend([1, 2, 3, 4, 5]);
        for data in [vec![], vec![9; 129], fixture(4097), mixed] {
            for stage in 0..=4 { for width in [2, 3, 4, 8] {
                let layout = Layout { width, big_endian: false, xor: false, stride: 12 };
                let plan = Plan { layout, stage, level: 3, selection_ms: 0., sample_pack_bytes: 0, sample_zstd_bytes: 0, sample_input_bytes: 0 };
                assert_eq!(decode(&encode_plan(&data, &plan)?)?, data);
            } }
            for dense in [false, true] { assert_eq!(decode(&encode(&data, dense)?.0)?, data); }
        } Ok(())
    }
    #[test]
    fn rejects_corruption_truncation_trailing_and_bombs() -> Result<()> {
        let data: Vec<u8> = (0..50000u16).flat_map(|i| i.to_le_bytes()).collect();
        let encoded = encode(&data, true)?.0;
        for n in [0, 7, 8, 15, 23, 28, 29, encoded.len()/2, encoded.len()-1] { assert!(decode(&encoded[..n]).is_err()); }
        for i in [0, 8, 16, 24, 25, 26, 27, 28, 29, encoded.len()-1] {
            let mut damaged = encoded.clone(); damaged[i] ^= 127; assert!(decode(&damaged).is_err());
        }
        let mut huge = encoded.clone(); huge[8..16].copy_from_slice(&u64::MAX.to_le_bytes()); assert!(decode(&huge).is_err());
        let mut extra = encoded; extra.push(0); assert!(decode(&extra).is_err());
        assert!(decompress(&compress(&vec![0; 100000], 1)?, 64).is_err());
        let mut frames = compress(&[1, 2], 1)?; frames.extend(compress(&[3, 4], 1)?); assert!(decompress(&frames, 4).is_err());
        Ok(())
    }
    #[test]
    fn every_bit_width_and_mutated_streams_are_bounded() -> Result<()> {
        for width in [2, 3, 4, 8] {
            let l = Layout { width, big_endian: false, xor: true, stride: 3 };
            for bits in 0..=l.bits() {
                let mask = if bits == 0 { 0 } else { u64::MAX >> (64 - bits) };
                let count = if width == 3 { 66 } else { 65 };
                let values: Vec<_> = (0..count).map(|i| if i % 2 == 0 { mask } else { 0 }).collect();
                let data = l.restore(values.clone(), &[]);
                assert_eq!(unpack(&pack(&values, &[]), data.len(), l, 1)?, data);
            }
        }
        let data: Vec<_> = (0..50000u32).flat_map(|n| (n / 12).to_be_bytes()).collect();
        for dense in [false, true] {
            let encoded = encode(&data, dense)?.0;
            // Valid or rejected is acceptable; mutated metadata may be unused in stored/fallback modes.
            // The invariant is that malformed input cannot panic or recover different unchecked content.
            for i in 0..512 {
                let mut changed = encoded.clone(); let at = i * 7919 % changed.len(); changed[at] ^= 1 << (i % 8);
                if let Ok(decoded) = decode(&changed) { assert_eq!(decoded, data); }
            }
        } Ok(())
    }
    #[test]
    fn simd_unaligned_slices_and_all_vector_edges() {
        let data=fixture(1024);
        for offset in 0..32 { for n in 0..194 { for stride in [1,3,12,64] {
            let slice=&data[offset..offset+n];
            for big_endian in [false,true] { for xor in [false,true] {
                let layout=Layout { width:2,big_endian,xor,stride };
                let values=layout.residuals(slice); let tail=&slice[layout.complete(n)..];
                assert_eq!(transform16::<true>(slice,layout),planes(&values,tail,layout));
            } }
        } } }
    }
    #[test]
    fn reused_zstd_context_never_retains_frame_history() -> Result<()> {
        for level in [1,3,9,3,1,3] { for n in [0,1,4096,CHUNK,17,8193] {
            let data=fixture(n);
            assert_eq!(compress(&data,level)?,zstd::bulk::compress(&data,level)?);
        } } Ok(())
    }
}
