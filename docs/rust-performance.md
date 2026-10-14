# Rust prototype: synthetic-corpus measurements

Measured 2026-10-12. **The Rust prototype offers useful CPU speed/size tradeoffs, but no single preset wins on every synthetic input.** ZIP Fast avoids substantial wasted work on the random input. TAR.ZST compresses the generated text quickly, while its Small preset is much better than Fast on the related-file corpus. These are narrow workload results and do not establish general codec rankings.

## Method and reproducibility

The release executable was `target/release/archiver.exe`, SHA-256 `224CBD7964D267D7BB80C55E1559171FC6144E8E0203ACCC768BBD9C0631EEA2`. It includes the Fast-only ZIP sampling policy. The script verified every original corpus SHA-256 against the retained manifest before starting. The exact input paths and hashes are preserved in the raw results.

Each corpus contains 32 MiB of file data. Inputs cover pseudorandom bytes, generated text, repeated blocks and related files. No real user files were sampled. The 84 creation runs cover four corpora, all three presets in both formats with eight requested workers, plus TAR.ZST Balanced with one worker, repeated three times. ZIP always compresses serially. Invocation order rotates between rounds.

```powershell
pwsh -NoProfile -File scripts/benchmark-rust.ps1
```

If reproducing with a preserved synthetic corpus, pass its project-relative path with `-CorpusPath`. Each invocation creates a new timestamped directory under `bench-output/rust-review`. Exact process argument arrays, archive sizes, process CPU/wall time, peak working set, engine timing, and source manifests are retained in [raw JSON](../research/legacy/rust-benchmark-results.json). The recorded run is `bench-output/rust-review/20261012T211013033Z`.

All **84 archive integrity tests passed**. An additional eight extraction round trips, one ZIP Fast and one TAR.ZST Balanced archive per corpus, reproduced all **70 file SHA-256 values**. Integrity checking and these extraction/hash comparisons are outside creation timings.

This report uses external **process wall time**, including process startup and file I/O. The cache was warm; thermal state, power mode, affinity, storage cache, and background activity were not controlled. Three repetitions and subsecond cases do not support precise rankings of close results. The full min–max range is shown below. These are practical configurations, not equal-memory tests or isolated compression-kernel benchmarks.

## Measurements

Archive percentage is output bytes divided by the 33,554,432 input bytes. Smaller is better. All sizes were identical across repetitions of the same case.

| Corpus | Format | Preset | Requested workers | Archive bytes | Original size retained | Median seconds | Range seconds |
|---|---|---|---:|---:|---:|---:|---:|
| random.bin | tar-zstd | balanced | 1 | 33555737 | 100.0039% | 0.153 | 0.152–0.169 |
| random.bin | tar-zstd | balanced | 8 | 33555737 | 100.0039% | 0.168 | 0.166–0.200 |
| random.bin | tar-zstd | fast | 8 | 33555735 | 100.0039% | 0.152 | 0.151–0.183 |
| random.bin | tar-zstd | small | 8 | 33555735 | 100.0039% | 0.184 | 0.168–0.188 |
| random.bin | zip | balanced | 8 | 33559875 | 100.0162% | 0.854 | 0.839–0.858 |
| random.bin | zip | fast | 8 | 33554590 | 100.0005% | 0.135 | 0.135–0.137 |
| random.bin | zip | small | 8 | 33559875 | 100.0162% | 1.010 | 0.995–1.084 |
| records.txt | tar-zstd | balanced | 1 | 3622239 | 10.7951% | 0.165 | 0.151–0.201 |
| records.txt | tar-zstd | balanced | 8 | 3622239 | 10.7951% | 0.138 | 0.137–0.152 |
| records.txt | tar-zstd | fast | 8 | 3179849 | 9.4767% | 0.133 | 0.120–0.135 |
| records.txt | tar-zstd | small | 8 | 3460386 | 10.3128% | 0.322 | 0.295–0.323 |
| records.txt | zip | balanced | 8 | 3544252 | 10.5627% | 0.469 | 0.459–0.495 |
| records.txt | zip | fast | 8 | 4094615 | 12.2029% | 0.169 | 0.166–0.393 |
| records.txt | zip | small | 8 | 3531734 | 10.5254% | 0.685 | 0.680–0.808 |
| repeating.bin | tar-zstd | balanced | 1 | 100240 | 0.2987% | 0.136 | 0.123–0.137 |
| repeating.bin | tar-zstd | balanced | 8 | 100240 | 0.2987% | 0.123 | 0.121–0.135 |
| repeating.bin | tar-zstd | fast | 8 | 68714 | 0.2048% | 0.120 | 0.118–0.121 |
| repeating.bin | tar-zstd | small | 8 | 68698 | 0.2047% | 0.139 | 0.138–0.153 |
| repeating.bin | zip | balanced | 8 | 33559881 | 100.0162% | 0.828 | 0.820–0.901 |
| repeating.bin | zip | fast | 8 | 33554596 | 100.0005% | 0.137 | 0.136–0.140 |
| repeating.bin | zip | small | 8 | 33559881 | 100.0162% | 0.903 | 0.822–0.945 |
| similar | tar-zstd | balanced | 1 | 3485457 | 10.3875% | 0.151 | 0.137–0.167 |
| similar | tar-zstd | balanced | 8 | 3485457 | 10.3875% | 0.167 | 0.153–0.168 |
| similar | tar-zstd | fast | 8 | 33572121 | 100.0527% | 0.200 | 0.186–0.201 |
| similar | tar-zstd | small | 8 | 1085832 | 3.2360% | 0.166 | 0.155–0.185 |
| similar | zip | balanced | 8 | 33564954 | 100.0314% | 0.887 | 0.885–0.934 |
| similar | zip | fast | 8 | 33559514 | 100.0151% | 0.166 | 0.162–0.167 |
| similar | zip | small | 8 | 33564954 | 100.0314% | 0.935 | 0.910–1.036 |

