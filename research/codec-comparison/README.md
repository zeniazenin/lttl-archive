# Numeric prototype versus standard codecs and archive tools

This compares the independently implemented lttl numeric prototype with Zstandard, LZ4, ZIP/Deflate, and the actual RAR and 7-Zip command-line applications. It is research, not a production feature or a claim to replace these formats. Product documentation remains separate from vendor comparisons.

See [measurements.md](measurements.md) for ratios, compression and extraction timings, and the measured conclusion. [results.json](results.json) retains individual runs, commands, binary hashes, GPU stage timings and archive verification. The numeric implementation itself is unchanged from the [GPU experiment](../numeric-gpu/README.md).

The completed comparison passed 660 exact recovery checks. On native EEG, numeric CPU creates an archive 0.7% smaller than RAR method 5 in about one fifth of the time. Results depend strongly on representation: original packed ECG and big-endian terrain favor established codecs, while delta-filtered 7z often achieves better numeric density. CPU/GPU numeric outputs are identical; the measured warm GPU gains are modest and variable. See the [size/time chart](tradeoffs.svg) and full report before generalizing these results.

## Compared configurations

| Label | Configuration |
|---|---|
| cpu-numeric | Independent delta16/zigzag/32-value tile packing; eight CPU workers; common sampled stride selection |
| intel / nvidia | Identical numeric bitstream on Intel Arc integrated graphics / RTX 4070 Laptop GPU; upload, compaction and readback included |
| adaptive | Previous research selector: numeric when its sampling threshold passes, otherwise Zstandard level 3; not production Auto |
| zstd-1 / zstd-3 / zstd-9 | Zstandard 1.5.7 through Rust `zstd` 0.13, eight requested compression workers, checksummed research frame with raw fallback |
| lz4 | Standard LZ4 frame through `lz4_flex` 0.11.6, content checksum; single-thread codec, not the native C implementation |
| store | Original bytes in a 26-byte checksummed research header; no compression |
| zip-deflate-6 | Actual 7-Zip ZIP writer, Deflate, level 6, eight requested threads |
| rar-3 / rar-5 | Installed RAR 7.20.3, RAR5 format, methods 3/5, 32 MiB requested dictionary, eight requested threads, default filters, configuration-file overrides disabled |
| 7z-1 / 7z-5 / 7z-9 | Official 7-Zip 26.04 x64 standalone console, real `.7z` output, LZMA2 levels 1/5/9, requested dictionaries 4/32/64 MiB, eight requested threads |
| 7z-delta-5 | Actual `.7z` with bytewise Delta followed by LZMA2, level 5, 32 MiB requested dictionary, eight requested threads |

Eight requested threads is not a guarantee that an algorithm uses eight cores on every file. 7-Zip can reduce its encoded dictionary to the input's useful size; its detailed archive listings are saved for every 7z configuration and dataset. Dictionary settings and process memory are not equal across codecs, and peak memory was not measured.

The delta-filtered competitor is important. The new numeric prototype is a specialization using established prediction and packing techniques; comparing it only with untransformed generic encoders would miss a relevant existing approach. `7z-delta-5` uses byte distance `2 * numeric_stride`, taken from the same prior content sample used by the numeric selector. This is a preconfigured filter comparison, not automatic content detection by 7-Zip. Its selection cost is excluded, favoring that competitor; numeric CPU/GPU selection is included. No per-dataset exhaustive filter or preset search was performed. A bytewise delta filter is not mathematically identical to the prototype's 16-bit modular difference.

The official 7-Zip package is downloaded from the project's [download page](https://www.7-zip.org/download.html) into ignored `tools/7zip-26.04/`; no system installation, shell registration, or runtime dependency is added to lttl. URLs and SHA-256 values are recorded in [tool-downloads.json](tool-downloads.json). Its license is retained alongside the local tool. The existing RAR installation is used without modification.

## Inputs and fairness

All configurations receive the exact same complete byte streams from the [numeric corpus manifest](../numeric-gpu/corpus.json), verified by SHA-256 before the experiment. There are 11 inputs:

