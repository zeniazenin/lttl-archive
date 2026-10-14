//! Process-level comparison worker. Preserves the original research implementations.
#[path = "../research/codec-lab/codec.rs"]
mod original;
#[path = "../research/numeric-gpu/cpu.rs"]
mod cpu;
#[path = "../research/numeric-gpu/gpu.rs"]
mod gpu;
use anyhow::{ensure, Result};
use std::{io::{Read, Write}, time::Instant};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(args.len() == 4, "encode|decode METHOD INPUT OUTPUT");
    let started = Instant::now();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(8).build()?;
    let mut data = Vec::new();
    std::fs::File::open(&args[2])?.take((original::MAX + 1048576) as u64 + 1).read_to_end(&mut data)?;
    ensure!(data.len() <= original::MAX + 1048576, "Research input limit");
    let codec = Instant::now();
    let mut metrics = None;
    let mut adapter = None;
    let mut initialization_ms = 0.;
    let output = if args[0] == "decode" {
        if args[1] == "lz4" {
            let decoder = lz4_flex::frame::FrameDecoder::new(&data[..]);
            let mut out = Vec::new(); decoder.take(original::MAX as u64 + 1).read_to_end(&mut out)?;
            ensure!(out.len() <= original::MAX, "Research output limit"); out
        } else { pool.install(|| original::decode(&data))? }
    } else {
        ensure!(args[0] == "encode" && data.len() <= original::MAX, "Invalid encode input");
        match args[1].as_str() {
            "cpu-numeric" => { let stride = cpu::select(&data); pool.install(|| cpu::encode(&data, stride))? },
            "intel" | "nvidia" => {
                let init = Instant::now(); let mut session = gpu::Session::new(&args[1])?;
                initialization_ms = init.elapsed().as_secs_f64() * 1000.;
                adapter = Some(session.info.clone());
                let stride = cpu::select(&data); let (output, stats) = session.encode(&data, stride)?;
                metrics = Some(stats); output
            },
            "lz4" => {
                let info = lz4_flex::frame::FrameInfo::new().content_checksum(true).content_size(Some(data.len() as u64));
                let mut encoder = lz4_flex::frame::FrameEncoder::with_frame_info(info, Vec::new());
                encoder.write_all(&data)?; encoder.finish()?
            },
            "store" => cpu::frame(&data, 1, data.clone()),
            method => pool.install(|| original::encode(&data, method, 8))?.0,
        }
    };
    let codec_ms = codec.elapsed().as_secs_f64() * 1000.;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&args[3])?;
    file.write_all(&output)?; file.sync_all()?;
    println!("{}", serde_json::json!({"input_bytes": data.len(), "output_bytes": output.len(),
        "operation_ms": started.elapsed().as_secs_f64()*1000., "codec_including_init_ms": codec_ms,
        "initialization_ms": initialization_ms, "metrics": metrics, "adapter": adapter})); Ok(())
}
