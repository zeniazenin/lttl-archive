use lttl::{CreateOptions, Format, Preset, create, extract, list, test};
use std::{fs, io::Write, path::Path, sync::atomic::{AtomicBool, Ordering}};
use tempfile::TempDir;

fn options(input: &Path, output: &Path, format: Format) -> CreateOptions {
    CreateOptions { inputs: vec![input.to_owned()], output: output.to_owned(), format, preset: Preset::Balanced, threads: 2 }
}
fn fixture(root: &Path) -> std::path::PathBuf {
    let input = root.join("source & Unicode-猫");
    fs::create_dir_all(input.join("nested/empty")).unwrap();
    fs::write(input.join("nested/data.bin"), (0u32..30_000).flat_map(u32::to_le_bytes).collect::<Vec<_>>()).unwrap();
    fs::write(input.join("empty.txt"), []).unwrap();
    fs::write(input.join("hello.txt"), b"Independent archive round trip\n".repeat(400)).unwrap();
    input
}
fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in entries {
        zip.start_file(*name, opts).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn both_formats_round_trip_nested_unicode_and_empty_entries() {
    for format in [Format::Zip, Format::TarZstd] {
        let temp = TempDir::new().unwrap();
        let input = fixture(temp.path());
        let output = temp.path().join(format!("archive.{}", format.extension()));
        let cancel = AtomicBool::new(false);
        let mut seen = 0;
        let stats = create(&options(&input, &output, format), &cancel, &mut |p| seen = p.bytes_done).unwrap();
        assert_eq!(stats.files, 3);
        assert_eq!(seen, stats.input_bytes);
        assert_eq!(test(&output, &cancel).unwrap().output_bytes, stats.input_bytes);
        assert_eq!(list(&output).unwrap().len(), 6);
        let destination = temp.path().join("restored");
        let extracted = extract(&output, &destination, &cancel, &mut |_| {}).unwrap();
        assert_eq!(extracted.files, 3);
        let restored = destination.join(input.file_name().unwrap());
        for name in ["hello.txt", "empty.txt", "nested/data.bin"] {
            assert_eq!(fs::read(input.join(name)).unwrap(), fs::read(restored.join(name)).unwrap());
        }
        assert!(restored.join("nested/empty").is_dir());
    }
}

#[test]
fn create_never_overwrites_existing_archive() {
    let temp = TempDir::new().unwrap();
    let input = fixture(temp.path());
    let output = temp.path().join("archive.zip");
    fs::write(&output, b"keep existing").unwrap();
    assert!(create(&options(&input, &output, Format::Zip), &AtomicBool::new(false), &mut |_| {}).is_err());
    assert_eq!(fs::read(output).unwrap(), b"keep existing");
}

#[test]
fn extraction_preflights_every_collision_before_publishing() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("archive.zip");
    make_zip(&archive, &[("a.txt", b"new"), ("z.txt", b"replacement")]);
    let destination = temp.path().join("destination");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("z.txt"), b"keep").unwrap();
    assert!(extract(&archive, &destination, &AtomicBool::new(false), &mut |_| {}).is_err());
    assert!(!destination.join("a.txt").exists());
    assert_eq!(fs::read(destination.join("z.txt")).unwrap(), b"keep");
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
}

#[test]
fn extraction_can_add_to_existing_directories() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("archive.zip");
    make_zip(&archive, &[("nested/new.txt", b"new")]);
    let destination = temp.path().join("destination");
    fs::create_dir_all(destination.join("nested")).unwrap();
    fs::write(destination.join("nested/keep.txt"), b"keep").unwrap();
    extract(&archive, &destination, &AtomicBool::new(false), &mut |_| {}).unwrap();
    assert_eq!(fs::read(destination.join("nested/new.txt")).unwrap(), b"new");
    assert_eq!(fs::read(destination.join("nested/keep.txt")).unwrap(), b"keep");
}

