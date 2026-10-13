# RTX 4070 codec comparison: measured GPU tradeoffs

Measured 2026-10-12. **Smaller Zstandard chunks substantially improved warm GPU compression on text and sensors. LZ4 and GDeflate offered faster alternatives at different compression ratios. None is a universal winner, and these memory-to-memory measurements do not establish faster complete archive creation.**

The isolated [probe source](../native/codec_probe.cpp) and [build/run script](../scripts/benchmark-codecs.ps1) do not change the application or its archive formats. [Complete results and input hashes](codec-probe-results.json) retain all 84 timed repetitions, the 28 first passes, initialization measurements, and verification scope. Original JSON lines are in `bench-output/codec-probe/gpu-codecs-20261012/results.jsonl`.

## Results

Each cell is **median compression milliseconds / percentage of original bytes retained**. Lower is better in both dimensions. The batch size is 32 MiB for every row. All inputs were already loaded in host memory; intervals include host staging, CPU-to-GPU transfers, compression, result transfers, and host packing. They exclude disk I/O, archive construction, extra content checksums, verification, and context creation.

| Codec / chunk size | enwik8, 100,000,000 bytes | CSV, 64 MiB | Raw sensors, 64 MiB | Noise, 64 MiB |
| --- | ---: | ---: | ---: | ---: |
| Zstandard / 64 KiB | 98.02 / 40.166% | 38.53 / 13.073% | 80.94 / 61.133% | 96.65 / 100.017% |
| Zstandard / 256 KiB | 147.32 / 38.926% | 70.18 / 13.305% | 100.28 / 59.076% | 58.57 / 100.008% |
| Zstandard / 1 MiB | 254.68 / 38.604% | 125.28 / 13.410% | 178.27 / 58.662% | 78.14 / 100.006% |
| LZ4 / 64 KiB | 70.41 / 60.007% | 30.15 / 25.194% | 48.39 / 82.611% | 39.56 / 100.394% |
| GDeflate mode 0 / 64 KiB | 37.89 / 63.658% | 25.56 / 61.764% | 27.53 / 77.179% | 30.28 / 100.397% |
| GDeflate mode 1 / 64 KiB | 77.98 / 47.492% | 30.45 / 19.493% | 57.27 / 66.898% | 49.55 / 100.397% |
| GDeflate mode 2 / 64 KiB | 133.36 / 42.582% | 53.96 / 16.905% | 106.62 / 63.297% | 99.93 / 100.397% |

The results show concrete choices:

- **Tune the existing GPU Zstandard path first.** Compared with 1 MiB chunks, 64 KiB was about 2.60 times faster on enwik8, 3.25 times faster on CSV, and 2.20 times faster on sensors. enwik8 retained 40.166% instead of 38.604%; sensors retained 61.133% instead of 58.662%. CSV improved both speed and size. Smaller chunks were not universally faster: noise favored 256 KiB.
- **LZ4 is a credible speed-oriented experiment with a substantial size tradeoff.** At 64 KiB it beat Zstandard's time on all four inputs, but retained 60.007% rather than 40.166% of enwik8 and 25.194% rather than 13.073% of CSV. Its standard block format still needs container framing and metadata before becoming a usable archive format.
- **GDeflate mode 0 prioritizes speed at the expense of compression.** It was fastest in this matrix, but CSV retained 61.764%, compared with 13.073% for Zstandard at 64 KiB. Mode 1 offered a more balanced alternative: 30.45 ms and 19.493% on CSV. Mode 2 was both slower and larger than Zstandard at 64 KiB for all four inputs here.
- **Avoid spending codec effort on proven incompressible content.** Every tested codec expanded the noise input. A stored path deserves priority over choosing the least expensive compressor for that workload. These tests do not establish a perfect classifier.

These are measured tradeoffs among GPU configurations. For comparison against CPU and complete archive creation, use the separate [application GPU report](gpu-performance.md) and [downloaded-sample report](downloaded-performance.md); their larger measurement boundaries must not be replaced with these codec-only values.

## Initialization and verification

Hardware was the **NVIDIA GeForce RTX 4070 Laptop GPU**, driver **592.82**, with 8188 MiB VRAM reported earlier. The probe used the verified nvCOMP **5.3.0.16 CUDA 12** package and CUDA runtime **12.9.79**. The runtime reported API version 12090; the installed driver reported CUDA driver API version 13010. This does not mean the probe was built against CUDA 13.

The first CUDA API initialization inside the probe process took **677.60 ms**. Individual codec-context allocations took **22.20–40.59 ms** and include buffers used only for benchmark verification. Those times were recorded separately. DLL load before `main`, machine-wide driver caches, thermal state, and GPU power state were not controlled; this is not a cold-machine startup experiment.

