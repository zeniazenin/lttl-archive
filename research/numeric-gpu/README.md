# Portable GPU numeric compression experiment

Follow-up: [comparison with standard codecs and archive tools](../codec-comparison/README.md), including actual RAR/7-Zip and a delta-filtered competitor.

This implements the [numeric prototype](../codec-lab/README.md) on wgpu and tests it on the local Intel Arc and NVIDIA RTX 4070 Laptop GPU. It is an optional research executable, not a production archive backend. It does not change Auto, encryption, or the Windows installer.

Read [measurements.md](measurements.md) for complete timings, compression ratios, and conclusions. [results.json](results.json) retains each job, device/driver details, and source hashes. [corpus.json](corpus.json) records source URLs, original and derived input hashes, file sizes, and transformations.

**Measured outcome:** the numeric transform improves ratio on real little-endian ECG, EEG and elevation data, but moving it to GPU does not produce a compelling advantage over the eight-worker CPU path. RTX is approximately tied on expanded ECG and slower on EEG and little-endian terrain; Arc is slower on these workloads. The two RTX speed margins above 10% occur on inputs stored uncompressed by the numeric codec. Keep the CPU path as the practical choice while exploring reduced host copies and overlapped batches.

## What is implemented

* GPU strided 16-bit prediction, zigzag encoding, tile-width reduction, and bit packing.
* Hierarchical prefix sums and GPU output compaction. Only useful compressed chunk payloads return to the host; stored chunks use the retained original input.
* Persistent device, pipelines, upload staging, and buffers across jobs. The current implementation processes 8 MiB batches serially; it does **not** overlap batches.
* Optional hardware timestamps for upload, tile encoding, scans/compaction, and compressed payload readback. Full host wall time is measured regardless of timestamp support.
* Exact compatibility with the original `LTEX0001` research numeric bitstream and CPU decoder. CPU one/eight-worker and both GPU encoders produce identical archive bytes at the same selected stride.
* Real ECG, EEG, and elevation samples, with unconverted and converted representations reported separately.

There is no GPU decompressor in this experiment. Recovery uses the original CPU decoder, including its checksum and framing checks. No lossy quantization or floating-point approximation is used.

## Kernel organization

The format uses independent 1 MiB chunks. Each tile has up to 32 little-endian 16-bit words, a one-byte width, and packed zigzag residuals. The GPU keeps four tiles in a 128-invocation workgroup, with one invocation per residual. Explicit workgroup barriers and reductions avoid assumptions about NVIDIA warp size or another vendor's subgroup width.

Five dispatches process a batch:

1. Compute residuals, reduce each tile's maximum bit width, and encode into a fixed 68-byte tile slot.
2. Scan tile lengths in groups of 256 tiles.
3. Scan 64 group lengths per chunk and choose packed or raw storage.
4. Calculate aligned offsets for the compressed chunks, excluding raw chunks from readback.
5. Gather tile bytes into a compact buffer.

Compaction assigns each aligned output word to exactly one invocation. When a word spans multiple tile boundaries, that invocation reads the necessary bytes from the following tiles. This prevents racing writes at byte-aligned boundaries without output atomics. Chunk padding is removed on the host, so it never changes the archive ratio.

The host first reads bounded chunk metadata, then copies exactly the required compressed extent. This requires two submission/readback phases per batch. The API does not expose a data-dependent indirect copy length here. Allocated buffers total approximately 41.5 MiB per session; this is explicit buffer capacity, not a measurement of total process or driver memory. The benchmark retains the input and output in host memory and is bounded to 512 MiB inputs.

## Selection and fair comparisons

`cpu-numeric`, `intel`, and `nvidia` run the **same forced numeric codec**, selecting the smallest estimated numeric representation from strides 1, 2, 4, 8, 16, 32, and 64. Selection examines three regions of at most 4 KiB and its time is charged to every numeric run. This selector deliberately does not decide whether numeric encoding is better than Zstandard: the purpose is to compare hardware execution at an identical ratio.

