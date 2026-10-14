//! Streaming archive engine. No WinRAR code is incorporated.

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File, Metadata, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
use tempfile::{NamedTempFile, TempDir};
use zip::write::SimpleFileOptions;
mod gpu;
mod portable;
pub use portable::{PortableOptions, PortableMetrics, adapters as portable_adapters, clear_sessions as clear_portable_sessions};
pub use gpu::{Backend, GpuOptions, GpuMetrics, gpu_status};
mod auto;
mod numeric;
mod native_archive;
pub use native_archive::Metrics as NumericMetrics;
pub mod identity;
pub use auto::Selection;

const MAX_BYTES: u64 = 100 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 1_000_000;
const BUFFER_SIZE: usize = 128 * 1024;
const ZIP_SAMPLE_MIN_SIZE: u64 = 1024 * 1024;
const ZIP_SAMPLE_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
pub enum Format { Lttl, Zip, TarZstd, TarLz4 }
impl Format {
    pub fn extension(self) -> &'static str {
        match self { Self::Lttl => "lttl", Self::Zip => "zip", Self::TarZstd => "tar.zst", Self::TarLz4 => "tar.lz4" }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
pub enum Preset { Fast, Balanced, Small }
#[derive(Debug, Clone)]
pub struct CreateOptions {
    pub inputs: Vec<PathBuf>, pub output: PathBuf, pub format: Format,
    pub preset: Preset, pub threads: u32,
}
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub phase: String, pub path: String, pub bytes_done: u64, pub bytes_total: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub input_bytes: u64, pub output_bytes: u64, pub files: u64, pub elapsed_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<GpuMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portable: Option<PortableMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numeric: Option<NumericMetrics>,
}
#[derive(Debug, Clone, Serialize)]
pub struct EntryInfo { pub path: String, pub size: u64, pub is_dir: bool }

fn cancelled(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "Operation cancelled");
    Ok(())
}
fn is_link(meta: &Metadata) -> bool {
    if meta.file_type().is_symlink() { return true; }
    #[cfg(windows)] {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 { return true; }
    }
    false
}

// Reject reparse points in every existing component, including the supplied
// destination and input ancestors. Call again immediately before publication.
fn check_path(path: &Path) -> Result<()> {
    let absolute = if path.is_absolute() { path.to_owned() } else { std::env::current_dir()?.join(path) };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        ensure!(!matches!(component, Component::ParentDir), "Parent-directory components are not allowed: {}", path.display());
        current.push(component);
        // A Windows prefix (notably \\?\C:) is not a complete directory
        // until the following root separator has been appended.
        if matches!(component, Component::Prefix(_)) { continue; }
        match fs::symlink_metadata(&current) {
            Ok(meta) => ensure!(!is_link(&meta), "Links/reparse points are not allowed: {}", current.display()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error).with_context(|| format!("Inspecting {}", current.display())),
        }
    }
    Ok(())
}
fn input_file(path: &Path) -> Result<File> {
    check_path(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)] {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options.open(path).with_context(|| format!("Opening {}", path.display()))?;
    let metadata = file.metadata()?;
    ensure!(!is_link(&metadata) && metadata.is_file(), "Not a regular input file: {}", path.display());
    Ok(file)
}
fn parent_dir(path: &Path) -> &Path {
    path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."))
}
fn absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => bail!("Refusing to overwrite {}", path.display()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Inspecting {}", path.display())),
    }
}

/// Portable Windows-safe member paths, also enforced on non-Windows hosts.
fn member_name(raw: &str, is_dir: bool) -> Result<String> {
    let name = if is_dir { raw.strip_suffix('/').unwrap_or(raw) } else { raw };
    ensure!(!name.is_empty() && name.len() <= 32_000, "Invalid archive member path length");
    ensure!(!name.contains('\\'), "Backslashes are not allowed in archive member paths: {name}");
    for part in name.split('/') {
        ensure!(!part.is_empty() && part != "." && part != "..", "Unsafe archive member path: {name}");
        ensure!(part.encode_utf16().count() <= 255, "Archive filename component is too long: {name}");
        ensure!(!part.ends_with(['.', ' ']), "Trailing dot/space in archive member: {name}");
        ensure!(!part.chars().any(|c| c < ' ' || ":<>\"|?*".contains(c)), "Unsafe Windows filename: {name}");
        let base = part.split('.').next().unwrap_or("").trim_end_matches(' ').to_uppercase();
        let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].iter().any(|prefix| {
                base.strip_prefix(prefix).is_some_and(|suffix| matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"))
            });
        ensure!(!reserved, "Reserved Windows device filename: {name}");
    }
    Ok(name.to_owned())
}

