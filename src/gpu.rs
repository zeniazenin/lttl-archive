//! Optional NVIDIA backend. The archive remains a standard concatenated Zstd stream.
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{io::{self, Write}, sync::atomic::{AtomicBool, Ordering}, time::Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
pub enum Backend { Auto, Cpu, CpuChunked, Gpu, Wgpu, LttlFast, LttlDense }
#[derive(Debug, Clone)]
pub struct GpuOptions { pub chunk_kib: usize, pub batch_mib: usize }
impl Default for GpuOptions { fn default() -> Self { Self { chunk_kib: 1024, batch_mib: 32 } } }
impl GpuOptions {
    fn validate(&self) -> Result<()> {
        ensure!((64..=16384).contains(&self.chunk_kib), "Chunk size must be 64–16384 KiB");
        ensure!((16..=128).contains(&self.batch_mib), "Batch size must be 16–128 MiB");
        ensure!(self.batch_mib * 1024 >= self.chunk_kib, "Batch size must cover one chunk");
        Ok(())
    }
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct GpuMetrics {
    pub device: String, pub chunk_bytes: usize, pub batch_bytes: usize,
    pub initialize_seconds: f64, pub host_wall_seconds: f64,
    pub host_to_device_ms: f64, pub compression_ms: f64, pub device_to_host_ms: f64,
    pub batches: u64, pub chunks: u64, pub h2d_bytes: u64, pub d2h_bytes: u64,
    pub checksum_seconds: f64,
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::ffi::{c_char, c_void};
    #[repr(C)]
    #[derive(Default)]
    struct Metrics { h2d: f64, compression: f64, d2h: f64, chunks: u64, h2d_bytes: u64, d2h_bytes: u64 }
    type Create = unsafe extern "C" fn(usize, usize, *mut c_char, usize) -> *mut c_void;
    type Compress = unsafe extern "C" fn(*mut c_void, *const u8, usize, *mut u8, usize, *mut usize, *mut Metrics, *mut c_char, usize) -> i32;
    type Destroy = unsafe extern "C" fn(*mut c_void);
    type Device = unsafe extern "C" fn(*mut c_void, *mut c_char, usize) -> i32;
    pub struct Session { ctx: *mut c_void, compress: Compress, destroy: Destroy, _library: libloading::Library, pub metrics: GpuMetrics }
    impl Session {
        pub fn new(options: &GpuOptions) -> Result<Self> {
            options.validate()?;
            let started = Instant::now();
            let folder = std::env::current_exe()?.parent().context("Executable has no directory")?.to_path_buf();
            let path = folder.join("gpu").join("lttl_gpu.dll");
            // Absolute primary path; dependencies search this DLL's folder and safe system directories.
            let library: libloading::Library = unsafe { libloading::os::windows::Library::load_with_flags(&path, 0x100 | 0x1000) }
                .with_context(|| format!("Load GPU runtime {}. Run scripts/build-gpu.ps1 for this build directory", path.display()))?.into();
            let create = unsafe { *library.get::<Create>(b"lttl_gpu_create\0")? };
            let compress = unsafe { *library.get::<Compress>(b"lttl_gpu_compress\0")? };
            let destroy = unsafe { *library.get::<Destroy>(b"lttl_gpu_destroy\0")? };
            let device = unsafe { *library.get::<Device>(b"lttl_gpu_device\0")? };
            let mut error = [0i8; 1024];
            let ctx = unsafe { create(options.chunk_kib * 1024, options.batch_mib * 1_048_576, error.as_mut_ptr(), error.len()) };
            ensure!(!ctx.is_null(), "GPU initialization: {}", error_text(&error));
            let mut name = [0i8; 256];
            unsafe { device(ctx, name.as_mut_ptr(), name.len()) };
            Ok(Self { ctx, compress, destroy, _library: library, metrics: GpuMetrics {
                device: error_text(&name), chunk_bytes: options.chunk_kib * 1024, batch_bytes: options.batch_mib * 1_048_576,
                initialize_seconds: started.elapsed().as_secs_f64(), ..Default::default()
            } })
        }
        pub fn compress(&mut self, input: &[u8]) -> Result<Vec<u8>> {
            ensure!(!input.is_empty() && input.len() <= self.metrics.batch_bytes, "Invalid GPU batch length");
            let started = Instant::now();
            let chunks = input.len().div_ceil(self.metrics.chunk_bytes);
            let mut output = vec![0u8; input.len() + input.len() / 128 + chunks * 1024 + 1_048_576];
            let mut bytes = 0usize;
            let mut metrics = Metrics::default();
            let mut error = [0i8; 1024];
            let result = unsafe { (self.compress)(self.ctx, input.as_ptr(), input.len(), output.as_mut_ptr(), output.len(), &mut bytes, &mut metrics, error.as_mut_ptr(), error.len()) };
            ensure!(result == 0, "GPU compression: {}", error_text(&error));
            ensure!(bytes <= output.len(), "Invalid native output size");
            output.truncate(bytes);
            self.metrics.host_wall_seconds += started.elapsed().as_secs_f64();
            self.metrics.host_to_device_ms += metrics.h2d;
            self.metrics.compression_ms += metrics.compression;
            self.metrics.device_to_host_ms += metrics.d2h;
            self.metrics.chunks += metrics.chunks;
            self.metrics.h2d_bytes += metrics.h2d_bytes;
            self.metrics.d2h_bytes += metrics.d2h_bytes;
            self.metrics.batches += 1;
            Ok(output)
        }
    }
    fn error_text(bytes: &[i8]) -> String {
        // Native bridge always terminates these bounded buffers, including on error.
        let end = bytes.iter().position(|&v| v == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end].iter().map(|&v| v as u8).collect::<Vec<_>>()).into_owned()
    }
    impl Drop for Session { fn drop(&mut self) { unsafe { (self.destroy)(self.ctx) }; } }
}
#[cfg(not(windows))]
mod native {
    use super::*;
    pub struct Session { pub metrics: GpuMetrics }
    impl Session {
        pub fn new(_: &GpuOptions) -> Result<Self> { anyhow::bail!("GPU backend currently requires Windows x64") }
        pub fn compress(&mut self, _: &[u8]) -> Result<Vec<u8>> { anyhow::bail!("GPU backend unavailable") }
    }
}

pub fn gpu_status() -> serde_json::Value {
    match native::Session::new(&GpuOptions::default()) {
        Ok(session) => serde_json::json!({"available": true, "device": session.metrics.device, "codec":"nvCOMP Zstd, fixed setting", "initialize_seconds":session.metrics.initialize_seconds}),
        Err(error) => serde_json::json!({"available": false, "reason":format!("{error:#}")}),
    }
}

/// A selector trial owns the context so a selected GPU does not initialize twice.
pub(crate) struct GpuProbe { session: native::Session }
impl GpuProbe {
    pub fn trial(options: &GpuOptions, sample: &[u8]) -> Result<(Self, f64)> {
        let mut session = native::Session::new(options)?;
        let encoded = session.compress(sample)?;
        ensure!(zstd::stream::decode_all(encoded.as_slice())? == sample, "GPU selection trial did not round-trip");
        let ratio = encoded.len() as f64 / sample.len().max(1) as f64;
        Ok((Self { session }, ratio))
    }
    pub fn metrics(&self) -> &GpuMetrics { &self.session.metrics }
    fn into_session(mut self) -> native::Session {
        let old = &self.session.metrics;
        // Trial costs are reported under selection, not counted twice as archive batches.
        self.session.metrics = GpuMetrics { device: old.device.clone(), chunk_bytes: old.chunk_bytes,
            batch_bytes: old.batch_bytes, ..Default::default() };
        self.session
    }
}

pub(crate) enum TarEncoder<'a, W: Write> {
    Portable(Box<crate::portable::Encoder<'a, W>>),
    Stream(zstd::stream::write::Encoder<'static, W>),
    Chunked(ChunkedWriter<'a, W>),
}
impl<'a, W: Write> TarEncoder<'a, W> {
    pub fn with_probe(writer: W, backend: Backend, options: &GpuOptions, level: i32, workers: u32, cancel: &'a AtomicBool, probe: Option<GpuProbe>) -> Result<Self> {
        ensure!(backend != Backend::Auto, "Auto backend must be resolved before encoding");
        if backend == Backend::Cpu {
            let mut encoder = zstd::stream::write::Encoder::new(writer, level)?;
            encoder.multithread(workers)?;
            encoder.include_checksum(true)?;
            return Ok(Self::Stream(encoder));
        }
        options.validate()?;
        let gpu = if backend == Backend::Gpu { Some(match probe { Some(p) => p.into_session(), None => native::Session::new(options)? }) } else { None };
        Ok(Self::Chunked(ChunkedWriter { writer, buffer: Vec::with_capacity(options.batch_mib * 1_048_576),
            chunk: options.chunk_kib * 1024, batch: options.batch_mib * 1_048_576,
            level, workers: (workers as usize).max(1), gpu, cancel, stored: false }))
    }
    pub fn stored(writer: W, cancel: &'a AtomicBool) -> Self {
        Self::Chunked(ChunkedWriter { writer, buffer: Vec::with_capacity(4 * 1_048_576), chunk: 4 * 1_048_576,
            batch: 4 * 1_048_576, level: 1, workers: 1, gpu: None, cancel, stored: true })
    }
    pub fn finish(self) -> Result<(Option<GpuMetrics>, Option<crate::PortableMetrics>)> {
        match self {
            Self::Portable(encoder) => Ok((None, Some(encoder.finish()?))),
            Self::Stream(encoder) => { encoder.finish()?.flush()?; Ok((None,None)) },
            Self::Chunked(mut encoder) => { encoder.emit()?; encoder.writer.flush()?; Ok((encoder.gpu.as_ref().map(|v| v.metrics.clone()), None)) }
        }
    }
}
impl<W: Write> Write for TarEncoder<'_, W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> { match self { Self::Portable(v) => v.write(data), Self::Stream(v) => v.write(data), Self::Chunked(v) => v.write(data) } }
    fn flush(&mut self) -> io::Result<()> { match self { Self::Portable(v) => v.flush(), Self::Stream(v) => v.flush(), Self::Chunked(v) => v.flush() } }
}
pub(crate) struct ChunkedWriter<'a, W: Write> { writer: W, buffer: Vec<u8>, chunk: usize, batch: usize, level: i32, workers: usize, gpu: Option<native::Session>, cancel: &'a AtomicBool, stored: bool }
impl<W: Write> ChunkedWriter<'_, W> {
    fn emit(&mut self) -> Result<()> {
        ensure!(!self.cancel.load(Ordering::Relaxed), "Operation cancelled");
        if self.buffer.is_empty() { return Ok(()); }
        if self.stored {
            // Standard single-segment Zstd frame: 4-byte content size, checksum,
            // and uncompressed blocks up to the format's 128 KiB block limit.
            self.writer.write_all(&[0x28, 0xb5, 0x2f, 0xfd, 0xa4])?;
            self.writer.write_all(&(self.buffer.len() as u32).to_le_bytes())?;
            let count = self.buffer.len().div_ceil(128 * 1024);
            for (index, block) in self.buffer.chunks(128 * 1024).enumerate() {
                let header = ((block.len() as u32) << 3) | u32::from(index + 1 == count);
                self.writer.write_all(&header.to_le_bytes()[..3])?;
                self.writer.write_all(block)?;
            }
            self.writer.write_all(&(xxhash_rust::xxh64::xxh64(&self.buffer, 0) as u32).to_le_bytes())?;
        } else if let Some(gpu) = &mut self.gpu {
            let compressed = gpu.compress(&self.buffer)?;
            let checksum_started = Instant::now();
            let checked = add_checksums(&compressed, &self.buffer, self.chunk)?;
            gpu.metrics.checksum_seconds += checksum_started.elapsed().as_secs_f64();
            ensure!(!self.cancel.load(Ordering::Relaxed), "Operation cancelled");
            self.writer.write_all(&checked)?;
        } else {
            let chunks: Vec<_> = self.buffer.chunks(self.chunk).collect();
            let level = self.level;
            // Match GPU dictionary resets, with bounded CPU parallelism and ordered output.
            for group in chunks.chunks(self.workers) {
                let outputs = std::thread::scope(|scope| {
                    let handles: Vec<_> = group.iter().map(|input| scope.spawn(move || -> Result<Vec<u8>> {
                        let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), level)?;
                        encoder.include_checksum(true)?;
                        encoder.write_all(input)?;
                        Ok(encoder.finish()?)
                    })).collect();
                    handles.into_iter().map(|v| v.join().map_err(|_| anyhow::anyhow!("CPU compression worker panicked"))?).collect::<Result<Vec<_>>>()
                })?;
                ensure!(!self.cancel.load(Ordering::Relaxed), "Operation cancelled");
                for output in outputs { self.writer.write_all(&output)?; }
            }
        }
        self.buffer.clear();
        Ok(())
    }
}
impl<W: Write> Write for ChunkedWriter<'_, W> {
    fn write(&mut self, mut data: &[u8]) -> io::Result<usize> {
        let length = data.len();
        while !data.is_empty() {
            let count = data.len().min(self.batch - self.buffer.len());
            self.buffer.extend_from_slice(&data[..count]); data = &data[count..];
            if self.buffer.len() == self.batch { self.emit().map_err(io::Error::other)?; }
        }
        Ok(length)
    }
    fn flush(&mut self) -> io::Result<()> { self.writer.flush() }
}

