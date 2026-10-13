# Portable GPU optimization results — 2026-10-12

All **360 archive creation/extraction pairs** passed. Every extracted file was checked against the corpus SHA-256 manifest. Optimized GPU archives are **bit-for-bit identical to v1** across all nine datasets, both GPUs, cold contexts and reused sessions. Compression ratio did not regress.

[Implementation and reproduction](portable-optimization.md) · [Raw measurements](portable-v2-results.json)

## Complete archive time, seconds

These are archive-operation wall times, including transfers, checksums, framing, file reads/writes, sync and publication. They exclude CLI process startup. V1 columns are medians of three fresh-process operations. Warm v2 columns are medians of three subsequent jobs sharing a GPU session. CPU columns use the same three subsequent jobs in one process; CPU pools are still recreated by the existing implementation.

| Dataset | Faster CPU LZ4 | CPU Zstd Fast | Intel v1 | Intel v2 warm | RTX v1 | RTX v2 warm |
|---|---:|---:|---:|---:|---:|---:|
| canterbury | 0.0200 | 0.0235 | 0.3907 | 0.0542 | 0.2892 | 0.0218 |
| enwik8 | 0.2660 | 0.1260 | 1.7496 | 1.2862 | 0.5854 | 0.3158 |
| heldout-json | 0.0746 | 0.0378 | 0.6089 | 0.2950 | 0.3183 | 0.0804 |
| heldout-mixed | 0.0728 | 0.0515 | 0.8106 | 0.1740 | 0.3534 | 0.0871 |
| independent-noise | 0.1723 | 0.1297 | 1.7309 | 0.2971 | 0.5374 | 0.1962 |
| precompressed-zip | 0.1795 | 0.1195 | 1.7536 | 0.3776 | 0.5350 | 0.2051 |
| raw-sensors | 0.1820 | 0.1083 | 1.5870 | 1.1985 | 0.5108 | 0.2555 |
| similar-files | 0.2122 | 0.1533 | 1.8229 | 0.3250 | 0.6027 | 0.2368 |
| structured-csv | 0.1351 | 0.0666 | 0.9422 | 0.5264 | 0.4112 | 0.1630 |

## Cold contexts and session reuse

Fresh-process v2 is the median of two first-job measurements (one from each v2 group). Recreated-context is the median of two later jobs that deliberately discard the cache; driver/instance startup is already warm. Cached lookup includes no GPU enumeration, pipeline creation or buffer allocation. Cold statistics here have fewer samples than warm statistics and should not be treated as precise estimates.

| Dataset | Intel fresh (s) | Intel recreated (s) | Intel acquire warm (µs) | RTX fresh (s) | RTX recreated (s) | RTX acquire warm (µs) |
|---|---:|---:|---:|---:|---:|---:|
| canterbury | 0.5945 | 0.1161 | 2.70 | 0.2889 | 0.0804 | 2.80 |
| enwik8 | 1.8466 | 1.3457 | 2.80 | 0.5486 | 0.3849 | 3.00 |
| heldout-json | 0.8544 | 0.3273 | 2.80 | 0.3218 | 0.1385 | 3.20 |
| heldout-mixed | 0.7110 | 0.2505 | 2.30 | 0.3156 | 0.1428 | 3.20 |
| independent-noise | 0.9030 | 0.3644 | 2.60 | 0.4451 | 0.2566 | 3.10 |
| precompressed-zip | 0.8852 | 0.4335 | 2.40 | 0.4565 | 0.2865 | 3.30 |
| raw-sensors | 1.8511 | 1.2598 | 3.30 | 0.7565 | 0.2892 | 3.20 |
| similar-files | 0.8709 | 0.3885 | 2.60 | 0.4498 | 0.3006 | 3.40 |
| structured-csv | 1.0727 | 0.6178 | 2.30 | 0.3767 | 0.2549 | 2.80 |

## GPU kernel and transfer timing

Each GPU cell is **v1 kernel → v2 warm kernel / v2 upload / v2 readback**, in milliseconds. Kernel timestamps isolate the shader change; complete archive times above include all host work. V2 uses persistent MAP_WRITE upload buffers. V1 mapped-at-creation allocations could perform staging before the timestamped copy, so upload-copy figures are not an isolated like-for-like transfer comparison.