#[derive(Default)]
struct Names {
    // Includes implicit parent directories, to reject file/directory collisions.
    nodes: BTreeMap<String, (String, bool)>,
    explicit: HashSet<String>,
    bytes: u64,
}
impl Names {
    fn add(&mut self, raw: &str, size: u64, is_dir: bool) -> Result<EntryInfo> {
        ensure!(self.explicit.len() < MAX_ENTRIES, "Archive exceeds {MAX_ENTRIES} entries");
        let name = member_name(raw, is_dir)?;
        let folded = name.to_lowercase();
        ensure!(self.explicit.insert(folded), "Duplicate archive member: {name}");
        let parts: Vec<_> = name.split('/').collect();
        for end in 1..=parts.len() {
            let partial = parts[..end].join("/");
            let directory = end < parts.len() || is_dir;
            if let Some((existing, was_dir)) = self.nodes.get(&partial.to_lowercase()) {
                ensure!(existing == &partial && *was_dir && directory, "Case or file/directory collision: {partial}");
            } else {
                ensure!(self.nodes.len() < MAX_ENTRIES, "Archive exceeds directory/entry limit");
                self.nodes.insert(partial.to_lowercase(), (partial, directory));
            }
        }
        self.bytes = self.bytes.checked_add(size).context("Archive expanded size overflow")?;
        ensure!(self.bytes <= MAX_BYTES, "Archive exceeds 100 GiB expansion limit");
        Ok(EntryInfo { path: name, size, is_dir })
    }
}

struct Source { path: PathBuf, info: EntryInfo }
fn sources(inputs: &[PathBuf], cancel: &AtomicBool) -> Result<(Vec<Source>, u64)> {
    ensure!(!inputs.is_empty(), "Select at least one input file or folder");
    let mut result = Vec::new();
    let mut names = Names::default();
    for input in inputs {
        cancelled(cancel)?;
        check_path(input)?;
        let absolute = fs::canonicalize(input).with_context(|| format!("Reading {}", input.display()))?;
        let root = absolute.file_name().context("Select a folder below the filesystem root")?.to_str().context("Input name is not valid Unicode")?;
        for entry in walkdir::WalkDir::new(&absolute).follow_links(false).sort_by_file_name() {
            cancelled(cancel)?;
            let entry = entry?;
            let meta = fs::symlink_metadata(entry.path())?;
            ensure!(!is_link(&meta), "Links/reparse points are not supported: {}", entry.path().display());
            ensure!(meta.is_dir() || meta.is_file(), "Unsupported input type: {}", entry.path().display());
            let relative = entry.path().strip_prefix(&absolute)?;
            let suffix = relative.components().map(|part| part.as_os_str().to_str().context("Input name is not valid Unicode")).collect::<Result<Vec<_>>>()?.join("/");
            let archive_path = if suffix.is_empty() { root.to_owned() } else { format!("{root}/{suffix}") };
            let info = names.add(&archive_path, if meta.is_file() { meta.len() } else { 0 }, meta.is_dir())?;
            result.push(Source { path: entry.path().to_owned(), info });
        }
    }
    Ok((result, names.bytes))
}

struct ProgressReader<'a, R> {
    inner: R, cancel: &'a AtomicBool, callback: &'a mut dyn FnMut(Progress),
    done: &'a mut u64, total: u64, path: &'a str, phase: &'a str,
}
impl<R: Read> Read for ProgressReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) { return Err(io::Error::other("Operation cancelled")); }
        let count = self.inner.read(buffer)?;
        *self.done += count as u64;
        if count > 0 {
            (self.callback)(Progress { phase: self.phase.into(), path: self.path.into(), bytes_done: *self.done, bytes_total: self.total });
        }
        Ok(count)
    }
}
fn copy_checked(reader: &mut dyn Read, writer: &mut dyn Write, expected: u64) -> Result<()> {
    let mut copied = 0u64;
    let mut buffer = vec![0; BUFFER_SIZE];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 { break; }
        copied = copied.checked_add(count as u64).context("Entry size overflow")?;
        ensure!(copied <= expected, "Entry exceeds declared size");
        writer.write_all(&buffer[..count])?;
    }
    ensure!(copied == expected, "Entry truncated or input changed: expected {expected} bytes, read {copied}");
    Ok(())
}

