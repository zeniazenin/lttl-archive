# Portable GPU benchmark — 2026-10-12

243/243 complete creation and extraction runs passed SHA-256 verification: nine datasets, nine configurations, three repetitions. All times below are medians. Smaller output percentage means better compression. The CPU is the Intel Core Ultra 9 185H; GPUs are Intel Arc Graphics and NVIDIA RTX 4070 Laptop GPU. Both portable GPU paths used Vulkan.

[Method, implementation and limitations](portable-gpu.md) · [Raw measurements](portable-benchmark-results.json)

## Complete archive creation

Each cell is **fresh-process seconds / output as % of original input**. Includes startup, GPU transfers and filesystem work. LZ4 uses 16 KiB independent blocks and 16 MiB batches; CPU entries here use 8 workers.

| Dataset | CPU Zstd Fast | CPU LZ4 | Intel LZ4 | RTX LZ4 |
|---|---:|---:|---:|---:|
| canterbury | 0.090 s / 24.57% | 0.098 s / 46.82% | 0.499 s / 50.63% | 0.346 s / 50.63% |
| enwik8 | 0.191 s / 40.68% | 0.333 s / 62.09% | 1.758 s / 67.24% | 0.724 s / 67.24% |
| heldout-json | 0.109 s / 12.77% | 0.143 s / 25.01% | 0.710 s / 25.33% | 0.422 s / 25.33% |
| heldout-mixed | 0.121 s / 52.95% | 0.156 s / 55.70% | 0.914 s / 55.88% | 0.471 s / 55.88% |
| independent-noise | 0.188 s / 100.00% | 0.257 s / 100.02% | 1.823 s / 100.03% | 0.666 s / 100.03% |
| precompressed-zip | 0.190 s / 99.51% | 0.257 s / 100.02% | 1.798 s / 100.02% | 0.695 s / 100.02% |
| raw-sensors | 0.187 s / 71.36% | 0.270 s / 84.84% | 1.660 s / 90.27% | 0.649 s / 90.27% |
| similar-files | 0.235 s / 100.05% | 0.296 s / 100.04% | 1.899 s / 100.05% | 0.722 s / 100.05% |
| structured-csv | 0.132 s / 14.84% | 0.212 s / 26.77% | 0.993 s / 27.33% | 0.507 s / 27.33% |

## CPU presets, worker count and Auto

Zstd Balanced uses level 3; Fast uses level 1. CPU LZ4 worker count does not change output. Auto LZ4 performs no sampling or GPU initialization. Its output matches explicit CPU LZ4 in every case.

| Dataset | Zstd Balanced seconds / output | CPU LZ4 1 worker (s) | CPU LZ4 8 workers (s) | Auto LZ4 (s) |
|---|---:|---:|---:|---:|
| canterbury | 0.103 / 22.49% | 0.098 | 0.098 | 0.102 |
| enwik8 | 0.234 / 35.44% | 0.525 | 0.333 | 0.332 |
| heldout-json | 0.135 / 13.35% | 0.151 | 0.143 | 0.147 |
| heldout-mixed | 0.129 / 53.03% | 0.149 | 0.156 | 0.165 |
| independent-noise | 0.205 / 100.00% | 0.254 | 0.257 | 0.255 |
| precompressed-zip | 0.204 / 99.82% | 0.251 | 0.257 | 0.252 |
| raw-sensors | 0.232 / 53.52% | 0.375 | 0.270 | 0.255 |
| similar-files | 0.181 / 10.00% | 0.287 | 0.296 | 0.299 |
| structured-csv | 0.156 / 16.57% | 0.218 | 0.212 | 0.212 |

## Intel Arc: device timing breakdown

Initialization is host wall time. Copy and kernel intervals are GPU timestamps. These are components, not a replacement for complete creation time; CPU staging, synchronization, checksums, framing and disk work account for additional time. Full worst-case output slots are copied back.

| Dataset | Init (ms) | Upload (ms) | Kernel (ms) | Readback (ms) | Uploaded / read back (MiB) |
|---|---:|---:|---:|---:|---:|
| canterbury | 334.59 | 0.12 | 29.12 | 0.08 | 2.69 / 2.72 |
| enwik8 | 335.99 | 3.12 | 1049.18 | 3.13 | 95.37 / 95.96 |
| heldout-json | 346.18 | 1.11 | 178.83 | 1.05 | 32.00 / 32.21 |
| heldout-mixed | 318.61 | 1.09 | 381.85 | 1.07 | 32.00 / 32.21 |
| independent-noise | 325.58 | 2.13 | 1165.70 | 2.13 | 64.00 / 64.41 |
| precompressed-zip | 340.41 | 2.08 | 1151.58 | 2.14 | 64.00 / 64.41 |
| raw-sensors | 327.22 | 2.10 | 1019.14 | 2.13 | 64.00 / 64.41 |
| similar-files | 329.48 | 2.03 | 1216.13 | 2.21 | 64.03 / 64.44 |
| structured-csv | 338.01 | 2.16 | 403.84 | 2.10 | 64.00 / 64.41 |

