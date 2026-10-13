//! Bounded, content-based archive policy. Predictions are heuristics, not an oracle.
use super::{Backend, GpuMetrics, GpuOptions, Preset, Progress, Source, cancelled, input_file};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{collections::{BTreeSet, HashSet}, io::{Read, Seek, SeekFrom}, sync::atomic::AtomicBool, time::Instant};

#[derive(Debug, Clone, Serialize)]
pub struct Selection {
    pub policy_version: u32, pub goal: Preset, pub content_class: String,
    pub backend: Backend, pub preset: Preset, pub stored: bool, pub threads: u32,
    pub chunk_kib: usize, pub batch_mib: usize,
    pub sampled_bytes: usize, pub sampled_files: usize, pub selection_seconds: f64,
    pub cpu_fast_ratio: Option<f64>, pub cpu_balanced_ratio: Option<f64>, pub cpu_small_ratio: Option<f64>,
    pub gpu_trial_ratio: Option<f64>, pub gpu_trial: Option<GpuMetrics>, pub gpu_unavailable: Option<String>,
    pub reason: String,
}
pub(crate) struct Plan { pub report: Selection, pub probe: Option<super::gpu::GpuProbe> }

pub(crate) fn select(sources: &[Source], total: u64, goal: Preset, threads: u32,
    cancel: &AtomicBool, progress: &mut dyn FnMut(Progress)) -> Result<Plan> {
    let start = Instant::now();
    let workers = if threads == 0 {
        if total < 8 * 1_048_576 { 1 } else { std::thread::available_parallelism().map(|v| v.get().min(8) as u32).unwrap_or(1) }
    } else { threads };
    let mut plan = Plan { report: Selection { policy_version: 2, goal, content_class: "small-input".into(),
        backend: Backend::Cpu, preset: goal, stored: false, threads: workers, chunk_kib: 1024, batch_mib: 32,
        sampled_bytes: 0, sampled_files: 0, selection_seconds: 0.0,
        cpu_fast_ratio: None, cpu_balanced_ratio: None, cpu_small_ratio: None, gpu_trial_ratio: None,
        gpu_trial: None, gpu_unavailable: None,
        reason: "Small input: keep CPU and avoid sampling/GPU startup; automatic worker count is one.".into() }, probe: None };
    if total < 8 * 1_048_576 {
        plan.report.selection_seconds = start.elapsed().as_secs_f64();
        return Ok(plan);
    }
    // Version 1's Fast probes did not justify their cost: the only alternative
    // was raw storage, which showed no reliable speed gain over CPU level 1.
    if goal == Preset::Fast {
        plan.report.content_class = "not-sampled".into();
        plan.report.reason = "Speed goal: use CPU Fast directly, with no sampling or GPU startup.".into();
        plan.report.selection_seconds = start.elapsed().as_secs_f64();
        return Ok(plan);
    }
    // Choose files by byte quantiles, rather than extension or only the first name.
    // At most eight files and 1.5 MiB are read, with start/middle/end coverage.
    let files: Vec<_> = sources.iter().filter(|s| !s.info.is_dir && s.info.size > 0).collect();
    let mut indices = BTreeSet::new();
    for q in 0..8u64 {
        let target = (total / 8) * q;
        let mut cumulative = 0u64;
        for (i, file) in files.iter().enumerate() {
            cumulative += file.info.size;
            if cumulative > target { indices.insert(i); break; }
        }
    }
    // Balanced needs a cheap screening comparison. Preserve 64 KiB prefixes
    // and all three positions, but avoid 512 KiB regions for single large files.
    // Small retains the original sample sizes for its finer ratio/GPU decision.
    let window = if goal == Preset::Balanced { 64 * 1024 }
        else { (1536 * 1024 / (indices.len().max(1) * 3)).min(512 * 1024) };
    let mut sample = Vec::with_capacity(window * indices.len() * 3);
    let mut prefixes = HashSet::new();
    let mut related = false;
    let mut regions = Vec::new();
    for index in indices {
        cancelled(cancel)?;
        let source = files[index];
        progress(Progress { phase: "Auto: sampling content".into(), path: source.info.path.clone(), bytes_done: 0, bytes_total: total });
        let mut file = input_file(&source.path)?;
        ensure!(file.metadata()?.len() == source.info.size, "Input changed while selecting compression");
        let length = source.info.size.min(window as u64) as usize;
        let offsets = if source.info.size <= (window * 3) as u64 {
            // The maximum three-window budget still applies to medium files.
            vec![0]
        } else { vec![0, (source.info.size - length as u64) / 2, source.info.size - length as u64] };
        for offset in offsets {
            cancelled(cancel)?;
            let mut bytes = vec![0u8; length];
            file.seek(SeekFrom::Start(offset))?; file.read_exact(&mut bytes)?;
            if offset == 0 && bytes.len() >= 65536 {
                related |= !prefixes.insert(xxhash_rust::xxh64::xxh64(&bytes[..65536], 0));
            }
            let begin = sample.len();
            sample.extend(bytes);
            regions.push(begin..sample.len());
        }
        ensure!(file.metadata()?.len() == source.info.size, "Input changed during compression sampling");
        plan.report.sampled_files += 1;
    }
    cancelled(cancel)?;
    plan.report.sampled_bytes = sample.len();
    // Related content resolves the policy without any codec or byte-classifier
    // probes. Keep the evidence collection above, but skip discarded work.
    if related {
        plan.report.content_class = "related-files".into();
        plan.report.preset = Preset::Small;
        plan.report.reason = "Repeated file-prefix fingerprints indicate shared history; preserve CPU Small streaming history without redundant codec trials.".into();
        plan.report.selection_seconds = start.elapsed().as_secs_f64();
        return Ok(plan);
    }
    let denominator = sample.len().max(1) as f64;
    // Reuse a compressor and its output allocation across CPU probes. Each
    // bulk call remains an independent frame; no sample history leaks between calls.
    let mut compressor = zstd::bulk::Compressor::new(1)?;
    let mut encoded = vec![0; zstd::zstd_safe::compress_bound(sample.len())];
    let fast = compressor.compress_to_buffer(&sample, encoded.as_mut_slice())? as f64 / denominator;
    cancelled(cancel)?;
    compressor.set_compression_level(3)?;
    let balanced = compressor.compress_to_buffer(&sample, encoded.as_mut_slice())? as f64 / denominator;
    plan.report.cpu_fast_ratio = Some(fast);
    plan.report.cpu_balanced_ratio = Some(balanced);
    // Per-region probes only matter if aggregate compression suggests storage.
    // Stop at the first compressible region and avoid scanning text three times.
    let mut every_region_resists = balanced >= 0.985 && !regions.is_empty();
    if every_region_resists {
        compressor.set_compression_level(1)?;
        for region in &regions {
            cancelled(cancel)?;
            let bytes = &sample[region.clone()];
            if (compressor.compress_to_buffer(bytes, encoded.as_mut_slice())? as f64 / bytes.len() as f64) < 0.99 {
                every_region_resists = false;
                break;
            }
        }
    }
    let (mut printable, mut lines, mut delimiters) = (0usize, 0usize, 0usize);
    for &b in &sample {
        printable += usize::from(b == b'\n' || b == b'\r' || b == b'\t' || (32..=126).contains(&b));
        lines += usize::from(b == b'\n');
        delimiters += usize::from(b == b',' || b == b'\t' || b == b';');
    }
    let printable = printable as f64 / denominator;
    let structured = printable > 0.95 && lines >= 16 && delimiters >= lines * 3;
    plan.report.content_class = if every_region_resists { "low-compression-benefit" }
        else if structured { "structured-text" } else if printable > 0.9 { "text" }
        else if fast < 0.2 { "repetitive-binary" } else { "binary-or-mixed" }.into();

    if every_region_resists {
        plan.report.stored = true;
        plan.report.chunk_kib = 4096; plan.report.batch_mib = 4; plan.report.threads = 1;
        plan.report.preset = Preset::Fast;
        plan.report.reason = "Every sampled region resisted compression; write checksummed standard Zstd raw blocks on CPU to avoid wasted search/transfers.".into();
    } else if goal == Preset::Balanced {
        plan.report.preset = if fast <= balanced * 1.03 { Preset::Fast } else { Preset::Balanced };
        plan.report.reason = if plan.report.preset == Preset::Fast {
            "CPU Fast sample is within 3% of Balanced size or smaller; favor its lower search cost. GPU has no measured speed advantage for this goal."
        } else { "CPU Balanced materially improves the sample over Fast; retain CPU solid history and avoid GPU startup." }.into();
    } else {
        cancelled(cancel)?;
        compressor.set_compression_level(9)?;
        let small = compressor.compress_to_buffer(&sample, encoded.as_mut_slice())? as f64 / denominator;
        plan.report.cpu_small_ratio = Some(small);
        let best = fast.min(balanced).min(small);
        // Small uses a documented 1% sample-size tolerance to avoid extra work for tiny savings.
        plan.report.preset = if fast <= best * 1.01 { Preset::Fast } else if balanced <= best * 1.01 { Preset::Balanced } else { Preset::Small };
        plan.report.reason = "Size goal: select the cheapest CPU preset within 1% of the smallest sampled output.".into();
        // Only the prior structured-text/size experiment justified a GPU candidate.
        // Trial actual hardware instead of assuming an extension or device name implies a win.
        if structured && total >= 32 * 1_048_576 {
            progress(Progress { phase: "Auto: comparing GPU size benefit".into(), path: String::new(), bytes_done: 0, bytes_total: total });
            let gpu_options = GpuOptions { chunk_kib: 64, batch_mib: 32 };
            match super::gpu::GpuProbe::trial(&gpu_options, &sample) {
                Ok((probe, ratio)) => {
                    plan.report.gpu_trial = Some(probe.metrics().clone()); plan.report.gpu_trial_ratio = Some(ratio);
                    // Require measured size improvement, not just a nearby ratio or fast kernel.
                    if ratio < best * 0.995 {
                        plan.report.backend = Backend::Gpu; plan.report.chunk_kib = 64;
                        plan.report.reason = "Size goal and structured text: actual GPU trial is at least 0.5% smaller than every CPU sample; reuse its context with 64 KiB chunks / 32 MiB batches.".into();
                        plan.probe = Some(probe);
                    }
                }
                Err(error) => { plan.report.gpu_unavailable = Some(format!("{error:#}")); plan.report.reason.push_str(" GPU trial unavailable; retain CPU before creating the archive."); }
            }
        }
    }
    cancelled(cancel)?;
    plan.report.selection_seconds = start.elapsed().as_secs_f64();
    Ok(plan)
}
