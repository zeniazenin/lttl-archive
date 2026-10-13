# Auto compression measurements — 2026-10-12

Policy version 1 on NVIDIA RTX 4070 Laptop GPU (8 GiB, driver 592.82), Core Ultra 9 185H (22 logical processors), Windows x64. CPU comparisons explicitly requested eight Zstandard workers. Three repetitions per configuration; medians below. Fresh processes include selection, GPU trials/initialization/transfers, checksums, reads/writes/sync/publication, and process launch/exit. Input/output cache is warm; no thermal, power, cold-cache, or background-load isolation. Small timing differences are not statistical proof of improvement.

Completed **189 creation/integrity checks, 189 timed extractions, and 1,743 matching extracted SHA256 hashes**. Verification and hash time are outside creation/extraction timing. Extraction includes lttl's staging/copy publication. Negative time/byte change is an improvement. Size is archive bytes as a percentage of original file bytes, including framing and checksums; a lower percentage is better.

The previous CLI default was CPU Balanced. Compare Auto Balanced with that baseline first. Small and Fast comparisons use the corresponding explicit CPU preset, so goals are not conflated. The original GPU default uses a fixed nvCOMP setting with 1 MiB chunks / 32 MiB batches.

[Selection policy and limitations](auto-compression.md) · [Raw results](auto-benchmark-results.json) · [Harness](../scripts/benchmark-auto.ps1)

## Balanced: Auto versus explicit CPU Balanced

| Dataset | Auto choice | CPU / Auto create ms | Time change | CPU / Auto size % | Archive byte change | CPU / Auto extract ms | Selection ms |
|---|---|---:|---:|---:|---:|---:|---:|
| canterbury | CPU Balanced | 115.8 / 122.1 | +5.4% | 22.4882 / 22.4882 | +0.000% | 169.3 / 173.1 | 0.00 |
| enwik8 | CPU Balanced | 269.8 / 276.8 | +2.6% | 35.4446 / 35.4446 | +0.000% | 419.0 / 405.4 | 20.44 |
| heldout-json | CPU Fast | 137.2 / 149.9 | +9.3% | 13.3521 / 12.7729 | -4.338% | 167.6 / 180.0 | 11.22 |
| heldout-mixed | CPU Fast | 144.4 / 149.5 | +3.5% | 53.0319 / 52.9512 | -0.152% | 168.1 / 161.7 | 8.70 |
| independent-noise | CPU stored | 222.2 / 203.3 | -8.5% | 100.0039 / 100.0057 | +0.002% | 219.4 / 217.0 | 8.33 |
| precompressed-zip | CPU Fast | 219.4 / 217.1 | -1.0% | 99.8167 / 99.5118 | -0.306% | 234.2 / 239.0 | 10.65 |
| raw-sensors | CPU Balanced | 250.9 / 271.6 | +8.3% | 53.5217 / 53.5217 | +0.000% | 323.6 / 315.1 | 21.72 |
| similar-files | CPU Small | 191.6 / 208.3 | +8.7% | 9.9976 / 1.6736 | -83.260% | 643.5 / 598.0 | 13.65 |
| structured-csv | CPU Fast | 186.5 / 164.2 | -11.9% | 16.5709 / 14.8423 | -10.432% | 240.2 / 250.1 | 11.18 |

## Small: Auto versus explicit CPU Small

| Dataset | Auto choice | CPU / Auto create ms | Time change | CPU / Auto size % | Archive byte change | CPU / Auto extract ms | Selection ms |
|---|---|---:|---:|---:|---:|---:|---:|
| canterbury | CPU Small | 153.0 / 150.4 | -1.7% | 21.1230 / 21.1230 | +0.000% | 171.0 / 173.9 | 0.00 |
| enwik8 | CPU Small | 885.1 / 927.3 | +4.8% | 31.1280 / 31.1280 | +0.000% | 405.8 / 414.0 | 52.99 |
| heldout-json | GPU 64 KiB | 331.4 / 318.8 | -3.8% | 12.1369 / 11.3530 | -6.459% | 180.7 / 183.6 | 139.42 |
| heldout-mixed | CPU Fast | 242.0 / 164.3 | -32.1% | 52.8060 / 52.9512 | +0.275% | 166.2 / 171.4 | 17.32 |
| independent-noise | CPU stored | 222.9 / 199.7 | -10.4% | 100.0038 / 100.0057 | +0.002% | 230.9 / 225.0 | 8.89 |
| precompressed-zip | CPU Fast | 276.5 / 223.4 | -19.2% | 99.5013 / 99.5118 | +0.010% | 243.7 / 234.9 | 15.72 |
| raw-sensors | CPU Small | 1145.1 / 1144.0 | -0.1% | 42.8593 / 42.8593 | +0.000% | 298.4 / 306.2 | 59.34 |
| similar-files | CPU Small | 220.3 / 218.0 | -1.1% | 1.6736 / 1.6736 | +0.000% | 629.3 / 601.4 | 14.00 |
| structured-csv | GPU 64 KiB | 453.1 / 390.4 | -13.8% | 13.6842 / 13.0791 | -4.422% | 237.1 / 255.3 | 148.18 |

## Fast: Auto versus explicit CPU Fast