// nvCOMP frames may omit content checksums. Add the standard checksum field,
// covering the ORIGINAL input, without recompressing or changing Zstd blocks.
fn add_checksums(compressed: &[u8], input: &[u8], chunk: usize) -> Result<Vec<u8>> {
    let mut remaining = compressed;
    let mut output = Vec::with_capacity(compressed.len() + input.len().div_ceil(chunk) * 4);
    for original in input.chunks(chunk) {
        ensure!(remaining.len() >= 6 && remaining[..4] == [0x28,0xb5,0x2f,0xfd], "GPU produced invalid Zstd frame");
        let size = zstd::zstd_safe::find_frame_compressed_size(remaining).map_err(|e| anyhow::anyhow!("GPU Zstd framing error {e}"))?;
        ensure!(size >= 6 && size <= remaining.len(), "GPU frame size is invalid");
        let frame = &remaining[..size];
        if let Ok(Some(content_size)) = zstd::zstd_safe::get_frame_content_size(frame) {
            ensure!(content_size == original.len() as u64, "GPU frame content size does not match input");
        }
        let checksum = (xxhash_rust::xxh64::xxh64(original, 0) as u32).to_le_bytes();
        let start = output.len();
        if frame[4] & 4 != 0 {
            ensure!(size >= 10 && frame[size-4..] == checksum, "GPU frame checksum does not match source");
            output.extend_from_slice(frame);
        } else {
            output.extend_from_slice(frame);
            output[start + 4] |= 4;
            output.extend_from_slice(&checksum);
        }
        remaining = &remaining[size..];
    }
    ensure!(remaining.is_empty(), "GPU produced unexpected extra frames");
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_checksums_detect_corruption() -> Result<()> {
        let input = b"independent GPU frames preserve standard Zstandard checksums".repeat(10000);
        let mut frames = Vec::new();
        for chunk in input.chunks(4096) { frames.extend(zstd::bulk::compress(chunk, 1)?); }
        let mut checked = add_checksums(&frames, &input, 4096)?;
        assert_eq!(zstd::stream::decode_all(checked.as_slice())?, input);
        let last = checked.len() - 1; checked[last] ^= 1;
        assert!(zstd::stream::decode_all(checked.as_slice()).is_err());
        assert!(add_checksums(&frames, &input[..input.len()-1], 4096).is_err());
        Ok(())
    }
}
