# lttl NVIDIA backend

lttl now has an optional working GPU compressor for Windows x64. It uses NVIDIA nvCOMP 5.3.0.16 with CUDA runtime 12.9.79, verified on this machine's RTX 4070 Laptop GPU (8 GiB, driver 592.82). The Rust engine owns archive construction, cancellation, validation, checksums, and publication; a small C++ ABI bridge calls NVIDIA's compiled kernels. No CUDA kernel compiler is invoked during the bridge build.

## Reproduce the installation

```powershell
.\scripts\setup-gpu.ps1
.\scripts\build-gpu.ps1
.\scripts\cargo.ps1 build --release --locked
.\target\release\lttl.exe doctor
.\target\release\lttl.exe create .\input -o .\new.tar.zst --backend gpu --chunk-mib 1 --batch-mib 32
```

The setup script pins the official [nvCOMP 5.3.0 manifest](https://developer.download.nvidia.com/compute/nvcomp/redist/redistrib_5.3.0.json) and [CUDA 12.9.1 manifest](https://developer.download.nvidia.com/compute/cuda/redist/redistrib_12.9.1.json) hashes. It downloads nvCOMP (~67 MB), CUDA runtime (~3.5 MB), and CUDA compiler-package headers (the archive is ~127 MB; only headers are extracted). MSVC x64 build tools are required. Runtime DLLs and vendor license/notice files are deployed beside the executable in `gpu/`. Keep that folder with `lttl.exe`. NVIDIA's proprietary dependency licenses remain separate from this project's code license.

CPU builds and CPU archiving do not require NVIDIA DLLs. Explicit GPU selection fails with a concrete error if its runtime/device is unavailable; it does not silently report CPU work as GPU work. Auto records an unavailable GPU trial and chooses CPU before creating output. No driver, registry, system PATH, or system CUDA installation is changed. The first suitable Volta-or-newer CUDA device is selected; this machine has the requested RTX 4070. Multi-GPU selection is not yet exposed.

## Format and execution

The GPU path streams TAR bytes into a bounded host batch, splits that batch into independent chunks, compresses them with nvCOMP's low-level Zstandard API, and writes ordinary concatenated Zstandard frames. lttl adds a standard content checksum derived from each original chunk if nvCOMP omits it. Extraction and testing use the existing CPU decoder. Recipients need a compatible Zstandard/TAR reader, not an NVIDIA GPU.

`--backend auto` is now the default; see the [content-aware policy](auto-compression.md) and [new comparison](auto-performance.md). Explicit `--backend cpu` retains streaming multithreaded Zstandard. `--backend gpu` uses NVIDIA's fixed compressor settings; CPU Fast/Balanced/Small presets do not apply. `--backend cpu-chunked` is a comparison/control mode using the same independent-frame boundaries with bounded CPU workers. Its workers are created for each group; it is not a perfectly matched implementation of the GPU algorithm.

Chunk size is 64–16384 KiB (`--chunk-kib`), batch size 16–128 MiB; defaults remain 1/32 MiB. The legacy `--chunk-mib` flag is retained and conflicts with an explicitly supplied `--chunk-kib`. The 16 MiB limit is conservative because the downloaded header's public maximum constant and function documentation differ. Buffers, scratch workspace, pinned staging, streams, and events are allocated once and reused throughout one archive job. Each new archive currently creates a new context. The native bridge checks workspace against available VRAM with a 256 MiB reserve, validates per-chunk status and output lengths, and copies back only valid compressed bytes.

The [same-run tuning comparison](gpu-tuning.md) measured useful, data-dependent improvements. For the synthetic CSV corpus, `--chunk-kib 64 --batch-mib 32` reduced complete GPU creation time by about 28% and output size by about 2.5% relative to the original GPU setting. For enwik8, `--chunk-mib 1 --batch-mib 128` reduced time by about 17% with identical output size. These settings still lost to CPU Balanced. The later Auto policy uses a real 64 KiB GPU sample only for sufficiently large structured text with the Small goal; it does not assume a universally faster GPU preset.

The current pipeline is sequential between batches. Host staging copies, device transfers, GPU computation, checksums, and file writing are not overlapped. Independent chunks reset history, and nvCOMP's own match-search choices may limit ratio even when chunks are enlarged. GPU is therefore an experimental choice, not a universal speed upgrade. See [measured comparisons](gpu-performance.md) and [RAR-informed improvement priorities](gpu-improvements.md).

## Timing definitions

- `Stats.elapsed_seconds`: complete in-process creation, including manifest walk, Auto selection/trials, runtime loading/initialization, allocations, input reads, TAR framing, compression, transfers, checksums, output writing, runtime cleanup, sync, and publication.
- Benchmark process wall time: additionally includes launching and exiting lttl and JSON output. This is the practical latency comparison.
- `gpu.initialize_seconds`: loading the DLL, creating the CUDA context, querying workspace, and allocating buffers.
- `gpu.host_wall_seconds`: sum of Rust native-compression call intervals, including output vector allocation, native host copies, CUDA calls, synchronization, and native frame validation. It excludes separate checksum/framing work and disk I/O.
- `gpu.host_to_device_ms`, `compression_ms`, `device_to_host_ms`: CUDA event intervals on one stream. Transfers include pointer/size/status tables; compression includes clearing status/output-size tables. These intervals include stream scheduling/launch gaps and are not a kernel-only profiler. D2H transfers only actual frame data, not unused stride padding.
- `gpu.checksum_seconds`: host frame validation, checksum computation, and checksum-bearing output assembly.
- Transfer byte counts exclude host-added checksum bytes. Compressed archive size includes all checksums and TAR/framing overhead.
- Auto records trial/initialization costs separately in `selection.gpu_trial`, then reuses the selected context. `Stats.gpu` contains subsequent archive-only work, with initialization zero. Both are included in complete elapsed/process time; sum trial and archive CUDA-event counters only when comparing component totals.

Do not add overlapping host and CUDA timings or subtract setup time to claim a measured warm-context result. GPU initialization, power state, file cache, laptop thermals, and driver overhead can vary across runs. Extraction measurements describe CPU decoding of each backend's output, including the current staging/copy publication process.

## Correctness checks

The dedicated GPU integration test exercises multiple batches, a partial chunk, empty files/directories, Unicode paths, CPU extraction with byte comparison, deliberate checksum corruption, and cancellation without publishing a partial archive. Native smoke tests independently decoded GPU Zstandard frames with Python 3.14's standard Zstandard decoder. Benchmark scripts perform integrity checks and extracted SHA256 comparisons outside the creation timer.

```powershell
# Build/deploy the bridge next to Cargo's integration-test binaries first.
.\scripts\build-gpu.ps1 -OutputDirectory .\target\debug\deps\gpu
.\scripts\cargo.ps1 -CargoArgs @('test','--locked','--test','gpu','--','--ignored')
```

The output is ordinary TAR.ZST, not a new `.lttl` format. RAR reading/writing, archive encryption, solid-group indexes, transform metadata, multi-GPU work, and GPU decompression in the app are not implemented. The separate [codec probe](codec-performance.md) uses GPU decoding only to verify its experimental compressed payloads.
