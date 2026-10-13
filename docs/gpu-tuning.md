# lttl GPU Zstandard chunk/batch tuning

Measured 2026-10-12. **Tuning produced real improvements over the GPU default in the same run, while CPU Balanced remained faster on all three inputs.** Smaller chunks improved both speed and size on the synthetic CSV; a larger batch improved enwik8 speed without changing its archive bytes. Neither result establishes a universal best configuration.

The [script](../scripts/benchmark-zstd-tuning.ps1) compared six fixed-nvCOMP GPU configurations—64/256/1024 KiB chunks crossed with 32/128 MiB batches—against streaming CPU Balanced. Each configuration ran three times on real enwik8 (100,000,000 bytes), synthetic structured CSV (64 MiB), and independent synthetic noise (64 MiB), totaling **63 successful archive creations and integrity checks**. All **21 extracted-file SHA-256 checks passed**.

The GPU 1024 KiB / 32 MiB configuration is repeated within this same run as the baseline. Earlier runs are useful context but are not substituted for this control. CPU Balanced uses eight requested workers. GPU encoder settings remain fixed; chunk and batch tuning do not expose a new compression level or proven search-window control.

Every input SHA-256 was verified against the existing corpus manifest before timing. Every output archive was integrity-tested; one archive for each dataset/configuration was CPU-extracted and compared by SHA-256. [Raw results](gpu-tuning-results.json) retain full process time, internal engine time, GPU initialization/copy/compression/checksum metrics, output size and verification times. Hashing is outside timed archive creation and extraction. Extraction measurements here are correctness checks, not a repeated unpacking performance study; that is covered by the [downloaded-sample report](downloaded-performance.md).

The device was the same **RTX 4070 Laptop GPU** reported by `doctor`. The release executable SHA-256 was `28A5AC838A71AC9039B5F2B4F25E53F89921BE93E3AB77C5A23123B3717AF612`; runtime DLL hashes and exact input hashes are included in the raw record. The retained run is `bench-output/zstd-tuning/20261012T214023961Z`.

```powershell
pwsh -NoProfile -File scripts/benchmark-zstd-tuning.ps1
```

Fresh processes and contexts include application setup and file I/O. Other development compilation/GPU workloads were paused during the timing window. OS/driver caches, thermals and background graphics activity remained uncontrolled. Results compare archive ratio and complete latency together: faster kernels with worse output size are not automatically better archival settings. No persistent-context speedup is inferred by subtracting initialization from wall time.

The script retains source hashes, executable/runtime DLL hashes, arguments and raw observations, limits individual processes and the whole run, and preserves partial data on failure. It removes only its own generated archives and verified extraction directories; original corpora remain untouched.

## Same-run results

Times are full creation-process wall seconds: median of three runs, with observed minimum–maximum. Retained size is archive bytes / original file bytes; smaller is better. All output sizes were identical across repetitions of each configuration. CPU Balanced is streaming level 3 with eight requested workers; GPU uses fixed encoder settings. `—` means chunk/batch parameters do not apply to the streaming CPU baseline.

| Dataset | Backend | Chunk KiB | Batch MiB | Median seconds | Range seconds | Retained size |
|---|---|---:|---:|---:|---:|---:|
| enwik8 | cpu | — | — | 0.259 | 0.257–0.287 | 35.4446% |
| enwik8 | gpu | 64 | 32 | 0.448 | 0.424–0.530 | 40.1699% |
| enwik8 | gpu | 64 | 128 | 0.513 | 0.498–0.593 | 40.1699% |
| enwik8 | gpu | 256 | 32 | 0.483 | 0.476–0.509 | 38.9258% |
| enwik8 | gpu | 256 | 128 | 0.485 | 0.472–0.504 | 38.9258% |
| enwik8 | gpu | 1024 | 32 | 0.606 | 0.591–0.610 | 38.6032% |
| enwik8 | gpu | 1024 | 128 | 0.502 | 0.495–0.511 | 38.6032% |
| independent-noise | cpu | — | — | 0.233 | 0.232–0.240 | 100.0039% |
| independent-noise | gpu | 64 | 32 | 0.457 | 0.448–0.469 | 100.0231% |
| independent-noise | gpu | 64 | 128 | 0.504 | 0.484–0.521 | 100.0231% |
| independent-noise | gpu | 256 | 32 | 0.433 | 0.422–0.441 | 100.0101% |
| independent-noise | gpu | 256 | 128 | 0.499 | 0.480–0.503 | 100.0101% |
| independent-noise | gpu | 1024 | 32 | 0.454 | 0.434–0.457 | 100.0061% |
| independent-noise | gpu | 1024 | 128 | 0.490 | 0.448–0.518 | 100.0061% |
| structured-csv | cpu | — | — | 0.172 | 0.165–0.175 | 16.5709% |
| structured-csv | gpu | 64 | 32 | 0.319 | 0.307–0.337 | 13.0791% |
| structured-csv | gpu | 64 | 128 | 0.376 | 0.375–0.378 | 13.0791% |
| structured-csv | gpu | 256 | 32 | 0.379 | 0.371–0.404 | 13.3064% |
| structured-csv | gpu | 256 | 128 | 0.392 | 0.389–0.431 | 13.3064% |
| structured-csv | gpu | 1024 | 32 | 0.443 | 0.439–0.446 | 13.4097% |
| structured-csv | gpu | 1024 | 128 | 0.388 | 0.381–0.397 | 13.4097% |

