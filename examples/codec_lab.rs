//! Isolated research executable: never used by the desktop or production archive reader.
#[path="../research/codec-lab/codec.rs"]
mod codec;
use anyhow::{ensure, Context, Result};
use std::{io::{Read,Write},time::Instant};

fn main()->Result<()> {
    let args:Vec<_>=std::env::args().collect();
    ensure!(args.len()==6,"Usage: codec_lab encode|decode METHOD INPUT OUTPUT THREADS");
    let threads:usize=args[5].parse()?;ensure!((1..=64).contains(&threads),"Use 1–64 workers");
    let pool=rayon::ThreadPoolBuilder::new().num_threads(threads).build()?;
    let mut data=Vec::new();std::fs::File::open(&args[3])?.take((codec::MAX+27) as u64).read_to_end(&mut data)?;
    ensure!(data.len()<=codec::MAX+26,"Input too large for research executable");
    let start=Instant::now();
    let (output,choice)=pool.install(||->Result<_>{
        match args[1].as_str(){"encode"=>codec::encode(&data,&args[2],threads),"decode"=>Ok((codec::decode(&data)?,"decode".into())),_=>anyhow::bail!("Unknown operation")}
    })?;
    let milliseconds=start.elapsed().as_secs_f64()*1000.0;
    let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&args[4]).context("Output must be a new file")?;
    file.write_all(&output)?;file.sync_all()?;
    println!("{}",serde_json::json!({"input_bytes":data.len(),"output_bytes":output.len(),"codec_ms":milliseconds,"choice":choice,"threads":threads}));
    Ok(())
}
