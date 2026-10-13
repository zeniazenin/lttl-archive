# Detailed user guide

An early Rust desktop archiver for Windows, with a simple interface, expandable advanced controls, and a command-line engine. It creates, lists, tests, and extracts **ZIP**, **TAR.ZST**, and **TAR.LZ4** archives. **Auto** is the default for TAR.ZST: it samples content and chooses CPU settings, uncompressed storage, or the NVIDIA GPU according to the selected goal. The experimental GPU backend has been exercised on an **RTX 4070 Laptop GPU**.

The project uses open compression libraries and NVIDIA nvCOMP. It does not create or extract RAR archives or provide full WinRAR compatibility. The [native investigation](native-analysis.md) informed architectural analysis; the app does not contain a reconstructed proprietary RAR codec.

The [Auto optimization comparison](auto-optimization.md) completed 324 verified creation/extraction runs against preserved Auto v1 and explicit CPU presets. Fast now skips sampling, and Balanced uses cheaper probes; all 18 Balanced/Small dataset-goal combinations retained their previous archive sizes. Selection overhead fell substantially, but some full-run regressions and GPU startup spikes remain. Auto is a bounded heuristic, not a guaranteed optimum. Explicit CPU/GPU choices remain available. The earlier [GPU baseline](gpu-performance.md) and [Auto v1 results](auto-performance.md) retain their original measurements.

The [portable wgpu LZ4 backend](portable-gpu.md) runs on Intel Arc and RTX 4070 through Vulkan. TAR.LZ4 has CPU/Auto/portable GPU choices and CPU extraction; AMD and Apple hardware remain untested. The latest [output compaction and two-batch pipeline](portable-transfers.md) passed [468 creation/extraction runs and 4,316 file-hash checks](portable-v3-results.md), retaining byte-identical GPU output. Readback traffic fell 10.3–99.95%. RTX complete warm archive creation improved 1.12–1.46× over v2 and beat CPU LZ4 on 7/9 datasets (5/9 by at least 5%); Intel had no CPU speed wins and mixed improvements. GPU archives remain larger than CPU archives, so portable Auto still uses CPU without sampling. Session reuse benefits repeated GUI/library jobs within one process; separate CLI invocations initialize new sessions. Earlier [kernel/session results](portable-v2-results.md) and the [initial baseline](portable-results.md) remain available.

## Build and open

Identity-protected sharing is available as a local preview: select recipients, create an encrypted `.lttl` archive, and require IDP sign-in to open it. The key service can run on **Azure, AWS, Google Cloud or self-hosted infrastructure**, independently of the IDP. The desktop generates combined registration/deployment instructions, and the broker wraps archive keys through native cloud KMS using workload identity. Serverless deployment uses Azure Functions, AWS Lambda or Google Cloud Run with shared PostgreSQL state. See [identity sharing](identity-sharing.md) and [deployment options](../identity-broker/deploy/README.md). Local protocol/crypto tests pass; live cloud deployment and IDP policy enforcement still need pilot validation.

The key broker requires Rust 1.92 or newer. For the desktop, use Windows x64, Rust 1.88 or newer with the MSVC toolchain, and the Visual Studio C++ build tools/Windows SDK. Dependency download requires network access on the first build. Run these commands from the repository root:

```powershell
.\scripts\cargo.ps1 test --locked
.\scripts\cargo.ps1 build --release
.\target\release\lttl.exe gui
```

Alternatively, build and launch together:

```powershell
.\scripts\cargo.ps1 run --release -- gui
```

The wrapper uses a project-local `.cargo-home` and the installed stable Windows MSVC toolchain when available. GUI support is enabled by default. For a command-line-only build:

```powershell
.\scripts\cargo.ps1 build --release --no-default-features
```

To build the optional GPU backend, run:

```powershell
.\scripts\setup-gpu.ps1
.\scripts\build-gpu.ps1
.\scripts\cargo.ps1 build --release
.\target\release\lttl.exe doctor
```

