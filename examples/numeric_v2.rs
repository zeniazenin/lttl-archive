#[path = "../research/numeric-v2/codec.rs"]
mod codec;
use anyhow::{ensure, Result};
use std::{io::{Read, Write}, time::Instant};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(args.len() == 4, "encode|decode fast|dense INPUT OUTPUT");
    ensure!(["fast", "dense"].contains(&args[1].as_str()), "Unknown preset");
    let started = Instant::now();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(8).build()?;
    let mut data = Vec::new(); std::fs::File::open(&args[2])?.take((codec::MAX + 30) as u64).read_to_end(&mut data)?;
    ensure!(data.len() <= codec::MAX + 29, "Input limit");
    let codec_start = Instant::now();
    let (output, plan) = match args[0].as_str() {
        "encode" => { let (out, plan) = pool.install(|| codec::encode(&data, args[1] == "dense"))?; (out, Some(plan)) },
        "decode" => (pool.install(|| codec::decode(&data))?, None),
        _ => anyhow::bail!("Unknown operation"),
    };
    let codec_ms = codec_start.elapsed().as_secs_f64() * 1000.;
    let mut file = std::fs::OpenOptions::new().create_new(true).write(true).open(&args[3])?;
    file.write_all(&output)?; file.sync_all()?;
    println!("{}", serde_json::json!({"input_bytes":data.len(), "output_bytes":output.len(), "plan":plan,
        "codec_ms":codec_ms, "operation_ms":started.elapsed().as_secs_f64()*1000.})); Ok(())
}