* Original packed MIT-BIH ECG records and a separately labeled lossless signed-16-bit expansion.
* Original native EDF+ EEG recordings, including headers and annotation channels.
* An original 12-lead PTB ECG file.
* A complete original big-endian elevation raster and a separately labeled reversible little-endian conversion.
* Synthetic sensor streams at 1, 16 and 64 MiB, independent noise, and a 16 MiB enwik8 real-text prefix.

Source attribution, original record identities and conversion checks are in the [GPU corpus documentation](../numeric-gpu/README.md#real-data-and-attribution). Conversion is outside every codec's timing. Original and converted inputs have separate rows and, for packed ECG, different input lengths; they must not be silently combined. A result on converted arrays is not a promise about compressing an original packed file automatically.

This is a payload comparison. RAR, 7z and ZIP store one member with its filename and metadata; the numeric and Zstandard research frames and LZ4 frame store only the payload. Their full output sizes are reported, including these differing headers. The inputs are at least approximately 0.88 MiB, so header overhead is relatively small, but this is not a complete multi-file archive feature comparison. Directory traversal, per-file metadata preservation, encryption, IDP policy, user interface, recovery records and multi-volume output are not being compared. Solid mode is requested for RAR/7z, but a one-member archive does not test cross-file solid grouping.

## Timing boundaries

The main tables use **fresh-process wall time** for every implementation. Each operation starts before launching its executable and ends after exit and an explicit host `fsync` of the resulting archive or extracted file. This includes process startup, reads, codec work, writes, durability flush, and—on the GPU—session creation, staging, transfers, dispatch, synchronization and framing. It avoids comparing a warm GPU kernel with a complete competing application invocation.

Three repetitions of all 16 configurations and 11 inputs run serially in seeded randomized order. OS/driver caches are not flushed. "Cold" here means a new application process and GPU session, not a cold operating system. No confidence intervals or significance claims are made from three samples; min/max ranges accompany medians.

A **separate reused-session table** reruns CPU numeric, Intel and NVIDIA using the existing session harness. One initial job is followed by three warm jobs per input/configuration. These operation times exclude executable startup and are explicitly not the same timing boundary as external RAR/7z process measurements. They show the cost of repeated jobs inside an already-open app. No equivalent persistent RAR/7z service is assumed. The raw report retains process-group timing as well.

Extraction is always timed separately. Numeric archives, including GPU-created archives, use the original CPU decoder with eight configured workers. Standard formats use their associated decoder/tool. Every restored file must match the source SHA-256 before the result is accepted. Verification and archive hashing happen outside the timed interval. CPU and GPU numeric archives must also match byte-for-byte across both fresh and reused sessions.

The machine is a Core Ultra 9 185H with integrated Intel Arc and an NVIDIA RTX 4070 Laptop GPU. CPU placement, power mode, temperature, background work and caches are uncontrolled. Small differences should not be treated as stable advantages. GPU transfer metrics are available in the raw results; the measured GPU codec and bitstream are unchanged from the previous experiment.

## Reproduction

The real numeric corpus must already be prepared using the prior experiment. These commands use the project's pinned Rust dependencies and the existing RAR executable at `C:\Program Files\WinRAR\Rar.exe`:

```powershell
python research/codec-comparison/get_7zip.py
```

Extract the portable tool with argument paths resolved explicitly:

```python
from pathlib import Path
import subprocess
tools = Path.cwd() / 'tools/7zip-26.04'
subprocess.run([str(tools/'7zr.exe'), 'x', str(tools/'7z2604-extra.7z'),
                '-o'+str(tools/'extra'), '-y'], check=True)
```

Then build and run:

```powershell
./scripts/cargo.ps1 build --release --example codec_compare --locked --offline
python research/codec-comparison/benchmark.py
python research/codec-comparison/summarize.py
python research/codec-comparison/plot.py
```

The session executable from the previous GPU experiment must also exist. The comparison worker is a new example, leaving the earlier source and reports intact. The harness checkpoints atomically and resumes only when source, binary and corpus hashes match. Preserve an existing results file under another name before starting a fresh comparison. Benchmark files are created under a new ignored work directory; only the specific generated archive and restored file are deleted after verification.
