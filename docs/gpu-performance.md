# lttl: measured RTX 4070 CPU/GPU comparison

Measured 2026-10-12. **GPU compression works and its archives round-trip through the CPU decoder, but CPU Fast and Balanced were faster on every baseline dataset in this run.** The GPU offered a narrow speed/size tradeoff on synthetic CSV versus CPU Small. Chunked compression substantially hurt some repetitive inputs. These findings support an optional GPU backend, not making it the default.

## Evidence and setup

The retained run is `bench-output/gpu-review/20261012T212845059Z`. [Raw results](../research/legacy/gpu-benchmark-results.json) include every invocation, source SHA-256, original and archive sizes, wall/engine timings, CPU time and GPU counters. The [reproducible harness](../scripts/benchmark-gpu.ps1) and [corpus/design notes](gpu-improvements.md) describe input generation.

- GPU: **NVIDIA GeForce RTX 4070 Laptop GPU**, driver **592.82**, `nvidia-smi` reports **8188 MiB** VRAM. This is a laptop GPU, not an assumed desktop RTX 4070 configuration.
- CPU: 22 logical processors reported; earlier registry inspection identified Intel Core Ultra 9 185H. CPU variants requested eight workers.
- Executable SHA-256: `48A9C795543CFBE4E9BA64D422379C2D59074033DDBA4F009C1FE33EC2F364D0`. GPU bridge/runtime DLL hashes are retained with the results.
- Three repetitions per configuration, sequential with rotated order. The parent task paused other build/compression work during this matrix. Background OS/graphics work, cache, thermals and power state were not controlled.
- All **147 creations and integrity checks passed**. **31 CPU extraction round trips** produced **493 matching file SHA-256 values**, including each backend and all chunk sizes tested. Hashing/verification were excluded from archive-creation timing.

The baseline covers seven exactly 64 MiB synthetic datasets, 4,257,184 bytes of installed executable test data, and a 172,785-byte real project-source snapshot. Synthetic data is deliberately chosen to exercise properties, not sampled to represent the user's typical workload. No GPU power, peak VRAM, or usable peak RAM measurement was obtained; null memory fields mean unavailable, not zero.

Every invocation is a fresh process and creates a new GPU context. DLL/driver/JIT caches may already be warm, so these are **fresh-context runs, not a controlled cold-machine test**. Reported external wall time includes startup, reading, processing, standard checksum work, output writes, publication, and teardown. Kernel-only throughput is not substituted for this total.

## Archive creation and output size

Each cell is **median seconds / archive percentage of original file bytes**. Smaller is better in both dimensions. Percentages include container overhead. GPU uses nvCOMP's fixed settings; CPU Fast/Balanced/Small use Zstandard levels 1/3/9 and are not equivalent quality settings. CPU-chunked Fast and GPU use 1 MiB independent frames and 32 MiB batches; streaming CPU retains different history.

| Input | CPU Fast | CPU Balanced | CPU Small | CPU-chunked Fast | GPU fixed |
|---|---:|---:|---:|---:|---:|
| independent-noise | 0.211 / 100.004% | 0.227 / 100.004% | 0.240 / 100.004% | 0.226 / 100.005% | 0.466 / 100.006% |
| installed-binaries | 0.105 / 60.221% | 0.124 / 53.874% | 0.171 / 47.273% | 0.115 / 60.360% | 0.329 / 56.134% |
| precompressed-zip | 0.206 / 99.512% | 0.219 / 99.817% | 0.260 / 99.501% | 0.229 / 99.513% | 0.489 / 99.403% |
| project-sources | 0.112 / 28.969% | 0.107 / 27.307% | 0.118 / 25.822% | 0.110 / 28.969% | 0.250 / 27.943% |
| raw-rgb | 0.208 / 96.493% | 0.256 / 61.746% | 1.123 / 44.912% | 0.240 / 96.854% | 0.612 / 69.367% |
| raw-sensors | 0.202 / 71.364% | 0.261 / 53.522% | 1.187 / 42.859% | 0.243 / 71.539% | 0.597 / 58.671% |
| repeating-binary | 0.139 / 0.107% | 0.149 / 0.201% | 0.163 / 0.107% | 0.155 / 6.263% | 0.329 / 12.134% |
| similar-files | 0.260 / 100.052% | 0.204 / 9.998% | 0.196 / 1.674% | 0.260 / 100.053% | 0.511 / 100.015% |
| structured-csv | 0.150 / 14.842% | 0.180 / 16.571% | 0.520 / 13.684% | 0.189 / 14.777% | 0.484 / 13.410% |

Three observations matter most:

1. **CSV is a narrow favorable GPU case against the densest CPU preset.** GPU retained 13.410% in 0.484 s versus CPU Small's 13.684% in 0.520 s. GPU's range was 0.452–0.523 s; CPU Small's was 0.507–0.568 s. The overlapping ranges mean the approximately 7% median timing difference is tentative. CPU Fast was substantially faster at 0.150 s with a larger 14.842% result.
2. **Already compressed and random data do not justify expensive GPU work here.** GPU took 0.489/0.466 s for precompressed ZIP/noise versus CPU Fast's 0.206/0.211 s. Savings on the precompressed ZIP were under 0.6%, and random data expanded slightly. Routing such data to low-effort or stored output is more promising than increasing encoder work.
3. **History/search choices can overwhelm hardware throughput.** Similar files compressed to 1.674% with CPU Small, while GPU and CPU Fast retained about 100%. Repeated 64 KiB data retained 0.107% with streaming CPU Fast, 6.263% with CPU-chunked Fast and 12.134% with GPU. The matched-chunk comparison demonstrates that framing is not the only difference: encoder search/coding behavior matters too.

