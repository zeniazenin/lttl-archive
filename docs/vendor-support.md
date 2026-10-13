# AMD, Intel and Apple silicon feasibility

**Implementation update:** the [portable LZ4 backend](portable-gpu.md) now executes on both local Intel Arc and RTX 4070 GPUs. The assessment below is the earlier feasibility snapshot; AMD and Apple hardware remain untested.

Reviewed 2026-10-12 against the current lttl source and primary vendor/project documentation. This is an implementation assessment; no additional GPU encoder or macOS build was added or benchmarked in this pass.

All three are feasible targets. CPU portability and GPU compression are separate tasks. The current Windows CPU engine does not require an NVIDIA device; its Intel CPU path is already measured. AMD x64 should use the same CPU codecs, but no AMD hardware run has been performed. Apple silicon needs an ARM64 macOS build and platform validation. The existing GPU encoder remains NVIDIA-only.

## Existing implementation and local hardware

`src/gpu.rs` loads an absolute Windows `gpu/lttl_gpu.dll` path and treats GPU sessions as CUDA/nvCOMP sessions. `native/gpu_bridge.cpp` calls the binary nvCOMP Zstandard encoder, CUDA allocations, transfers and events. The non-Windows implementation explicitly reports GPU unavailable. Porting this wrapper to HIP, SYCL or Metal would not supply the missing encoder kernels.

`pnputil /enum-devices /class Display /connected` reports both **Intel Arc Graphics** and **NVIDIA GeForce RTX 4070 Laptop GPU**, with status Started, on this machine. Intel PCI vendor/device is `8086:7D55`; NVIDIA is `10DE:2820`. This establishes an available Intel test target, not successful execution of a compression kernel. CIM inventory was unavailable in the current environment; PnP enumeration succeeded. No AMD or Apple hardware was tested.

The Rust archive layer can retain input validation, cancellation, staging/publication and CPU extraction. The main portability changes are GPU runtime/session selection, capability reporting, metrics, build packaging and platform integration. The GUI presently labels its GPU option NVIDIA. Explorer registration is Windows-specific; a Mac release would need separate Finder integration. macOS also needs tests for case sensitivity, Unicode normalization, filesystem metadata, dialogs and archive interoperability.

## Vendor assessment

| Target | Practical GPU route | Main qualification |
|---|---|---|
| AMD Radeon / supported ROCm devices | Portable compute encoder, or experimental hipCOMP LZ4 through a native HIP bridge | hipCOMP currently lacks Zstandard compression; exact GPU/OS support must be validated |
| Intel Arc / supported integrated GPUs | Portable compute encoder through D3D12/Vulkan, or custom SYCL kernels | The current CUDA encoder cannot execute on Arc; GPU compute tooling does not itself provide an equivalent codec |
| Apple M-series | ARM64 macOS build plus Metal compute, directly or through wgpu | Requires Mac hardware testing and new kernels; current DLL loader is Windows-only |

### AMD

