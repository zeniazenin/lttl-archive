# CPU and GPU compression research

Research date: 2026-10-12. This document separates documented upstream capabilities, local observations, and proposed engineering work. It does not establish WinRAR's private implementation or claim benchmark results.

**Historical planning record — pre-GPU implementation pass, 2026-10-12.** The availability checks and proposed work below describe the earlier investigation, before project-local NVIDIA packages and the working lttl backend were added. Later on the same date, an experimental nvCOMP Zstandard backend ran on the RTX 4070 Laptop GPU. See [current GPU measurements](gpu-performance.md) and [implementation/improvement review](gpu-improvements.md) for completed validation, timing results, and remaining work. Historical “not implemented” and “no measurements” statements below are not the current app status.

## Recommendation

Build the first reliable archive path around streaming TAR + multithreaded Zstandard (`.tar.zst`), with ZIP/Deflate for exchange compatibility. Give users Fast, Balanced, and Compact presets plus explicit CPU-worker and compression-level controls. Benchmark real files before selecting defaults. Keep GPU compression an optional backend whose availability requires an actual successful initialization and round trip, not merely an NVIDIA device name.

Open formats satisfy the user's stated preference. Writing an application in Rust does not require rewriting mature codec kernels in Rust: safe Rust orchestration over maintained compression libraries is a practical first implementation. Reimplementing a codec would create significant correctness and performance work before offering user value.

## Evidence and capability boundary

