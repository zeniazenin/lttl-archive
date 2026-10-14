# lttl GPU evaluation and practical compression improvements

This report separates lttl design proposals from measured results. The benchmark's retained `doctor` output confirms **NVIDIA GeForce RTX 4070 Laptop GPU**. No speedup is assumed from that product name or from GPU utilization alone.

## Benchmark status

The working GPU build completed **147 synthetic/real-snapshot creation and integrity cases**, with **31 extracted archive checks and 493 matching file hashes**. See [measured GPU performance](gpu-performance.md) and [raw results](../research/legacy/gpu-benchmark-results.json). CPU Fast and Balanced were faster on every baseline dataset; GPU had a tentative favorable tradeoff versus CPU Small on generated CSV, with overlapping timing ranges. Larger chunks did not restore useful compression for the deliberately related files under the tested Fast/fixed settings.

Downloaded public-corpus creation and unpacking measurements are complete in the [Canterbury/enwik8 report](downloaded-performance.md): 30 full creation/test/extraction cases and 180 matching hashes. CPU Balanced was faster and smaller than the GPU setting on both. These separate records keep real samples and the initial synthetic tests distinguishable.

Prepared corpus: `bench-output/gpu-review/20261012T212532860Z/corpus`.

```powershell
# Generate fresh inputs only; does not start compression benchmarks.
pwsh -NoProfile -File scripts/benchmark-gpu.ps1 -PrepareOnly -IncludeInstalledBinaries

# Once the CPU/GPU build is ready, reuse the exact prepared bytes.
pwsh -NoProfile -File scripts/benchmark-gpu.ps1 `
  -CorpusDirectory bench-output/gpu-review/20261012T212532860Z/corpus