#[test]
fn reject_traversal_absolute_ads_reserved_and_ambiguous_names() {
    for name in ["../escape.txt", "/absolute.txt", "C:/drive.txt", "a\\..\\escape.txt", "file:stream", "NUL.txt", "COM1", "folder./bad", "folder /bad", "foo//bar", "./dot"] {
        let temp = TempDir::new().unwrap();
        let archive = temp.path().join("bad.zip");
        make_zip(&archive, &[(name, b"malicious")]);
        let destination = temp.path().join("out");
        assert!(extract(&archive, &destination, &AtomicBool::new(false), &mut |_| {}).is_err(), "accepted {name}");
        assert!(!destination.exists());
        assert!(list(&archive).is_err(), "listed unsafe {name}");
    }
}

#[test]
fn reject_case_and_parent_file_collisions() {
    for entries in [vec![("Readme", b"a".as_slice()), ("README", b"b".as_slice())], vec![("parent", b"a".as_slice()), ("parent/child", b"b".as_slice())], vec![("Folder/a", b"a".as_slice()), ("folder/b", b"b".as_slice())]] {
        let temp = TempDir::new().unwrap();
        let archive = temp.path().join("bad.zip");
        make_zip(&archive, &entries);
        assert!(test(&archive, &AtomicBool::new(false)).is_err());
        assert!(extract(&archive, &temp.path().join("out"), &AtomicBool::new(false), &mut |_| {}).is_err());
        assert!(!temp.path().join("out").exists());
    }
}

#[test]
fn detects_zip_crc_corruption_before_publishing() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("bad.zip");
    let payload = b"unique payload for CRC failure";
    make_zip(&archive, &[("data.txt", payload)]);
    let mut bytes = fs::read(&archive).unwrap();
    let index = bytes.windows(payload.len()).position(|p| p == payload).unwrap();
    bytes[index] ^= 1;
    fs::write(&archive, bytes).unwrap();
    assert!(test(&archive, &AtomicBool::new(false)).is_err());
    assert!(extract(&archive, &temp.path().join("out"), &AtomicBool::new(false), &mut |_| {}).is_err());
    assert!(!temp.path().join("out").exists());
}

#[test]
fn detects_zstd_checksum_corruption_after_tar_end_marker() {
    let temp = TempDir::new().unwrap();
    let input = fixture(temp.path());
    let archive = temp.path().join("bad.tar.zst");
    create(&options(&input, &archive, Format::TarZstd), &AtomicBool::new(false), &mut |_| {}).unwrap();
    let mut bytes = fs::read(&archive).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&archive, bytes).unwrap();
    assert!(test(&archive, &AtomicBool::new(false)).is_err());
    assert!(list(&archive).is_err());
    assert!(extract(&archive, &temp.path().join("out"), &AtomicBool::new(false), &mut |_| {}).is_err());
    assert!(!temp.path().join("out").exists());
}

#[test]
fn cancellation_cleans_up_incomplete_archive_and_extraction_staging() {
    for format in [Format::Zip, Format::TarZstd] {
        let temp = TempDir::new().unwrap();
        let input = fixture(temp.path());
        let archive = temp.path().join(format!("archive.{}", format.extension()));
        let cancel = AtomicBool::new(false);
        assert!(create(&options(&input, &archive, format), &cancel, &mut |_| cancel.store(true, Ordering::Relaxed)).is_err());
        assert!(!archive.exists());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
        cancel.store(false, Ordering::Relaxed);
        create(&options(&input, &archive, format), &cancel, &mut |_| {}).unwrap();
        let destination = temp.path().join("out");
        assert!(extract(&archive, &destination, &cancel, &mut |_| cancel.store(true, Ordering::Relaxed)).is_err());
        assert!(!destination.exists());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
    }
}

