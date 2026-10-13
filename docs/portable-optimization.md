# Portable LZ4 kernel and session optimization

The later [transfer and pipeline update](portable-transfers.md) adds compacted readback and two pending batches. This page describes the v2 baseline retained for the [new benchmark](portable-v3-results.md).

The second implementation keeps the same match dictionary, greedy parse and standard LZ4 framing as v1. It changes how work executes and how GPU resources are retained. [Measured results](portable-v2-results.md) compare it with a preserved v1 executable, CPU LZ4 with one/eight workers, CPU Zstd Fast/Balanced, and Auto LZ4.

## Kernel changes

- Read unaligned four-byte words with one or two storage loads rather than four individual byte loads.
- Compare match extensions four bytes at a time, then handle the exact byte tail.
- On Intel, inspect at most four blocks from a batch on the CPU (64 KiB at the benchmark setting; at most 256 KiB with 64 KiB blocks). Count valid dictionary matches over up to 512 middle positions in each sampled block. Dense batches use the preserved serial kernel; sparse batches use the cooperative kernel. This small selector changes execution only, never output. NVIDIA bypasses the selector and uses the cooperative kernel. Its cost is reported as `kernel_selection_seconds`; `serial_batches` and `cooperative_batches` report the chosen paths.
- In cooperative search, have all 64 lanes inspect adjacent candidate positions in each search window. Lane zero consumes shared candidates in the original greedy order. A shared flag skips windows with no valid matches. Uniform workgroup loads synchronize the next search position, including jumps across long matches.
- Align literal output, then copy whole words. All lanes cooperate on the final literal run, which is especially important for incompressible blocks.

The hash function, earliest-position dictionary, 16 KiB benchmark blocks, minimum/final match rules and byte layout remain unchanged. The harness compares complete archive SHA-256 values against v1, so equal size alone cannot hide a different parse. Hardware correctness tests also cover 4 and 64 KiB blocks, overlapping repeated matches, mixed noise/repetitions, multi-batch input, truncation and checksums.

An initial experiment split dictionary building and encoding into separate dispatches with one block per lane. It preserved correctness but underfilled the GPU on these workloads and added global table traffic; it was not retained. An always-cooperative version also regressed dense-text performance on Intel because of barrier overhead. A combined adaptive shader did not resolve that regression. The final Intel implementation therefore retains the original serial shader as a separate pipeline for dense batches, with a bounded host selector. NVIDIA uses the optimized cooperative shader. The final kernel keeps one workgroup per block and requests 17 KiB of workgroup storage for its dictionary and shared scratch state. Intel Arc and RTX 4070 were tested through Vulkan. AMD, Apple and other operating systems remain untested.

## Reusable sessions

An in-process cache retains the device, pipeline, query objects, and buffers after successful encoding. Keys include the normalized adapter selector, chunk size and batch size. Taking a session transfers exclusive ownership to that encoder, so GUI worker threads do not share mutable GPU state. The mutex is held only while moving entries; compression, initialization and device destruction occur outside it.

The cache holds at most two idle sessions with a combined **256 MiB buffer allocation budget**. Driver allocations and active jobs are outside that idle-buffer budget. Oversized sessions are usable but not cached. The oldest idle entry is evicted as necessary. Failed/cancelled encoders discard their sessions; device-loss/error callbacks mark contexts unusable. A marked idle context is discarded and recreated on the next explicit request. Device removal was not physically induced during validation.

Upload buffers are now persistent `MAP_WRITE | COPY_SRC` allocations. Batches copy directly into mapped memory, avoiding the old temporary padded vector and per-batch GPU upload allocation. Upload buffers are remapped along with readback completion. Readback still transfers worst-case output slots; compaction is not implemented.

Metrics reset on acquisition. `session_reused` distinguishes warm sessions; `initialize_seconds` is zero on reuse; `session_acquire_seconds` includes cache lookup and, on a miss, initialization. Do not add acquisition and initialization together. Existing upload/kernel/readback timestamps remain available, and full archive wall time includes host work and transfers.

V1 mapped-at-creation upload allocations could stage bytes before the timestamped buffer copy. V2 uses an explicitly host-mappable upload allocation. Consequently, the upload timestamp alone is not a fair old/new total-transfer comparison; complete archive and batch wall times include both paths' staging costs.

The cache benefits repeated desktop jobs and repeated library calls. It does **not** span separate CLI processes, persist on disk, or install a daemon. Library callers can release idle contexts through `lttl::clear_portable_sessions()`.

## Reproduce warm and cold jobs

```powershell
New-Item -ItemType Directory .qa\session-check
.\target\release\lttl.exe benchmark-session C:\Data --adapter nvidia --output-dir .qa\session-check --runs 4
```

The first archive initializes a context; the next three reuse it. Replace `nvidia` with `intel`. Use `--cold` with a different empty directory to discard idle contexts before every job. Output archives are retained for independent verification. The output directory must be outside every input directory, so later runs cannot accidentally compress earlier output archives. `benchmark-session` itself reports timings; it does not claim integrity verification.

For the full verified comparison:

```powershell
python scripts/benchmark-portable-v2.py
python scripts/summarize-portable-v2.py
```

The harness uses the existing nine-dataset corpus and `target/release/lttl-portable-v1.exe` as the saved control. It randomizes configuration/dataset groups and runs them sequentially. Each new normal group creates four jobs in one process; three later jobs supply warm medians. Additional GPU groups clear contexts for three jobs, and v1 controls use three fresh processes. Every archive is extracted and every file checked against the corpus SHA-256 manifest. Final GPU archive hashes must all match v1 for each dataset.

There are 360 creation/extraction pairs. CPU comparisons use the same in-process operation boundary. Individual operation times exclude CLI startup; raw results also retain whole-process group times and per-process times for v1. OS and shader caches are not flushed. Laptop power/thermal state and background activity can affect these measurements; a close median win alone is not a reliable general routing rule.

## Validation and Auto

The ordinary release suite contains 28 passing tests. Four explicitly run hardware tests cover Intel/NVIDIA round trips and session lifecycle: reuse across worker threads, per-job metric/checksum reset, changed and shorter inputs, configuration isolation, eviction, explicit cache clearing, and cancellation without recaching the interrupted encoder.

Auto eligibility must consider both full-job speed and archive size. The report separately counts raw speed wins, at least 5% speed wins, and those that also produce no larger archive than CPU LZ4. Maintaining v1's ratio is not the same as matching CPU LZ4 or Zstandard's ratio. No automatic GPU routing should be inferred from kernel speed alone.

The completed comparison found no warm GPU speed wins over the faster CPU LZ4 configuration on any of the nine datasets. GPU output remained byte-identical to v1 but larger than CPU LZ4 output. Portable Auto remains on CPU without sampling. RTX kernel times improved 1.45–16.7×; Intel sparse workloads improved up to 11.4×, while dense results were mixed. Cached acquisition took approximately 2–3 microseconds. These kernel gains do not imply the same full-job speedup.

The exact measured v2 executable is preserved as `target/release/lttl-portable-v2-benchmark.exe`. A subsequent CLI-only change rejects benchmark output directories nested inside an input directory; compression/session code is unchanged. The rebuilt `target/release/lttl.exe` passed that rejection check and two additional verified archive/extraction jobs each on CPU, Intel and NVIDIA, including GPU session reuse. [Validation metadata](portable-v2-validation.json) records both executable hashes. The full benchmark completed 360 verified archive/extraction pairs and 3,320 extracted-file SHA-256 checks.