The original research `adaptive` selector and direct Zstandard levels 1 and 3 are separate CPU controls. They help identify cases where accelerating the numeric codec would still be a poor archival choice. The original experiment's source and results remain frozen in `research/codec-lab/`.

This is a payload-codec benchmark, not an end-to-end desktop archive workflow. It reads a byte stream, compresses and syncs an experimental archive, then reads, decodes and syncs the restored byte stream. It does not traverse source directories, construct/extract TAR members, encrypt, or authenticate with an IDP. Multiple source files are concatenated in the documented order, without repetitions or inserted padding. Original source lengths are recorded in the corpus manifest.

## Timing definitions

* **Cold create:** input read, codec execution, output write and sync, plus device/pipeline/buffer or CPU-pool initialization. "Cold" means a new session; OS and driver caches are not flushed.
* **Warm create:** the same operation with an existing session/pool. Five runs follow the first cold job. Executable startup is excluded from both; group process time is retained separately.
* **Codec:** selection, host staging, GPU transfers, all GPU dispatches, synchronization, compact-output assembly, framing and original-data checksum. Input/output file I/O and initialization are excluded. CPU controls use the same boundary.
* **Upload/compute/readback:** device timestamp intervals. Upload covers the staging-buffer-to-storage-buffer copy; readback covers compact-payload-to-readback-buffer copying. They exclude host mapping/copying and queue submission overhead. Tiny parameter, metadata and timestamp transfers are accounted in byte counts or host wall time, not all included in these device intervals.
* **Metadata/payload waits:** CPU wall time spent waiting for mapping/submissions. These already contain device work and must not be added to GPU timestamp intervals as if they were separate costs.
* **Recovery:** reading the created archive, original CPU decoding, writing and syncing restored bytes. Decode-only timing is retained separately. One/eight-worker CPU configurations use their named pool; GPU configurations use eight CPU workers for recovery.

Each job independently verifies the restored file SHA-256 after timing. GPU outputs are also compared byte-for-byte with a CPU reference outside the timed region. The harness subsequently checks identical numeric archive hashes across all four numeric configurations. Verification work runs between jobs and can affect cache and thermal state. Configurations execute serially in a seeded randomized order, with no concurrent benchmark GPU workloads. Power state, background activity, filesystem cache, and heterogeneous CPU placement are uncontrolled, so small timing differences should not be treated as reliable wins.

## Real data and attribution

Downloads are saved under the ignored `bench-output/numeric-real-corpus/` directory. The repository contains reproducible preparation code, hashes and measurements, not copies of the datasets. Subsequent preparation runs check downloaded bytes against the saved source hashes and reject changed inputs.

