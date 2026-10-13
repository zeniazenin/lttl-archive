use anyhow::Result;
use lttl::{Backend, CreateOptions, Format, GpuOptions, Preset, create_with_backend, extract, test};
use std::{fs, sync::atomic::{AtomicBool, Ordering}};

fn noise(bytes: usize) -> Vec<u8> {
    let mut state = 0x9d365bad2124a115u64;
    (0..bytes).map(|_| { state ^= state << 13; state ^= state >> 7; state ^= state << 17; state as u8 }).collect()
}
fn options(input: std::path::PathBuf, output: std::path::PathBuf) -> CreateOptions {
    CreateOptions { inputs: vec![input], output, format: Format::TarZstd, preset: Preset::Balanced, threads: 0 }
}
#[test]
fn raw_frames_roundtrip_and_detect_corruption_despite_misleading_extension() -> Result<()> {
    let dir = tempfile::tempdir()?; let path = dir.path().join("looks-like-text.csv");
    let data = noise(9 * 1_048_576 + 53); fs::write(&path, &data)?;
    let output = dir.path().join("archive.tar.zst");
    let stats = create_with_backend(&options(path, output.clone()), Backend::Auto, &GpuOptions::default(), &AtomicBool::new(false), &mut |_| {})?;
    let choice = stats.selection.unwrap(); assert!(choice.stored); assert_eq!(choice.content_class, "low-compression-benefit");
    assert!(choice.sampled_bytes <= 1536 * 1024 && stats.elapsed_seconds >= choice.selection_seconds);
    assert!(stats.gpu.is_none()); test(&output, &AtomicBool::new(false))?;
    let dest = dir.path().join("unpacked"); extract(&output, &dest, &AtomicBool::new(false), &mut |_| {})?;
    assert_eq!(fs::read(dest.join("looks-like-text.csv"))?, data);
    let mut broken = fs::read(&output)?; let last = broken.len() - 1; broken[last] ^= 1;
    fs::write(&output, broken)?; assert!(test(&output, &AtomicBool::new(false)).is_err());
    Ok(())
}
#[test]
fn related_content_selects_cpu_history_and_small_input_avoids_trials() -> Result<()> {
    let dir = tempfile::tempdir()?; let source = dir.path().join("related"); fs::create_dir(&source)?;
    let base = noise(1_048_576);
    for i in 0..10 { let mut bytes = base.clone(); bytes[1_048_575] = i; fs::write(source.join(format!("{i}.jpg")), bytes)?; }
    let stats = create_with_backend(&options(source, dir.path().join("related.tar.zst")), Backend::Auto, &GpuOptions::default(), &AtomicBool::new(false), &mut |_| {})?;
    let selected = stats.selection.unwrap(); assert_eq!(selected.backend, Backend::Cpu); assert_eq!(selected.preset, Preset::Small);
    assert_eq!(selected.content_class, "related-files"); assert!(!selected.stored);
    assert!(selected.cpu_fast_ratio.is_none() && selected.cpu_balanced_ratio.is_none() && selected.cpu_small_ratio.is_none());
    assert!(stats.output_bytes < stats.input_bytes / 3);
    let tiny = dir.path().join("tiny"); fs::write(&tiny, "a small file")?;
    let small = create_with_backend(&options(tiny, dir.path().join("small.tar.zst")), Backend::Auto, &GpuOptions::default(), &AtomicBool::new(false), &mut |_| {})?.selection.unwrap();
    assert_eq!(small.sampled_bytes, 0); assert_eq!(small.threads, 1); assert!(small.gpu_trial.is_none());
    Ok(())
}
#[test]
fn cancellation_during_selection_publishes_nothing() -> Result<()> {
    let dir = tempfile::tempdir()?; let input = dir.path().join("large"); fs::write(&input, vec![7u8; 9 * 1_048_576])?;
    let output = dir.path().join("out.tar.zst"); let cancel = AtomicBool::new(false);
    let result = create_with_backend(&options(input, output.clone()), Backend::Auto, &GpuOptions::default(), &cancel, &mut |p| {
        if p.phase.starts_with("Auto:") { cancel.store(true, Ordering::Relaxed); }
    });
    assert!(result.is_err()); assert!(!output.exists()); Ok(())
}

#[test]
fn fast_bypasses_sampling_and_matches_explicit_cpu() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("noise.csv"); fs::write(&input, noise(9 * 1_048_576))?;
    let archive = dir.path().join("auto.tar.zst");
    let mut opts = options(input, archive.clone()); opts.preset = Preset::Fast;
    let stats = create_with_backend(&opts, Backend::Auto, &GpuOptions::default(), &AtomicBool::new(false), &mut |p| {
        assert!(!p.phase.starts_with("Auto:"), "Fast must not start content/GPU probes");
    })?;
    let choice = stats.selection.unwrap();
    assert_eq!(choice.policy_version, 2); assert_eq!(choice.sampled_bytes, 0); assert_eq!(choice.sampled_files, 0);
    assert_eq!(choice.backend, Backend::Cpu); assert_eq!(choice.preset, Preset::Fast); assert!(!choice.stored);
    assert!(choice.cpu_fast_ratio.is_none() && choice.cpu_balanced_ratio.is_none() && choice.gpu_trial.is_none());
    opts.output = dir.path().join("cpu.tar.zst");
    create_with_backend(&opts, Backend::Cpu, &GpuOptions::default(), &AtomicBool::new(false), &mut |_| {})?;
    assert_eq!(fs::read(archive)?, fs::read(opts.output)?);
    Ok(())
}

#[test]
fn balanced_reduced_samples_still_check_tail_and_small_probes_reset_state() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("data.zip");
    let mut data = noise(9 * 1_048_576);
    let length = data.len(); data[length - 1_048_576..].fill(0);
    fs::write(&input, &data)?;
    let opts = options(input.clone(), dir.path().join("balanced.tar.zst"));
    let selected = create_with_backend(&opts, Backend::Auto, &GpuOptions::default(), &AtomicBool::new(false), &mut |_| {})?.selection.unwrap();
    assert_eq!(selected.sampled_bytes, 192 * 1024);
    assert!(!selected.stored, "Compressible tail must prevent the raw-storage shortcut");
    let mut opts = options(input, dir.path().join("small.tar.zst")); opts.preset = Preset::Small;
    let choice = create_with_backend(&opts, Backend::Auto, &GpuOptions::default(), &AtomicBool::new(false), &mut |_| {})?.selection.unwrap();
    let width = 512 * 1024;
    let sample: Vec<u8> = [0, (length - width) / 2, length - width].into_iter().flat_map(|start| data[start..start+width].iter().copied()).collect();
    // Independent fresh contexts establish that reusing a probe context does not
    // leak compression history/settings from an earlier level into a later one.
    for (level, ratio) in [(1, choice.cpu_fast_ratio), (3, choice.cpu_balanced_ratio), (9, choice.cpu_small_ratio)] {
        let expected = zstd::bulk::compress(&sample, level)?.len() as f64 / sample.len() as f64;
        assert_eq!(ratio, Some(expected));
    }
    test(&opts.output, &AtomicBool::new(false))?;
    Ok(())
}
