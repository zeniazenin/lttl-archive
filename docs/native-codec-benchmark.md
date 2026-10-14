# Production lttl codec benchmark

Production LTTLAR01 container, each original file separately; eight CPU workers, serial randomized jobs. All creation timers include sampling, input/output and publication; process_ms includes executable startup. Recovered SHA256 checked outside timing. OS cache, thermal state, background activity uncontrolled. Auto uses bounded samples and is not guaranteed optimal. These are development-corpus results.

336 creation/extraction pairs passed exact SHA-256 recovery checks. Medians of three runs. Creation times below include sampling and file IO inside the app; process startup is excluded from these columns. Raw results also contain complete process timings and extraction timings. Lower output/input is smaller.

| Input | CPU Fast ms / % | Auto Fast ms / % | CPU Balanced ms / % | Auto Balanced ms / % | lttl Fast ms / % | lttl Dense ms / % | CPU Small ms / % | Auto Small ms / % |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| mitbih-original-212 | 26.2 / 68.05% | 19.9 / 38.08% | 41.6 / 58.95% | 23.6 / 32.10% | 16.8 / 38.08% | 20.0 / 32.10% | 100.5 / 52.10% | 34.4 / 32.10% |
| mitbih-s16le | 28.1 / 50.41% | 18.3 / 28.56% | 52.6 / 45.16% | 23.1 / 24.07% | 17.2 / 28.56% | 19.5 / 24.07% | 155.9 / 40.54% | 33.0 / 24.07% |
| eeg-edf-original | 36.5 / 62.00% | 28.0 / 44.65% | 81.8 / 53.66% | 28.7 / 38.77% | 24.9 / 44.65% | 28.5 / 38.77% | 313.6 / 50.87% | 37.6 / 38.77% |
| ptb-12lead-original | 12.0 / 76.88% | 12.6 / 76.88% | 14.5 / 76.59% | 18.2 / 40.52% | 13.8 / 44.54% | 16.4 / 40.52% | 25.4 / 75.82% | 34.5 / 40.52% |
| terrain-hgt-be | 65.1 / 44.80% | 46.2 / 25.58% | 140.7 / 37.35% | 57.3 / 21.87% | 45.6 / 25.58% | 53.7 / 21.87% | 624.8 / 32.42% | 78.0 / 21.87% |
| terrain-s16le | 77.1 / 44.78% | 44.8 / 25.58% | 132.7 / 37.32% | 55.7 / 21.87% | 43.8 / 25.58% | 52.0 / 21.87% | 546.3 / 32.41% | 84.3 / 21.87% |
| raw-sensors-1MiB | 17.2 / 71.48% | 15.1 / 26.67% | 21.5 / 54.12% | 20.9 / 24.94% | 13.6 / 26.67% | 15.3 / 24.94% | 38.9 / 47.30% | 33.4 / 24.94% |
| raw-sensors-16MiB | 59.0 / 71.40% | 32.0 / 26.69% | 106.0 / 53.46% | 47.6 / 24.92% | 29.7 / 26.69% | 44.8 / 24.92% | 585.8 / 43.11% | 57.8 / 24.92% |
| raw-sensors-64MiB | 186.7 / 71.41% | 105.7 / 26.69% | 356.0 / 53.50% | 148.6 / 24.92% | 96.6 / 26.69% | 141.8 / 24.92% | 2310.2 / 43.11% | 205.0 / 24.92% |
| independent-noise-16MiB | 61.6 / 100.00% | 66.4 / 100.00% | 57.5 / 100.00% | 60.0 / 100.00% | 42.7 / 100.00% | 38.0 / 100.00% | 72.5 / 100.00% | 75.1 / 100.00% |
| enwik8-16MiB | 46.7 / 41.14% | 44.8 / 41.14% | 86.0 / 35.75% | 79.4 / 35.75% | 70.1 / 35.75% | 81.9 / 35.75% | 478.0 / 31.40% | 461.9 / 31.40% |
| synthetic-u32be-12ch | 22.6 / 79.84% | 15.7 / 13.30% | 25.3 / 79.92% | 18.5 / 8.93% | 15.2 / 13.30% | 15.3 / 8.93% | 52.4 / 78.33% | 29.3 / 8.93% |
| synthetic-u64le-3ch | 18.6 / 18.09% | 15.2 / 8.06% | 21.0 / 18.40% | 17.9 / 4.93% | 13.1 / 8.06% | 15.4 / 4.93% | 54.5 / 19.05% | 27.9 / 4.93% |
| synthetic-f64be-3ch | 21.3 / 30.65% | 21.3 / 30.65% | 24.8 / 34.73% | 18.1 / 6.81% | 24.4 / 34.73% | 14.1 / 6.81% | 70.1 / 17.04% | 31.6 / 6.81% |

