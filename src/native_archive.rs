//! Bounded, per-file native archive framing and content-aware CPU selection.
use super::*;
use serde::Deserialize;
use std::collections::BTreeMap;

pub(crate) const MAGIC: &[u8; 8] = b"LTTLAR01";
// Preserve alignment of packed triples and 16/32/64-bit words across blocks.
pub(crate) const BLOCK: usize = 16 * 1_048_560;
const META_LIMIT: usize = 192_128;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Metrics {
    pub policy_version: u32,
    pub blocks: BTreeMap<String, u64>,
    pub sampled_bytes: u64,
    pub selection_seconds: f64,
    pub threads: usize,
}

fn name(plan: &numeric::Plan) -> &'static str {
    match plan.stage { 0 => "stored", 1 => "lttl-fast", 2 | 4 => "lttl-dense", _ => "zstd" }
}

fn sample_ranges(data: &[u8], width: usize) -> Vec<std::ops::Range<usize>> {
    let width = width.min(data.len());
    let span = data.len() - width;
    [span / 8, span / 2, span * 7 / 8].into_iter()
        .map(|at| { let at = at / 24 * 24; at..at + width }).collect()
}

fn trial(data: &[u8], plan: &numeric::Plan, ranges: &[std::ops::Range<usize>]) -> Result<(usize, f64)> {
    let started = Instant::now();
    let mut bytes = 0;
    for range in ranges { bytes += numeric::encode_plan(&data[range.clone()], plan)?.len(); }
    Ok((bytes, started.elapsed().as_secs_f64()))
}

fn choose(data: &[u8], backend: Backend, goal: Preset, metrics: &mut Metrics, cancel: &AtomicBool) -> Result<numeric::Plan> {
    let level = match goal { Preset::Fast => 1, Preset::Balanced => 3, Preset::Small => 9 };
    let baseline = numeric::plain_plan(level);
    if backend == Backend::Cpu { return Ok(baseline); }
    let started = Instant::now();
    let result = (|| -> Result<_> {
        if matches!(backend, Backend::LttlFast | Backend::LttlDense) {
            metrics.sampled_bytes += data.len().min(8192) as u64 * 3;
            return numeric::select(data, backend == Backend::LttlDense);
        }
        // Tiny files and text avoid the full numeric layout search. Every block
        // is reconsidered; one member's decision cannot leak into another.
        let minimum = if goal == Preset::Fast { 1_048_576 } else { 262_144 };
        if data.len() < minimum { return Ok(baseline); }
        let screening = sample_ranges(data, 4096);
        let printable = screening.iter().flat_map(|r| &data[r.clone()])
            .filter(|&&b| b == b'\n' || b == b'\r' || b == b'\t' || (32..=126).contains(&b)).count();
        let screening_len: usize = screening.iter().map(|r| r.len()).sum();
        metrics.sampled_bytes += screening_len as u64;
        if printable * 100 > screening_len * 95 { return Ok(baseline); }
        cancelled(cancel)?;
        let fast = numeric::select(data, false)?;
        metrics.sampled_bytes += fast.sample_input_bytes as u64;
        // Noise and unsupported layouts do not justify Dense trials.
        if fast.sample_pack_bytes * 100 >= fast.sample_input_bytes * 90 { return Ok(baseline); }
        let ranges = sample_ranges(data, 65520);
        let sample_len: usize = ranges.iter().map(|r| r.len()).sum();
        metrics.sampled_bytes += sample_len as u64;
        let (base_size, base_time) = trial(data, &baseline, &ranges)?;
        let mut best = baseline;
        let mut best_size = base_size;
        let mut best_time = base_time;
        let mut candidates = vec![fast];
        if goal != Preset::Fast {
            cancelled(cancel)?;
            let dense = numeric::select(data, true)?;
            metrics.sampled_bytes += dense.sample_input_bytes as u64;
            candidates.push(dense);
        }
        for candidate in candidates {
            if !matches!(candidate.stage, 1 | 2 | 4) { continue; }
            cancelled(cancel)?;
            let (size, seconds) = trial(data, &candidate, &ranges)?;
            // Latency margin includes amortized selection, not just the probe's
            // compression time. Size selection retains a material 3% margin.
            let overhead = started.elapsed().as_secs_f64() * sample_len as f64 / data.len() as f64;
            let faster = seconds + overhead < best_time * 0.85 && size * 100 <= best_size * 101;
            let smaller = size * 100 < best_size * 97;
            let accept = match goal { Preset::Fast => faster,
                Preset::Balanced => smaller || faster, Preset::Small => smaller };
            if accept { best = candidate; best_size = size; best_time = seconds; }
        }
        Ok(best)
    })();
    metrics.selection_seconds += started.elapsed().as_secs_f64();
    result
}

