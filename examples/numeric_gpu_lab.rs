//! GPU numeric research executable; never used by the production archive reader or Auto.
#[path = "../research/codec-lab/codec.rs"]
mod original;
#[path = "../research/numeric-gpu/cpu.rs"]
mod cpu;
#[path = "../research/numeric-gpu/gpu.rs"]
mod gpu;
use anyhow::{ensure, Result};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path, time::Instant};

fn hash(data: &[u8]) -> String { format!("{:x}", Sha256::digest(data)) }
fn write(path: &Path, data: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(data)?; file.sync_all()?; Ok(())
}
fn fixture(n: usize, kind: usize) -> Vec<u8> {
    let mut state = 1234567u64;
    (0..n.div_ceil(2)).flat_map(|i| {
        state ^= state << 13; state ^= state >> 7; state ^= state << 17;
        let value = match kind {
            0 => 0,
            1 => ((i / 16) + (i % 16) * 400 + (state as usize % 7)) as u16,
            _ => state as u16,
        };
        value.to_le_bytes()
    }).take(n).collect()
}
fn validate(adapter: &str) -> Result<()> {
    let started = Instant::now(); let mut session = gpu::Session::new(adapter)?;
    let mut count = 0;
    for n in [0, 1, 2, 3, 63, 64, 65, 16383, 16384, 16385, cpu::CHUNK - 1, cpu::CHUNK, cpu::CHUNK + 1, cpu::CHUNK + 65, gpu::BATCH + 1] {
        for kind in 0..3 {
            let data = fixture(n, kind);
            for stride in cpu::STRIDES {
                let expected = cpu::encode(&data, stride)?;
                let (encoded, _) = session.encode(&data, stride)?;
                ensure!(encoded == expected, "GPU/CPU mismatch: {adapter} n={n} kind={kind} stride={stride}");
                ensure!(original::decode(&encoded)? == data, "Original decoder mismatch");
                count += 1;
            }
        }
    }
    // Exercise mixed stored/packed chunks, stale buffer contents, and multiple full GPU batches.
    let mut data = fixture(cpu::CHUNK, 2);
    data.extend(fixture(gpu::BATCH * 2 + 3, 1));
    let expected = cpu::encode(&data, 16)?;
    let (encoded, _) = session.encode(&data, 16)?;
    ensure!(encoded == expected && original::decode(&encoded)? == data, "Mixed batch mismatch"); count += 1;
    // Independent old encoder agrees when it selects the numeric path.
    let data = fixture(cpu::CHUNK * 2, 1);
    let (old, _) = original::encode(&data, "numeric", 8)?;
    let expected = cpu::encode(&data, if old[8] == 2 { old[9] as usize } else { 1 })?;
    ensure!(old == expected, "Frozen encoder reference mismatch");
    println!("{}", serde_json::json!({"adapter": session.info, "gpu_cpu_exact_cases": count,
        "original_decoder_verified": true, "original_encoder_verified": true, "elapsed_ms": started.elapsed().as_secs_f64()*1000.})); Ok(())
}
fn benchmark(args: &[String]) -> Result<()> {
    ensure!(args.len() == 6, "bench INPUT BACKEND THREADS OUTDIR RUNS");
    let source = Path::new(&args[1]); let backend = &args[2];
    let threads: usize = args[3].parse()?; let runs: usize = args[5].parse()?;
    ensure!((1..=64).contains(&threads) && (1..=20).contains(&runs), "Invalid benchmark bounds");
    let outdir = Path::new(&args[4]); std::fs::create_dir_all(outdir)?;
    let group = Instant::now();
    let init = Instant::now();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build()?;
    let mut session = if ["intel", "nvidia"].contains(&backend.as_str()) { Some(gpu::Session::new(backend)?) } else { None };
    let init_ms = init.elapsed().as_secs_f64() * 1000.;
    let mut rows = Vec::new();
    for run in 0..runs {
        let whole = Instant::now();
        let data = std::fs::read(source)?;
        ensure!(data.len() <= original::MAX, "Research input limit");
        let read_ms = whole.elapsed().as_secs_f64() * 1000.;
        let codec = Instant::now();
        let selection = Instant::now();
        let stride = if backend == "cpu-numeric" || session.is_some() { cpu::select(&data) } else { 0 };
        let selection_ms = selection.elapsed().as_secs_f64() * 1000.;
        let (encoded, metrics) = if let Some(gpu) = session.as_mut() {
            let (encoded, metrics) = gpu.encode(&data, stride)?; (encoded, Some(metrics))
        } else if backend == "cpu-numeric" {
            (pool.install(|| cpu::encode(&data, stride))?, None)
        } else {
            (pool.install(|| original::encode(&data, backend, threads))?.0, None)
        };
        let codec_ms = codec.elapsed().as_secs_f64() * 1000.;
        let archive = outdir.join(format!("{run}.ltex"));
        let io = Instant::now(); write(&archive, &encoded)?; let write_ms = io.elapsed().as_secs_f64() * 1000.;
        let create_ms = whole.elapsed().as_secs_f64() * 1000. + if run == 0 { init_ms } else { 0. };
        let extraction = Instant::now();
        let saved = std::fs::read(&archive)?;
        let decode_start = Instant::now();
        let decoded = pool.install(|| original::decode(&saved))?;
        let decode_ms = decode_start.elapsed().as_secs_f64() * 1000.;
        let restored = outdir.join(format!("{run}.restored")); write(&restored, &decoded)?;
        let recover_ms = extraction.elapsed().as_secs_f64() * 1000.;
        ensure!(decoded == data && hash(&std::fs::read(&restored)?) == hash(&data), "Disk round-trip mismatch");
        let exact_cpu = if session.is_some() {
            let cpu = pool.install(|| cpu::encode(&data, stride))?; ensure!(cpu == encoded, "GPU/CPU archive mismatch"); true
        } else { false };
        rows.push(serde_json::json!({"run": run, "input_bytes": data.len(), "output_bytes": encoded.len(),
            "input_sha256": hash(&data), "archive_sha256": hash(&encoded), "sha256_verified": true,
            "gpu_cpu_identical": exact_cpu, "stride": stride, "mode": encoded[8], "selection_ms": selection_ms,
            "read_ms": read_ms, "write_ms": write_ms, "codec_ms": codec_ms, "create_ms": create_ms,
            "decode_ms": decode_ms, "recover_ms": recover_ms, "metrics": metrics}));
        std::fs::remove_file(archive)?; std::fs::remove_file(restored)?;
    }
    println!("{}", serde_json::json!({"backend": backend, "threads": threads, "initialization_ms": init_ms,
        "adapter": session.as_ref().map(|s| &s.info), "rows": rows, "group_ms": group.elapsed().as_secs_f64()*1000.})); Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("validate") if args.len() == 2 => validate(&args[1]),
        Some("bench") => benchmark(&args),
        _ => anyhow::bail!("Usage: numeric_gpu_lab validate intel|nvidia OR bench INPUT BACKEND THREADS OUTDIR RUNS"),
    }
}
