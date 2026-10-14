# Independent compression research for lttl

Follow-up: the [portable GPU numeric experiment](../numeric-gpu/README.md) implements and measures this numeric format on Intel Arc and NVIDIA, including real-data comparisons. The original CPU source and measurements below remain unchanged.

This is an isolated research experiment, not a new production archive format. It contains an independently implemented numeric encoder, a content selector, and a standard Zstandard fallback. It does not replace lttl's Auto settings, encryption, or existing decoders. The vendor comparisons belong here rather than in the product documentation.

The practical question is whether specialization can improve both size and speed on some workloads while retaining a strong general-purpose fallback. No lossless compressor can shorten every possible input: there are fewer short bit strings than possible inputs of a given length. Better means a measured improvement for a specified workload, hardware, memory budget, and objective.

See [measurements.md](measurements.md) for measured results and limitations, [results.json](results.json) for raw measurements, [codec.rs](codec.rs) for the prototype, and [benchmark.py](benchmark.py) for the comparison harness.

## What the established algorithms do

### Dictionary matching and parsing

Dictionary compression replaces repeated bytes with a distance and length. The difficult part is choosing which matches to emit, how much history to search, and how to encode the resulting symbols. A simplified parsing recurrence is:

```text
D[i] = min(literal_cost(i) + D[i+1],
           match_cost(length, distance) + D[i+length])
```

Costs roughly correspond to `-log2(probability)`. Real encoders have state: recently used distances, literal contexts, and probability models change after each decision. Therefore a position-only recurrence is an approximation; a bounded search with estimated costs is more practical than an exhaustive search over every adaptive state. Larger search budgets can improve density at substantial CPU and memory cost.

### LZMA / LZMA2 and the 7z container