pub(crate) fn create(writer: &mut dyn Write, sources: &[Source], total: u64, options: &CreateOptions,
    backend: Backend, cancel: &AtomicBool, progress: &mut dyn FnMut(Progress)) -> Result<Metrics> {
    let threads = if options.threads == 0 { std::thread::available_parallelism().map(|n| n.get().min(8)).unwrap_or(1) }
        else { options.threads as usize };
    // One pool per archive, retained across all members/blocks and probe jobs.
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build()?;
    let mut metrics = Metrics { policy_version: 1, threads, ..Default::default() };
    writer.write_all(MAGIC)?;
    let mut done = 0;
    for source in sources {
        cancelled(cancel)?;
        let metadata = serde_json::to_vec(&source.info)?;
        ensure!(metadata.len() <= META_LIMIT, "Native archive metadata limit");
        writer.write_all(&(metadata.len() as u32).to_le_bytes())?;
        writer.write_all(&metadata)?;
        writer.write_all(&xxhash_rust::xxh64::xxh64(&metadata, 0).to_le_bytes())?;
        if source.info.is_dir { continue; }
        let mut input = input_file(&source.path)?;
        ensure!(input.metadata()?.len() == source.info.size, "Input size changed");
        let mut remaining = source.info.size;
        while remaining > 0 {
            cancelled(cancel)?;
            let length = remaining.min(BLOCK as u64) as usize;
            let mut data = vec![0; length]; input.read_exact(&mut data)?;
            let (encoded, selected) = pool.install(|| -> Result<_> {
                let plan = choose(&data, backend, options.preset, &mut metrics, cancel)?;
                cancelled(cancel)?;
                Ok((numeric::encode_plan(&data, &plan)?, name(&plan)))
            })?;
            cancelled(cancel)?;
            // Whole-block fallback can store data even after a numeric selection.
            let actual = if encoded[28] == 1 { "stored" } else { selected };
            *metrics.blocks.entry(actual.into()).or_default() += 1;
            writer.write_all(&(encoded.len() as u32).to_le_bytes())?;
            writer.write_all(&encoded)?;
            remaining -= length as u64; done += length as u64;
            progress(Progress { phase: format!("Creating lttl ({actual})"), path: source.info.path.clone(), bytes_done: done, bytes_total: total });
        }
        ensure!(input.read(&mut [0; 1])? == 0 && input.metadata()?.len() == source.info.size, "Input size changed while archiving");
    }
    writer.write_all(&0u32.to_le_bytes())?;
    Ok(metrics)
}

fn integer(reader: &mut dyn Read) -> Result<usize> {
    let mut bytes = [0; 4]; reader.read_exact(&mut bytes)?; Ok(u32::from_le_bytes(bytes) as usize)
}

struct Member<'a> {
    reader: &'a mut dyn Read, cancel: &'a AtomicBool, remaining: u64,
    block: io::Cursor<Vec<u8>>,
}
impl Read for Member<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() { return Ok(0); }
        let work = (|| -> Result<usize> {
            cancelled(self.cancel)?;
            if self.block.position() == self.block.get_ref().len() as u64 {
                if self.remaining == 0 { return Ok(0); }
                let expected = self.remaining.min(BLOCK as u64) as usize;
                let size = integer(self.reader)?;
                ensure!((29..=expected + 29).contains(&size), "Invalid lttl block length");
                let mut encoded = vec![0; size]; self.reader.read_exact(&mut encoded)?;
                // Validate decoded size BEFORE calling the inner allocator.
                ensure!(&encoded[..8] == b"LTNV0002" && u64::from_le_bytes(encoded[8..16].try_into()?) == expected as u64,
                    "Invalid lttl block declaration");
                let decoded = numeric::decode(&encoded)?;
                ensure!(decoded.len() == expected, "Invalid lttl decoded length");
                self.remaining -= expected as u64;
                self.block = io::Cursor::new(decoded);
            }
            Ok(self.block.read(output)?)
        })();
        work.map_err(io::Error::other)
    }
}

pub(crate) fn visit(path: &Path, cancel: &AtomicBool, read_files: bool,
    callback: &mut dyn FnMut(&EntryInfo, &mut dyn Read) -> Result<()>) -> Result<(Vec<EntryInfo>, Names)> {
    let mut file = BufReader::new(input_file(path)?);
    let mut magic = [0; 8]; file.read_exact(&mut magic)?;
    ensure!(&magic == MAGIC, "Unsupported native lttl archive version");
    let mut names = Names::default(); let mut entries = Vec::new();
    loop {
        cancelled(cancel)?;
        let size = integer(&mut file)?;
        if size == 0 { ensure!(file.read(&mut [0; 1])? == 0, "Trailing lttl archive data"); break; }
        ensure!(size <= META_LIMIT, "Native archive metadata limit");
        let mut metadata = vec![0; size]; file.read_exact(&mut metadata)?;
        let mut checksum = [0; 8]; file.read_exact(&mut checksum)?;
        ensure!(xxhash_rust::xxh64::xxh64(&metadata, 0) == u64::from_le_bytes(checksum), "Native metadata checksum mismatch");
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Header { path: String, size: u64, is_dir: bool }
        let header: Header = serde_json::from_slice(&metadata)?;
        ensure!(!header.is_dir || header.size == 0, "Directory has content");
        let info = names.add(&header.path, header.size, header.is_dir)?;
        let mut member = Member { reader: &mut file, cancel, remaining: info.size, block: io::Cursor::new(Vec::new()) };
        if read_files { callback(&info, &mut member)?; }
        // Listing also verifies frames and consumes every bounded member.
        io::copy(&mut member, &mut io::sink())?;
        entries.push(info);
    }
    Ok((entries, names))
}
