# lttl compression: layout detection and optional dense compression

Follow-up: [Dense CPU optimization](../dense-opt/README.md) reduces implementation overhead with identical compressed bytes and compares all Rust controls in one executable. This directory preserves the original v2 implementation and measurements.

The algorithm is named **lttl**, matching the app and its `.lttl` archive extension. Its two presets are **lttl Fast** and **lttl Dense**. This research implementation is version 2 of the numeric prototype; paths and raw benchmark configuration IDs retain `numeric-v2`, `v2-fast` and `v2-dense` for reproducibility. Naming the codec does not make its experimental payload frame interchangeable with the production `.lttl` archive container.

This research prototype extends the original numeric codec with little- and big-endian 16/32/64-bit words, packed 12-bit WFDB-212 samples, non-power-of-two channel strides, and an optional Zstandard stage. It is independently implemented prediction and packing combined with an existing entropy compressor, not a claim to have invented these mathematical techniques.

The [measurements](measurements.md) compare Fast and Dense with the frozen v1 numeric encoder, its adaptive fallback, Zstandard 3, actual RAR method 5 and actual 7-Zip normal/ultra and Delta + LZMA2. [Raw results](results.json) retain each command, hash, detected layout, sampling time, codec time, creation time and extraction time. [Charts](tradeoffs.svg) show both size and time.

This is an executable prototype under `examples/numeric_v2.rs`, not the Windows application's production compression engine. The production archive format, GPU kernels, Auto settings, identity controls and installer are unchanged. Earlier sources, executable hashes and reports remain preserved. V2 compression and decompression run on the CPU; no v2 GPU speedup is claimed.

## Numeric representations

Supported candidate layouts:

* 16-, 32- and 64-bit words in either byte order.
* Packed WFDB-212: two 12-bit words in three bytes, reconstructed byte-for-byte, including final incomplete groups. The [public format specification](https://archive.physionet.org/physiotools/wag/signal-5.htm) describes the low bytes and shared high-nibble byte. No filename, medical interpretation or external header is required by the encoder.
* Interleaved channel strides 1, 2, 3, 4, 6, 8, 12, 16, 24, 32 and 64, counted in decoded words.
* Modular-difference or XOR prediction. XOR can represent floating-point bit patterns without floating-point arithmetic; NaN payloads, infinities and signed zero are preserved. Selecting a word width is a compression hypothesis, not a semantic assertion about a file's contents.

For word width `w`, stride `s` and value `x[i]`, the integer residual is `d[i] = (x[i] - x[i-s]) mod 2^w`. Values before the first complete stride use a zero predictor. Zigzag folding maps small positive and negative differences to small unsigned values. XOR uses `x[i] XOR x[i-s]` instead. Both operations invert exactly, including wraparound.

Tiles contain up to 32 residuals and store a width byte followed by tightly packed values. Dense additionally compares Zstandard level 3 over these packed tiles with Zstandard over residual byte planes. Byte planes group each residual's low byte, then its next byte, and so on, making predictable high bytes easy to compress. Zstandard supplies established dictionary/entropy coding; see its [project and format reference](https://facebook.github.io/zstd/).

## lttl Fast and lttl Dense selection

Both presets inspect three interior regions of at most 8 KiB each. Offsets are aligned to 24 bytes, preserving alignment for all candidate word widths and packed pairs. Interior positions reduce the influence of file headers and trailers. The encoder scores tile widths for all 143 candidate layouts in parallel; it does not compress the entire file under every candidate. Stable candidate ordering makes selection deterministic regardless of worker scheduling.

Fast tries the best estimated packed layout, with ordinary Zstandard 3 or stored data as alternatives. It favors packing when estimated output is under 65% of the sample input and within 25% of ordinary Zstandard's sampled size. This explicitly trades some sampled density for cheaper encoding. Otherwise a numeric candidate needs a 3% advantage over the ordinary sample baseline.

Dense evaluates the four lowest packed-size candidates with both Zstandard-over-planes and Zstandard-over-packed-tiles. Each replacement needs a 3% sampled size advantage. This is a bounded heuristic, not exhaustive optimization and not a proof that the chosen layout is globally best. Sampling and all transformations are included in benchmark timings.

Numeric processing uses independent 1,048,560-byte chunks (divisible by 3 and 8), eight Rayon workers and local prediction resets. Specialized 16-bit/packed-12 paths read directly from input rather than expanding every value into a 64-bit array. Tests compare these optimized paths with the generic reference. Each chunk may be stored raw if its selected compressed representation expands. Ordinary Zstandard fallback retains a file-wide stream and eight requested workers so text retains the original fallback's dictionary scope. Incompressible samples may select immediate storage. Total output is capped at original bytes plus the 29-byte frame header through a whole-file stored fallback.

V2 uses a new `LTNV0002` experimental frame. Metadata records the exact chosen layout, word order, predictor and chunk stage; decoding never redetects these. The frame carries original length and XXH64. These detect accidental corruption, not malicious forgery. The decoder validates modes, widths, lengths, padding, frame boundaries, bounded Zstandard expansion and final content checksum. Input is limited to 512 MiB. Numeric scratch buffers are per active chunk, not full-file arrays of expanded words.

### Experimental frame layout

All integer header fields are little endian regardless of the input's byte order.

| Offset | Bytes | Meaning |
|---:|---:|---|
| 0 | 8 | `LTNV0002` |
| 8 | 8 | Original byte length, at most 512 MiB |
| 16 | 8 | XXH64 of original bytes, seed zero |
| 24 | 1 | Word-width code: 2/4/8 bytes, or 3 for packed-212 pairs |
| 25 | 1 | Endian flag: 0 little, 1 big; packed-212 requires 0 |
| 26 | 1 | Predictor: 0 modular difference plus zigzag, 1 XOR |
| 27 | 1 | Word stride from the supported list |
| 28 | 1 | Frame payload: 0 chunk sequence, 1 original bytes, 2 one ordinary Zstd frame |

Each numeric chunk starts with a one-byte stage and a four-byte compressed length. Stages are 0 stored, 1 packed tiles, 2 Zstd residual byte planes, 3 ordinary Zstd block (accepted by the decoder), and 4 Zstd packed tiles. Decoded chunk length is implied by the original size and fixed chunk size. Incomplete final words or packed pairs follow the transformed complete words unchanged. There is no alignment padding or implicit trailing frame. The current ordinary fallback writer uses a single frame-wide Zstd stream rather than stage-3 chunks.

## Fair comparison

The [manifest](corpus.json) retains all 11 earlier inputs at identical byte lengths and SHA-256 values. These include original packed ECG, native EDF EEG, 12-channel ECG, native big-endian terrain, separately labeled converted ECG/terrain, synthetic sensors, noise and text. Attribution and download/conversion evidence remain in the [original corpus documentation](../numeric-gpu/README.md#real-data-and-attribution).

Three additional 2 MiB synthetic controls exercise 12-channel big-endian 32-bit counters, three-channel little-endian 64-bit counters above the exact integer range of binary64, and big-endian binary64 with exceptional bit patterns. These validate broader layouts, but are not real scientific or financial data. Their deterministic generator is [prepare_corpus.py](prepare_corpus.py). No new download, quantization or duplicated padding is used. Unlike the separately converted reference inputs, the original packed ECG and big-endian terrain are transformed inside the timed codec operation.

Nine configurations receive every complete input in a seeded randomized serial order, three repetitions each. RAR 7.20.3 uses `-m5 -md32m -mt8`; 7-Zip 26.04 x64 uses LZMA2 normal/ultra with 32/64 MiB requested dictionaries and eight requested threads. These match the preceding comparison. Eight requested workers does not guarantee all cores are used; peak memory was not measured.

The filtered competitor uses actual 7z byte Delta followed by LZMA2 normal. It receives the new Fast selector's byte distance, including distance 24 for the 12-channel ECG and distance 3 for packed ECG. Detection is preconfigured outside 7z's timed interval, favoring the competitor. This is stronger than retaining the old selector's unsuitable distances. It remains a bytewise transform and is not equivalent to 12/16/32/64-bit modular prediction. Distances are bounded to 1–256, matching the [official filter's accepted range](https://github.com/ip7z/7zip/blob/main/CPP/7zip/Compress/DeltaFilter.cpp). All used distances and archive method listings are recorded. No exhaustive per-file RAR/7z tuning is performed.

Main timings begin before starting each executable and end after exit plus a common explicit output `fsync`. Both creation and extraction include process launch, file reading/writing and durability flush. For Rust workers, separate in-process codec timings include selection, transforms, checksums and compression but exclude file IO and thread-pool startup. These expose algorithm costs that process startup could hide. V1 and v2 use different executables, with different launch overhead; a full-process improvement alone does not establish faster numeric math. Do not compare these internal times directly with RAR/7z full-process times.

Every restored file is SHA-256 verified outside timing. RAR/7z include a member name and metadata; the experimental streams hold only the byte payload. Full output bytes are counted for all methods. This is not a multi-file solid-archive, encryption or GUI benchmark. The machine is a Core Ultra 9 185H; GPU devices are unused in this experiment. OS caches, CPU placement, temperatures, power and background work are uncontrolled. Report medians and observed min/max, not statistical significance from three runs.

## Validation and reproduction

Five test groups cover all supported widths/orders/predictors/strides; odd tails and incomplete packed triples; all residual widths through 64 bits; wrapping and known WFDB-212 bytes; floating-point exceptional bit patterns; every frame stage, mixed raw/compressed chunks and chunk boundaries; truncated/corrupt streams; invalid flags; excessive decoded sizes; bounded Zstandard expansion; trailing frames/data; and 1,024 deterministic mutated streams.

Use the existing pinned toolchain, corpus and portable archive tools from the [earlier comparison](../codec-comparison/README.md):

```powershell
python research/numeric-v2/prepare_corpus.py
./scripts/cargo.ps1 test --release --example numeric_v2 --locked --offline
./scripts/cargo.ps1 build --release --example numeric_v2 --locked --offline
python research/numeric-v2/benchmark.py
python research/numeric-v2/summarize.py
python research/numeric-v2/plot.py
```

An individual round trip:

```powershell
./target/release/examples/numeric_v2.exe encode dense INPUT.bin OUTPUT.ltnv
./target/release/examples/numeric_v2.exe decode dense OUTPUT.ltnv RESTORED.bin
```

Use `fast` for the cheaper preset. Both presets use the same decoder and the stored metadata determines decoding. Output files must not already exist. The benchmark resumes only when sources, binaries and corpus hashes match; preserve `results.json` under a different name to start a new experiment after edits. `pilot.py` is a development sweep, not the final randomized comparison.

## Remaining limits

* Arbitrary record schemas, unaligned prefixes, heterogeneous fields, mixed numeric/text regions and channel counts outside the list can defeat sample selection. Correctness does not depend on recognizing the layout, but compression quality does.
* No lossy mode, scientific file-container parser, GPU dense stage, production integration or encrypted archive support is added by this prototype.
* Dense compression consumes more CPU than packing alone on some inputs. Compare its measured cost with v1 and Fast, not just with much slower high-ratio archive presets.
* Chunk boundaries and slightly different selection can make Fast a little larger than v1 even on a supported original layout. Dense is optional so this tradeoff is visible.
* Results on the corpus used during development are exploratory. A separate holdout corpus and larger repeated-job tests are needed before changing production Auto.