* **MIT-BIH ECG:** complete records 100 and 101 from the [MIT-BIH Arrhythmia Database, version 1.0.0](https://physionet.org/content/mitdb/1.0.0/), Moody and Mark (2005), DOI [10.13026/C2F305](https://doi.org/10.13026/C2F305). Original publication: Moody and Mark, *The impact of the MIT-BIH Arrhythmia Database*, IEEE Engineering in Medicine and Biology 20(3), 45–50 (2001). The original packed WFDB-212 bytes and a signed 16-bit little-endian expansion are separate inputs. The converter verifies exact repacking to the original bytes; conversion time is outside the benchmark. [WFDB signal format reference](https://physionet.org/physiotools/wag/signal-5.htm).
* **EEG:** complete original EDF+ recordings S001R03, S002R03, S003R03 and S004R03 from the [EEG Motor Movement/Imagery Dataset, version 1.0.0](https://physionet.org/content/eegmmidb/1.0.0/), Schalk (2009), DOI [10.13026/C28G6P](https://doi.org/10.13026/C28G6P). Original publication: Schalk et al., *BCI2000: A General-Purpose Brain-Computer Interface System*, IEEE Transactions on Biomedical Engineering 51(6), 1034–1043 (2004). EDF headers, record counts and sample counts are checked. Original headers and annotation channels remain in the tested bytes.
* **PTB ECG:** original `patient001/s0010_re.dat`, the 12-channel file from the [PTB Diagnostic ECG Database, version 1.0.0](https://physionet.org/content/ptbdb/1.0.0/). Its native 12-channel stride is outside the prototype's candidate set; this is intentionally retained as a limitation test. No clinical interpretation or patient metadata analysis is performed.
* **Terrain elevation:** complete `N37W122.hgt` from [Terrain Tiles on AWS Open Data](https://registry.opendata.aws/terrain-tiles/), accessed 2026-10-13. The original big-endian HGT bytes after gzip transport decompression and a reversibly byte-swapped little-endian array are separate inputs. Both conversion directions are checked. Neither gzip removal nor byte swapping is timed as codec work. Data attribution follows the [Tilezen source attribution list](https://github.com/tilezen/joerd/blob/master/docs/attribution.md); [Skadi format description](https://github.com/tilezen/joerd/blob/master/docs/formats.md).

MIT-BIH and EEG are distributed under the [Open Data Commons Attribution License v1.0](https://opendatacommons.org/licenses/by/1-0/). See each dataset page for its applicable terms. PhysioNet citation: Pollard et al., *PhysioNet as a global platform for biomedical research*, Nature Health (2026), [doi:10.1038/s44360-026-00096-z](https://doi.org/10.1038/s44360-026-00096-z).

Synthetic sensor and independent-noise controls, and the real enwik8 text control, come from the previously hash-verified local corpus. Sensor inputs are 1, 16 and 64 MiB prefixes of the existing 64 MiB stream, not duplicated data. Public real data are tested at their complete downloaded sizes, with no prefix cap.

## Reproduction

Requires the existing local Auto corpus for the retained controls, Python 3 with standard-library modules, and working Vulkan drivers on Windows/Linux. The code selects Metal on macOS, but that path is not hardware-validated here. AMD is likewise untested.

```powershell
python research/numeric-gpu/prepare_corpus.py
./scripts/cargo.ps1 test --example numeric_gpu_lab --locked --offline
./scripts/cargo.ps1 build --release --example numeric_gpu_lab --locked --offline
./target/release/examples/numeric_gpu_lab.exe validate intel > .qa/numeric-intel-validation.log
./target/release/examples/numeric_gpu_lab.exe validate nvidia > .qa/numeric-nvidia-validation.log
python research/numeric-gpu/benchmark.py
python research/numeric-gpu/summarize.py
```

The harness checkpoints atomically and resumes completed groups only when the binary, corpus manifest and measurement sources match. To run a fresh experiment, preserve the existing results under another filename first. Rerunning corpus preparation updates its retrieval metadata, so archive the previous results before preparing a new corpus run. The recorded experiment resumed after one transient Windows checkpoint-write error; codec code, inputs and timing boundaries were unchanged, and the report records both harness revisions.

An individual reusable-session comparison:

```powershell
./target/release/examples/numeric_gpu_lab.exe bench INPUT.bin nvidia 8 bench-output/numeric-job 6
```

Replace `nvidia` with `intel`, `cpu-numeric`, `zstd-1`, `zstd-3`, or `adaptive`. Outputs use the original experimental frame, never the production lttl archive extension. The benchmark verifies and deletes only the two per-run files that it creates; input files remain intact. There is no persistent background GPU service.

Validation covers every supported stride, zeros, noise, wrapped values, partial tiles, odd tails, 1 MiB boundaries, 8 MiB batch boundaries, changing input lengths, mixed packed/raw chunks, and multi-batch streams. Each adapter checks 316 GPU/CPU pairs plus compatibility with the frozen encoder and decoder. This is not a substitute for production fuzzing, device-loss handling, cancellation, or adversarial resource testing.
