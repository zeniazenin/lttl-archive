use anyhow::Result;
use lttl::{Backend, CreateOptions, Format, GpuOptions, PortableOptions, Preset};
use std::{fs, sync::atomic::AtomicBool};

fn roundtrip(backend: Backend, adapter: Option<&str>, chunk_kib:usize) -> Result<()> {
    let root=tempfile::tempdir()?; let input=root.path().join("fixtures"); fs::create_dir(&input)?;
    let mut state=123456789u64;
    for (index,length) in [0,1,4,12,13,4095,4096,16383,16384,65535,65536,2*1048576+19].into_iter().enumerate() {
        let bytes=(0..length).map(|i| {state^=state<<13;state^=state>>7;state^=state<<17;
            if i%131072<65536 {state as u8} else {(i%37) as u8}}).collect::<Vec<_>>();
        fs::write(input.join(format!("{index}-測試.bin")),bytes)?;
    }
    let archive=root.path().join("output.tar.lz4"); let cancel=AtomicBool::new(false);
    let stats=lttl::create_with_provider(&CreateOptions{inputs:vec![input.clone()],output:archive.clone(),format:Format::TarLz4,preset:Preset::Fast,threads:2},
        backend,&GpuOptions::default(),&PortableOptions{adapter:adapter.map(str::to_owned),chunk_kib,batch_mib:1},&cancel,&mut |_|{})?;
    println!("{}",serde_json::to_string(&stats)?);
    let metrics=stats.portable.unwrap(); assert!(metrics.batches>=3);
    if backend==Backend::Auto { assert!(metrics.auto_reason.is_some()); assert_eq!(metrics.api,"CPU"); }
    lttl::test(&archive,&cancel)?;
    let destination=root.path().join("extracted"); lttl::extract(&archive,&destination,&cancel,&mut |_|{})?;
    for entry in fs::read_dir(&input)? {let entry=entry?;assert_eq!(fs::read(entry.path())?,fs::read(destination.join("fixtures").join(entry.file_name()))?);}
    let original=fs::read(&archive)?;
    // Separate standard frame parser must accept our generated wire format too.
    let mut standard=lz4_flex::frame::FrameDecoder::new(original.as_slice());
    assert!(std::io::copy(&mut standard,&mut std::io::sink())?>stats.input_bytes);
    for length in [7,original.len()-1,original.len()-4,original.len()-8] {
        fs::write(&archive,&original[..length])?; assert!(lttl::test(&archive,&cancel).is_err(),"Accepted truncated archive of {length} bytes");
    }
    let mut broken=original; let n=broken.len(); broken[n-1]^=1; fs::write(&archive,broken)?;
    assert!(lttl::test(&archive,&cancel).is_err());
    let cancelled_output=root.path().join("cancelled.tar.lz4");
    let cancelled=lttl::create_with_provider(&CreateOptions{inputs:vec![input],output:cancelled_output.clone(),format:Format::TarLz4,preset:Preset::Fast,threads:1},
        backend,&GpuOptions::default(),&PortableOptions{adapter:adapter.map(str::to_owned),chunk_kib,batch_mib:1},&cancel,&mut |_|{cancel.store(true,std::sync::atomic::Ordering::Relaxed);});
    assert!(cancelled.is_err()); assert!(!cancelled_output.exists());
    Ok(())
}
#[test] fn cpu_lz4_frame_integrity() -> Result<()> { for chunk in [4,16,64] {roundtrip(Backend::Auto,None,chunk)?;} Ok(()) }
#[test] #[ignore="requires Intel Arc hardware"]
fn intel_lz4_hardware() -> Result<()> {for chunk in [4,16,64] {roundtrip(Backend::Wgpu,Some("intel"),chunk)?;} Ok(())}
#[test] #[ignore="requires NVIDIA hardware"]
fn nvidia_lz4_hardware() -> Result<()> {for chunk in [4,16,64] {roundtrip(Backend::Wgpu,Some("nvidia"),chunk)?;} Ok(())}

