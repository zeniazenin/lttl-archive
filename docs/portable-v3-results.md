# GPU transfer and pipeline results — 2026-10-12

All **468 archive creation/extraction pairs** passed, including **4,316 extracted-file SHA-256 checks**. GPU archive hashes are identical across the previous implementation, all four transfer modes, both GPUs, and cold/warm jobs.

[Implementation and reproduction](portable-transfers.md) · [Raw measurements](portable-v3-results.json)

## Complete archive creation

Seconds; medians of three warm jobs following a cold first job in each process. Includes host staging, GPU transfers, compression, checksums, framing, file I/O, synchronization and publication. Excludes CLI process startup. CPU uses the same boundary. Faster CPU LZ4 is the better of one/eight workers. V2 controls were rerun in this experiment, not copied from the previous report.

| Dataset | CPU LZ4 | CPU Zstd Fast | Intel v2 | Intel compact pipeline | RTX v2 | RTX compact pipeline |
|---|---:|---:|---:|---:|---:|---:|
| canterbury | 0.0199 | 0.0221 | 0.0503 | 0.0482 | 0.0258 | 0.0205 |
| enwik8 | 0.2643 | 0.1233 | 1.1547 | 1.0791 | 0.2881 | 0.1977 |
| heldout-json | 0.0694 | 0.0374 | 0.2234 | 0.2473 | 0.0712 | 0.0608 |
| heldout-mixed | 0.0691 | 0.0538 | 0.1719 | 0.1487 | 0.0782 | 0.0696 |
| independent-noise | 0.1637 | 0.1188 | 0.2866 | 0.2378 | 0.1897 | 0.1581 |
| precompressed-zip | 0.1683 | 0.1109 | 0.3344 | 0.2768 | 0.1845 | 0.1551 |
| raw-sensors | 0.1766 | 0.1124 | 1.2445 | 0.9796 | 0.2190 | 0.1680 |
| similar-files | 0.1934 | 0.1459 | 0.2985 | 0.2264 | 0.2170 | 0.1734 |
| structured-csv | 0.1279 | 0.0670 | 0.4762 | 0.4154 | 0.1460 | 0.1043 |

## Isolating compaction and pipelining

All four v3 modes write directly from mapped readback memory, avoiding per-block host allocations. Fixed modes transfer complete output slots. Compact modes pack only useful compressed blocks; raw blocks use retained host bytes. Pipeline modes allow two pending batches; serial modes finish each batch before accepting the next. Values are warm archive seconds, not kernel-only times.

| Dataset | GPU | Fixed serial | Fixed pipeline | Compact serial | Compact pipeline |
|---|---|---:|---:|---:|---:|
| canterbury | intel | 0.0477 | 0.0477 | 0.0469 | 0.0482 |
| canterbury | nvidia | 0.0212 | 0.0222 | 0.0228 | 0.0205 |
| enwik8 | intel | 1.3099 | 1.1857 | 1.4362 | 1.0791 |
| enwik8 | nvidia | 0.2670 | 0.1915 | 0.2600 | 0.1977 |
| heldout-json | intel | 0.2378 | 0.2406 | 0.2319 | 0.2473 |
| heldout-json | nvidia | 0.0850 | 0.0586 | 0.0771 | 0.0608 |
| heldout-mixed | intel | 0.1806 | 0.1528 | 0.1705 | 0.1487 |
| heldout-mixed | nvidia | 0.0779 | 0.0703 | 0.0786 | 0.0696 |
| independent-noise | intel | 0.2844 | 0.1742 | 0.2387 | 0.2378 |
| independent-noise | nvidia | 0.1650 | 0.1481 | 0.1620 | 0.1581 |
| precompressed-zip | intel | 0.3079 | 0.2336 | 0.3090 | 0.2768 |
| precompressed-zip | nvidia | 0.1732 | 0.1508 | 0.1632 | 0.1551 |
| raw-sensors | intel | 1.1738 | 1.1024 | 1.0255 | 0.9796 |
| raw-sensors | nvidia | 0.1994 | 0.1575 | 0.2151 | 0.1680 |
| similar-files | intel | 0.2753 | 0.2162 | 0.2981 | 0.2264 |
| similar-files | nvidia | 0.2016 | 0.1785 | 0.1832 | 0.1734 |
| structured-csv | intel | 0.4604 | 0.4419 | 0.4598 | 0.4154 |
| structured-csv | nvidia | 0.1471 | 0.1186 | 0.1420 | 0.1043 |