GPU setup downloads the pinned NVIDIA redistributable packages into the project. The native bridge is built with MSVC; a system-wide CUDA toolkit installation is not required. Keep the complete `target/release/gpu` directory, including DLLs and vendor notices, beside `lttl.exe` when moving the app. CPU operation does not require these GPU files. `doctor` attempts actual GPU initialization and reports the device or a concrete failure reason; it is not a compression round-trip test. Explicit GPU creation fails if the backend cannot initialize.

## Current behavior

| Capability | Implementation |
|---|---|
| Desktop interface | Create, extract, inspect, and test views; file/folder selection; background operations; progress and cancellation for supported jobs |
| ZIP | Deflate compression, one CPU compression stream; presets use levels 1 / 6 / 9; Fast can store files whose samples show little compression benefit |
| Auto TAR.ZST | Fast directly uses CPU Fast with no sampling; Balanced uses 64 KiB sample regions; Small retains up to 1.5 MiB samples and actual GPU trials for structured content; records its decision |
| TAR.LZ4 | Standard independent LZ4 blocks with content checksum; multithreaded CPU or portable wgpu encoder, strict CPU extraction |
| CPU TAR.ZST | Streaming TAR with Zstandard compression and a frame checksum; presets use levels 1 / 3 / 9 |
| GPU TAR.ZST | NVIDIA nvCOMP's fixed Zstandard setting; independent, checksummed standard frames encoding successive TAR-stream chunks; CPU extraction |
| CPU controls | Zstandard worker count; `0` automatically chooses up to eight available logical processors; ZIP remains serial |
| NVIDIA Zstd controls | Chunk size 64–16384 KiB and batch size 16–128 MiB; defaults 1 MiB / 32 MiB; CPU presets do not apply |
| Presets | Auto `fast`, `balanced`, `small` goals; explicit CPU uses fixed levels; smaller output is workload-dependent |
| Existing files | Creation refuses an existing archive output; extraction refuses existing file collisions |
| Explorer | Optional per-user classic context-menu actions for one selected item |
| Identity sharing | Encrypted `.lttl` envelopes, enrolled-recipient picker, fresh OIDC authorization and server-side revocation; SAML via federation |
| Key-service hosting | Azure Functions + Key Vault; AWS Lambda + KMS; Google Cloud Run + Cloud KMS. Native wrapping, shared PostgreSQL state; live cloud deployment still requires validation |

ZIP Fast samples the first, middle, and last 64 KiB of files at least 1 MiB long using Zstandard level 1. If all three compressed probes retain at least 99% of their original size, the ZIP member is stored without compression. This is a bounded heuristic: samples can miss redundancy, and Zstandard estimates are not exact Deflate predictions. Balanced and Small still apply Deflate normally; input contents are unchanged.

CPU TAR.ZST preserves compression history across the TAR stream. GPU chunks have independent histories, so related files or repetitions across chunk boundaries may compress less effectively. Larger chunks trade memory and parallelism against history retention. The CLI also exposes `cpu-chunked` as a comparison backend using the same chunk partitioning; its encoder setting is still not equivalent to nvCOMP's fixed setting. Bounded solid groups, content-based grouping, configurable dictionary/window controls, and file-type transforms remain future work.

The desktop Create view offers Auto/CPU/NVIDIA GPU selection for TAR.ZST and shows Auto's choice and reason after creation. Explicit GPU chunk and batch controls are under Advanced controls; Auto chooses these internally. ZIP always uses the CPU and retains its existing preset policy. [Earlier interface preview](interface-preview.png) shows the **pre-GPU prototype interface**, before the lttl rename and backend controls; it is not a screenshot of the current build.

## Command-line examples

Use your own existing input path. The archive output's parent directory must exist, and the output filename must be new.

