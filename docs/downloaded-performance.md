# lttl: downloaded samples, archive creation and unpacking

Measured 2026-10-12. **CPU Balanced was both faster and smaller than the current fixed GPU encoder on both downloaded datasets.** GPU-created archives unpacked successfully with the CPU decoder. GPU-accelerated decompression is not implemented or claimed by these measurements.

## Samples and provenance

- **Canterbury:** 11 unmodified files, **2,810,784 bytes**, including English text, HTML, C/LISP source, an Excel spreadsheet, fax-test data, a SPARC executable, and a manual page. The collection is documented by the [University of Canterbury](https://corpus.canterbury.ac.nz/descriptions/). The university archive download was blocked by Cloudflare, so the download script used a public mirror pinned to commit `c380ef9dc1dc62691757fdfeebdc2f85710e1e46`, verified publisher-listed lengths and mirror SHA-1 values, and retained local SHA-256 values. These are the original small benchmark files, not a modern application/media workload.
- **enwik8:** **100,000,000 bytes** of UTF-8 Wikipedia XML/text. Downloaded from the benchmark publisher; file length and publisher SHA-1 `57b8363b814821dc9d47aa4d41f58733519076b2` were checked. See the [publisher's dataset description and checksums](https://mattmahoney.net/dc/textdata.html).

Exact source URLs and hashes are retained in [provenance](downloaded-sample-sources.json) and the [corpus manifest](downloaded-corpus-manifest.json). Original downloads and unpacked inputs remain under `bench-output/downloaded-samples`. Files were treated as inert data; the executable sample was never launched.

## Method

This separate run used the same release build and RTX 4070 Laptop GPU setup as the [synthetic GPU comparison](gpu-performance.md), with one MiB frames, 32 MiB batches, eight requested CPU workers and fresh processes. CPU presets are Zstandard levels 1/3/9; GPU uses fixed nvCOMP options. CPU-chunked Fast resets at the same frame boundaries as GPU but uses a different encoder.

All **30 creations, archive integrity tests, and extractions passed**, and all **180 extracted file SHA-256 comparisons matched**. Every configuration was repeated three times with rotated order. The parent task paused competing development builds/compression during timing. OS cache, background work, power and thermal state were not controlled; these are small-sample exploratory results.

**Creation**, **archive integrity testing**, **extraction**, and **post-extraction hashing** were timed separately. Extraction includes CPU decoding, temporary staging, filesystem validation, copying/publishing files and syncing outputs. It is not a pure decompression-kernel timer. Test/hash operations are outside extraction timing. Test-before-extraction also warms archive data in the filesystem cache. Each table below reports medians; individual observations and ranges remain in the [raw JSON](downloaded-benchmark-results.json).

```powershell
pwsh -NoProfile -File scripts/download-samples.ps1
pwsh -NoProfile -File scripts/benchmark-gpu.ps1 `
  -CorpusDirectory bench-output/downloaded-samples/corpus `
  -TimeExtraction -SkipChunkSweep
```

The retained timing run is `bench-output/gpu-review/20261012T213334866Z`. An earlier attempt stopped because the downloaded manifest's dataset totals were null; no failed-attempt observations are merged into this run. Before this complete run, the harness derived missing totals from individually size/hash-verified files. The downloader was subsequently corrected.

## Complete process timings

Size percentage is archive bytes divided by original file bytes; smaller is better. “Unpack” uses the CPU for all rows, regardless of the encoder backend.

| Dataset | Encoder | Setting | Archive bytes | Retained | Create wall | CPU unpack wall |
|---|---|---|---:|---:|---:|---:|
| canterbury | cpu | balanced | 632096 | 22.488% | 0.116 | 0.171 |
| canterbury | cpu | fast | 690530 | 24.567% | 0.111 | 0.171 |
| canterbury | cpu | small | 593722 | 21.123% | 0.157 | 0.175 |
| canterbury | cpu-chunked | fast | 694731 | 24.717% | 0.120 | 0.169 |
| canterbury | gpu | nvcomp-fixed | 718404 | 25.559% | 0.371 | 0.177 |
| enwik8 | cpu | balanced | 35444627 | 35.445% | 0.273 | 0.416 |
| enwik8 | cpu | fast | 40675282 | 40.675% | 0.213 | 0.418 |
| enwik8 | cpu | small | 31128044 | 31.128% | 0.885 | 0.437 |
| enwik8 | cpu-chunked | fast | 40794322 | 40.794% | 0.269 | 0.391 |
| enwik8 | gpu | nvcomp-fixed | 38603223 | 38.603% | 0.708 | 0.414 |

For **enwik8**, CPU Balanced took 0.273 s versus GPU's 0.708 s and produced 35,444,627 versus 38,603,223 bytes. That is approximately **2.6x faster creation and 8.2% smaller output** for CPU Balanced on this file. GPU was faster than CPU Small, but its archive was about 24% larger, so that comparison trades compression density for time.

For **Canterbury**, CPU Balanced took 0.116 s and retained 22.488%; GPU took 0.371 s and retained 25.559%. Its small input makes process/context startup a substantial share of the complete operation. This corpus was archived as a group; the totals do not establish a winner for each constituent file type.

Unpacking times were close: roughly 0.17–0.18 s for Canterbury and 0.39–0.44 s for enwik8 across variants. Differences in these short medians should not be interpreted as robust decoder speedups. In particular, a “GPU” row describes **CPU extraction of a GPU-created standard Zstandard archive**.

## Engine timing and verification, kept separate

All values are seconds. Engine intervals exclude process launch/teardown and JSON output. Hash time is only the post-extraction SHA-256 comparison stage.

| Dataset | Encoder / setting | Create engine | Unpack engine | Integrity test wall | Integrity test engine | Hash |
|---|---|---:|---:|---:|---:|---:|
| canterbury | cpu / balanced | 0.030 | 0.080 | 0.093 | 0.005 | 0.006 |
| canterbury | cpu / fast | 0.024 | 0.076 | 0.093 | 0.005 | 0.005 |
| canterbury | cpu / small | 0.065 | 0.081 | 0.093 | 0.006 | 0.005 |
| canterbury | cpu-chunked / fast | 0.022 | 0.081 | 0.097 | 0.005 | 0.006 |
| canterbury | gpu / nvcomp-fixed | 0.279 | 0.081 | 0.097 | 0.005 | 0.006 |
| enwik8 | cpu / balanced | 0.189 | 0.315 | 0.232 | 0.141 | 0.101 |
| enwik8 | cpu / fast | 0.125 | 0.309 | 0.200 | 0.111 | 0.099 |
| enwik8 | cpu / small | 0.803 | 0.337 | 0.207 | 0.124 | 0.099 |
| enwik8 | cpu-chunked / fast | 0.180 | 0.293 | 0.193 | 0.102 | 0.103 |
| enwik8 | gpu / nvcomp-fixed | 0.619 | 0.319 | 0.226 | 0.140 | 0.103 |

## GPU components on downloaded data

These medians are milliseconds. Host-call wall time includes the device intervals, host staging/submission and waits; it is not additive with them. Checksum work is CPU processing after the native call. Complete process time also includes initialization and filesystem/container work.

| Dataset | Init | H2D | Compression | D2H | Host call | Checksum | Chunks / batches |
|---|---:|---:|---:|---:|---:|---:|---:|
| canterbury | 102.7 | 0.3 | 130.0 | 0.2 | 131.1 | 0.5 | 3 / 1 |
| enwik8 | 117.1 | 8.3 | 303.6 | 4.3 | 343.0 | 21.1 | 96 / 3 |

The device compression stage itself consumed about 130 ms on Canterbury and 304 ms on enwik8, while transfers were much smaller. This run therefore does not support blaming PCIe alone, or predicting a win merely by reusing initialization. Persistent contexts and overlapping batches need their own measured implementation.

These public samples strengthen the case for a CPU default and an explicit experimental GPU option. They do not cover all real datasets, storage devices or GPUs. The [improvement analysis](gpu-improvements.md) lists specific next experiments for lttl’s independent CPU and GPU backends.
