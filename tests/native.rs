use anyhow::Result;
use lttl::{Backend, CreateOptions, Format, Preset, GpuOptions, create_with_backend, extract, list, test};
use std::{fs, sync::atomic::AtomicBool};

fn sensor(size: usize) -> Vec<u8> {
    let mut random = 91u64; let mut channels = [12000u16; 16];
    (0..size/2).flat_map(|i| {
        random ^= random << 13; random ^= random >> 7; random ^= random << 17;
        channels[i%16] = channels[i%16].wrapping_add((random % 7) as u16).wrapping_sub(3);
        channels[i%16].to_le_bytes()
    }).collect()
}

#[test]
fn native_presets_auto_mixed_files_and_encryption_round_trip() -> Result<()> {
    let dir = tempfile::tempdir()?; let root = dir.path().join("input"); fs::create_dir_all(root.join("empty"))?;
    fs::write(root.join("sensor.bin"), sensor(3*1024*1024))?;
    fs::write(root.join("text.txt"), b"readable text with no numeric channels\n".repeat(10_000))?;
    fs::write(root.join("zero.txt"), [])?;
    let cancel = AtomicBool::new(false);
    for (i, backend) in [Backend::LttlFast, Backend::LttlDense, Backend::Auto, Backend::Cpu].into_iter().enumerate() {
        let archive = dir.path().join(format!("{i}.lttl"));
        let options = CreateOptions { inputs: vec![root.clone()], output: archive.clone(), format: Format::Lttl, preset: Preset::Balanced, threads: 2 };
        let stats = create_with_backend(&options, backend, &GpuOptions::default(), &cancel, &mut |_| {})?;
        let metrics = stats.numeric.unwrap();
        if backend == Backend::Auto { assert!(metrics.blocks.contains_key("lttl-dense") || metrics.blocks.contains_key("lttl-fast")); assert!(metrics.blocks.contains_key("zstd")); }
        assert!(!lttl::identity::is_protected(&archive)?);
        assert_eq!(test(&archive, &cancel)?.output_bytes, stats.input_bytes);
        assert_eq!(list(&archive)?.len(), 5);
        let dest = dir.path().join(format!("out-{i}")); extract(&archive, &dest, &cancel, &mut |_| {})?;
        for name in ["sensor.bin", "text.txt", "zero.txt"] { assert_eq!(fs::read(root.join(name))?, fs::read(dest.join("input").join(name))?); }
        let key = lttl::identity::crypto::key();
        let header = lttl::identity::crypto::Header::new("https://keys.example".into(), lttl::identity::crypto::identifier(), "lttl".into());
        let mut protected = Vec::new();
        lttl::identity::crypto::encrypt(&mut fs::File::open(&archive)?, &mut protected, &header, &key, &cancel)?;
        let mut restored = Vec::new();
        lttl::identity::crypto::decrypt(&mut protected.as_slice(), &mut restored, &key, &cancel)?;
        assert_eq!(restored, fs::read(&archive)?);
    }
    Ok(())
}

#[test]
fn multiple_blocks_and_tail_have_independent_verified_frames() -> Result<()> {
    let dir = tempfile::tempdir()?; let input = dir.path().join("large.bin");
    let mut bytes = sensor(16*1_048_560+18); bytes.push(0x97); fs::write(&input, &bytes)?;
    let archive = dir.path().join("large.lttl");
    let options = CreateOptions { inputs: vec![input], output: archive.clone(), format: Format::Lttl, preset: Preset::Fast, threads: 2 };
    let cancel = AtomicBool::new(false);
    let stats = create_with_backend(&options, Backend::LttlDense, &GpuOptions::default(), &cancel, &mut |_| {})?;
    assert_eq!(stats.numeric.unwrap().blocks.values().sum::<u64>(), 2);
    let dest = dir.path().join("out"); extract(&archive, &dest, &cancel, &mut |_| {})?;
    assert_eq!(fs::read(dest.join("large.bin"))?, bytes);
    Ok(())
}

fn header(path: &str, size: u64) -> Vec<u8> {
    let metadata = serde_json::to_vec(&serde_json::json!({"path":path,"size":size,"is_dir":false})).unwrap();
    let mut out = b"LTTLAR01".to_vec(); out.extend((metadata.len() as u32).to_le_bytes());
    out.extend(&metadata); out.extend(xxhash_rust::xxh64::xxh64(&metadata,0).to_le_bytes()); out
}

#[test]
fn malformed_native_frames_are_bounded_and_never_published() -> Result<()> {
    let dir = tempfile::tempdir()?; let archive = dir.path().join("bad.lttl"); let cancel = AtomicBool::new(false);
    let mut cases = Vec::new();
    for path in ["../escape", "CON", "a:b", "a\\b", "/absolute"] { let mut b = header(path,0); b.extend(0u32.to_le_bytes()); cases.push(b); }
    cases.push(header("huge",100*1024*1024*1024+1));
    let mut oversized = header("oversized",1); oversized.extend(u32::MAX.to_le_bytes()); cases.push(oversized);
    let mut bomb = header("bomb",1); bomb.extend(30u32.to_le_bytes()); bomb.extend(b"LTNV0002");
    bomb.extend((512u64*1024*1024).to_le_bytes()); bomb.extend([0;14]); cases.push(bomb);
    let mut trailing = b"LTTLAR01".to_vec(); trailing.extend([0;5]); cases.push(trailing);
    let mut duplicate = header("file",0); duplicate.extend(&header("FILE",0)[8..]); duplicate.extend([0;4]); cases.push(duplicate);
    let mut metadata = header("file",0); metadata[15] ^= 1; cases.push(metadata);
    for data in cases { fs::write(&archive,data)?;
        assert!(test(&archive,&cancel).is_err()); assert!(list(&archive).is_err());
        assert!(extract(&archive,&dir.path().join("out"),&cancel,&mut |_| {}).is_err()); assert!(!dir.path().join("out").exists());
    }
    // Corruption and truncation of valid payloads exercise the decoder as well.
    let input = dir.path().join("original"); fs::write(&input,sensor(300_000))?;
    let good = dir.path().join("good.lttl");
    let opts = CreateOptions { inputs: vec![input], output: good.clone(), format: Format::Lttl, preset: Preset::Balanced, threads: 1 };
    create_with_backend(&opts, Backend::LttlDense, &GpuOptions::default(), &cancel, &mut |_| {})?;
    let original = fs::read(&good)?;
    for end in [0, 7, 12, original.len()-1, original.len()-5] { fs::write(&archive,&original[..end])?; assert!(test(&archive,&cancel).is_err()); }
    let mut changed = original; let at = changed.len()-6; changed[at] ^= 0x80; fs::write(&archive,changed)?;
    assert!(extract(&archive,&dir.path().join("out"),&cancel,&mut |_| {}).is_err()); assert!(!dir.path().join("out").exists());
    Ok(())
}
