# GPU codec candidates for lttl

Research and measurements checked 2026-10-12. Target: NVIDIA GeForce RTX 4070 Laptop GPU, with the project's installed nvCOMP 5.3.0.16 CUDA 12 package. This document separates published guidance, completed lttl archive measurements, in-memory codec probes, and proposed experiments. Vendor rankings are not substituted for measurements on this laptop.

**Keep CPU as the default.** Prioritize smaller Zstandard chunks and resource reuse first, then evaluate LZ4/Snappy as speed-oriented alternatives. GDeflate deserves a separate GPU-decoding experiment; Cascaded and Bitcomp belong in explicit numeric-data experiments. Changing a codec also changes its compression ratio, decoding requirements and container work.

## What the completed measurements establish

In the [original nine-dataset evaluation](gpu-performance.md), CPU Fast and Balanced created archives faster than the fixed GPU Zstandard configuration on every dataset. GPU's small median advantage over CPU Small on generated CSV had overlapping timing ranges. On [downloaded Canterbury and enwik8](downloaded-performance.md), CPU Balanced was both faster and smaller. For enwik8, CPU Balanced took 0.273 s and produced 35,444,627 bytes; GPU took 0.708 s and produced 38,603,223 bytes. These are complete creation times, including process startup.

The downloaded-data GPU timings also show that device compression, not merely transfer, needs attention: enwik8 had approximately 304 ms of device compression, 117 ms initialization, and 13 ms combined H2D/D2H transfers. Reusing setup can help repeat jobs, but these intervals do not demonstrate that setup reuse alone will beat CPU. Warm-context performance must be measured directly.

## Codec comparison

The suitability column is an engineering assessment for lttl, not a promise of speed. “Standard” refers to the compressed representation; a file archiver still needs filenames, lengths, metadata, checksums and a tested reader.