## Useful gains and their tradeoffs

- **Synthetic CSV: 64 KiB / 32 MiB was faster and smaller than the GPU baseline.** Median latency fell from 0.443 to 0.319 s, **28% less time**. Output fell from 8,999,117 to 8,777,246 bytes, **2.47% smaller**. The observed timing ranges did not overlap (baseline 0.439–0.446 s; tuned 0.307–0.337 s). CPU Balanced was still faster at 0.172 s, but produced 11,120,539 bytes. This is a generator-specific speed/size tradeoff, not proof that all CSV benefits equally.
- **enwik8: 1024 KiB / 128 MiB preserved the GPU baseline's exact 38,603,223-byte output while cutting median time from 0.606 to 0.502 s, about 17%.** Its timing range (0.495–0.511 s) was below the 32 MiB batch baseline (0.591–0.610 s). The 64 KiB / 32 MiB setting was faster at 0.448 s, but enlarged output to 40,169,865 bytes, approximately 4.1% larger than the GPU baseline. CPU Balanced remained preferable for both metrics on this file: 0.259 s and 35,444,627 bytes.
- **Independent noise: tuning does not create compressibility.** The lowest GPU median was 0.433 s at 256 KiB / 32 MiB, versus 0.454 s for the baseline, with overlapping ranges. Every setting slightly expanded the input. CPU Balanced took 0.233 s. This is a stronger case for avoiding expensive compression than for selecting another GPU tuning value.

GPU batch changes at a fixed chunk size produced identical archive bytes in this experiment. Chunk changes altered ratio in a data-dependent, non-monotonic way: smaller chunks improved the generated CSV ratio but worsened enwik8 ratio. That is an observation about this fixed encoder and corpus, not a general claim that smaller history improves compression.

## Selected GPU components

All component medians are milliseconds. “Host call” includes the native call, device operations, staging and waits, so it must not be added to H2D/compression/D2H. Initialization and checksum are separate diagnostic intervals. Independently computed medians do not form an exact partition of any one run.

| Dataset / chunk KiB / batch MiB | Init | H2D | Compression | D2H | Host call | Checksum |
|---|---:|---:|---:|---:|---:|---:|
| enwik8 / 1024 / 32 | 99.6 | 8.4 | 236.8 | 4.0 | 263.1 | 16.1 |
| enwik8 / 1024 / 128 | 120.1 | 8.4 | 94.9 | 4.1 | 123.5 | 18.1 |
| enwik8 / 64 / 32 | 93.2 | 8.4 | 88.3 | 13.8 | 123.0 | 17.2 |
| structured-csv / 1024 / 32 | 96.4 | 5.7 | 155.1 | 1.2 | 168.1 | 7.3 |
| structured-csv / 64 / 32 | 91.4 | 5.7 | 21.5 | 7.0 | 41.1 | 7.2 |
| independent-noise / 1024 / 32 | 96.8 | 5.7 | 60.9 | 5.9 | 90.4 | 20.5 |
| independent-noise / 256 / 32 | 99.1 | 5.7 | 42.9 | 6.8 | 77.5 | 21.7 |

For enwik8, increasing the batch at 1024 KiB chunks reduced the measured compression interval from about 237 to 95 ms while initialization rose from about 100 to 120 ms. For CSV, the 64 KiB / 32 MiB configuration reduced the compression interval from about 155 to 21 ms; D2H rose from about 1 to 7 ms because more frames require more result handling/copies. Complete process latency improved by much less than the kernel interval, illustrating why the application timer is the decision metric.

These observations are consistent with a parallelism/scheduling effect, but kernel occupancy and hardware counters were not collected. No exact occupancy explanation is established. Peak RAM/VRAM and power were also not measured, and the 128 MiB batch allocates more host/device working storage than 32 MiB.

## Product recommendation

Keep the CPU default and explicit GPU controls. Present `--chunk-kib` and `--batch-mib` as workload tuning with measured speed/size tradeoffs. A user prioritizing dense GPU output on this enwik8 workload can benefit from a larger batch; the generated CSV benefits from smaller chunks. Neither setting should be silently generalized from three datasets into an automatic universal choice.

This experiment changes batching and framing, not the nvCOMP Zstandard algorithm. A new matcher, codec, content classifier, warm-session pipeline, or automatic configuration policy needs its own implementation, integrity coverage, and end-to-end comparison. The [RAR-informed proposals](gpu-improvements.md) remain separate from these implemented tuning controls.
