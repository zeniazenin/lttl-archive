use anyhow::Result;
use lttl::{Backend, CreateOptions, Format, GpuOptions, Preset, create_with_backend, extract, test};
use std::{fs, sync::atomic::{AtomicBool, Ordering}};

fn roundtrip(backend: Backend, chunk_kib: usize) -> Result<()> {
    let temp = tempfile::tempdir()?;
    let input = temp.path().join("data-é"); fs::create_dir(&input)?;
    fs::create_dir(input.join("empty"))?;
    fs::write(input.join("zero.txt"), [])?;
    let bytes: Vec<_> = (0..17*1024*1024).map(|n| ((n * 13 + n / 65537) % 251) as u8).collect();
    fs::write(input.join("mixed.bin"), &bytes)?;
    let output = temp.path().join("archive.tar.zst");
    let options = CreateOptions { inputs: vec![input.clone()], output: output.clone(), format: Format::TarZstd, preset: Preset::Fast, threads: 4 };
    let cancel = AtomicBool::new(false);
    let stats = create_with_backend(&options, backend, &GpuOptions { chunk_kib, batch_mib: 16 }, &cancel, &mut |_| {})?;
    assert_eq!(stats.input_bytes, bytes.len() as u64);
    if backend == Backend::Gpu {
        let gpu = stats.gpu.expect("actual GPU metrics");
        assert!(gpu.batches >= 2 && gpu.chunks >= 18 && gpu.h2d_bytes >= bytes.len() as u64);
    }
    test(&output, &cancel)?;
    let dest = temp.path().join("restored");
    extract(&output, &dest, &cancel, &mut |_| {})?;
    assert_eq!(fs::read(dest.join("data-é/mixed.bin"))?, bytes);
    assert!(dest.join("data-é/empty").is_dir());
    assert_eq!(fs::metadata(dest.join("data-é/zero.txt"))?.len(), 0);
    // Every published frame, including the final TAR padding frame, has a checksum.
    let mut archive = fs::read(&output)?;
    let last = archive.len() - 1; archive[last] ^= 1;
    fs::write(temp.path().join("corrupt.tar.zst"), archive)?;
    assert!(test(&temp.path().join("corrupt.tar.zst"), &cancel).is_err());
    let cancelled_output = temp.path().join("cancelled.tar.zst");
    let cancelled_options = CreateOptions { output: cancelled_output.clone(), ..options };
    assert!(create_with_backend(&cancelled_options, backend, &GpuOptions::default(), &cancel, &mut |p| {
        if p.bytes_done > 1024 * 1024 { cancel.store(true, Ordering::Relaxed); }
    }).is_err());
    assert!(!cancelled_output.exists());
    Ok(())
}

#[test]
fn chunked_cpu_multiframe_roundtrip_and_cancellation() -> Result<()> { roundtrip(Backend::CpuChunked, 1024) }

#[test]
#[ignore = "requires installed NVIDIA runtime and GPU; run with --ignored"]
fn actual_gpu_multibatch_roundtrip_and_cancellation() -> Result<()> { roundtrip(Backend::Gpu, 1024)?; roundtrip(Backend::Gpu, 64) }

#[test]
fn rejects_invalid_options_before_publication() -> Result<()> {
    let temp = tempfile::tempdir()?; let input = temp.path().join("input"); fs::write(&input, b"test")?;
    let output = temp.path().join("out.tar.zst");
    let options = CreateOptions { inputs: vec![input], output: output.clone(), format: Format::TarZstd, preset: Preset::Fast, threads: 1 };
    assert!(create_with_backend(&options, Backend::CpuChunked, &GpuOptions {chunk_kib:0,batch_mib:32}, &AtomicBool::new(false), &mut |_| {}).is_err());
    assert!(!output.exists());
    Ok(())
}