## Transfer volume and GPU timing

Transfer bytes include block-size metadata and padded compressed payload, excluding timing instrumentation (48 bytes per batch in v3, 32 in v2). Incompressible blocks need no payload readback. Compaction, upload and payload-copy timestamps exclude host mapping/waits and metadata-copy time; complete archive times include them. Readback wait is host blocking time, which may include queued compression. Two pending batches establishes bounded pipelining, not proof of simultaneous GPU copy-engine and shader execution.

| Dataset | GPU | V2 readback MiB | V3 readback MiB | Reduction | Upload ms | Compression ms | Compact ms | Payload copy ms | Host wait ms | Pending peak |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| canterbury | intel | 2.720 | 1.357 | 50.085 | 0.063 | 26.583 | 0.083 | 0.040 | 28.544 | 1.000 |
| canterbury | nvidia | 2.720 | 1.357 | 50.085 | 0.244 | 2.734 | 0.054 | 0.117 | 3.395 | 1.000 |
| enwik8 | intel | 95.957 | 64.131 | 33.167 | 2.863 | 939.569 | 3.853 | 1.967 | 885.100 | 2.000 |
| enwik8 | nvidia | 95.957 | 64.131 | 33.167 | 8.243 | 65.060 | 0.825 | 5.629 | 20.926 | 2.000 |
| heldout-json | intel | 32.211 | 8.109 | 74.825 | 0.967 | 202.795 | 0.684 | 0.215 | 187.867 | 2.000 |
| heldout-json | nvidia | 32.211 | 8.109 | 74.825 | 2.783 | 16.271 | 0.268 | 0.713 | 5.852 | 2.000 |
| heldout-mixed | intel | 32.211 | 1.899 | 94.105 | 0.976 | 91.899 | 0.386 | 0.057 | 76.533 | 2.000 |
| heldout-mixed | nvidia | 32.211 | 1.899 | 94.105 | 2.782 | 7.868 | 0.257 | 0.196 | 2.843 | 2.000 |
| independent-noise | intel | 64.406 | 0.030 | 99.953 | 1.979 | 118.844 | 0.856 | 0.009 | 71.410 | 2.000 |
| independent-noise | nvidia | 64.406 | 0.030 | 99.953 | 6.967 | 7.397 | 0.453 | 0.023 | 0.510 | 2.000 |
| precompressed-zip | intel | 64.406 | 0.031 | 99.951 | 1.918 | 162.402 | 0.632 | 0.028 | 124.203 | 2.000 |
| precompressed-zip | nvidia | 64.406 | 0.031 | 99.951 | 11.201 | 11.419 | 0.462 | 0.030 | 0.509 | 2.000 |
| raw-sensors | intel | 64.406 | 57.778 | 10.291 | 1.965 | 858.935 | 2.897 | 1.742 | 828.420 | 2.000 |
| raw-sensors | nvidia | 64.406 | 57.778 | 10.291 | 5.555 | 52.390 | 0.816 | 4.791 | 17.784 | 2.000 |
| similar-files | intel | 64.438 | 0.811 | 98.741 | 1.925 | 102.462 | 0.781 | 0.054 | 39.450 | 2.000 |
| similar-files | nvidia | 64.438 | 0.811 | 98.741 | 11.227 | 7.539 | 0.514 | 0.176 | 0.642 | 2.000 |
| structured-csv | intel | 64.406 | 17.495 | 72.837 | 1.917 | 347.222 | 1.265 | 0.446 | 305.351 | 2.000 |
| structured-csv | nvidia | 64.406 | 17.495 | 72.837 | 5.567 | 33.223 | 0.542 | 1.528 | 6.398 | 2.000 |