AMD's **hipCOMP 2.3.0** is an early-access GPU compression project. Its documented encoder set includes LZ4, Snappy, Cascaded and RLE; Zstandard and Deflate/GZIP are decompression-only. AMD explicitly describes the GPU implementations as experimental and not performance-optimized. The documented tested architectures include gfx1030, gfx1100 and several Instinct families. This makes LZ4 a credible prototype, but not a drop-in replacement for lttl's nvCOMP Zstandard encoder. The reviewed build instructions do not establish a tested Windows lttl configuration. [hipCOMP source and support table](https://github.com/ROCm/hipCOMP-core/)

HIP itself supports Linux and Windows, with platform/API differences and hardware requirements; support for HIP does not establish support for every hipCOMP combination. [AMD HIP FAQ](https://rocmdocs.amd.com/projects/HIP/en/latest/faq.html)

AOCL-Compression is a separate **CPU** optimization option for AMD Zen processors. It should be benchmarked as a CPU candidate rather than reported as GPU acceleration. [AMD AOCL documentation](https://docs.amd.com/r/en-US/68552-AOCL-api-guide/AOCL-Compression)

### Intel

Intel's oneAPI compiler supports GPU-oriented SYCL development. A custom compression implementation could use that route; the reviewed toolkit documentation does not establish a ready replacement for our nvCOMP Zstandard encoder. A common compute backend would avoid introducing a second native toolchain solely for Arc. [Intel oneAPI toolkit](https://www.intel.com/content/www/us/en/developer/tools/oneapi/oneapi-toolkit.html)

Intel QPL targets Intel IAA and includes a software CPU path. IAA is distinct from the Intel Arc GPU in this laptop. QATzip similarly targets QuickAssist accelerators; neither should be advertised as Arc compression. [QPL](https://github.com/intel/qpl/), [QATzip](https://github.com/intel/QATzip)

DirectStorage's GPU decompression support is relevant to a future extraction experiment, but its read/decompression pipeline does not provide the archive-creation backend sought here. [Microsoft DirectStorage](https://github.com/microsoft/DirectStorage)

### Apple silicon

The Rust CPU archive engine is the initial port target. The current GUI framework supports macOS, but that does not demonstrate that this app's dependencies, packaging and filesystem policies have passed Mac tests. [eframe platform documentation](https://docs.rs/eframe/latest/eframe/)

Metal shared resources let CPU and GPU access system memory, with synchronization requirements. This creates an opportunity to reduce explicit discrete-GPU transfer work, but shared bandwidth, allocations, dispatch and synchronization still cost time. Treat an Apple speed advantage as a hypothesis until a full archive benchmark verifies it. [Apple shared storage mode](https://developer.apple.com/documentation/metal/mtlstoragemode/shared)

Apple's Compression framework exposes several algorithms, including LZ4 and LZFSE. Its algorithm list does not establish a Metal Zstandard encoder that can replace our CUDA path. These APIs are optional platform-codec candidates, not evidence that archive work is occurring on the GPU. [Apple compression algorithms](https://developer.apple.com/documentation/Compression/compression_algorithm)

## Recommended implementation sequence

1. Separate execution provider from codec/container. Keep NVIDIA nvCOMP Zstd; add explicit provider capability records for codec, encoding/decoding, hardware identity, OS and verified status. Discovery must not label a present GPU as a working compressor.
2. Prototype a Rust **wgpu** compute backend on the locally available Intel Arc and RTX 4070. wgpu provides native D3D12, Vulkan and Metal backends; compression kernels still have to be implemented. This is the most direct route to sharing kernel logic across the requested vendors. [wgpu project](https://github.com/gfx-rs/wgpu)
3. Use a bounded block-based LZ4 prototype to establish correctness and end-to-end economics before committing to a full GPU Zstandard encoder. For archive use it needs standard LZ4 framing, checksums, incompressible-block handling, CPU decoding and format detection, for example an explicit TAR.LZ4 option. LZ4 payloads cannot be placed in a TAR.ZST file. [LZ4 frame specification](https://github.com/lz4/lz4/blob/dev/doc/lz4_Frame_format.md)
4. Preserve Zstandard as the ratio-focused path. Our existing [NVIDIA codec probe](codec-performance.md) showed LZ4 speed/size tradeoffs, not a universal improvement. A more ambitious standard-Zstd route is GPU match/sequence generation followed by CPU entropy coding through libzstd's sequence APIs. The locally vendored Zstd 1.5.7 header exposes `ZSTD_compressSequences` and `ZSTD_registerSequenceProducer`; this would require version-pinned advanced bindings, sequence validation and its own matcher. It is an unimplemented design proposal, with no promised speedup.
5. Validate the same portable kernel on an AMD machine and an Apple silicon Mac. Add vendor-specific HIP/Metal optimizations only where complete-job measurements justify the maintenance cost. hipCOMP can be an AMD-specific comparison candidate.

Keep Fast's zero-sampling CPU path until a measured size threshold and specific device/codec provide a repeatable end-to-end gain. For Auto, learn or select per-device settings rather than copying RTX thresholds to every GPU. Cache capability checks and compiled kernels, reuse bounded sessions safely, and record unavailable providers separately from failed jobs.

## Acceptance measurements

Use the retained text, JSON/CSV, sensor, related-file, precompressed and noise corpus. Compare CPU, existing NVIDIA, and each new provider with the same goal and input hashes. Include cold pipeline/context compilation, warm persistent sessions, staging/shared-memory synchronization, input/output, checksums and transfer time. Retain separate timing modes instead of comparing a warm GPU kernel to a full CPU process. On shared-memory GPUs, report the actual copies/synchronizations performed; zero discrete PCIe transfer is not zero overhead.

Every claimed provider must pass CPU extraction with exact SHA256 equality, malformed-output and corruption checks, partial chunks, incompressible expansion handling, cancellation, unavailable-device fallback and independent decoder interoperability. Run on physical AMD/Intel/Apple hardware before marking that platform supported. GPU enumeration and successful compilation alone are insufficient.

The next practical experiment is the portable backend on the **Intel Arc already present here**. AMD and Apple feasibility is supported by their compute platforms, but performance remains unmeasured and neither is enabled by this assessment.
