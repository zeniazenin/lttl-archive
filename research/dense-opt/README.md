# lttl Dense: lower CPU overhead, identical compressed bytes

This experiment optimizes the CPU implementation of **lttl Dense** without changing its sampling decisions, compression settings or `LTNV0002` frame. The [measured results](measurements.md) compare optimized Dense with the frozen Dense implementation, the original numeric codec, ordinary Zstandard 3, RAR and 7-Zip. The [chart](codec-time.svg) isolates codec time; the report also includes complete creation and extraction times.

The implementation remains a research executable, `examples/dense_opt.rs`. It is not yet the Windows application's production codec or Auto policy. No GPU implementation or installer is changed. The original [numeric-v2 experiment](../numeric-v2/README.md) documents the algorithm, frame, candidate layouts and dataset provenance.

## Implementation changes

* Reuse a Zstandard compression context per worker thread. Each call still produces an independent frame with no cross-frame dictionary or history. Contexts survive until their worker exits, trading retained worker memory for fewer allocations and initialization steps.
* Use AVX2 for 16-bit delta/zigzag or XOR prediction and byte-plane output, with both input byte orders supported. Runtime feature detection selects this path only on capable x86-64 CPUs; the scalar implementation remains the fallback. Packed 12-bit and wider-word transforms retain their existing paths.
* Run the four independent Dense sampling candidates in parallel on the existing Rayon pool. Reuse each candidate's residuals for the two trial representations. Apply results in the original order with the original 3% replacement margin, preserving selection semantics.
* Allocate the final frame once and append chunks directly, avoiding the intermediate full payload copy. Immediately emit stored frames when sampling selects storage.

The level-3 entropy stage, independent chunk boundaries, original checksum, candidate set and sample positions are unchanged. The decoder is unchanged. Faster creation does not imply a new decoding algorithm. Profiling instrumentation remains enabled in the measured optimized executable.

## Comparison boundaries

The machine is an Intel Core Ultra 9 185H. All Rust configurations run through the **same executable**, file IO routines and eight-worker pool. This removes the different-executable startup confound in the preceding experiment. `original` means the original numeric codec, `before` means frozen lttl Dense, and `after` means optimized lttl Dense.

The same [14-input manifest](../numeric-v2/corpus.json) is used: original packed ECG, EDF EEG, 12-channel ECG, big-endian terrain, separately labeled converted ECG/terrain, synthetic sensors at three sizes, noise, text and three wider-word synthetic controls. Original and converted files are separate cases. These are development-corpus results, not holdout validation. No files were downloaded or altered for this experiment.

`benchmark.py` runs seven configurations in seeded randomized serial order, three repetitions each: **294 creation/extraction pairs**. Main timers include executable launch through exit and common output `fsync`. Rust codec timers separately include selection, transformation, compression and checksum, excluding file IO and pool initialization. Do not compare internal Rust codec times directly with external RAR/7z process times.

Actual RAR 7.20.3 uses method 5, a 32 MiB dictionary and eight requested threads. Actual 7-Zip 26.04 uses Ultra LZMA2 with a 64 MiB dictionary, or byte Delta + normal LZMA2 with a 32 MiB dictionary, with eight requested threads. Delta distances are the previous experiment's detected distances, supplied outside 7z's timer. Commands and method listings are retained in [results.json](results.json). These are specified settings, not exhaustive tuning. RAR/7z store a member name and metadata; research frames store only payload. Output sizes include each format's headers.

`memory_benchmark.py` separately runs original/before/after with seven consecutive jobs per process: **294 in-memory round trips**. It records the first job separately and summarizes the following six as warm jobs. The input and Rayon pool persist; timing excludes file IO and byte-equality verification. This shows benefits of context reuse across jobs, while the fresh-process experiment includes initially empty compressor contexts. Both experiments use the same binary hash. Randomization is across groups; jobs within a group are consecutive observations, not independent randomized samples.

Every file round trip is SHA-256 verified. Every before/after archive must also match the **frozen previous Dense archive's SHA-256**, proving identical output on this corpus, rather than merely similar compression ratios. In-memory jobs verify exact restored bytes and matching encoded sizes/XXH64. The report generator checks source, binary, corpus and baseline hashes and the completeness of both experiments.

OS cache, CPU placement, thermal state, power and background activity are uncontrolled. Report medians and observed ranges; these sample counts do not establish statistical significance. Peak memory and total CPU usage were not measured. Parallel sampling can reduce latency without reducing every component's aggregate CPU work. Profiling worker times are sums of elapsed worker durations, not CPU utilization, and must not be added to wall times because work overlaps. Stored and ordinary-Zstd fallback paths do not have detailed phase instrumentation.

## Validation and reproduction

The release example has 16 passing tests across the original, frozen and optimized modules. Tests include layout/tail/frame validation, corrupt streams, SIMD comparisons at unaligned offsets and vector boundaries, Zstandard context reuse across sizes/levels, and exact before/after frame equality plus cross-decoding with one and eight workers. AVX2 executes on this test machine; other CPU architectures were not benchmarked.

Use the pinned toolchain, existing input files and archive tools:

```powershell
./scripts/cargo.ps1 test --release --example dense_opt --locked --offline
./scripts/cargo.ps1 build --release --example dense_opt --locked --offline
python research/dense-opt/memory_benchmark.py
python research/dense-opt/benchmark.py
python research/dense-opt/summarize.py
python research/dense-opt/plot.py
```

Preserve the raw reports under other names before a new experiment. The file benchmark resumes only if its recorded hashes and configuration agree. Individual commands (output files must not already exist):

```powershell
./target/release/examples/dense_opt.exe encode after INPUT.bin OUTPUT.ltnv
./target/release/examples/dense_opt.exe decode after OUTPUT.ltnv RESTORED.bin
./target/release/examples/dense_opt.exe memory after INPUT.bin 7
```

Use `before`, `original` or `zstd-3` for the comparison implementations. `.ltnv` denotes the experimental frame; renaming it `.lttl` does not make it a production archive container.