/// Fast-preset policy, not a guarantee of incompressibility: three bounded
/// samples can miss redundancy elsewhere, and Zstandard's estimate is not
/// Deflate's exact ratio. Store only when every probe retains at least 99% of
/// its original bytes. The complete original file is subsequently copied;
/// sample bytes and compressed probes never become archive payloads.
fn zip_samples_favor_storing(file: &mut File, size: u64, cancel: &AtomicBool) -> Result<bool> {
    if size < ZIP_SAMPLE_MIN_SIZE { return Ok(false); }
    let sample_size = ZIP_SAMPLE_SIZE as u64;
    let offsets = [0, (size - sample_size) / 2, size - sample_size];
    let mut buffer = vec![0; ZIP_SAMPLE_SIZE];
    let mut compressor = zstd::bulk::Compressor::new(1)?;
    let mut all_incompressible = true;
    for offset in offsets {
        cancelled(cancel)?;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut buffer).context("Input changed or could not be read during ZIP sampling")?;
        let compressed = compressor.compress(&buffer)?;
        all_incompressible &= compressed.len() * 100 >= buffer.len() * 99;
    }
    file.seek(SeekFrom::Start(0))?;
    cancelled(cancel)?;
    Ok(all_incompressible)
}

pub fn create(options: &CreateOptions, cancel: &AtomicBool, progress: &mut dyn FnMut(Progress)) -> Result<Stats> {
    create_with_backend(options, Backend::Cpu, &GpuOptions::default(), cancel, progress)
}

pub fn create_with_backend(options: &CreateOptions, backend: Backend, gpu_options: &GpuOptions, cancel: &AtomicBool, progress: &mut dyn FnMut(Progress)) -> Result<Stats> {
    create_with_provider(options, backend, gpu_options, &PortableOptions::default(), cancel, progress)
}