## What the results mean

**Adaptive storage produces a useful measurable speed difference.** On pseudorandom data, ZIP Fast took 0.135 s versus 0.854 s for ZIP Balanced, about 6.3x less time, and produced 5,285 fewer bytes. Fast's sample policy selected Store; Balanced performed Deflate. This comparison also changes preset, so it is not an isolated on/off ablation of sampling.

**Format constraints matter more than extra CPU effort in some cases.** The repeated-block input repeats every 64 KiB. ZIP Deflate cannot refer back more than 32 KiB, so it cannot exploit those full-block repeats; Balanced/Small slightly expanded the input. ZIP Fast stored it, while TAR.ZST Fast produced 68,714 bytes in 0.120 s. The Deflate distance limit is defined by [RFC 1951, section 2](https://www.rfc-editor.org/rfc/rfc1951).

**“Higher level” is not a promise of a smaller archive.** On generated text, TAR.ZST Fast produced 3,179,849 bytes in 0.133 s; Balanced produced 3,622,239 in 0.138 s; Small produced 3,460,386 in 0.322 s. Choosing the best result from a synthetic corpus cannot establish a general codec ranking.

**Related files need sufficient history/search capability.** TAR.ZST Fast nearly stored the 32 related files unchanged (33,572,121 bytes); Balanced reached 3,485,457 bytes, and Small reached 1,085,832 bytes. This is an intentionally favorable repetition case. The observed preset gap motivates configurable history and grouping; the precise internal cause was not profiled here, and the current app has no automatic window selector.

**More workers do not help every small job.** TAR.ZST Balanced's one/eight-worker medians were 0.165/0.138 s for text, 0.136/0.123 s for repeated blocks, 0.153/0.168 s for random input, and 0.151/0.167 s for related files. Startup, scheduling and I/O are a large part of these short runs, and ranges overlap. This experiment does not establish an optimal worker count. Test larger files and sustained jobs before changing automatic CPU defaults.

## Implications for the next implementation

1. Keep ZIP Fast's sampling transparent and overridable through the other presets. The policy uses three 64 KiB samples, not a proof of global incompressibility, and Zstandard is only a proxy for Deflate.
2. Expose the format tradeoff clearly: ZIP interoperability versus TAR.ZST's larger-history and stream-compression capabilities. A high ZIP level cannot overcome Deflate's distance limit.
3. Add a bounded-memory window/group strategy before claiming smart file-type adaptation. Related-file and long-distance-repetition tests belong in future performance regression suites.
4. Benchmark larger real corpora with equal resource budgets, startup separated from sustained throughput, controlled background load, and enough repetitions to quantify uncertainty. Preserve correctness and independent extraction checks.
5. Treat GPU support as a separate implementation and measurement project. No GPU compression was executed in either benchmark; these results justify CPU/policy work and cannot predict GPU acceleration.
