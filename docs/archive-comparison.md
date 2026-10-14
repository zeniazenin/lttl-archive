# Production archive comparison

Production single-file archives, serial seeded randomized jobs, three repetitions, eight requested CPU workers (ZIP serial). Whole-process create/extract times plus common output fsync; includes startup, sampling, checksums and IO. RAR 3/5 with 32 MiB dictionary; ZIP Deflate 1/6. No GPU. OS cache, thermals, placement and background load uncontrolled. Development corpus, not a holdout or exhaustive tuning. Original numeric representations, no conversion. Exact SHA256 recovery outside timing.

Median of three runs; cells show **creation ms / output as % of input**. Lower is better for both. All 90 archive/extraction pairs passed exact recovery.

| Input | lttl Fast | lttl Dense | ZIP level 1 | ZIP level 6 | RAR method 3 | RAR method 5 |
|---|---:|---:|---:|---:|---:|---:|
| mitbih-original-212 | 97.0 / 38.08% | 98.9 / 32.10% | 134.2 / 66.00% | 219.8 / 59.07% | 216.7 / 34.67% | 267.3 / 34.58% |
| eeg-edf-original | 110.1 / 44.65% | 113.5 / 38.77% | 178.0 / 63.53% | 747.9 / 55.03% | 377.5 / 45.24% | 502.5 / 44.89% |
| terrain-hgt-be | 116.0 / 25.58% | 129.7 / 21.87% | 240.1 / 48.29% | 680.4 / 38.88% | 802.1 / 23.03% | 1248.0 / 22.62% |
| raw-sensors-16MiB | 114.8 / 26.69% | 120.6 / 24.92% | 213.4 / 72.02% | 593.6 / 54.89% | 980.9 / 35.50% | 1420.5 / 35.56% |
| enwik8-16MiB | 183.4 / 35.75% | 166.8 / 35.75% | 220.0 / 48.98% | 779.6 / 36.95% | 762.1 / 29.60% | 861.0 / 28.91% |

Complete commands, versions, binary/input hashes and all creation/extraction times: [raw report](archive-comparison.json).
Input attribution and original versus converted representations: [corpus manifest](../research/numeric-v2/corpus.json).
lttl Dense combines independently implemented numeric layout detection/prediction with Zstandard; these measurements are CPU results.
Results are workload-specific. Three samples do not establish statistical significance or performance on arbitrary files.
