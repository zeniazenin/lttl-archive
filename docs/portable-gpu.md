# Portable LZ4 compression

**Transfer/pipeline update:** see [output compaction and overlapping host/GPU work](portable-transfers.md) and the [verified CPU/GPU comparison](portable-v3-results.md). Earlier measurements below remain historical baselines.

**Kernel/session update:** see [the optimization implementation](portable-optimization.md) and [cold/warm results](portable-v2-results.md). The initial benchmark below remains the v1 baseline.

The Rust application now contains a wgpu/WGSL LZ4 encoder, a multithreaded CPU LZ4 encoder, and strict CPU extraction of independent LZ4 frames. Both create standard **TAR.LZ4** archives with a content checksum. This is separate from the existing NVIDIA nvCOMP Zstandard backend. No CUDA installation or NVIDIA DLL is needed for portable LZ4.

## Usage

```powershell
.\target\release\lttl.exe gpu-list
.\target\release\lttl.exe create C:\Data -o data.tar.lz4 -f tar-lz4 --backend wgpu --adapter intel
.\target\release\lttl.exe create C:\Data -o data-rtx.tar.lz4 -f tar-lz4 --backend wgpu --adapter nvidia
.\target\release\lttl.exe create C:\Data -o data-cpu.tar.lz4 -f tar-lz4 --backend cpu --threads 8
.\target\release\lttl.exe extract data.tar.lz4 -d unpacked
```

The desktop interface offers TAR + LZ4, CPU/Auto/Portable GPU, and an adapter name/vendor field. Explicit adapter selection fails if no matching hardware GPU exists; software adapters are excluded. `gpu-list` includes software devices for diagnostics, but the encoder never selects them.

`--chunk-kib` accepts 4, 8, 16, 32 or 64 for LZ4; the default is 16. `--batch-mib` accepts 1–64. The library/GUI batch default is 16 MiB; the CLI's shared Zstd/LZ4 option defaults to 32 MiB. CPU and GPU use the same independent block framing, but different match finders, so their sizes need not be identical. LZ4 has a fixed setting: Fast/Balanced/Small do not change it. The benchmark explicitly sets a 16 MiB batch for all LZ4 configurations.

## Implementation and limits

Each workgroup compresses one independent block. Sixty-four lanes build a 4,096-entry shared hash table with atomic minimum positions. The updated kernel selects serial or cooperative search according to a bounded Intel match-density probe (NVIDIA uses cooperative search), uses word comparisons/copies, and copies final literal runs cooperatively. One lane retains the deterministic greedy parse. Different blocks execute in parallel. The deterministic earliest-position dictionary is simple and portable, but collisions and serial token emission limit its speed and compression ratio. This is a working experimental encoder, not an optimized replacement for nvCOMP or CPU Zstandard.

GPU sessions and mapped upload buffers are now reused across successful jobs in the same process, subject to cache limits. An explicit mapped upload buffer stages each input batch; explicit copies transfer input to the device and output back. Readback currently transfers every block's worst-case output allocation, including unused space. CPU framing chooses stored blocks when compression would expand the data. Content checksums cover the original TAR bytes.

The Windows and Linux path uses Vulkan; the macOS path selects Metal. Initial D3D12 testing did not finish initialization promptly, so that path is not enabled. Intel Arc and RTX 4070 hardware tests exercise Vulkan. **AMD, Linux and Apple silicon have not been built or hardware-tested here.** Their support remains a portability target, not a verified claim.

Extraction supports independent LZ4 frames with standard block/header/content checksums and optional content size. Dependent blocks, external dictionaries, concatenated/skippable frames and legacy framing are rejected. The reader bounds block allocation to the declared standard maximum (at most 4 MiB), requires the end marker, checks available checksums, and retains the existing TAR path, size, entry, cancellation and publication protections. Extraction runs on the CPU.

## Auto policy

Auto for TAR.LZ4 takes the CPU path without sampling or GPU initialization and records its reason in `stats.portable.auto_reason`. It does not change the requested archive format. TAR.ZST retains its existing content-aware CPU/NVIDIA policy. Portable GPU selection requires a demonstrated complete-archive advantage; kernel throughput alone is insufficient.

## Results

[Complete benchmark tables](portable-results.md): **243/243** creation/extraction pairs passed, including **2,241** extracted-file SHA-256 checks. Neither GPU won any of the 36 dataset/settings comparisons against the faster CPU LZ4 baseline. GPU initialization and the serial token-emission kernel dominate; bulk transfer time is comparatively small. Auto therefore keeps CPU LZ4.

## Measurement definitions

`Stats.elapsed_seconds` includes input discovery, device/pipeline initialization, batching, compression, transfers, framing, archive writing, sync and publication. The harness also records fresh CLI process wall time, including startup and teardown. Each extraction is separately timed and every extracted file is checked against the corpus SHA-256 manifest.

`portable.initialize_seconds` covers device/pipeline/buffer initialization. Optional GPU timestamps report upload-copy, compression and readback-copy intervals. They exclude CPU staging, queue/map synchronization and framing; those costs remain in complete-archive wall time. `host_wall_seconds` for the GPU covers batch staging/dispatch/map/output collection, not the entire archive. `h2d_bytes` and `d2h_bytes` count actual bulk data copies, excluding uniforms and timestamp bookkeeping. Arc's integrated memory architecture still performs explicit logical buffer copies in this implementation.

The harness uses deterministic randomized order, sequential execution and fresh processes. OS and shader caches are not flushed. First-use driver compilation and power/thermal state can affect results. It compares CPU Zstd levels 1 and 3 (8 workers), CPU LZ4 (1 and 8 workers), Auto LZ4, and both GPUs at 4 and 16 KiB blocks. Three repetitions of nine corpus datasets yield 243 complete creation/extraction runs.

Run `python scripts/benchmark-portable.py --runs 3`. Raw results are saved to `docs/portable-benchmark-results.json`; the file includes executable SHA-256, adapter inventory, settings, every timing, ratio, and verification outcome. Smaller ratio means a smaller archive.

## Validation

`tests/portable.rs` exercises empty/tiny files, 4/16/64 KiB boundaries, mixed repetitive/noise content, Unicode filenames, multiple batches, standard lz4_flex frame interoperability, full byte comparison after extraction, damaged checksums, truncated end markers, format/backend mismatch and cancellation without publication. Intel and NVIDIA tests are ignored by default so ordinary builds do not require those adapters; run the portable test target with `--include-ignored --test-threads=1` on this machine.

The wire format follows the [LZ4 frame specification](https://github.com/lz4/lz4/blob/dev/doc/lz4_Frame_format.md). The compute abstraction is [wgpu](https://github.com/gfx-rs/wgpu), pinned here to 0.19.4. Future work should improve the match finder and parallel token construction, compact output before readback, and validate exact devices before broadening Auto eligibility. Session reuse and cooperative search/literal copies are now implemented.

The exact benchmark executable is preserved at `target/release/lttl-portable-benchmark.exe`. A subsequent CLI-only cleanup clarifies portable chunk options and rejects `--chunk-mib` for LZ4; compression, extraction and Auto behavior are unchanged.

Final validation: 28 ordinary regression tests passed, plus both explicitly run hardware tests. The independent C LZ4 decoder (python-lz4 4.4.5) and Python TAR reader verified both GPU archives, covering 22 Canterbury files. See [interop evidence](portable-interop-results.json) and [final build checks](portable-validation.json). Windows built-in tar could not decode LZ4 because its external `lz4` helper is absent, so it was not used as a passing interoperability check. Explorer scripts passed syntax checking and an eight-command installer dry run; no registry changes were made.