## RTX 4070: device timing breakdown

Initialization is host wall time. Copy and kernel intervals are GPU timestamps. These are components, not a replacement for complete creation time; CPU staging, synchronization, checksums, framing and disk work account for additional time. Full worst-case output slots are copied back.

| Dataset | Init (ms) | Upload (ms) | Kernel (ms) | Readback (ms) | Uploaded / read back (MiB) |
|---|---:|---:|---:|---:|---:|
| canterbury | 221.50 | 0.03 | 4.39 | 0.23 | 2.69 / 2.72 |
| enwik8 | 239.32 | 0.74 | 131.52 | 7.68 | 95.37 / 95.96 |
| heldout-json | 215.96 | 0.31 | 23.75 | 2.59 | 32.00 / 32.21 |
| heldout-mixed | 227.32 | 0.31 | 51.50 | 2.59 | 32.00 / 32.21 |
| independent-noise | 209.86 | 0.52 | 152.34 | 5.17 | 64.00 / 64.41 |
| precompressed-zip | 221.22 | 0.54 | 162.59 | 5.17 | 64.00 / 64.41 |
| raw-sensors | 224.24 | 0.54 | 126.44 | 5.17 | 64.00 / 64.41 |
| similar-files | 248.82 | 0.53 | 160.04 | 5.17 | 64.03 / 64.44 |
| structured-csv | 222.40 | 0.55 | 51.19 | 5.17 | 64.00 / 64.41 |

## Smaller GPU blocks

Each cell is seconds / output percentage. Smaller blocks expose more independent work but sacrifice history and add framing/table overhead. Neither setting is assumed best without measurement.

| Dataset | Intel 4 KiB | Intel 16 KiB | RTX 4 KiB | RTX 16 KiB |
|---|---:|---:|---:|---:|
| canterbury | 0.497 / 55.57% | 0.499 / 50.63% | 0.361 / 55.57% | 0.346 / 50.63% |
| enwik8 | 1.908 / 70.31% | 1.758 / 67.24% | 0.733 / 70.31% | 0.724 / 67.24% |
| heldout-json | 0.776 / 27.09% | 0.710 / 25.33% | 0.423 / 27.09% | 0.422 / 25.33% |
| heldout-mixed | 0.971 / 57.11% | 0.914 / 55.88% | 0.459 / 57.11% | 0.471 / 55.88% |
| independent-noise | 1.927 / 100.10% | 1.823 / 100.03% | 0.707 / 100.10% | 0.666 / 100.03% |
| precompressed-zip | 1.854 / 100.10% | 1.798 / 100.02% | 0.734 / 100.10% | 0.695 / 100.02% |
| raw-sensors | 1.710 / 92.29% | 1.660 / 90.27% | 0.655 / 92.29% | 0.649 / 90.27% |
| similar-files | 1.862 / 100.11% | 1.899 / 100.05% | 0.730 / 100.11% | 0.722 / 100.05% |
| structured-csv | 1.023 / 30.76% | 0.993 / 27.33% | 0.487 / 30.76% | 0.507 / 27.33% |

## CPU extraction time

Fresh-process seconds, including staging and publication. All configurations use CPU extraction. SHA-256 verification occurs after the timed extraction command.

| Dataset | CPU Zstd Fast archive | CPU LZ4 archive | Intel LZ4 archive | RTX LZ4 archive |
|---|---:|---:|---:|---:|
| canterbury | 0.161 | 0.166 | 0.164 | 0.152 |
| enwik8 | 0.377 | 0.440 | 0.393 | 0.423 |
| heldout-json | 0.158 | 0.185 | 0.213 | 0.181 |
| heldout-mixed | 0.149 | 0.179 | 0.164 | 0.193 |
| independent-noise | 0.210 | 0.220 | 0.238 | 0.237 |
| precompressed-zip | 0.223 | 0.256 | 0.227 | 0.232 |
| raw-sensors | 0.278 | 0.294 | 0.301 | 0.273 |
| similar-files | 0.584 | 0.620 | 0.621 | 0.698 |
| structured-csv | 0.222 | 0.254 | 0.279 | 0.283 |

## Auto decision

Portable GPU configurations beat the faster CPU LZ4 baseline in **0 of 36** dataset/configuration median comparisons. Auto therefore retains CPU LZ4 and avoids GPU enumeration, compilation and sampling. This is a measured exclusion, not a GPU performance claim. TAR.ZST Auto remains unchanged.

The dominant GPU cost is the current match/token kernel and initialization, not the measured bulk copies. Smaller blocks alone do not solve this. A faster implementation needs parallel token construction and better match finding; warm session reuse and output compaction are additional opportunities. The present results do not establish performance on AMD or Apple silicon.

Executable SHA-256: `AA142507BEED7EB1AC63D32E8C0AB4D3887F2CF4BE626F061C190759C3CE5F1A`.
