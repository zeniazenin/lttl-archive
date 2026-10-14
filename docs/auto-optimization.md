# Auto sampling optimization — same-run results

Policy version 2 removes all Fast-mode sampling, reduces Balanced regions to 64 KiB, skips codec probes when repeated prefixes already determine the choice, and reuses CPU probe context/output buffers. Small retains version 1's sample sizes and GPU decision thresholds. Raw-storage region checks now run only when aggregate compression indicates that storage might be useful, and stop at the first compressible region. See [current policy](auto-compression.md).

**18/18 Balanced/Small dataset-goal combinations retained exactly the same archive byte count as original Auto.** The table below includes every tested dataset, including regressions. File content was hash-verified after extraction; byte-count equality is not a claim that archive headers are byte-identical.

Completed 324 creation/integrity checks, 324 timed extractions, and 2,988 matching file hashes. 4 fresh-process repetitions per case; medians below. All three versions were interleaved within each repetition on the same source hashes, with rotated order and reversed order on even repetitions. Original Auto was preserved before rebuilding, and both binaries used the same adjacent NVIDIA DLLs.

RTX 4070 Laptop GPU, Core Ultra 9 185H, Windows x64, eight requested CPU workers. Full process times include file reads/writes, selection, any GPU trials/initialization/transfers, checksums, and publication. Extraction includes staging/copy publication; verification and SHA256 time are outside the timed creation/extraction. Warm filesystem cache, no controlled thermal/power/background-load isolation. Small differences and overlapping ranges do not prove a speed advantage. The corpus is reused for this optimization, not a new held-out evaluation.

[Raw results](../research/legacy/auto-v2-benchmark-results.json) · [Reproduction script](../scripts/benchmark-auto-v2.ps1) · [Original policy measurements](auto-performance.md)

Repetitions 1–3 followed by a separate fourth reversed-order pass, so each Auto version runs first equally often. No observations removed or retimed; raw source paths and hashes retained.

## Fast creation

Negative time change is less elapsed time; size is the percentage of original file bytes retained, including archive overhead. Lower size is better.

| Dataset | CPU ms | Auto v1 ms | Auto v2 ms | v2 vs v1 time | v2 vs CPU time | CPU / v1 / v2 size % | v2 choice |
|---|---:|---:|---:|---:|---:|---:|---|
| canterbury | 116.7 | 114.3 | 113.8 | -0.4% | -2.4% | 24.5672 / 24.5672 / 24.5672 | CPU Fast |
| enwik8 | 213.8 | 245.1 | 211.0 | -13.9% | -1.3% | 40.6753 / 40.6753 / 40.6753 | CPU Fast |
| heldout-json | 133.8 | 140.5 | 137.1 | -2.4% | +2.5% | 12.7729 / 12.7729 / 12.7729 | CPU Fast |
| heldout-mixed | 157.5 | 156.9 | 147.2 | -6.2% | -6.5% | 52.9512 / 52.9512 / 52.9512 | CPU Fast |
| independent-noise | 224.8 | 213.4 | 220.3 | +3.2% | -2.0% | 100.0038 / 100.0057 / 100.0038 | CPU Fast |
| precompressed-zip | 216.2 | 221.8 | 206.2 | -7.0% | -4.6% | 99.5118 / 99.5118 / 99.5118 | CPU Fast |
| raw-sensors | 202.7 | 222.4 | 194.3 | -12.6% | -4.1% | 71.3642 / 71.3642 / 71.3642 | CPU Fast |
| similar-files | 252.0 | 272.9 | 253.7 | -7.0% | +0.7% | 100.0519 / 100.0519 / 100.0519 | CPU Fast |
| structured-csv | 166.6 | 180.6 | 164.3 | -9.0% | -1.4% | 14.8423 / 14.8423 / 14.8423 | CPU Fast |