## Compression ratio and extraction

Archive size as percentage of source file bytes (including archive framing/metadata overhead). GPU ratio is unchanged from v2, but that does not imply parity with CPU. Extraction uses the same CPU decoder; values are warm medians in seconds for the default compact pipeline archives.

| Dataset | CPU LZ4 | GPU LZ4 | Extra GPU bytes | CPU Zstd Fast | Extract Intel archive | Extract RTX archive |
|---|---:|---:|---:|---:|---:|---:|
| canterbury | 46.8246% | 50.6328% | 107,040 | 24.5672% | 0.0840 | 0.0823 |
| enwik8 | 62.0910% | 67.2371% | 5,146,068 | 40.6753% | 0.3167 | 0.3212 |
| heldout-json | 25.0106% | 25.3312% | 107,561 | 12.7729% | 0.1266 | 0.1173 |
| heldout-mixed | 55.6993% | 55.8803% | 60,724 | 52.9512% | 0.1005 | 0.0825 |
| independent-noise | 100.0248% | 100.0263% | 1,004 | 100.0038% | 0.1675 | 0.1445 |
| precompressed-zip | 100.0248% | 100.0248% | 2 | 99.5118% | 0.1361 | 0.1501 |
| raw-sensors | 84.8397% | 90.2695% | 3,643,855 | 71.3642% | 0.1904 | 0.2481 |
| similar-files | 100.0411% | 100.0467% | 3,776 | 100.0519% | 0.5252 | 0.5255 |
| structured-csv | 26.7731% | 27.3265% | 371,390 | 14.8423% | 0.1476 | 0.1982 |

## Cold first jobs

One first-job sample per configuration; these are descriptive, not stable cold-start estimates. Session caching applies only within the same process. OS/shader caches were not flushed.

| Dataset | Intel v2 | Intel v3 | RTX v2 | RTX v3 |
|---|---:|---:|---:|---:|
| canterbury | 0.6377 | 0.6710 | 0.3155 | 0.2391 |
| enwik8 | 1.6328 | 1.5352 | 0.5033 | 0.4265 |
| heldout-json | 0.7127 | 0.7941 | 0.3018 | 0.3019 |
| heldout-mixed | 0.6579 | 0.6662 | 0.3025 | 0.2808 |
| independent-noise | 1.4986 | 0.7629 | 0.4089 | 0.3768 |
| precompressed-zip | 0.8233 | 0.8154 | 0.4212 | 0.3954 |
| raw-sensors | 1.7874 | 1.5389 | 0.4574 | 0.3734 |
| similar-files | 0.7971 | 0.7333 | 0.4234 | 0.4366 |
| structured-csv | 1.1259 | 0.9241 | 0.4111 | 0.3412 |

## Decision

- intel: complete warm archive speedup over v2 **0.90–1.32×**; faster than CPU LZ4 on **0/9** datasets, at least 5% faster on **0/9**, and at least 5% faster with no larger archive on **0/9**.
- nvidia: complete warm archive speedup over v2 **1.12–1.46×**; faster than CPU LZ4 on **7/9** datasets, at least 5% faster on **5/9**, and at least 5% faster with no larger archive on **0/9**.

The strict joint criterion uses no archive-size tolerance. Auto remains on CPU if that criterion is unmet. Laptop power/thermal state and background activity affect small differences; groups were sequential and randomized, with only three warm observations per group. Raw samples are retained. AMD/Apple hardware was not tested.

V2 executable SHA-256: `7E1BBB60DDA8E06A136EEAB19103A1047C6066940339053A4F7BB5CB4D4164E6`. V3: `EE8521BCFE1FE99CB9A81F091F6625997D34221727AAB2CFE0C265EB1B0FD1FC`.
