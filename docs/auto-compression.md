# Content-aware compression in lttl

Auto is now the CLI and desktop default for TAR.ZST. It inspects bytes and selects a backend and settings according to the Fast, Balanced, or Small goal. Explicit `--backend cpu` and `--backend gpu` remain available. ZIP keeps its previous CPU policy. Archives remain ordinary TAR.ZST files and extract on CPU without NVIDIA software.

The [original 189-case comparison](auto-performance.md) exposed sampling regressions. The [version-2 optimization comparison](auto-optimization.md) runs preserved Auto v1, optimized Auto, and explicit CPU settings on the same corpus. Both include sampling, trial work, GPU initialization, transfers, checksums, filesystem work, and process startup. Auto does not guarantee the best setting for arbitrary files or machines.

## Policy version 2

For inputs below 8 MiB, keep the requested CPU preset without sampling or GPU startup. **Fast skips all sampling at every size and directly uses CPU Fast.** The earlier raw-storage alternative did not show a reliable Fast speed benefit. For other goals, sample at most eight files chosen by byte quantiles of the sorted input manifest. Balanced uses 64 KiB regions at start/middle/end: 192 KiB for a single large file, up to 1.5 MiB across eight files. Small retains the original region sizes (up to 512 KiB) and total 1.5 MiB budget to preserve its finer size/GPU comparison. Smaller files use prefix samples. Classify actual contents rather than trusting extensions. Classes include structured text, other text, related files, repetitive binary, low compression benefit, and mixed/binary content; unsampled Fast inputs report `not-sampled`.

Repeated 64 KiB prefix fingerprints across sampled files indicate potentially useful shared history and now resolve the choice before any codec probes. Otherwise the selector measures CPU Zstandard levels 1 and 3 on the sample; Small also tries level 9 unless an earlier rule resolves the choice. One compressor and output allocation are reused for these independent-frame probes. Per-region level-1 checks run only when the combined level-3 sample suggests raw storage and stop on the first compressible region. A single pass counts ASCII printability, newlines, and delimiters; this is not a full CSV/JSON parser and can miss Unicode-heavy or unusual layouts.

| Condition / goal | Choice |
|---|---|
| Every sampled region retains at least 99% of its size and combined level-3 sample retains at least 98.5%, with no repeated prefixes | CPU storage using checksummed raw Zstd blocks; avoid match searching and device transfers |
| Related sampled file prefixes, Balanced or Small | CPU Small streaming encoder to retain shared compression history |
| Fast (checked first) | CPU Fast immediately, with no storage check, content sampling, or GPU trial |
| Balanced | CPU Fast if its sampled output is within 3% of level 3 or smaller; otherwise CPU Balanced |
| Small | Cheapest CPU preset within 1% of the smallest sampled output |
| Small, structured text, at least 32 MiB | Also perform a real GPU trial using 64 KiB chunks / 32 MiB batches; select GPU only if its sampled output is at least 0.5% smaller than every CPU candidate |

The GPU trial is CPU-decoded and byte-compared before it can be selected. Its context and allocations are reused during archive encoding, avoiding a second initialization. If initialization or the trial fails, Auto records the error and uses CPU before archive creation starts. An error during actual selected-backend encoding still fails and removes temporary output; there is no mid-archive backend substitution. Explicit GPU requests still report an error if unavailable.

Automatic CPU worker count is one below 8 MiB and up to eight otherwise. Explicit thread counts are respected for streaming CPU choices. Storage does not use compression workers. Manual chunk/batch flags apply to explicit GPU or CPU-chunked operation; Auto selects them internally. nvCOMP uses a fixed encoder and does not implement the CPU compression levels.

## Output, timing and integration

One decision applies to the entire TAR stream, so CPU compression can exploit related files. This implementation does not reorder files, apply lossy transforms, mix per-file codecs, or introduce a new decoder requirement. It does not add LZ4/GDeflate archive formats, although earlier experiments informed the design.

The raw-storage path writes independent 4 MiB Zstandard frames with blocks no larger than 128 KiB, content sizes, and standard low-32-bit XXH64 checksums, following the [Zstandard frame specification](https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md). Frame/TAR overhead means incompressible output can be slightly larger than the input. Raw and Auto-GPU outputs were independently extracted with Windows bsdtar/libzstd and SHA256-compared.

JSON `selection` reports the policy version, chosen settings, content class, sampled bytes/files, CPU/GPU sample ratios, explanation, selection duration, trial timings, and any GPU unavailability. Skipped probes report null ratios; related-file choices now have no codec ratios, and Fast has zero sampled bytes/files. The desktop completion view shows the choice and reason. `Stats.elapsed_seconds` includes selection, trial costs, and cleanup. The selection interval ends at the decision and does not include destruction of local probe buffers. When GPU is selected, initialization and trial counters are in `selection.gpu_trial`; archive-only GPU counters are in `Stats.gpu`. The latter's initialization is zero because the context is reused. Do not omit the former when measuring total cost or add overlapping CUDA/host intervals together.

```powershell
# Auto Balanced is the new default.
.\target\release\lttl.exe create .\input -o .\backup.tar.zst

# Favor size; structured content may select the GPU.
.\target\release\lttl.exe create .\input -o .\small.tar.zst --backend auto --preset small

# Preserve explicit CPU behavior without selection overhead.
.\target\release\lttl.exe create .\input -o .\fixed.tar.zst --backend cpu --preset balanced
```

The library's `create()` convenience function intentionally remains CPU-only for compatibility; callers opt in through `create_with_backend(..., Backend::Auto, ...)`.

## What the results do and do not show

Version 1's thresholds were set before its comparison. Five retained synthetic datasets and two public corpora were joined by two new deterministic JSON/mixed fixtures with misleading extensions; those fixtures were not used to set the initial thresholds. Version 2 deliberately reuses this corpus to evaluate the sampling optimization and is not a new held-out evaluation. All results, including regressions and startup spikes, are retained.

Samples can miss redundancy, rare content, dependencies across distant files, or a poorly compressing tail. Concatenated sample compression is only an estimate of full streaming compression. Prefix equality can reflect a shared header rather than broadly related content. Sample tolerances do not guarantee the same tolerance on full output. A single archive-wide choice may be inferior for heterogeneous inputs. Small favors size with a tolerance; it need not produce the absolute smallest possible archive.

Fast's version-1 classification/probe cost is removed. Balanced's smaller sample can miss content that the old sample would find; equal choices on this corpus cannot establish equivalence on arbitrary data. Balanced does not select GPU because prior complete-archive evidence did not justify it. Auto is calibrated conservatively from this RTX 4070 Laptop / Core Ultra 9 machine; it does not learn hardware speed models, predict PCIe bandwidth, or benchmark every supported setting on every job. GPU startup varies substantially and can erase a compression speed benefit; the optimization comparison includes such observations for both versions.

Reproduce the optimization comparison with `pwsh -NoProfile -File scripts/benchmark-auto-v2.ps1`; it requires the retained corpus/manifest and a preserved v1 executable alongside the same GPU runtime. Four repetitions balance forward/reverse order. The original `scripts/benchmark-auto.ps1` compares CPU presets, explicit GPU defaults, and whichever Auto policy is in the current executable. `python scripts/verify-auto.py` checks independent extraction, Auto fallback without NVIDIA DLLs, and explicit-GPU failure. These scripts do not modify registry integration.

Implementation: [selector](../src/auto.rs), [encoders](../src/gpu.rs), [integration tests](../tests/auto.rs), [current performance tables](auto-optimization.md), [original performance tables](auto-performance.md), and [validation record](validation.md).