```powershell
# Let Auto choose based on content (the default).
.\target\release\lttl.exe create .\input -o .\backup.tar.zst --format tar-zstd --preset balanced --threads 8

# Favor size; Auto may compare the GPU on structured content.
.\target\release\lttl.exe create .\input -o .\small.tar.zst --backend auto --preset small

# Bypass sampling and retain the previous fixed CPU behavior.
.\target\release\lttl.exe create .\input -o .\cpu.tar.zst --backend cpu --preset balanced

# Create with the NVIDIA GPU's fixed Zstandard setting.
.\target\release\lttl.exe create .\input -o .\gpu-backup.tar.zst --format tar-zstd --backend gpu --chunk-mib 1 --batch-mib 32

# GPU setting measured on generated CSV: smaller parallel chunks.
.\target\release\lttl.exe create .\input -o .\fast-gpu.tar.zst --backend gpu --chunk-kib 64 --batch-mib 32

# Create a broadly compatible ZIP archive.
.\target\release\lttl.exe create .\input -o .\backup.zip --format zip --preset fast

# Inspect names/sizes, then read and verify the archive's file data.
.\target\release\lttl.exe list .\backup.tar.zst
.\target\release\lttl.exe test .\backup.tar.zst

# Extract into a new directory whose parent already exists.
.\target\release\lttl.exe extract .\backup.tar.zst -d .\restored

# Probe the GPU runtime, or benchmark six CPU format/preset combinations.
.\target\release\lttl.exe doctor
.\target\release\lttl.exe benchmark .\input --threads 8 --runs 3
```

Most command results are JSON; progress goes to stderr. Auto results include `selection` with the content class, selected settings, sample ratios, reason, time, and any GPU trial or unavailability. GPU results include the actual device, transfer/compression timings, chunks, batches, and checksum time. These component intervals do not replace complete process wall time. `benchmark` covers CPU codecs, creates temporary archives, verifies each one, and removes its output. The [optimization harness](../scripts/benchmark-auto-v2.ps1) interleaves current Auto, preserved Auto v1, and CPU presets with timed extraction and hashes; the [original harness](../scripts/benchmark-auto.ps1) also compares explicit GPU defaults. `--help` and each subcommand's `--help` list the available options.

## Explorer integration

Build the GUI-enabled executable before reviewing or installing the integration, and keep that executable at a stable location:

```powershell
.\scripts\install-explorer.ps1 -ExecutablePath .\target\release\lttl.exe -WhatIf
.\scripts\install-explorer.ps1 -ExecutablePath .\target\release\lttl.exe
```

The installer adds **Create archive**, **Extract here**, and **Extract to named folder** for the current user. Actions open the corresponding interface with paths prefilled; they wait for the user to start. Extraction entries apply to ZIP, TAR.ZST, TAR.LZ4, and encrypted .lttl files. On Windows 11, these classic entries normally appear under **Show more options**. Selection is limited to one item, and default file associations are unchanged.

To preview removal and then remove the project's entries:

```powershell
.\scripts\uninstall-explorer.ps1 -WhatIf
.\scripts\uninstall-explorer.ps1
```

See [Explorer integration details](explorer-integration.md) for registration scope, conflict handling, and manual acceptance checks. `-WhatIf` reviews planned registry changes; it does not validate Explorer rendering or dispatch.

## Integrity, extraction, and compatibility limits

Archive creation uses a temporary output and publishes it without overwriting. Extraction validates and decompresses into staging before copying files into the destination. This requires temporary disk space and adds an extra write/copy; a whole extraction is not one atomic filesystem transaction. If publication fails, cleanup of files created by that operation is best effort.

The engine rejects unsafe member paths, duplicate/case-conflicting names, file/directory collisions, links, Windows reparse points, and special file entries. It enforces a 100 GiB total expanded-file limit and a one-million-entry/directory limit. A new extraction destination's parent must already exist. These conservative rules can reject otherwise valid third-party archives.

`test` reads file contents and checks declared lengths and codec integrity, including ZIP CRC and available Zstandard checksums. Listing a ZIP alone is not a full integrity test. Archives created here include a Zstandard checksum; checksums detect accidental corruption and are not cryptographic authenticity checks.