On raw RGB/sensor patterns, GPU is faster than CPU Small but produces materially larger archives. CPU Balanced is both faster and smaller than GPU on those patterns. On actual binaries and the small source snapshot, GPU setup/work overhead outweighs any useful latency gain.

## GPU time components

All component values below are **medians in milliseconds**, aggregated across batches for each run. They are diagnostic intervals, not additive independent measurements.

| Input | Init | H2D | Compression | D2H | Host call | Checksum | Process |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| independent-noise | 95.3 | 5.7 | 92.5 | 5.9 | 120.8 | 20.4 | 465.7 |
| installed-binaries | 98.6 | 0.4 | 97.0 | 0.3 | 98.7 | 1.0 | 329.0 |
| precompressed-zip | 102.3 | 5.7 | 96.6 | 6.8 | 125.1 | 22.5 | 489.1 |
| project-sources | 90.3 | 0.1 | 21.9 | 0.1 | 22.2 | 0.1 | 250.0 |
| raw-rgb | 114.3 | 11.3 | 236.5 | 5.9 | 262.2 | 16.0 | 611.9 |
| raw-sensors | 114.7 | 11.3 | 242.0 | 5.0 | 270.5 | 13.6 | 596.6 |
| repeating-binary | 99.9 | 5.7 | 24.2 | 1.2 | 42.9 | 6.8 | 329.2 |
| similar-files | 99.1 | 5.7 | 95.7 | 5.9 | 124.1 | 19.7 | 511.2 |
| structured-csv | 107.8 | 11.4 | 171.4 | 1.5 | 189.5 | 7.2 | 483.8 |

- **Init:** loading the bridge/runtime and creating device buffers, pinned buffers, stream, events and nvCOMP workspace, as measured by `Session::new`.
- **H2D:** CUDA events around input and pointer/size metadata copies to the device.
- **Compression:** CUDA events around status/size clearing and nvCOMP compression, not a complete application interval.
- **D2H:** event intervals for result metadata and actual compressed-frame transfers. Transferred byte counters include metadata, so they differ from payload/archive size.
- **Host call:** Rust wall time around output allocation and the native compression call, including host staging, submission and waits; it already contains the device intervals.
- **Checksum:** CPU wall time validating frame structure and adding/verifying standard Zstandard content checksums over original data; it is outside the host-call timer.
- **Process:** external complete creation process, including remaining read/TAR construction, writes/syncs, publication and teardown.

Taking the median of each interval independently also prevents treating a row as a precise partition of one run. Transfers were often smaller than compression and initialization costs, so “PCIe is the bottleneck” is not established by these numbers. Nor can a persistent-process result be claimed by subtracting initialization from total latency. A warm-context application must be measured directly.

## Larger chunks: measured related-file sweep

The same 32 MiB batch was retained while independent frame size changed. Each row has three repetitions. CPU streaming Small remains a useful reference: 0.196 s and 1.674% of the original size.

| Backend | Chunk MiB | Median seconds | Range seconds | Original size retained |
|---|---:|---:|---:|---:|
| cpu-chunked | 1 | 0.260 | 0.254–0.273 | 100.0532% |
| cpu-chunked | 4 | 0.276 | 0.257–0.280 | 100.0523% |
| cpu-chunked | 16 | 0.288 | 0.276–0.289 | 100.0520% |
| gpu | 1 | 0.511 | 0.496–0.548 | 100.0151% |
| gpu | 4 | 0.491 | 0.462–0.508 | 100.0142% |
| gpu | 16 | 0.529 | 0.523–0.548 | 100.0139% |

Increasing the chunk size from 1 to 4 or 16 MiB did **not** recover meaningful compression on this corpus with either CPU Fast or fixed nvCOMP settings. Larger frames alone do not guarantee that the codec's search window/strategy can exploit the relevant repeats. The detailed internal cause was not profiled, so this result does not establish a specific nvCOMP window limit. It does argue against advertising larger chunks as a general compression fix.

## What to change next

Keep CPU as the default and make GPU a selectable, accurately labeled option. The strongest current opportunities are content-aware effort selection, preserving useful history, and avoiding unnecessary copies/allocations. A persistent GPU session, pipelined batches and output packing are candidates for further measurement, not demonstrated speedups.

These results come from lttl’s independent Zstandard encoders and apply to the recorded configurations and input corpus. See [practical improvement analysis](gpu-improvements.md) for the evidence-linked priorities and remaining limits.

Downloaded real-world sample creation **and unpacking** measurements are complete in the [Canterbury/enwik8 report](downloaded-performance.md). CPU Balanced was faster and smaller than GPU on both collections. These separate timings preserve their corpus and measurement distinctions.