## Observed Auto decisions

| Input / goal | Block choices across three runs |
|---|---|
| mitbih-original-212 / auto-fast | {"lttl-fast": 1}; {"zstd": 1} |
| mitbih-original-212 / auto-balanced | {"lttl-dense": 1} |
| mitbih-original-212 / auto-small | {"lttl-dense": 1} |
| mitbih-s16le / auto-fast | {"lttl-fast": 1} |
| mitbih-s16le / auto-balanced | {"lttl-dense": 1} |
| mitbih-s16le / auto-small | {"lttl-dense": 1} |
| eeg-edf-original / auto-fast | {"lttl-fast": 1} |
| eeg-edf-original / auto-balanced | {"lttl-dense": 1} |
| eeg-edf-original / auto-small | {"lttl-dense": 1} |
| ptb-12lead-original / auto-fast | {"zstd": 1} |
| ptb-12lead-original / auto-balanced | {"lttl-dense": 1} |
| ptb-12lead-original / auto-small | {"lttl-dense": 1} |
| terrain-hgt-be / auto-fast | {"lttl-fast": 2} |
| terrain-hgt-be / auto-balanced | {"lttl-dense": 2} |
| terrain-hgt-be / auto-small | {"lttl-dense": 2} |
| terrain-s16le / auto-fast | {"lttl-fast": 2} |
| terrain-s16le / auto-balanced | {"lttl-dense": 2} |
| terrain-s16le / auto-small | {"lttl-dense": 2} |
| raw-sensors-1MiB / auto-fast | {"lttl-fast": 1} |
| raw-sensors-1MiB / auto-balanced | {"lttl-dense": 1} |
| raw-sensors-1MiB / auto-small | {"lttl-dense": 1} |
| raw-sensors-16MiB / auto-fast | {"lttl-fast": 1, "zstd": 1} |
| raw-sensors-16MiB / auto-balanced | {"lttl-dense": 1, "zstd": 1} |
| raw-sensors-16MiB / auto-small | {"lttl-dense": 1, "zstd": 1} |
| raw-sensors-64MiB / auto-fast | {"lttl-fast": 4, "zstd": 1} |
| raw-sensors-64MiB / auto-balanced | {"lttl-dense": 4, "zstd": 1} |
| raw-sensors-64MiB / auto-small | {"lttl-dense": 4, "zstd": 1} |
| independent-noise-16MiB / auto-fast | {"stored": 2} |
| independent-noise-16MiB / auto-balanced | {"stored": 2} |
| independent-noise-16MiB / auto-small | {"stored": 2} |
| enwik8-16MiB / auto-fast | {"zstd": 2} |
| enwik8-16MiB / auto-balanced | {"zstd": 2} |
| enwik8-16MiB / auto-small | {"zstd": 2} |
| synthetic-u32be-12ch / auto-fast | {"lttl-fast": 1} |
| synthetic-u32be-12ch / auto-balanced | {"lttl-dense": 1} |
| synthetic-u32be-12ch / auto-small | {"lttl-dense": 1} |
| synthetic-u64le-3ch / auto-fast | {"lttl-fast": 1} |
| synthetic-u64le-3ch / auto-balanced | {"lttl-dense": 1} |
| synthetic-u64le-3ch / auto-small | {"lttl-dense": 1} |
| synthetic-f64be-3ch / auto-fast | {"zstd": 1} |
| synthetic-f64be-3ch / auto-balanced | {"lttl-dense": 1} |
| synthetic-f64be-3ch / auto-small | {"lttl-dense": 1} |

Auto speed decisions use timed probes and can vary with load. Three runs do not establish statistical significance. Small inputs/text skip numeric trials; block resets can lose dictionary history compared with TAR.ZST. Prior prototype ratios/timings should not be substituted for these production-container measurements.