```

The default matrix uses three repetitions of CPU Fast/Balanced/Small, CPU-chunked Fast, and GPU's fixed nvCOMP configuration. GPU results are labeled `nvcomp-fixed`, not assigned a nonexistent equivalent CPU compression level. The baseline uses 1 MiB chunks and 32 MiB batches. A focused related-file sweep adds 4 and 16 MiB chunks for CPU-chunked and GPU; all three sizes completed successfully.

Each process receives explicit argument arrays, with no shell interpolation. Every archive is tested. At least one archive per dataset/backend/chunk size is extracted and every recovered file is SHA-256 checked against the immutable corpus manifest. Verification is outside creation timings. Temporary extraction directories are removed only after resolving their absolute paths and checking that they stay under this benchmark run. Archive outputs are deleted after verification unless `-KeepArchives` is set. Raw JSON and source corpora are retained.

External process wall time, engine elapsed time, process CPU time, archive bytes, and all returned GPU metrics are retained. Peak working set was unavailable and remains null. GPU component fields include initialization, H2D copies, compression, D2H copies, chunk/batch counts, transfer bytes, host-call time and checksum processing. The measured-performance report maps those intervals to the actual implementation. Components must not be blindly summed into an alleged total. With `-TimeExtraction`, every variant/run also records separate external/engine extraction times, archive-test times, and post-extraction hash duration.

Runs are sequential, with ordering rotated between repetitions. Each invocation creates a new process/GPU context; startup is deliberately included, although driver/DLL caches may be warm. Other development compression/build work was paused during the timings. Filesystem cache, thermals, laptop power mode and graphics activity were not controlled. Subsecond differences and three repetitions warrant cautious conclusions. The script stops at the first failed creation/integrity check and preserves partial results; it also limits each child process and total run duration. A partial run is not a successful comparison.

## Input coverage

The primary synthetic datasets are **exactly 64 MiB each**. They represent deliberately chosen properties, not a statistical sample of the user's files.

| Dataset | Construction | What it can test |
|---|---|---|
| Structured CSV | Variable counters, timestamps, sensor IDs, bounded random fields and repeated messages | Short string reuse, text search effort and entropy coding |
| Raw numeric sensors | Little-endian 16-bit values across repeated channel patterns, slow changes and small independent noise | Numeric predictability; baseline byte compressors before any custom transform |
| Raw RGB-like bytes | Gradient/channel patterns with local independent noise | Structured image-like bytes; not JPEG/PNG and not a real photograph |
| Repeated binary | One independently generated 64 KiB block repeated to length | Dictionary distance, chunk boundary and framing overhead |
| Independent noise | A deterministic xorshift-based byte stream, with new bytes generated throughout | Little useful repetition; avoid futile codec work |
| Similar files | 64 separate 1 MiB files with shared random content and distinct 1 KiB tails | Cross-file reuse lost at resets and frame boundaries |
| Precompressed ZIP | Real Deflate streams for sensor/RGB prefixes, independent random bytes in a Stored member, and a short random ZIP comment to reach exactly 64 MiB | Recompressing a valid precompressed container without repeated or compressible padding |

Two smaller real-file collections use their actual sizes: an explicit snapshot of project source/Cargo/README files and, when requested, installed executable test data. The latter are copied as inert test data; their contents are not multiplied to simulate a larger real binary collection. No private user document folders are searched.

The C# generator is deterministic at the byte-pattern level and records its runtime plus every source hash. The ZIP stream bytes also depend on the .NET compressor version. Source snapshots and installed executable bytes can change; hashes identify the exact inputs in each run. Sharing the same retained corpus is preferable to regenerating it for another backend.

## Comparing CPU and GPU fairly

Three baselines answer different questions:

1. **CPU streaming** is the practical baseline for the application. It can preserve history across more input and may use threaded Zstandard internals.
2. **CPU-chunked** uses the same frame/chunk partitioning as GPU. It helps identify the cost of history resets separately from device execution, but CPU Zstandard Fast and nvCOMP's fixed encoder still have different search/encoding choices.
3. **GPU fixed** measures the actual available GPU codec and transfers. Container compatibility, final byte count, CPU extraction and hashing matter as much as kernel timing.

A GPU win means lower complete archive-creation latency at an acceptable output size/resource budget. A faster kernel that produces a much larger archive may lose after writing or transferring those additional bytes. A GPU run with high startup cost may become competitive in a persistent application context, but that requires a separate measured warm-context experiment. It must not be inferred by subtracting initialization from an unrelated wall-time sample.

The chunk sweep targets a concrete suspected tradeoff: related files can share bytes farther away than a 1 MiB independent frame permits. Larger chunks may restore useful history while increasing device workspace and reducing the number of parallel tasks. The measured result, not the theoretical expectation, should choose defaults.

## Prioritized design candidates

These are proposals until implemented and measured; they are not all current lttl features.

| Priority | Change | Adoption test |
|---|---|---|
| 1 | Keep explicit CPU/GPU choices and expose measured bytes/time, including GPU setup | Users can distinguish a functioning backend from a promise; errors do not silently claim GPU work |
| 1 | Route already compressed/noisy content to low-effort or stored output when the chosen format permits it | Sampling cost plus storage time beats compression, with verified byte identity and an override |
| 1 | Reuse GPU context and allocations for a queue of archive jobs | Full persistent-process runs improve over cold-start runs without stale state, memory growth, or broken cancellation |
| 2 | Pipeline input staging, H2D, compression, D2H and output writes with bounded memory | Transfer-inclusive wall time improves; CUDA-event timings and host synchronization are correctly accounted for |
| 2 | Tune chunk and batch sizes to content, device memory and job length | Related-file ratio improves where useful; small files do not pay disproportionate setup/launch overhead |
| 2 | Preserve related-file history with larger or grouped frames | Compression savings justify extra memory, slower random access and larger recovery units |
| 3 | Add codec-compatible numeric/executable transforms or optional dictionaries | Independent standard readers can decode the declared format; reversible transforms carry the metadata their inverse requires |
| 3 | Calibrate an automatic backend selector on this hardware | Repeated out-of-sample jobs beat a CPU-only policy under stated ratio and resource limits |

Raw numeric data is a plausible target for delta/bitshuffle-like preprocessing, and executable data can benefit from branch transforms. Applying an undeclared transform before writing an ordinary Zstandard frame would produce a syntactically valid stream whose decompressed bytes are wrong for the original file. Such ideas require an interoperable container/codec mechanism or an explicitly new format, not just an internal optimization switch.

### A parallel matcher experiment

An ordered position-history matcher reads the predecessor of each hash slot and then replaces that slot in input order. **Unordered GPU atomic exchanges do not preserve which earlier position each input observes.** An independently designed candidate could compute `(hash, position)` pairs in parallel, sort by hash and then position, and perform segmented predecessor selection before verifying candidate bytes against the original input. It would need to preserve the intended prefix/hash classes, window expiration and boundary semantics of its own reference algorithm; a hash collision is not a valid string match by itself.

This is a **future design** and is not a component of lttl. Sorting, scratch memory, compaction and transfer costs may exceed the CPU loop's cost. Benchmark candidate generation plus verification and the resulting archive ratio, rather than only the sort kernel. A CPU reference and adversarial repeated/colliding inputs are necessary before claiming correctness.

Bounded history overlap or codec dictionaries could preserve cross-chunk matches where an interoperable decoder can obtain the needed history/dictionary. The current nvCOMP Zstandard option structure exposes reserved fields rather than a supported window/search tuning knob. The implemented backend therefore cannot promise that increasing chunk size increases the effective matcher window; the measured 1/4/16 MiB sweep confirms that chunk size alone did not solve this corpus.

### Implemented flexibility versus proposals

Currently implemented: explicit streaming CPU, CPU-chunked and GPU backends; CPU compression presets and worker count; GPU chunk/batch controls within accepted limits; standard per-frame checksums; CPU extraction of concatenated standard frames; and recorded GPU timing components. The separate ZIP Fast sampling policy can select Store for sampled incompressible files.

The subsequent [content-aware implementation](auto-compression.md) adds bounded CPU/GPU routing and settings selection; its [benchmark](auto-performance.md) records benefits and regressions. Still not implemented: a new match finder, a persistent cross-job GPU context, overlapping batch pipeline, a tunable nvCOMP Zstandard search window, cross-frame history/dictionary support, custom file-type transforms, or GPU decompression. Those remain proposals with compatibility, correctness and performance tests attached.

Benchmark conclusions must come from complete measured archive operations, including transfer time and compression ratio. A vendor’s decompression-engine peak does not establish archive-creation performance.