| Dataset | Auto choice | CPU / Auto create ms | Time change | CPU / Auto size % | Archive byte change | CPU / Auto extract ms | Selection ms |
|---|---|---:|---:|---:|---:|---:|---:|
| canterbury | CPU Fast | 106.1 / 122.5 | +15.4% | 24.5672 / 24.5672 | +0.000% | 166.7 / 166.1 | 0.00 |
| enwik8 | CPU Fast | 209.5 / 235.7 | +12.5% | 40.6753 / 40.6753 | +0.000% | 397.0 / 381.9 | 21.70 |
| heldout-json | CPU Fast | 132.2 / 140.2 | +6.0% | 12.7729 / 12.7729 | +0.000% | 181.6 / 181.5 | 11.62 |
| heldout-mixed | CPU Fast | 149.1 / 150.8 | +1.2% | 52.9512 / 52.9512 | +0.000% | 170.3 / 163.2 | 9.66 |
| independent-noise | CPU stored | 206.9 / 207.1 | +0.1% | 100.0038 / 100.0057 | +0.002% | 215.4 / 212.9 | 8.88 |
| precompressed-zip | CPU Fast | 203.3 / 221.0 | +8.7% | 99.5118 / 99.5118 | +0.000% | 245.4 / 235.4 | 10.60 |
| raw-sensors | CPU Fast | 191.6 / 226.6 | +18.3% | 71.3642 / 71.3642 | +0.000% | 287.8 / 286.9 | 25.45 |
| similar-files | CPU Fast | 260.7 / 282.4 | +8.3% | 100.0519 / 100.0519 | +0.000% | 619.0 / 614.0 | 15.34 |
| structured-csv | CPU Fast | 152.4 / 165.1 | +8.3% | 14.8423 / 14.8423 | +0.000% | 241.6 / 241.3 | 11.60 |

Balanced: CSV improved both time and size; related files became much smaller at an 8.7% median time cost. JSON and mixed content saved space but took longer. enwik8 and raw sensors retained the same encoder and paid sampling overhead. Canterbury skips sampling; its small time changes reflect process/system variation. Fast showed no reliable speed benefit and mostly regressed: explicit CPU Fast avoids this selection work. Small accepted slightly larger mixed/precompressed outputs in exchange for less CPU search. Stored noise has slightly higher framing overhead.

## Actual GPU selections versus the original GPU defaults

| Dataset | Original GPU / Auto Small create ms | Time change | Original GPU / Auto size % | Archive byte change |
|---|---:|---:|---:|---:|
| heldout-json | 371.3 / 318.8 | -14.1% | 11.7029 / 11.3530 | -2.99% |
| structured-csv | 474.6 / 390.4 | -17.7% | 13.4097 / 13.0791 | -2.47% |

GPU trial and archive event costs below are combined per run, then their medians are taken. These intervals overlap native host-call timing; they must not be added to total wall time. Initialization is included once, under selection. These values do not imply a warm-context-only benchmark.

| Dataset | H2D ms | GPU compression ms | D2H ms | Initialization ms | CPU Small create range ms | Auto Small create range ms |
|---|---:|---:|---:|---:|---:|---:|
| heldout-json | 3.01 | 23.28 | 3.84 | 92.34 | 323.6–359.9 | 313.5–332.8 |
| structured-csv | 5.89 | 45.69 | 7.45 | 98.38 | 421.4–453.7 | 377.0–398.9 |

CSV Auto Small was 13.8% faster and 4.42% smaller than CPU Small in this run, with nonoverlapping observed creation ranges. JSON Auto Small was 6.46% smaller; its 3.8% median time improvement had overlapping ranges and is not a reliable demonstrated speed win. CPU Fast/Balanced remained faster than either GPU path for these datasets. A separate first CSV pilot took 2.068 seconds including 1.885 seconds initialization; this matrix is not a driver-cold startup study.

## Corpus and provenance

| Dataset | Input bytes | Files |
|---|---:|---:|
| canterbury | 2,810,784 | 11 |
| enwik8 | 100,000,000 | 1 |
| heldout-json | 33,554,432 | 1 |
| heldout-mixed | 33,554,432 | 2 |
| independent-noise | 67,108,864 | 1 |
| precompressed-zip | 67,108,864 | 1 |
| raw-sensors | 67,108,864 | 1 |
| similar-files | 67,108,864 | 64 |
| structured-csv | 67,108,864 | 1 |

Canterbury and enwik8 are downloaded public corpora with provenance in [downloaded-file results](downloaded-performance.md). Five synthetic datasets were retained from the earlier GPU baseline. The new heldout JSON and mixed fixtures were generated after policy thresholds were frozen; misleading `.jpg`, `.csv`, and `.bin` extensions exercise byte-based selection. These are bounded experiments, not a representative claim about all file types. Full hashes, arguments, repeat-level selection reports, and component timings are retained in the raw JSON.

Benchmark executable SHA256: `A23BF354EF63084D93A8FDFBF1F90A563D4AC15A92B8AF09BA901315A0F31B20`. The final build subsequently changed CLI help text only; its hash is recorded in [validation](validation.md). Generate these tables with `python scripts/summarize-auto.py`. Historical benchmark reports retain their own executable hashes.