fn reuse_across_threads(adapter:&str)->Result<()> {
    lttl::clear_portable_sessions();
    let root=tempfile::tempdir()?;
    let input=root.path().join("input.bin");
    let bytes=(0..2*1048576+513).map(|i| (i%233) as u8).collect::<Vec<_>>();
    fs::write(&input,&bytes)?;
    let run=|index:usize,chunk:usize|->Result<lttl::Stats> {
        let options=CreateOptions {inputs:vec![input.clone()],output:root.path().join(format!("{index}.tar.lz4")),format:Format::TarLz4,preset:Preset::Fast,threads:1};
        let portable=PortableOptions{adapter:Some(adapter.into()),chunk_kib:chunk,batch_mib:1};
        std::thread::spawn(move ||lttl::create_with_provider(&options,Backend::Wgpu,&GpuOptions::default(),&portable,&AtomicBool::new(false),&mut |_|{})).join().unwrap()
    };
    let first=run(0,16)?.portable.unwrap();
    let second=run(1,16)?.portable.unwrap();
    assert!(!first.session_reused && second.session_reused);
    assert_eq!(second.initialize_seconds,0.);
    assert_eq!((first.batches,first.chunks,first.h2d_bytes),(second.batches,second.chunks,second.h2d_bytes));
    assert_eq!(fs::read(root.path().join("0.tar.lz4"))?,fs::read(root.path().join("1.tar.lz4"))?);
    // Shorter, different input must not retain a previous job's checksum/data.
    fs::write(&input,b"short replacement")?;
    assert!(run(2,16)?.portable.unwrap().session_reused);
    let destination=root.path().join("extracted");
    lttl::extract(&root.path().join("2.tar.lz4"),&destination,&AtomicBool::new(false),&mut |_|{})?;
    assert_eq!(fs::read(destination.join("input.bin"))?,b"short replacement");
    assert!(!run(3,4)?.portable.unwrap().session_reused);
    assert!(!run(4,64)?.portable.unwrap().session_reused);
    // The oldest of three configurations was evicted from the two-entry cache.
    assert!(!run(5,16)?.portable.unwrap().session_reused);
    lttl::clear_portable_sessions();
    assert!(!run(6,16)?.portable.unwrap().session_reused);
    let cancel=AtomicBool::new(false);
    let cancelled_output=root.path().join("cancelled.tar.lz4");
    let cancelled=lttl::create_with_provider(&CreateOptions{inputs:vec![input.clone()],output:cancelled_output.clone(),format:Format::TarLz4,preset:Preset::Fast,threads:1},
        Backend::Wgpu,&GpuOptions::default(),&PortableOptions{adapter:Some(adapter.into()),chunk_kib:16,batch_mib:1},&cancel,
        &mut |_|{cancel.store(true,std::sync::atomic::Ordering::Relaxed);});
    assert!(cancelled.is_err() && !cancelled_output.exists());
    assert!(!run(7,16)?.portable.unwrap().session_reused,"Cancelled encoder must not return its session to the cache");
    lttl::clear_portable_sessions();
    Ok(())
}
#[test] #[ignore="requires Intel hardware; run GPU tests serially"]
fn intel_session_reuse() -> Result<()> {reuse_across_threads("intel")}
#[test] #[ignore="requires NVIDIA hardware; run GPU tests serially"]
fn nvidia_session_reuse() -> Result<()> {reuse_across_threads("nvidia")}
#[test] fn incompatible_format_rejected() {
    let options=CreateOptions{inputs:vec![],output:"unused".into(),format:Format::TarZstd,preset:Preset::Fast,threads:1};
    assert!(lttl::create_with_backend(&options,Backend::Wgpu,&GpuOptions::default(),&AtomicBool::new(false),&mut |_|{}).is_err());
}