pub fn create_with_provider(options: &CreateOptions, backend: Backend, gpu_options: &GpuOptions, portable_options: &PortableOptions, cancel: &AtomicBool, progress: &mut dyn FnMut(Progress)) -> Result<Stats> {
    let is_auto = backend == Backend::Auto;
    let started = Instant::now();
    cancelled(cancel)?;
    ensure!(matches!(backend, Backend::Cpu | Backend::Auto) || (options.format == Format::Lttl && matches!(backend, Backend::LttlFast | Backend::LttlDense)) || (options.format == Format::TarZstd && matches!(backend, Backend::Gpu | Backend::CpuChunked)) || (options.format == Format::TarLz4 && backend == Backend::Wgpu), "lttl Fast/Dense require LTTL; wgpu requires TAR.LZ4; NVIDIA Zstd and CPU-chunked require TAR.ZST");
    ensure!(options.threads <= 256, "CPU thread count must be at most 256");
    check_path(&options.output)?;
    absent(&options.output)?;
    let (sources, total) = sources(&options.inputs, cancel)?;
    let mut selected = None;
    let mut prepared = None;
    let mut effective = options.clone();
    let mut gpu_settings = gpu_options.clone();
    let mut stored = false;
    let backend = if backend == Backend::Auto && options.format == Format::TarZstd {
        let plan = auto::select(&sources, total, options.preset, options.threads, cancel, progress)?;
        effective.preset = plan.report.preset;
        effective.threads = plan.report.threads;
        gpu_settings = GpuOptions { chunk_kib: plan.report.chunk_kib, batch_mib: plan.report.batch_mib };
        stored = plan.report.stored;
        let actual = plan.report.backend;
        prepared = plan.probe;
        selected = Some(plan.report);
        actual
    } else if backend == Backend::Auto && options.format != Format::Lttl { Backend::Cpu } else { backend };
    let options = &effective;
    let mut temporary = NamedTempFile::new_in(parent_dir(&options.output))?;
    let mut done = 0;
    let mut files = 0;
    let mut gpu_stats = None;
    let mut portable_stats = None;
    let mut numeric_stats = None;
    match options.format {
        Format::Lttl => {
            let mut writer = BufWriter::new(temporary.as_file_mut());
            numeric_stats = Some(native_archive::create(&mut writer, &sources, total, options, backend, cancel, progress)?);
            writer.flush()?;
            files = sources.iter().filter(|s| !s.info.is_dir).count() as u64;
        }
        Format::Zip => {
            let mut archive = zip::ZipWriter::new(BufWriter::new(temporary.as_file_mut()));
            let level = match options.preset { Preset::Fast => 1, Preset::Balanced => 6, Preset::Small => 9 };
            let file_options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).compression_level(Some(level)).large_file(true);
            for source in &sources {
                cancelled(cancel)?;
                if source.info.is_dir { archive.add_directory(format!("{}/", source.info.path), file_options)?; continue; }
                let mut file = input_file(&source.path)?;
                ensure!(file.metadata()?.len() == source.info.size, "Input size changed: {}", source.path.display());
                let store = if options.preset == Preset::Fast && source.info.size >= ZIP_SAMPLE_MIN_SIZE {
                    progress(Progress { phase: "Sampling ZIP compression benefit".into(), path: source.info.path.clone(), bytes_done: done, bytes_total: total });
                    zip_samples_favor_storing(&mut file, source.info.size, cancel)?
                } else { false };
                let entry_options = if store {
                    file_options.compression_method(zip::CompressionMethod::Stored).compression_level(None)
                } else { file_options };
                archive.start_file(&source.info.path, entry_options)?;
                let phase = if store { "Storing ZIP (samples show little compression benefit)" } else { "Creating ZIP" };
                let mut reader = ProgressReader { inner: file, cancel, callback: progress, done: &mut done, total, path: &source.info.path, phase };
                copy_checked(&mut reader, &mut archive, source.info.size)?;
                files += 1;
            }
            archive.finish()?.flush()?;
        }
        Format::TarZstd | Format::TarLz4 => {
            let level = match options.preset { Preset::Fast => 1, Preset::Balanced => 3, Preset::Small => 9 };
            let workers = if options.threads == 0 { std::thread::available_parallelism().map(|v| v.get().min(8) as u32).unwrap_or(1) } else { options.threads };
            let encoder = if options.format == Format::TarLz4 { gpu::TarEncoder::Portable(Box::new(portable::Encoder::new(BufWriter::new(temporary.as_file_mut()), backend == Backend::Wgpu, portable_options, workers, is_auto, cancel)?)) } else if stored { gpu::TarEncoder::stored(BufWriter::new(temporary.as_file_mut()), cancel) }
                else { gpu::TarEncoder::with_probe(BufWriter::new(temporary.as_file_mut()), backend, &gpu_settings, level, workers, cancel, prepared)? };
            let mut archive = tar::Builder::new(encoder);
            for source in &sources {
                cancelled(cancel)?;
                let mut header = tar::Header::new_gnu();
                header.set_size(source.info.size);
                header.set_mode(if source.info.is_dir { 0o755 } else { 0o644 });
                header.set_mtime(0);
                header.set_entry_type(if source.info.is_dir { tar::EntryType::Directory } else { tar::EntryType::Regular });
                header.set_cksum();
                if source.info.is_dir {
                    archive.append_data(&mut header, &source.info.path, io::empty())?;
                } else {
                    let mut file = input_file(&source.path)?;
                    ensure!(file.metadata()?.len() == source.info.size, "Input size changed: {}", source.path.display());
                    let before = done;
                    let mut reader = ProgressReader { inner: &mut file, cancel, callback: progress, done: &mut done, total, path: &source.info.path, phase: "Creating TAR archive" };
                    archive.append_data(&mut header, &source.info.path, &mut reader)?;
                    ensure!(done - before == source.info.size, "Input size changed while reading: {}", source.path.display());
                    ensure!(file.metadata()?.len() == source.info.size, "Input size changed while archiving: {}", source.path.display());
                    files += 1;
                }
            }
            (gpu_stats, portable_stats) = archive.into_inner()?.finish()?;
        }
    }
    cancelled(cancel)?;
    temporary.as_file().sync_all()?;
    let output_bytes = temporary.as_file().metadata()?.len();
    check_path(&options.output)?;
    temporary.persist_noclobber(&options.output).map_err(|error| error.error).context("Publishing archive without overwriting")?;
    progress(Progress { phase: "Complete".into(), path: options.output.display().to_string(), bytes_done: total, bytes_total: total });
    Ok(Stats { input_bytes: total, output_bytes, files, elapsed_seconds: started.elapsed().as_secs_f64(), gpu: gpu_stats, portable: portable_stats, selection: selected, numeric: numeric_stats })
}

// Bounds TAR metadata/padding as well as member data; this also observes cancel
// while skipping entries during listing or while consuming final frame checksums.
struct LimitedReader<'a, R> { inner: R, count: u64, cancel: &'a AtomicBool }
impl<R: Read> Read for LimitedReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) { return Err(io::Error::other("Operation cancelled")); }
        let count = self.inner.read(buffer)?;
        self.count += count as u64;
        if self.count > MAX_BYTES + MAX_ENTRIES as u64 * 2048 { return Err(io::Error::other("TAR stream exceeds expanded size limit")); }
        Ok(count)
    }
}

