#[path = "../research/codec-lab/codec.rs"]
mod original;
#[path = "../research/numeric-gpu/cpu.rs"]
mod numeric;
#[path = "../research/numeric-v2/codec.rs"]
mod before;
#[path = "../research/dense-opt/codec.rs"]
mod after;
use anyhow::{ensure, Result};
use std::{io::{Read, Write}, time::Instant};

fn encode(data: &[u8], method: &str) -> Result<(Vec<u8>, serde_json::Value)> {
    match method {
        "original" => Ok((numeric::encode(data, numeric::select(data))?, serde_json::Value::Null)),
        "before" => { let (out,p) = before::encode(data,true)?; Ok((out,serde_json::json!(p))) },
        "after" => { let (out,p) = after::encode(data,true)?;
            let mut plan=serde_json::json!(p); plan["profile"]=serde_json::json!(after::last_profile()); Ok((out,plan)) },
        "zstd-3" => Ok((original::encode(data,"zstd-3",8)?.0, serde_json::Value::Null)),
        _ => anyhow::bail!("Unknown method"),
    }
}
fn decode(data: &[u8], method: &str) -> Result<Vec<u8>> {
    match method {
        "original" | "zstd-3" => original::decode(data),
        "before" => before::decode(data),
        "after" => after::decode(data),
        _ => anyhow::bail!("Unknown method"),
    }
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(args.len()==4,"encode|decode|memory METHOD INPUT OUTPUT_OR_ITERATIONS");
    let started=Instant::now();
    let pool=rayon::ThreadPoolBuilder::new().num_threads(8).build()?;
    let mut data=Vec::new(); std::fs::File::open(&args[2])?.take((after::MAX+30) as u64).read_to_end(&mut data)?;
    ensure!(data.len()<=after::MAX+29,"Input limit");
    if args[0]=="memory" {
        let iterations: usize=args[3].parse()?; ensure!((1..=30).contains(&iterations),"Iteration limit");
        let mut rows=Vec::new();
        for run in 0..iterations {
            let start=Instant::now(); let (output,plan)=pool.install(||encode(&data,&args[1]))?;
            let encode_ms=start.elapsed().as_secs_f64()*1000.;
            let start=Instant::now(); let restored=pool.install(||decode(&output,&args[1]))?;
            let decode_ms=start.elapsed().as_secs_f64()*1000.; ensure!(restored==data,"Recovery mismatch");
            rows.push(serde_json::json!({"run":run,"encode_ms":encode_ms,"decode_ms":decode_ms,
                "output_bytes":output.len(),"output_xxh64":xxhash_rust::xxh64::xxh64(&output,0),"plan":plan,"verified":true}));
        }
        println!("{}",serde_json::json!({"rows":rows})); return Ok(());
    }
    let start=Instant::now();
    let (output,plan)=match args[0].as_str() {
        "encode" => pool.install(||encode(&data,&args[1]))?,
        "decode" => (pool.install(||decode(&data,&args[1]))?,serde_json::Value::Null),
        _ => anyhow::bail!("Unknown operation"),
    };
    let codec_ms=start.elapsed().as_secs_f64()*1000.;
    let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&args[3])?;
    file.write_all(&output)?; file.sync_all()?;
    println!("{}",serde_json::json!({"input_bytes":data.len(),"output_bytes":output.len(),"codec_ms":codec_ms,
        "operation_ms":started.elapsed().as_secs_f64()*1000.,"plan":plan})); Ok(())
}

#[cfg(test)]
mod compatibility {
    use super::*;
    #[test]
    fn optimized_frames_match_frozen_dense_and_cross_decode() -> Result<()> {
        let mut state=87u64;
        let noise: Vec<_>=(0..1_100_001).map(|_| { state^=state<<13; state^=state>>7; state^=state<<17; state as u8 }).collect();
        let numeric: Vec<_>=(0..600_000u32).flat_map(|i| ((i/12+i%12*800) as u16).to_be_bytes()).collect();
        for threads in [1,8] {
            let pool=rayon::ThreadPoolBuilder::new().num_threads(threads).build()?;
            pool.install(|| -> Result<()> {
                for data in [&[][..], &noise[..129], &noise[..], &numeric[..]] { for dense in [false,true] {
                    let baseline=before::encode(data,dense)?.0;
                    let optimized=after::encode(data,dense)?.0;
                    ensure!(baseline==optimized,"Changed compressed bytes");
                    ensure!(before::decode(&optimized)?==data && after::decode(&baseline)?==data,"Cross decode mismatch");
                } } Ok(())
            })?;
        } Ok(())
    }
}