| Dataset | v1 / v2 selection ms | v1 / v2 sampled KiB | CPU / v1 / v2 extraction ms | CPU create range ms | v2 create range ms |
|---|---:|---:|---:|---:|---:|
| canterbury | 0.000 / 0.000 | 0 / 0 | 174.1 / 172.9 / 181.6 | 113.8–122.0 | 110.9–116.8 |
| enwik8 | 24.096 / 0.001 | 1536 / 0 | 387.4 / 412.9 / 409.8 | 209.7–265.7 | 204.1–235.3 |
| heldout-json | 11.026 / 0.001 | 1536 / 0 | 178.8 / 181.6 / 179.3 | 127.2–139.3 | 135.3–145.1 |
| heldout-mixed | 9.819 / 0.001 | 1536 / 0 | 165.4 / 174.2 / 180.4 | 144.9–186.1 | 135.0–158.1 |
| independent-noise | 9.127 / 0.001 | 1536 / 0 | 222.3 / 226.6 / 222.4 | 201.4–246.1 | 204.6–227.9 |
| precompressed-zip | 10.467 / 0.000 | 1536 / 0 | 247.5 / 242.4 / 246.3 | 207.1–226.8 | 193.6–216.5 |
| raw-sensors | 23.322 / 0.001 | 1536 / 0 | 319.6 / 292.7 / 284.0 | 196.0–217.0 | 187.7–212.9 |
| similar-files | 15.318 / 0.001 | 1536 / 0 | 630.2 / 659.2 / 611.1 | 246.4–293.9 | 251.3–268.4 |
| structured-csv | 12.875 / 0.001 | 1536 / 0 | 281.3 / 273.6 / 270.1 | 154.3–173.9 | 157.7–170.7 |

## Balanced creation

Negative time change is less elapsed time; size is the percentage of original file bytes retained, including archive overhead. Lower size is better.

| Dataset | CPU ms | Auto v1 ms | Auto v2 ms | v2 vs v1 time | v2 vs CPU time | CPU / v1 / v2 size % | v2 choice |
|---|---:|---:|---:|---:|---:|---:|---|
| canterbury | 123.6 | 117.2 | 116.4 | -0.7% | -5.8% | 22.4882 / 22.4882 / 22.4882 | CPU Balanced |
| enwik8 | 273.2 | 317.7 | 269.8 | -15.1% | -1.2% | 35.4446 / 35.4446 / 35.4446 | CPU Balanced |
| heldout-json | 148.9 | 139.4 | 131.2 | -5.9% | -11.9% | 13.3521 / 12.7729 / 12.7729 | CPU Fast |
| heldout-mixed | 144.9 | 159.5 | 145.0 | -9.1% | +0.0% | 53.0319 / 52.9512 / 52.9512 | CPU Fast |
| independent-noise | 222.6 | 205.3 | 192.4 | -6.3% | -13.6% | 100.0039 / 100.0057 / 100.0057 | CPU stored |
| precompressed-zip | 220.3 | 215.5 | 226.5 | +5.1% | +2.8% | 99.8167 / 99.5118 / 99.5118 | CPU Fast |
| raw-sensors | 245.1 | 278.5 | 260.0 | -6.6% | +6.1% | 53.5217 / 53.5217 / 53.5217 | CPU Balanced |
| similar-files | 205.7 | 228.1 | 220.7 | -3.2% | +7.3% | 9.9976 / 1.6736 / 1.6736 | CPU Small |
| structured-csv | 194.3 | 174.4 | 172.3 | -1.2% | -11.3% | 16.5709 / 14.8423 / 14.8423 | CPU Fast |

| Dataset | v1 / v2 selection ms | v1 / v2 sampled KiB | CPU / v1 / v2 extraction ms | CPU create range ms | v2 create range ms |
|---|---:|---:|---:|---:|---:|
| canterbury | 0.000 / 0.000 | 0 / 0 | 180.1 / 174.8 / 189.2 | 119.1–147.2 | 111.5–119.5 |
| enwik8 | 22.717 / 2.853 | 1536 / 192 | 415.3 / 415.2 / 413.7 | 239.8–278.3 | 266.4–309.9 |
| heldout-json | 10.636 / 2.019 | 1536 / 192 | 182.4 / 175.9 / 188.1 | 142.2–152.2 | 122.7–151.4 |
| heldout-mixed | 9.196 / 2.920 | 1536 / 384 | 164.0 / 169.6 / 173.6 | 140.5–188.5 | 135.5–152.9 |
| independent-noise | 9.361 / 1.501 | 1536 / 192 | 225.9 / 229.4 / 233.1 | 213.8–271.2 | 188.7–207.6 |
| precompressed-zip | 10.536 / 2.061 | 1536 / 192 | 238.6 / 249.1 / 239.2 | 210.7–243.3 | 201.4–244.0 |
| raw-sensors | 22.738 / 3.368 | 1536 / 192 | 326.1 / 324.6 / 325.4 | 238.0–269.6 | 242.5–266.9 |
| similar-files | 15.558 / 5.801 | 1536 / 1536 | 618.0 / 626.1 / 609.2 | 196.3–213.2 | 194.3–227.7 |
| structured-csv | 12.957 / 2.199 | 1536 / 192 | 277.0 / 265.0 / 268.1 | 179.9–249.2 | 162.9–181.5 |