fn visit(archive_path: &Path, cancel: &AtomicBool, read_files: bool,
    callback: &mut dyn FnMut(&EntryInfo, &mut dyn Read) -> Result<()>) -> Result<(Vec<EntryInfo>, Names)> {
    cancelled(cancel)?;
    let mut probe = input_file(archive_path)?;
    let mut signature = [0; 4];
    probe.read_exact(&mut signature).context("Archive is too short")?;
    drop(probe);
    if &signature == b"LTTL" { return native_archive::visit(archive_path, cancel, read_files, callback); }
    let mut names = Names::default();
    let mut entries = Vec::new();
    if signature[0..2] == *b"PK" {
        let mut archive = zip::ZipArchive::new(BufReader::new(input_file(archive_path)?))?;
        ensure!(archive.len() <= MAX_ENTRIES, "ZIP exceeds entry limit");
        for index in 0..archive.len() {
            cancelled(cancel)?;
            let mut file = archive.by_index(index)?;
            let is_dir = file.is_dir();
            if let Some(mode) = file.unix_mode() {
                let kind = mode & 0o170000;
                ensure!(kind == 0 || kind == 0o100000 || (kind == 0o040000 && is_dir), "Links and special ZIP entries are unsupported");
            }
            ensure!(!is_dir || file.size() == 0, "Directory has file content");
            let info = names.add(file.name(), file.size(), is_dir)?;
            if read_files { callback(&info, &mut file)?; }
            entries.push(info);
        }
    } else if signature == [0x28, 0xb5, 0x2f, 0xfd] || signature == [4,34,77,24] {
        let decoder: Box<dyn Read> = if signature == [4,34,77,24] { Box::new(portable::Decoder::new(input_file(archive_path)?)?) } else {
        let mut decoder = zstd::stream::read::Decoder::new(input_file(archive_path)?)?;
        decoder.window_log_max(27)?; // At most a 128 MiB decoder window.
        Box::new(decoder) };
        let bounded = LimitedReader { inner: decoder, count: 0, cancel };
        let mut archive = tar::Archive::new(bounded);
        let mut long_name: Option<String> = None;
        let mut raw_count = 0usize;
        // Raw mode avoids unbounded allocation of attacker-controlled GNU/PAX
        // extension bodies by tar's automatic metadata expansion.
        for entry in archive.entries()?.raw(true) {
            cancelled(cancel)?;
            raw_count += 1;
            ensure!(raw_count <= MAX_ENTRIES * 2, "TAR exceeds metadata/entry limit");
            let mut entry = entry?;
            let kind = entry.header().entry_type();
            if kind.is_gnu_longname() {
                ensure!(long_name.is_none() && entry.size() <= 32_001, "Invalid or oversized GNU long filename");
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes)?;
                if bytes.last() == Some(&0) { bytes.pop(); }
                long_name = Some(String::from_utf8(bytes).context("GNU filename is not Unicode")?);
                continue;
            }
            ensure!(kind.is_file() || kind.is_dir(), "Links and special TAR entries are unsupported");
            let raw = match long_name.take() { Some(name) => name, None => std::str::from_utf8(&entry.path_bytes())?.to_owned() };
            let is_dir = kind.is_dir();
            ensure!(!is_dir || entry.size() == 0, "Directory has file content");
            let info = names.add(&raw, entry.size(), is_dir)?;
            if read_files { callback(&info, &mut entry)?; }
            entries.push(info);
        }
        ensure!(long_name.is_none(), "TAR ends with an orphaned long filename");
        // TAR may stop at its end marker before Zstandard verifies its checksum.
        // Drain to EOF, allowing only TAR zero padding after the end marker.
        let mut remainder = archive.into_inner();
        let mut buffer = vec![0; BUFFER_SIZE];
        loop {
            let count = remainder.read(&mut buffer)?;
            if count == 0 { break; }
            ensure!(buffer[..count].iter().all(|byte| *byte == 0), "Unexpected data after TAR end marker");
        }
    } else { bail!("Unsupported archive format. Supported: LTTL, ZIP, TAR.ZST and TAR.LZ4"); }
    Ok((entries, names))
}