7z is a container supporting several codecs and filters; it is not one compression algorithm. LZMA combines dictionary matches, repeated-distance coding, contextual literal probabilities, and adaptive range coding. Its public encoder has fast and more expensive parsing paths. LZMA2 adds chunk control, reset behavior, and uncompressed chunks; independent encoder blocks also permit parallel compression. It would be incorrect to describe LZMA2 as inherently limited to one CPU core. [7z format overview](https://www.7-zip.org/7z.html), [public LZMA encoder](https://raw.githubusercontent.com/ip7z/7zip/main/C/LzmaEnc.c), [public LZMA2 encoder](https://raw.githubusercontent.com/ip7z/7zip/main/C/Lzma2Enc.c).

For a binary range-coding decision, the interval is split according to a modeled probability. In simplified notation, `bound = floor(range / P) * p`; the chosen interval becomes the new range and the probability is adjusted toward the observed bit. Renormalization emits bytes. That evolving state is efficient in bits, but successive decisions within one stream depend on earlier decisions. Independent streams provide parallelism at a cost in shared history, model reuse, and framing. [Public LZMA encoder](https://raw.githubusercontent.com/ip7z/7zip/main/C/LzmaEnc.c).

### RAR: public behavior versus unknown encoder details

The public RAR specification documents the container, dictionary sizing, and solid-history behavior. The vendor's release notes describe long-range and more exhaustive search options with speed and memory tradeoffs. These sources do not disclose the complete proprietary compressor's match scoring, parser, or probability models. I therefore do not claim to have recovered its exact encoder or to have mathematically proved an improvement over it. The comparison below uses an installed encoder as a black box. [RAR format specification](https://www.rarlab.com/technote.htm), [official release notes](https://www.rarlab.com/rarnew.htm).

The transferable ideas are large useful history, reversible data transforms, good match selection, and balancing search effort against benefit. Reimplementing a proprietary encoder is unnecessary to test an independent approach to those problems.

### The baseline is already sophisticated

Zstandard combines dictionary sequences with Huffman-coded literals and finite-state entropy coding of sequence information. It is a serious baseline, not merely a placeholder. A custom format must justify its maintenance and compatibility costs with results against existing optimized codecs. [Zstandard format specification](https://www.rfc-editor.org/rfc/rfc8878.html).

## Implemented experiment: strided prediction and tiled packing

The new Rust path treats input as little-endian 16-bit words and tests strides of 1, 2, 4, 8, 16, 32, and 64 words. It does not trust filename extensions. For a candidate stride `s`:

```text
d[i] = (x[i] - x[i-s]) mod 65536
```

The predecessor is zero at the beginning of each independent 1 MiB chunk. Interpret `d` as a signed 16-bit value, then zigzag-map small positive and negative residuals to small unsigned integers:

```text
z = (unsigned(d) << 1) XOR unsigned(signed(d) >> 15)
```

All operations above use 16-bit wrapping semantics. Inverse zigzag followed by wrapping addition recovers every original bit, including numeric wraparound. An odd final byte is stored unchanged.

For each tile of up to 32 residuals, calculate `b = bit_length(OR(z))`. Write one byte specifying `b`, then exactly `n*b` packed bits, rounded up to a byte. For a full tile with 4-bit residuals, this is 17 bytes instead of 64 bytes, before chunk and file framing. There is no entropy coder in this path.

Three 4 KiB samples, near the beginning, middle, and end, estimate each stride's packed size. The numeric path is selected only when its estimate is below 85% of the sample's Zstandard-level-1 size and below 90% of raw size. Otherwise the full input goes through Zstandard level 3. These are provisional fixed thresholds, not trained or universally optimal parameters. Sample-local prediction resets differ from full-chunk resets, and samples can miss heterogeneous regions. Raw fallbacks bound expansion but cannot guarantee the selector always matches or beats Zstandard's ratio.

Chunks are processed in parallel with Rayon. The current packing and reconstruction loops are scalar Rust; there is no explicit SIMD implementation or GPU dispatch in this experiment. `adaptive-1` and `adaptive-8` allow the benefit of chunk parallelism to be measured separately from the transform's compression benefit.

Delta transforms and bit packing are established techniques. The contribution here is an independent implementation and measured lttl-specific selection experiment, not a claim to have invented those mathematical ideas. SIMD integer coding and GPU delta/packing pipelines provide relevant prior art. [Lemire and Boytsov](https://arxiv.org/abs/1209.2137), [NVIDIA cascaded compression](https://docs.nvidia.com/cuda/nvcomp/cascaded.html).

### Experimental framing and validation

`LTEX0001` is a research-only marker. Its 26-byte header contains the mode, numeric stride, original length, and original XXH64 checksum. Modes are stored, Zstandard, or numeric. Numeric chunks contain a bounded length and raw/packed flag. Decoders reject invalid lengths, widths, strides, truncation, trailing bytes, and nonzero padding; output is limited to 512 MiB. Zstandard frames have their own checksum as well.

XXH64 detects accidental corruption; it is not cryptographic authentication. This format does not provide encryption, recipient policy, or adversarial tamper resistance. Any production integration would need explicit format versioning, mature fuzzing, resource limits, and the existing authenticated archive protection.

## Proposed next architecture — not implemented yet

### Preserve history without making every stage serial

Separate the history horizon from the execution tile. Explore groups with 8–64 MiB of useful history and 64–256 KiB coding tiles. Group related content using inexpensive byte statistics and confirmed repetition, then make a bounded, immutable dictionary available to independently processed tiles. Cross-tile dependencies must be explicit and limited; arbitrary references into concurrently decoded tiles would destroy independent decoding.

Use byte-verified matches, including for coarse duplicate chunks. Hash equality alone cannot establish lossless equality. Compare dictionaries, grouping, and checkpoints against solid compression: smaller independent units can lose substantial ratio on text and near-duplicate files. These sizes are experimental search ranges, not established optimum settings.

On CPUs, use parallel hash tables or bounded candidate indexes and SIMD match verification. A bounded cost-aware parser can evaluate several useful matches per position. On GPUs, bulk hash construction, candidate filtering, and match-length checks are promising; serial adaptive parsing is less attractive. A sorted `(hash, position)` index or an immutable index of earlier positions avoids relying on unpredictable atomic insertion order. Every candidate needs distance, bounds, and byte-equality checks.

### Use parallel entropy streams when they earn their overhead

A future independent general-purpose path could combine LZ parsing with several interleaved rANS states. For a symbol with integer frequency `f_s`, cumulative offset `c_s`, and total `M`, the basic state mapping is:

```text
C_s(x) = M * floor(x / f_s) + (x mod f_s) + c_s
r = encoded_state mod M
choose s with c_s <= r < c_s + f_s
x = f_s * floor(encoded_state / M) + r - c_s
```

A real implementation also needs renormalization, bounded state precision, frequency tables, and an unambiguous stream layout. Multiple independent states can map to CPU SIMD lanes or GPU work items. Block-static models reduce serial adaptation but can cost extra bits on context-sensitive data. Interleaving ANS is established research, not a newly invented entropy coder. [Giesen, Interleaved entropy coders](https://arxiv.org/abs/1402.3392).

This is a research direction, not a claim that rANS alone will beat LZMA or that replacing Zstandard's entropy coder is necessary. First isolate the benefit of better transforms and history selection while retaining its mature backend.

### Specialized numeric and structured paths

The prototype uses each tile's largest residual width, so a single outlier can inflate an entire tile. A next experiment should evaluate exception packing: choose a smaller width and store exceptional high bits separately. Minimize the full cost, including width headers, exception masks or positions, and exception payloads. Compare against a simple bit shuffle followed by Zstandard before adding a custom format.

Other reversible candidates include 32/64-bit integer deltas, floating-point XOR prediction on the exact bit patterns, channel deinterleaving, and field-aware transforms for structured records. Floating-point transformations must preserve NaNs, signed zero, and all payload bits. Parsing structured formats requires strict validation and a byte-exact fallback; reconstructing equivalent JSON is not sufficient for lossless archiving.

### Target hardware with different execution strategies

* **Modern CPUs:** explicit AVX2 or NEON packing kernels, scalar fallbacks, bounded allocations, reusable worker pools, and scheduling that accounts for memory bandwidth and heterogeneous cores. Benchmark encode and decode separately; predictor reconstruction can be the slower half.
* **NVIDIA, AMD, and Intel GPUs:** coalesced transforms and packing, prefix scans for offsets, output compaction, persistent sessions, and double/triple buffering. Query subgroup capabilities; portable code must not assume every device has 32-lane warps. Large batches are needed to amortize dispatch and synchronization.
* **Apple silicon:** unified memory can reduce explicit device-copy cost, but synchronization, memory bandwidth, cache behavior, and buffer ownership still matter. No Apple hardware was tested here.

For a discrete GPU without overlap, the approximate comparison is:

```text
T_gpu_total = session_setup + N / upload_bandwidth
            + GPU_compute + C / readback_bandwidth + host_overhead
```

`N` is input bytes and `C` is compressed bytes. With effective pipelining, steady-state throughput approaches the slowest stage rather than the sum; startup, final drain, dependencies, and synchronization remain. Compression ratio matters twice: it changes both the archive size and readback cost. A GPU path should be selected only when measured total time beats the CPU at an acceptable ratio and memory budget. Existing lttl portable-backend measurements already show why kernel-only timing is insufficient; see [prior portable GPU results](../../docs/portable-v3-results.md).

No new GPU kernel, device-transfer measurements, explicit SIMD kernel, or custom LZ/rANS path is included in this prototype. Its operations are suitable candidates for those experiments, not evidence of GPU superiority.

## How to evaluate a production candidate

Use a Pareto comparison of compressed bytes, compression time, decompression time, and peak memory. For one transfer, a useful objective is `compression_time + compressed_bytes / network_bandwidth + decompression_time`; offline backups may weight storage much more heavily. Provide Fast and Dense policy choices instead of claiming one universal best setting.

Before enabling this in Auto:

1. Validate on independent real numeric, text, binary, media, and mixed corpora, including small files and changing data distributions. The current synthetic sensor result is not sufficient.
2. Compare cheap selection against forced transforms and direct Zstandard; measure sampling cost and misclassification. Test alternative widths, endian order, stride changes, and outliers.
3. Run cold and warm end-to-end archive creation and extraction, with peak memory, CPU load, GPU transfers, repeated sessions, and exact ratio recorded. Current measurements stop at recovery of the canonical TAR payload.
4. Fuzz the decoder and framing, test resource exhaustion and malicious inputs, and define forward-compatible format identifiers. Three unit tests and a small benchmark corpus do not establish production readiness.
5. Enable a new backend only for regions where repeatable measurements justify it. Keep a reliable general-purpose fallback and make selection inspectable.

## Reproduce this experiment

Requires this repository's existing local corpus at `bench-output/auto-corpus/manifest.json`, the normal Rust dependencies, and Python with `lzma`. The benchmark verifies corpus SHA-256 values before selecting prefixes. The corpus is not embedded in the research directory.

```powershell
./scripts/cargo.ps1 test --example codec_lab --locked --offline
./scripts/cargo.ps1 build --release --example codec_lab --locked --offline
python research/codec-lab/benchmark.py --repeats 3 --mib 16
python research/codec-lab/summarize.py
```

The installed RAR executable is optional and its path can be supplied with `--rar`. If absent, those two methods are omitted. LZMA2 is tested through Python/liblzma in an XZ container, not through the 7-Zip application. Every codec receives identical canonical TAR bytes. Exact commands, selected paths, binary hashes, and verification results are preserved in the JSON report.