#[test]
fn gnu_long_filenames_round_trip() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join(format!("{}.txt", "n".repeat(110)));
    fs::write(&input, b"long path").unwrap();
    let archive = temp.path().join("long.tar.zst");
    create(&options(&input, &archive, Format::TarZstd), &AtomicBool::new(false), &mut |_| {}).unwrap();
    let destination = temp.path().join("out");
    extract(&archive, &destination, &AtomicBool::new(false), &mut |_| {}).unwrap();
    assert_eq!(fs::read(destination.join(input.file_name().unwrap())).unwrap(), b"long path");
}

#[test]
fn reject_tar_links_and_oversized_headers() {
    for kind in [tar::EntryType::Symlink, tar::EntryType::Link, tar::EntryType::GNULongName] {
        let temp = TempDir::new().unwrap();
        let archive = temp.path().join("bad.tar.zst");
        let encoder = zstd::stream::write::Encoder::new(fs::File::create(&archive).unwrap(), 1).unwrap();
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(kind);
        header.set_mode(0o644);
        let payload = if kind == tar::EntryType::GNULongName { vec![b'x'; 32_002] } else { vec![] };
        header.set_size(payload.len() as u64);
        header.set_link_name("../outside").unwrap();
        header.set_cksum();
        tar.append_data(&mut header, "entry", payload.as_slice()).unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        assert!(test(&archive, &AtomicBool::new(false)).is_err());
    }
}

#[test]
fn reject_zstd_frames_requesting_excessive_decoder_memory() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("huge-window.tar.zst");
    // Non-single-segment frame, no content size, 1 GiB window, final empty raw
    // block. The window is rejected before any potentially large allocation.
    fs::write(&archive, [0x28, 0xb5, 0x2f, 0xfd, 0x00, 0xa0, 0x01, 0x00, 0x00]).unwrap();
    let error = test(&archive, &AtomicBool::new(false)).unwrap_err();
    assert!(format!("{error:#}").to_lowercase().contains("too much memory"), "{error:#}");
}

#[test]
fn archive_inside_input_is_excluded_from_manifest() {
    for format in [Format::Zip, Format::TarZstd] {
        let temp = TempDir::new().unwrap();
        let input = fixture(temp.path());
        let archive = input.join(format!("output.{}", format.extension()));
        create(&options(&input, &archive, format), &AtomicBool::new(false), &mut |_| {}).unwrap();
        let entries = list(&archive).unwrap();
        assert_eq!(entries.len(), 6);
        assert!(!entries.iter().any(|entry| entry.path.contains("output.")));
    }
}

#[test]
fn changing_input_length_aborts_creation_without_publishing() {
    for format in [Format::Zip, Format::TarZstd] {
        let temp = TempDir::new().unwrap();
        let input = temp.path().join("changing.bin");
        fs::write(&input, vec![42; 500_000]).unwrap();
        let archive = temp.path().join(format!("out.{}", format.extension()));
        let mut changed = false;
        let result = create(&options(&input, &archive, format), &AtomicBool::new(false), &mut |_| {
            if !changed {
                let mut file = fs::OpenOptions::new().append(true).open(&input).unwrap();
                file.write_all(b"appended during compression").unwrap();
                changed = true;
            }
        });
        assert!(result.is_err());
        assert!(!archive.exists());
    }
}

#[test]
fn truncated_tar_member_and_over_limit_size_are_rejected() {
    for declared_size in [4, 100 * 1024 * 1024 * 1024 + 1] {
        let temp = TempDir::new().unwrap();
        let archive = temp.path().join("truncated.tar.zst");
        let mut header = tar::Header::new_gnu();
        header.set_path("data.bin").unwrap();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(declared_size);
        header.set_mode(0o644);
        header.set_cksum();
        let mut encoder = zstd::stream::write::Encoder::new(fs::File::create(&archive).unwrap(), 1).unwrap();
        encoder.include_checksum(true).unwrap();
        encoder.write_all(header.as_bytes()).unwrap();
        encoder.write_all(b"abc").unwrap();
        encoder.finish().unwrap();
        assert!(test(&archive, &AtomicBool::new(false)).is_err());
        assert!(extract(&archive, &temp.path().join("out"), &AtomicBool::new(false), &mut |_| {}).is_err());
        assert!(!temp.path().join("out").exists());
    }
}