pub fn list(archive: &Path) -> Result<Vec<EntryInfo>> {
    let cancel = AtomicBool::new(false);
    Ok(visit(archive, &cancel, false, &mut |_, _| Ok(()))?.0)
}
pub fn test(archive: &Path, cancel: &AtomicBool) -> Result<Stats> {
    let started = Instant::now();
    let (entries, names) = visit(archive, cancel, true, &mut |entry, reader| {
        cancelled(cancel)?;
        let mut no_progress = |_: Progress| {};
        let mut done = 0;
        let mut checked = ProgressReader { inner: reader, cancel, callback: &mut no_progress, done: &mut done, total: entry.size, path: &entry.path, phase: "Testing" };
        copy_checked(&mut checked, &mut io::sink(), entry.size)
    })?;
    Ok(Stats { input_bytes: fs::metadata(archive)?.len(), output_bytes: names.bytes,
        files: entries.iter().filter(|entry| !entry.is_dir).count() as u64, elapsed_seconds: started.elapsed().as_secs_f64(), gpu: None, portable: None, selection: None, numeric: None })
}

fn destination_preflight(destination: &Path, names: &Names) -> Result<()> {
    check_path(destination)?;
    if let Ok(meta) = fs::symlink_metadata(destination) {
        ensure!(meta.is_dir() && !is_link(&meta), "Extraction destination is not an ordinary directory");
    }
    for (name, is_dir) in names.nodes.values() {
        let target = destination.join(name);
        check_path(&target)?;
        match fs::symlink_metadata(&target) {
            Ok(meta) => ensure!(*is_dir && meta.is_dir() && !is_link(&meta), "Refusing to overwrite {}", target.display()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub fn extract(archive: &Path, destination: &Path, cancel: &AtomicBool, progress: &mut dyn FnMut(Progress)) -> Result<Stats> {
    let started = Instant::now();
    cancelled(cancel)?;
    check_path(destination)?;
    let staging_parent = if destination.is_dir() { destination } else { parent_dir(destination) };
    ensure!(staging_parent.is_dir(), "Extraction destination's parent directory must exist");
    let staging = TempDir::new_in(staging_parent)?;
    let mut done = 0;
    let (entries, names) = visit(archive, cancel, true, &mut |entry, reader| {
        cancelled(cancel)?;
        let path = staging.path().join(&entry.path);
        if entry.is_dir { fs::create_dir_all(path)?; return Ok(()); }
        fs::create_dir_all(parent_dir(&path))?;
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let mut writer = BufWriter::new(file);
        let mut reader = ProgressReader { inner: reader, cancel, callback: progress, done: &mut done, total: 0, path: &entry.path, phase: "Extracting to staging" };
        copy_checked(&mut reader, &mut writer, entry.size)?;
        writer.flush()?;
        Ok(())
    })?;
    cancelled(cancel)?;
    destination_preflight(destination, &names)?;
    let mut created_files = Vec::new();
    let mut created_dirs = Vec::new();
    let publication = (|| -> Result<()> {
        if !destination.exists() { fs::create_dir(destination)?; created_dirs.push(destination.to_owned()); }
        for (name, is_dir) in names.nodes.values() {
            cancelled(cancel)?;
            let target = destination.join(name);
            check_path(&target)?;
            if *is_dir {
                if !target.exists() { fs::create_dir(&target)?; created_dirs.push(target); }
            } else {
                // create_new is the final atomic no-clobber check, including a
                // concurrent file appearing after the preflight.
                let mut output = OpenOptions::new().write(true).create_new(true).open(&target)?;
                created_files.push(target.clone());
                let mut input = File::open(staging.path().join(name))?;
                let mut buffer = vec![0; BUFFER_SIZE];
                loop {
                    cancelled(cancel)?;
                    let count = input.read(&mut buffer)?;
                    if count == 0 { break; }
                    output.write_all(&buffer[..count])?;
                }
                output.sync_all()?;
            }
        }
        Ok(())
    })();
    if let Err(error) = publication {
        for path in created_files.iter().rev() { let _ = fs::remove_file(path); }
        for path in created_dirs.iter().rev() { let _ = fs::remove_dir(path); }
        return Err(error).context("Publishing extracted files; newly created output cleanup was attempted");
    }
    progress(Progress { phase: "Complete".into(), path: destination.display().to_string(), bytes_done: names.bytes, bytes_total: names.bytes });
    Ok(Stats { input_bytes: fs::metadata(archive)?.len(), output_bytes: names.bytes,
        files: entries.iter().filter(|entry| !entry.is_dir).count() as u64, elapsed_seconds: started.elapsed().as_secs_f64(), gpu: None, portable: None, selection: None, numeric: None })
}