| Item | Evidence | What it establishes |
| --- | --- | --- |
| Local GPU | `nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader` | NVIDIA GeForce RTX 4070 Laptop GPU; driver 592.82; 8188 MiB reported memory. |
| Local CUDA development tools | `Get-Command nvcc` and existence check of `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA` | Neither was found by these bounded checks. This is not a whole-disk inventory and does not establish absence of every CUDA runtime DLL. |
| CPU Zstandard | Official Rust `Encoder` documentation | `multithread(n_workers)` requires the `zstdmt` Cargo feature; zero disables workers, one separates compression from caller I/O. `finish()` must be checked to finalize output. [Source](https://docs.rs/zstd/latest/zstd/stream/write/struct.Encoder.html) |
| ZIP methods | Official Rust `zip` documentation | ZIP supports Deflate and optional Zstandard; file options select compression method and level. This is a library capability, not proof every recipient's ZIP reader supports Zstandard. [Crate](https://docs.rs/zip/latest/zip/), [options](https://docs.rs/zip/latest/zip/write/struct.FileOptions.html) |
| GPU codec | NVIDIA nvCOMP documentation | GPU compression and decompression include Zstandard, Deflate, LZ4, and other formats. GDeflate is a separate format and must not be mislabeled ZIP Deflate. [Source](https://docs.nvidia.com/cuda/nvcomp/) |
| GPU platform | NVIDIA installation requirements | Windows 10+, Volta or newer GPU, CUDA Toolkit 12+, Windows driver 527.41+, and Visual C++ redistributable 14+ are listed. Matching the GPU/driver subset does not establish a working installation. [Source](https://docs.nvidia.com/cuda/nvcomp/installation.html) |

No GPU compression job was run during this research. No nvCOMP libraries or CUDA toolchain were installed by this research task. Application support is determined by the actual code and verification results, not this capability table.

## CPU design

Stream filesystem input into a TAR writer, then a Zstandard encoder, then a buffered output file. This permits cross-file history and bounded application buffering without loading the archive into memory. Use explicit encoder finalization and propagate disk-full, read, and close errors. Build output under a temporary name and publish only after successful completion; cancellation must leave a clearly incomplete artifact or remove the temporary file.

Proposed presets for measurement are Zstandard level 1 (Fast), 3 (Balanced), and 9 (Compact). They are product starting points, not measured winners. Reserve levels 19–22 for advanced use after measuring time and memory. Upstream documents that more threads consume more memory and higher levels generally require larger tables; low levels are optimized for smaller memory budgets. [Zstandard CLI](https://github.com/facebook/zstd/blob/dev/programs/zstd.1.md), [memory guidance](https://github.com/facebook/zstd/wiki/Using-libzstd-in-a-memory-constrained-environment)

For ZIP, compress files independently when adding parallelism, retain bounded queues, and serialize container assembly. Do not share one mutable `ZipWriter` across unsynchronized threads or assume setting a thread count automatically parallelizes ZIP. Avoid nested parallelism: a pool of many archive jobs, each starting many codec workers, can oversubscribe the machine. TAR's sequential layout also makes selective extraction less convenient than ZIP; describe this tradeoff in format selection.

Keep the UI responsive through background jobs and throttled progress messages. Report bytes read, bytes written, elapsed time, actual backend, and actual worker count. A small-file workload may spend more time enumerating and opening files than compressing. Already compressed media and encrypted/random data may provide little size reduction; measure before adding a sampling-based stored mode.

## Proposed GPU path from the pre-implementation pass

Use a separately loadable Windows backend with a narrow C ABI and a safe Rust owner for handles and buffers. NVIDIA documents C++ managed APIs and a lower-level C batched API; the latter requires application-managed workspace, output buffers, and device pointer arrays. Pin the nvCOMP version and use that version's headers and libraries rather than manually assuming ABI layouts. [API comparison](https://docs.nvidia.com/cuda/nvcomp/api_comparison.html)

Proposed pipeline:

1. Read bounded batches into reusable host buffers.
2. Copy input to device memory; enqueue compression on CUDA streams.
3. Check each chunk's status, synchronize before consuming results, and copy only actual compressed bytes back.
4. Write a documented interoperable container representation and validate extraction with an independent CPU decoder.
5. On initialization failure, select CPU and display the concrete reason. On mid-job failure, restart cleanly or return an error; do not silently mix incompatible stream encodings.

The documented C API exposes `nvcompBatchedZstdCompressAsync`, device workspace sizing, alignment requirements, and output-size queries. Its Zstandard compression documentation recommends 64 KiB chunks and states a maximum of 16 MiB. It requires stream synchronization or correctly ordered operations before using results. Respect the installed version's limits; documentation and release notes may differ between revisions. [C API](https://docs.nvidia.com/cuda/nvcomp/c_api.html)

Independent chunk compression loses history across chunk boundaries, so compare its compression ratio with the CPU streaming baseline. A batch of compressed blocks is not automatically a valid ZIP, TAR.ZST, or LZ4-frame file. Standard frame/container assembly and independent interoperability tests are required. GDeflate or proprietary NVIDIA formats would add recipient/tooling requirements and should not become the default exchange format.

End-to-end time must include reading, host/device transfers, allocation, initialization, container construction, checksums, and writing. GPU kernel throughput alone cannot demonstrate a faster archiver. Reuse allocations and overlap stages only after correctness is established. For small inputs the startup cost may dominate; for larger inputs the SSD or PCIe transfers may dominate. The crossover point must be measured on this laptop under its actual power/thermal settings.

NVIDIA's selection guide favors Zstandard when CPU decoding matters and identifies some other codecs as useful for specialized numerical workloads. Its dedicated hardware decompression engine discussion concerns specified Blackwell datacenter GPUs, not a promise about this RTX 4070. [Algorithm selection](https://docs.nvidia.com/cuda/nvcomp/algorithm_selection.html)

The current release notes list input-validity limitations for several GPU decompressors and specific Zstandard scratch-space caveats. Use the established CPU extraction path for arbitrary downloaded archives until malformed-input behavior is independently assessed. GPU compression can still be investigated separately. [Release notes](https://docs.nvidia.com/cuda/nvcomp/release_notes.html)

## Benchmark acceptance criteria

Use a reproducible corpus covering source trees/small files, plain text/logs, large binaries, mixed documents, already compressed media, and pseudorandom data. Save the corpus manifest, byte sizes, and hashes. Do not benchmark only repeated zeros.

Measure release builds at worker counts 1, 2, 4, and a machine-appropriate maximum; compare Zstandard levels 1, 3, 9 and ZIP Deflate levels 1, 6, 9. Include an uncompressed copy baseline. Run repeated warm-cache tests and separately identify cold-cache experiments. Publish medians, compressed bytes, input MiB/s, extraction time, peak RAM/VRAM, CPU/GPU utilization, power mode, and whether output was flushed to durable storage.

For every candidate backend, validate extracted bytes against the source hashes using an independent compatible decoder. Test empty files/directories, Unicode names, large files, cancellation, truncated streams, inaccessible files, incompressible data, and output-full errors. Keep benchmarks separate from correctness tests. Ship GPU as an enabled choice only after it produces interoperable archives and wins end-to-end for a documented workload; otherwise report it as unavailable or experimental with the reason.

## Remaining gaps

- No measured CPU or GPU speed/ratio comparison is supplied by this report.
- CUDA/nvCOMP runtime availability and redistribution packaging remain to be established for a GPU implementation.
- A Rust-to-nvCOMP binding, stream/container interoperability, and laptop-specific performance threshold remain engineering work.
- UI defaults should reflect measurements and the user's requested simple view with advanced controls.