| Dataset | Intel (ms) | RTX (ms) |
|---|---:|---:|
| canterbury | 31.09 → 32.35 / 0.06 / 0.08 | 4.39 → 2.72 / 0.24 / 0.23 |
| enwik8 | 1156.52 → 1060.51 / 2.81 / 3.11 | 116.02 → 78.74 / 8.24 / 7.70 |
| heldout-json | 190.05 → 229.38 / 0.99 / 1.10 | 23.76 → 16.37 / 2.78 / 2.59 |
| heldout-mixed | 345.70 → 101.43 / 0.96 / 1.05 | 51.40 → 7.86 / 2.76 / 2.59 |
| independent-noise | 1175.95 → 108.97 / 1.94 / 2.12 | 106.45 → 7.46 / 11.23 / 10.27 |
| precompressed-zip | 1159.81 → 189.61 / 1.91 / 2.07 | 109.13 → 11.42 / 11.15 / 10.28 |
| raw-sensors | 1022.73 → 1021.07 / 1.91 / 2.14 | 96.64 → 52.50 / 11.14 / 10.28 |
| similar-files | 1191.64 → 104.20 / 1.94 / 2.07 | 126.17 → 7.55 / 11.18 / 10.28 |
| structured-csv | 430.11 → 407.21 / 1.93 / 2.05 | 51.25 → 33.00 / 5.53 / 5.18 |

## Intel execution selection

The bounded host match-density probe is included in full archive wall time. Dense batches retain the v1 kernel on Intel; sparse batches use the optimized cooperative kernel. Counts below are medians per archive. NVIDIA uses the cooperative kernel without this data probe.

| Dataset | Serial batches | Cooperative batches | Host selection (ms) |
|---|---:|---:|---:|
| canterbury | 1 | 0 | 0.080 |
| enwik8 | 6 | 0 | 0.435 |
| heldout-json | 3 | 0 | 0.181 |
| heldout-mixed | 2 | 1 | 0.140 |
| independent-noise | 0 | 5 | 0.319 |
| precompressed-zip | 1 | 4 | 0.287 |
| raw-sensors | 5 | 0 | 0.313 |
| similar-files | 0 | 5 | 0.328 |
| structured-csv | 4 | 1 | 0.284 |

## Compression ratio

Output as a percentage of source-file bytes. GPU v1 and v2 output hashes match exactly. Keeping v1 ratio is distinct from matching the CPU encoder: GPU output remains larger on these fixtures. No tolerance is used for the strict no-ratio-sacrifice comparison.

| Dataset | CPU LZ4 | GPU LZ4 v1 = v2 | Extra GPU bytes vs CPU LZ4 | CPU Zstd Fast | CPU Zstd Balanced |
|---|---:|---:|---:|---:|---:|
| canterbury | 46.8246% | 50.6328% | 107,040 | 24.5672% | 22.4882% |
| enwik8 | 62.0910% | 67.2371% | 5,146,068 | 40.6753% | 35.4446% |
| heldout-json | 25.0106% | 25.3312% | 107,561 | 12.7729% | 13.3521% |
| heldout-mixed | 55.6993% | 55.8803% | 60,724 | 52.9512% | 53.0319% |
| independent-noise | 100.0248% | 100.0263% | 1,004 | 100.0038% | 100.0039% |
| precompressed-zip | 100.0248% | 100.0248% | 2 | 99.5118% | 99.8167% |
| raw-sensors | 84.8397% | 90.2695% | 3,643,855 | 71.3642% | 53.5217% |
| similar-files | 100.0411% | 100.0467% | 3,776 | 100.0519% | 9.9976% |
| structured-csv | 26.7731% | 27.3265% | 371,390 | 14.8423% | 16.5709% |

## Decision

- intel: kernel speedup **0.83–11.44×**. Warm operation faster than the better 1/8-worker CPU LZ4 median on **0/9** datasets; at least 5% faster on **0/9**; at least 5% faster with no larger archive on **0/9**.
- nvidia: kernel speedup **1.45–16.72×**. Warm operation faster than the better 1/8-worker CPU LZ4 median on **0/9** datasets; at least 5% faster on **0/9**; at least 5% faster with no larger archive on **0/9**.

Auto eligibility requires an end-to-end speed improvement without larger output. Preserving the previous GPU ratio alone does not establish that. CPU remains the conservative default when this joint condition is unmet. The original TAR.ZST Auto policy is unchanged.

Warm-session benefits apply to repeated jobs in the desktop process, library callers, and `benchmark-session`. Separate `lttl create` invocations each start a new process and cannot share this cache. No daemon or background service was installed.

V1 executable: `D47D944BD5201B170AA60F2FAF06C3499B9EF6209B872FB5044ED20482BEC992`. V2 executable: `E3D8D056306678959F07BA36B19EE41A5E630B201A942E230B4D43D3A6FB3EDE`.
Verified extracted files: **3,320**.