| Codec | Published design / likely experiment | Archive compatibility and proposed role |
|---|---|---|
| **Zstandard** | General byte compression with entropy coding. NVIDIA recommends it when ratio and CPU decoding matter. The installed low-level compression options have only reserved bytes; there is no supported CPU-like level/search knob in that header. | Current lttl GPU backend: TAR inside concatenated, checksummed standard Zstandard frames, decoded on CPU. Lowest integration risk; tune chunking and execution before adding a format. |
| **LZ4** | Byte-oriented matching without the heavier entropy stage. NVIDIA parallelizes independent blocks; block size affects throughput and ratio. | Strong candidate for an optional fast `.tar.lz4` mode, after implementing standard LZ4 framing and checksums and checking an independent CPU reader. Raw GPU block output is insufficient. [NVIDIA LZ4](https://docs.nvidia.com/cuda/nvcomp/lz4.html), [LZ4 frame specification](https://github.com/lz4/lz4/blob/dev/doc/lz4_Frame_format.md). |
| **Snappy** | Another general byte codec; NVIDIA says its GPU implementation often performs better than LZ4, while LZ4 generally fares better on CPU. Treat this as a hypothesis to test here. | An open CPU-decodable format with an explicit framing specification, including checksums. A possible fast-mode experiment; adding it still requires a TAR/container convention and reader support. [NVIDIA selection guide](https://docs.nvidia.com/cuda/nvcomp/algorithm_selection.html), [Snappy framing](https://github.com/google/snappy/blob/main/framing_format.txt). |
| **GDeflate** | Reformats DEFLATE's stream to expose parallel decoding. Its modes trade compression effort for ratio. NVIDIA's guidance is workload-dependent: GPU decoding can favor GDeflate, while its prose says Zstd can compress faster than GDeflate modes 2–5 at similar ratios. | GPU-oriented, **not strictly GPU-only**: Microsoft's reference includes CPU compression/decompression and an HLSL decoder. It is not ZIP Deflate and cannot replace a `.tar.zst` payload. Requires a declared container and CPU extraction path before product integration. [Microsoft GDeflate](https://github.com/microsoft/DirectStorage/blob/main/GDeflate/README.md), [NVIDIA selection guide](https://docs.nvidia.com/cuda/nvcomp/algorithm_selection.html). |
| **Cascaded** | Configurable run-length, delta and bitpacking stages suit repeated or slowly changing typed columns. Random or poorly grouped values can expand. | NVIDIA-specific compressed representation; no general archive interchange path is established here. Test on explicitly described sensor columns, with type/ordering metadata and exact byte reconstruction. An arbitrary mixed-file archive is not automatically a typed column. [NVIDIA Cascaded](https://docs.nvidia.com/cuda/nvcomp/cascaded.html). |
| **Bitcomp** | NVIDIA's proprietary scientific numeric codec has default and sparse modes. Its batched API is lossless; the native API also offers lossy modes and host-side operations. | A specialized numeric experiment, not a drop-in general archive format. Use lossless mode only for archival, retain type/length metadata, and establish a distributable CPU extraction path. Host APIs mean “GPU-only” is too strong a description. [NVIDIA Bitcomp](https://docs.nvidia.com/cuda/nvcomp/bitcomp.html). |

Zstandard's published role comes from [NVIDIA's selection guide](https://docs.nvidia.com/cuda/nvcomp/algorithm_selection.html). Installed API details above can be checked in [zstd.h](../tools/nvcomp-windows-x86_64-5.3.0.16_cuda12-archive/include/nvcomp/zstd.h). Online documentation can advance ahead of this package; never put values into reserved options based on a newer guide. The installed [gdeflate.h](../tools/nvcomp-windows-x86_64-5.3.0.16_cuda12-archive/include/nvcomp/gdeflate.h) exposes algorithm modes, but their stated performance is vendor guidance.

## RTX 4070 is not a Blackwell decompression benchmark

NVIDIA documents its fixed-function Decompression Engine for B200, B300, GB200 and GB300. It accelerates decoding of LZ4, Snappy, Deflate and Gzip. This RTX 4070 does not have that engine, and the requested operation is archive **creation**. Blackwell DE throughput figures therefore cannot predict this application's compression speed; codec probes here use CUDA computation. [NVIDIA DE documentation](https://docs.nvidia.com/cuda/nvcomp/decompression_engine_faq.html).

## Concrete next experiments

1. **Tune the existing interoperable Zstandard backend.** The app now exposes `--chunk-kib` from 64 through 16384 KiB, retaining the 1 MiB default and legacy `--chunk-mib` option. Begin with 64, 128, 256 and 1024 KiB at a fixed 32 MiB batch; then vary batch size on the useful settings. NVIDIA recommends 64 KiB for this low-level compressor; that is a test point, not an established optimum for lttl. Its [C API documentation](https://docs.nvidia.com/cuda/nvcomp/c_api.html) and the installed zstd header describe the recommendation. Compare against CPU using the same independent chunk boundaries as well as ordinary CPU Fast/Balanced. More chunks offer parallel work but reset history more often and add framing/checksum work. Smaller chunks will not repair the related-file history problem by themselves.

2. **Probe codecs before creating new file formats.** Compare Zstd, LZ4, Snappy and representative GDeflate modes on the same immutable inputs and valid codec-specific chunk sizes. Keep output bytes, initialization, allocation, H2D, kernel, D2H and transfer-inclusive host times distinct. Validate every decoded byte. A fast in-memory LZ4 result earns a standard-frame prototype, not an immediate claim that a complete archive is faster. Cascaded/Bitcomp tests should separately label typed numeric inputs and include lossless reconstruction checks.

3. **Reuse and pipeline bounded resources.** Current archives already reuse a session across their batches; the next opportunity is retaining context/workspace across a queue of jobs and avoiding repeated batch allocations where possible. Compare fresh-process and persistent-process jobs directly. After that, test bounded overlap of input staging, H2D, compression, D2H and output writes. Include checksums and final writes in the archive result. Do not manufacture a warm timing by subtracting setup from an unrelated run. See the [implementation proposals and timing cautions](gpu-improvements.md).

For any proposed default change, require repeated complete creation measurements at acceptable output size, independent CPU extraction where interchange is promised, SHA-256 byte checks, bounded memory, cancellation and corruption tests. Report per-dataset results and timing ranges; a kernel-throughput ranking alone cannot choose the default archiver.

## What the RAR evidence contributes

The [static review](native-analysis.md) observed procedure `0x140049b0c` reading old hash-position entries before replacing them in input order (evidence H2: `ev_08f7ddcc68d2b730b3f7e73bad4aac9e8144c81a487e46a6d05e602d885aba5b`). Concurrent atomic exchanges can change the predecessors seen by later positions. This is a reason to redesign and verify ordered matching, not evidence that RAR's whole compressor or its bottleneck has been recovered.

A future independent matcher could group `(hash, position)` pairs, select ordered predecessors and verify actual strings and window bounds. Sorting, memory and verification costs must count toward the result. The existing [matcher proposal](gpu-improvements.md#a-concrete-rar-informed-matcher-experiment) remains research; none of the codec comparisons establishes a GPU RAR encoder.

## Alternative-codec measurements

The [complete archive sweep](gpu-tuning.md) verified useful changes to the working backend: CSV creation took 0.319 s with 64 KiB chunks / 32 MiB batches versus 0.443 s with 1 MiB / 32 MiB, while its archive shrank by 2.47%. On enwik8, increasing the batch to 128 MiB with 1 MiB chunks reduced time from 0.606 s to 0.502 s with identical output size. CPU Balanced still took only 0.259 s on enwik8. These are same-run, transfer- and filesystem-inclusive comparisons; all 63 archives passed integrity checks.

The isolated [codec probe](codec-performance.md) completed 28 configurations across enwik8, CSV, raw sensors, and independent noise, with three measured repetitions each. Every repetition was GPU-decoded and compared outside its compression timer; LZ4 also passed an independent CPU block decoder. The following **warm-context, memory-to-memory** enwik8 figures include host staging and H2D/D2H transfers, but exclude initialization, input/output files, TAR framing, checksums, and verification. They cannot be presented as complete archive times.

| GPU codec, 64 KiB chunks | Median compression + transfers | Original bytes retained |
|---|---:|---:|
| Zstandard | 98.0 ms | 40.17% |
| LZ4 | 70.4 ms | 60.01% |
| GDeflate mode 0, entropy only | 37.9 ms | 63.66% |
| GDeflate mode 1 | 78.0 ms | 47.49% |

LZ4 and GDeflate can trade size for speed on this GPU, but neither provides an unconditional improvement over tuned Zstandard. Zstandard's 1 MiB enwik8 configuration took 254.7 ms in this probe; moving to 64 KiB reduced that interval to 98.0 ms while increasing retained size from 38.60% to 40.17%. On CSV, the same change improved both time (125.3 to 38.5 ms) and retained size (13.41% to 13.07%). These distinct outcomes support content-aware configuration, not a single universal chunk size.

**Implemented:** GPU Zstandard archive creation and the smaller-chunk/larger-batch controls. The subsequent [Auto policy](auto-compression.md) selects CPU/GPU settings from bounded content samples and has its own [measured comparison](auto-performance.md). **Experimental only:** the LZ4/GDeflate probe; lttl does not yet create LZ4/GDeflate archives. **Still unmeasured here:** Snappy, Cascaded, Bitcomp, persistent archive sessions, and pipelined archive I/O. LZ4 is a reasonable next standard-format fast-mode prototype, while specialized numeric codecs require separately described data and decoding metadata.