## Small creation

Negative time change is less elapsed time; size is the percentage of original file bytes retained, including archive overhead. Lower size is better.

| Dataset | CPU ms | Auto v1 ms | Auto v2 ms | v2 vs v1 time | v2 vs CPU time | CPU / v1 / v2 size % | v2 choice |
|---|---:|---:|---:|---:|---:|---:|---|
| canterbury | 158.3 | 151.7 | 157.4 | +3.8% | -0.6% | 21.1230 / 21.1230 / 21.1230 | CPU Small |
| enwik8 | 915.1 | 945.3 | 987.7 | +4.5% | +7.9% | 31.1280 / 31.1280 / 31.1280 | CPU Small |
| heldout-json | 351.8 | 626.2 | 633.8 | +1.2% | +80.2% | 12.1369 / 11.3530 / 11.3530 | GPU 64 KiB |
| heldout-mixed | 249.9 | 163.5 | 156.8 | -4.1% | -37.3% | 52.8060 / 52.9512 / 52.9512 | CPU Fast |
| independent-noise | 249.8 | 207.1 | 204.5 | -1.2% | -18.1% | 100.0038 / 100.0057 / 100.0057 | CPU stored |
| precompressed-zip | 273.7 | 229.7 | 236.3 | +2.8% | -13.7% | 99.5013 / 99.5118 / 99.5118 | CPU Fast |
| raw-sensors | 1198.4 | 1217.8 | 1193.1 | -2.0% | -0.4% | 42.8593 / 42.8593 / 42.8593 | CPU Small |
| similar-files | 200.6 | 234.5 | 220.2 | -6.1% | +9.8% | 1.6736 / 1.6736 / 1.6736 | CPU Small |
| structured-csv | 548.1 | 429.3 | 439.1 | +2.3% | -19.9% | 13.6842 / 13.0791 / 13.0791 | GPU 64 KiB |

| Dataset | v1 / v2 selection ms | v1 / v2 sampled KiB | CPU / v1 / v2 extraction ms | CPU create range ms | v2 create range ms |
|---|---:|---:|---:|---:|---:|
| canterbury | 0.001 / 0.001 | 0 / 0 | 173.1 / 177.8 / 173.2 | 156.7–162.6 | 144.0–165.1 |
| enwik8 | 59.733 / 48.461 | 1536 / 1536 | 443.7 / 438.8 / 443.8 | 835.7–992.9 | 934.3–1034.6 |
| heldout-json | 438.070 / 439.201 | 1536 / 1536 | 181.7 / 183.9 / 176.7 | 312.8–489.2 | 324.9–2122.1 |
| heldout-mixed | 17.155 / 14.117 | 1536 / 1536 | 173.3 / 175.1 / 169.3 | 238.5–255.9 | 142.4–166.0 |
| independent-noise | 8.301 / 5.152 | 1536 / 1536 | 228.6 / 231.3 / 224.9 | 225.5–255.8 | 185.0–212.6 |
| precompressed-zip | 15.994 / 11.261 | 1536 / 1536 | 244.2 / 243.0 / 236.6 | 257.3–288.4 | 212.2–252.1 |
| raw-sensors | 63.974 / 53.524 | 1536 / 1536 | 302.6 / 309.9 / 305.4 | 1121.2–1224.7 | 1127.0–1414.7 |
| similar-files | 16.318 / 6.382 | 1536 / 1536 | 604.4 / 622.6 / 615.4 | 197.0–228.8 | 211.7–251.2 |
| structured-csv | 166.659 / 160.307 | 1536 / 1536 | 266.3 / 250.4 / 253.3 | 534.3–630.1 | 378.3–2222.3 |