#[test]
fn truncated_zstd_frame_is_rejected_even_after_tar_entries() {
    let temp = TempDir::new().unwrap();
    let input = fixture(temp.path());
    let archive = temp.path().join("truncated.tar.zst");
    create(&options(&input, &archive, Format::TarZstd), &AtomicBool::new(false), &mut |_| {}).unwrap();
    let mut bytes = fs::read(&archive).unwrap();
    bytes.truncate(bytes.len() - 4);
    fs::write(&archive, bytes).unwrap();
    assert!(test(&archive, &AtomicBool::new(false)).is_err());
    assert!(list(&archive).is_err());
}

fn seeded_bytes(len: usize) -> Vec<u8> {
    let mut state = 0x9e3779b97f4a7c15u64;
    (0..len).map(|_| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state as u8
    }).collect()
}

#[test]
fn zip_fast_stores_sampled_random_data_but_deflates_text_and_round_trips() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input");
    fs::create_dir(&input).unwrap();
    let random = seeded_bytes(2 * 1024 * 1024);
    let text = b"Repeated text should be compressed regardless of file extension.\n".repeat(40_000);
    // Deliberately misleading extensions ensure content controls the decision.
    fs::write(input.join("random.txt"), &random).unwrap();
    fs::write(input.join("text.jpg"), &text).unwrap();
    let output = temp.path().join("fast.zip");
    let mut opts = options(&input, &output, Format::Zip);
    opts.preset = Preset::Fast;
    let mut saw_storing = false;
    create(&opts, &AtomicBool::new(false), &mut |progress| {
        saw_storing |= progress.phase.starts_with("Storing ZIP");
    }).unwrap();
    assert!(saw_storing);
    {
        let mut zip = zip::ZipArchive::new(fs::File::open(&output).unwrap()).unwrap();
        assert_eq!(zip.by_name("input/random.txt").unwrap().compression(), zip::CompressionMethod::Stored);
        assert_eq!(zip.by_name("input/text.jpg").unwrap().compression(), zip::CompressionMethod::Deflated);
    }
    test(&output, &AtomicBool::new(false)).unwrap();
    let destination = temp.path().join("out");
    extract(&output, &destination, &AtomicBool::new(false), &mut |_| {}).unwrap();
    assert_eq!(fs::read(destination.join("input/random.txt")).unwrap(), random);
    assert_eq!(fs::read(destination.join("input/text.jpg")).unwrap(), text);
}

#[test]
fn zip_sampling_is_fast_only_and_requires_every_region_to_resist_compression() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("random.bin");
    let random = seeded_bytes(1024 * 1024);
    fs::write(&input, &random).unwrap();
    for preset in [Preset::Balanced, Preset::Small] {
        let output = temp.path().join(format!("{preset:?}.zip"));
        let mut opts = options(&input, &output, Format::Zip);
        opts.preset = preset;
        create(&opts, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
        assert_eq!(zip.by_index(0).unwrap().compression(), zip::CompressionMethod::Deflated);
    }
    let mut mixed = random.clone();
    let middle = (mixed.len() - 64 * 1024) / 2;
    mixed[middle..middle + 64 * 1024].fill(0);
    for (filename, bytes) in [("small.bin", &random[..random.len() - 1]), ("mixed.bin", mixed.as_slice())] {
        let input = temp.path().join(filename);
        fs::write(&input, bytes).unwrap();
        let output = temp.path().join(format!("{filename}.zip"));
        let mut opts = options(&input, &output, Format::Zip);
        opts.preset = Preset::Fast;
        create(&opts, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
        assert_eq!(zip.by_index(0).unwrap().compression(), zip::CompressionMethod::Deflated);
    }
}