For the first enwik8 Zstandard/64 KiB pass, compression summed to **123.31 ms**, compared with the subsequent **98.02 ms** median. Its first batch alone took **44.24 ms**, including a **30.12 ms** compression event span. The JSON records the complete first pass, first batch, and first compression-event span separately, so first-use costs are not silently folded into the warm result or inferred by subtracting unrelated medians.

All **28 configurations** passed a complete warmup and **three measured repetitions**. Every batch from every pass was GPU-decompressed, its reported size checked, and its bytes compared with the original outside the compression timer: **112 complete corpus round trips** in total. LZ4 additionally passed **16 independent CPU corpus round trips** through a small bounded scalar block decoder written from the public LZ4 block specification. That verifier is experimental validation code, not a production extraction library. GDeflate and Zstandard results in this particular probe use GPU self-roundtrip validation; CPU interoperability checks for the application's Zstandard path remain separate evidence.

The runs used fixed sequential configuration order in an exclusive compression window, with no other agent builds or GPU benchmarks overlapping. OS/desktop activity was not excluded. Each repeated wall time sums independently measured batch intervals; verification is interleaved between batches outside those intervals. Three repetitions demonstrate repeatability here, not a broad statistical guarantee. The JSON retains minimum/maximum information through all original per-run values.

## What the formats and APIs allow

NVIDIA describes GDeflate as a rearranged Deflate bitstream that exposes 32 parallel substreams for GPU decoding. **It is a different stream format from ordinary Deflate**, so putting it into a ZIP Deflate entry would be incorrect. This experiment creates no such archive. [NVIDIA GDeflate description](https://docs.nvidia.com/cuda/nvcomp/gdeflate.html)

The shipped `nvcomp/gdeflate.h` options define mode 0 as entropy-only compression, mode 1 as the default high-throughput setting, and mode 2 as a medium compression setting. Modes 4 and 5 also exist but were outside this matrix; mode 3 is described as a placeholder mapping to the medium setting. The probe uses 64 KiB chunks, following the headers' caution about cost and scratch space at larger sizes. Zstandard's shipped compression options contain reserved bytes, with no CPU-style level parameter. LZ4 here uses bytes with bitshuffle disabled; its output is raw LZ4 blocks, not automatically an LZ4 frame file. [LZ4 block specification](https://github.com/lz4/lz4/blob/dev/doc/lz4_Block_format.md)

NVIDIA's selection guide favors different codecs for different data and distinguishes general byte streams from typed numeric data. Cascaded or Bitcomp might warrant a later structured-data experiment, but their specialized requirements and outputs should not be assumed useful for arbitrary desktop folders. Hardware decompression-engine features described for Blackwell datacenter GPUs do not establish support on this RTX 4070. [NVIDIA algorithm selection](https://docs.nvidia.com/cuda/nvcomp/algorithm_selection.html)

The probe uses the low-level batched compression/decompression APIs, querying per-codec alignment, output bounds, and scratch space. Allocations are reused. Only actual compressed lengths are transferred back, with pointer, size, and status traffic counted separately. GPU decoding is attempted only on the probe's own freshly compressed valid buffers; this does not establish malformed-input safety. No concurrent streams or device-side compaction kernels were tested.

`compression_stage_ms` is a CUDA event interval around status initialization and codec submission; it can include scheduling or host-launch gaps and **is not isolated kernel execution time**. Compression wall time already includes all event intervals, so they must not be added to it again. Payload sizes omit an archive container and added content checksums; LZ4/GDeflate also need out-of-band block boundaries/sizes, whose eventual storage overhead is not included. Peak RAM/VRAM and power were not measured.

## Reproduce

Build without executing GPU work:

```powershell
.\scripts\benchmark-codecs.ps1
```

Run during an exclusive GPU timing window:

```powershell
.\scripts\benchmark-codecs.ps1 -Run -BatchMiB 32 -Repeats 3 -InputPaths @(
  'bench-output/downloaded-samples/corpus/enwik8/enwik8',
  'bench-output/gpu-review/20261012T212532860Z/corpus/structured-csv/events.csv',
  'bench-output/gpu-review/20261012T212532860Z/corpus/raw-sensors/sensor-u16le.raw',
  'bench-output/gpu-review/20261012T212532860Z/corpus/independent-noise/noise.bin'
)
```

The script accepts batches up to 128 MiB, but this report measures **32 MiB only**. It reads existing inputs and records SHA-256 values; it does not generate new corpora or install system software. The measured executable SHA-256 is `150478d0592a1703ca477b2096c36b6800db6e47500ba591cff7e51819e83b52`.