## Interpretation

Fast selection fell from 9.1–24.1 ms on sampled inputs to about 0.001 ms. Its full creation medians ranged from 6.5% faster to 2.5% slower than explicit CPU Fast, with overlapping observed ranges; this supports removal of the sampling penalty, not a claim that Auto's identical encoder is inherently faster. Relative to original Auto, Fast enwik8 improved 13.9%, raw sensors 12.6%, and CSV 9.0%. Noise was 3.2% slower than original Auto but 2.0% faster than CPU Fast; the old raw-storage shortcut is gone.

Balanced selection fell 62.7–87.4% on the eight sampled datasets. CSV kept its 10.4% archive-byte saving over CPU Balanced and took 11.3% less time; JSON kept its 4.3% saving and took 11.9% less time. enwik8 matched CPU Balanced's size and was within 1.2% of its median time. However, raw sensors remained 6.1% slower, precompressed ZIP 2.8% slower, and related files 7.3% slower (while retaining the 83.3% archive-byte saving). Reduced selection cost does not guarantee a full-run speedup in noisy measurements.

Small retained all output sizes but not a universal timing improvement: enwik8 was 4.5% slower than original Auto and 7.9% slower than CPU Small in this comparison. GPU JSON had roughly 80% greater median latency than CPU Small while saving 6.5% archive bytes; initialization varied from about 0.1 to 1.9 seconds across GPU observations. These startup spikes affected both Auto versions and are not removed from the results. Small continues to favor size rather than promise low latency.

Fast now executes the same CPU encoder/settings as explicit CPU Fast with no content reads or compression trials before encoding. No-sampling does not guarantee identical measured wall time: process launch, cache state, clocks and background work still vary. Version 1's raw-storage alternative for Fast is removed; on noise, the normal CPU frame was slightly smaller in this corpus. Balanced/Small retain raw-storage selection.

Balanced screening still has a nonzero cost. A smaller sample can miss content that a larger one would find, and preserving decisions on this corpus is not proof of equal decisions on other files. Related-file routing retains 64 KiB prefix fingerprints and shared CPU history. Small retains expensive size/GPU trials where those can preserve its compression benefit; it is not a promise of CPU-like latency.

The largest Auto GPU startup observations are retained in the ranges and raw report. Fresh process does not imply a driver-cold GPU each time. CUDA trial and archive event totals are listed below; both are included in wall time and must not be added to overlapping host timings.

| Dataset / version | GPU H2D ms | GPU compression ms | GPU D2H ms | Initialization ms |
|---|---:|---:|---:|---:|
| heldout-json / auto-v1 | 3.06 | 30.56 | 4.25 | 386.08 |
| heldout-json / auto | 3.16 | 30.68 | 4.41 | 393.98 |
| structured-csv / auto-v1 | 5.82 | 45.73 | 7.44 | 112.96 |
| structured-csv / auto | 6.12 | 45.59 | 8.41 | 110.31 |

## Reproduce

Preserve the policy-v1 release as `target/release/lttl-auto-v1.exe` before rebuilding version 2. Both use the existing `target/release/gpu` runtime folder. Run:

```powershell
pwsh -NoProfile -File scripts/benchmark-auto-v2.ps1
```

The harness validates the retained `bench-output/auto-corpus/manifest.json` and every source hash. Copy the completed run's `results.json` to `research/legacy/auto-v2-benchmark-results.json`, then regenerate this report with `python scripts/summarize-auto-v2.py`. No sources are downloaded or changed by the comparison.

Original Auto executable SHA256: `59887D30DFDB846828EAA29E22387B49B3D86D0E356793B93749FB2904F5A929`.

Optimized executable SHA256: `BFA3ECC1F79BD550D7C9091D591A08819717FCABACCD538377CACAAE19CC2A5B`.