This prototype supports ordinary files/directories, ZIP Store/Deflate reading, and a restricted TAR subset within Zstandard, including GNU long filenames. It does not support standalone `.zst` files, RAR, 7z, password-encrypted ZIP/7z archives, split volumes, recovery records, PAX/sparse TAR entries, or archive updates. It does not preserve Windows ACLs, alternate data streams, all timestamps, or complete filesystem metadata. It does not take a filesystem snapshot, so archive inputs that are actively being modified are unsuitable for reliable backups.

Use copies of representative data while evaluating this alpha. Test the resulting archive and check an extracted copy against the originals before relying on it.

The Auto implementation passes 26 ordinary tests, including raw-frame corruption detection, related-content selection, misleading extensions, sampling cancellation, Fast/CPU output equivalence, and independent compression-context reset checks. Its optimization benchmark completed 324 creation/integrity/extraction cases with 2,988 matching file hashes. Earlier GPU hardware and benchmark checks are retained in the validation record. Default `cargo test` skips the hardware-dependent GPU test. These results cover the recorded inputs and configurations, not every archive or device.

## Investigation and benchmarks

- [AMD, Intel and Apple silicon feasibility](vendor-support.md): current CPU portability, vendor GPU options, local Intel Arc availability, and the earlier proposal for the now-implemented wgpu backend; AMD/Apple hardware remain untested.
- [Auto sampling optimization](auto-optimization.md), [raw v1/v2/CPU comparison](auto-v2-benchmark-results.json), and [reproduction script](../scripts/benchmark-auto-v2.ps1).
- [Content-aware policy and measured results](auto-compression.md), [raw Auto comparison](auto-benchmark-results.json), and [reproduction script](../scripts/benchmark-auto.ps1).
- [Measured CPU/GPU performance](gpu-performance.md), [raw GPU comparison results](gpu-benchmark-results.json), and [benchmark harness](../scripts/benchmark-gpu.ps1).
- [GPU design and improvement review](gpu-improvements.md): chunk history, complete pipeline costs, and remaining opportunities.
- [Measured chunk/batch tuning](gpu-tuning.md): 64 KiB chunks improved generated CSV; 128 MiB batches improved enwik8 while preserving output size. CPU remains faster on these inputs.
- [GPU codec choices](gpu-algorithms.md) and [measured codec probes](codec-performance.md): Zstandard, LZ4 and GDeflate speed/ratio tradeoffs. Alternative codecs are experiments, not additional app archive formats.
- [Public sample download script](../scripts/download-samples.ps1): verified Canterbury and enwik8 inputs, with retained provenance and file hashes; see the performance report for completed downloaded-sample results.
- [RAR compressor review and measured results](rar-compressor-review.md): controlled synthetic-corpus results, documented RAR behavior, and evidence limits.
- [Raw RAR benchmark CSV](rar-benchmark-results.csv) and [reproduction script](../scripts/benchmark-rar.ps1).
- [Rust comparison on the same corpus](rust-performance.md), [raw results](rust-benchmark-results.json), and [reproduction script](../scripts/benchmark-rust.ps1).
- [Performance research](performance-research.md): open-codec and CPU/GPU considerations.
- [Native REA findings](native-analysis.md): bounded static analysis with evidence IDs and unresolved questions.
- [Validation record](validation.md) and [decompiler setup](decompiler-setup.md).
- [Internal API contract](api-contract.md): the library/GUI interface.

The RAR measurements compare settings of the installed RAR executable. Earlier Rust CPU results reuse those synthetic inputs; the later GPU report uses its own recorded comparison matrix. These experiments do not establish a universal performance winner. The older native-analysis and performance-research documents retain their dated pre-GPU investigation scope; current implementation and timing results are in the GPU report.

To prepare public samples and run the reproducible comparison (PowerShell 7):

```powershell
pwsh -NoProfile -File .\scripts\download-samples.ps1
pwsh -NoProfile -File .\scripts\benchmark-gpu.ps1 -CorpusDirectory .\bench-output\downloaded-samples\corpus -Repeats 3
```

The downloader treats all sample contents as inert data and records provenance and hashes. Benchmark checks run outside archive-creation timing. Small timing differences need repeated, isolated measurements rather than a conclusion based on a single run.
