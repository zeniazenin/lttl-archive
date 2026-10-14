# GPU numeric benchmark results

Measured on the Intel Core Ultra 9 185H host. See [methodology, implementation and data attribution](README.md), [raw results](results.json), and [input provenance](corpus.json).

All **462 benchmark creation/recovery pairs** passed restored-file SHA-256 checks. All numeric CPU/GPU archive hashes are identical per dataset. Both GPUs also passed 316 additional edge-case pairs each, decoded by the original CPU decoder. The three original Rust framing/round-trip/corruption tests passed.

## Devices and session cost

| Device | Driver | Backend | Initialization median ms | Initialization range ms |
|---|---|---|---:|---:|
| Intel(R) Arc(TM) Graphics | 101.8860 | Vulkan | 411.07 | 371.49–1031.83 |
| NVIDIA GeForce RTX 4070 Laptop GPU | 592.82 | Vulkan | 280.98 | 266.86–2136.19 |

These are new-session startup costs with uncontrolled OS/driver caches, not cold-machine costs. Both devices support timestamp queries. Explicit per-session GPU/staging/readback buffer capacity is 43,518,184 bytes (about 41.5 MiB); process peak RAM and driver allocations were not measured. Sessions reuse resources and process 8 MiB batches serially.

## Complete warm compression time

Milliseconds, medians of five jobs after one cold job. Includes file reads/writes, output sync, selection, host staging, upload, GPU compute, readback, synchronization, framing and checksum. Initialization and process startup are excluded. CPU uses the same operation boundary. The first four configurations produce exactly the same compressed bytes.

| Input | MiB | CPU numeric 1 | CPU numeric 8 | Intel Arc | RTX 4070 | CPU Zstd 1 | CPU Zstd 3 | Original adaptive |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| mitbih-original-212 | 3.72 | 23.24 | 17.45 | 17.09 | 14.71 | 22.85 | 42.09 | 42.62 |
| mitbih-s16le | 4.96 | 19.47 | 12.30 | 17.17 | 11.96 | 22.27 | 52.69 | 13.35 |
| eeg-edf-original | 9.83 | 37.64 | 22.89 | 30.69 | 23.83 | 31.92 | 86.70 | 79.03 |
| ptb-12lead-original | 0.88 | 7.20 | 7.80 | 8.86 | 7.27 | 9.18 | 11.35 | 12.32 |
| terrain-hgt-be | 24.73 | 104.48 | 63.90 | 73.97 | 63.69 | 54.17 | 93.23 | 91.70 |
| terrain-s16le | 24.73 | 88.41 | 39.23 | 62.97 | 42.27 | 55.52 | 87.49 | 38.66 |
| raw-sensors-1MiB | 1.00 | 7.57 | 7.39 | 7.58 | 6.99 | 12.84 | 16.75 | 7.15 |
| raw-sensors-16MiB | 16.00 | 51.72 | 27.46 | 36.79 | 28.69 | 49.64 | 105.47 | 27.62 |
| raw-sensors-64MiB | 64.00 | 223.98 | 90.12 | 119.55 | 99.35 | 161.22 | 238.84 | 87.68 |
| independent-noise-16MiB | 16.00 | 84.64 | 47.40 | 55.03 | 44.83 | 59.16 | 72.92 | 56.34 |
| enwik8-16MiB | 16.00 | 81.62 | 49.19 | 54.41 | 43.55 | 41.62 | 79.58 | 78.23 |

## Compression ratio and selected stride

Output bytes / input bytes, in percent; lower is smaller. GPU and CPU numeric columns are identical. Converted inputs have their own denominator; do not compare their percentages with the original representations as if conversion were free or input sizes were equal. Input conversion is outside timing.

| Input | Stride (16-bit words) | Numeric output bytes | CPU/GPU numeric | Zstd 1 | Zstd 3 | Original adaptive |
|---|---:|---:|---:|---:|---:|---:|
| mitbih-original-212 | 64 | 3,900,026 | 100.00% | 68.04% | 58.94% | 58.94% |
| mitbih-s16le | 2 | 1,485,120 | 28.56% | 50.41% | 45.16% | 28.56% |
| eeg-edf-original | 1 | 4,594,850 | 44.59% | 62.00% | 53.66% | 53.66% |
| ptb-12lead-original | 64 | 638,790 | 69.31% | 76.87% | 76.53% | 76.53% |
| terrain-hgt-be | 1 | 18,945,820 | 73.05% | 44.80% | 37.33% | 37.33% |
| terrain-s16le | 1 | 6,634,304 | 25.58% | 44.78% | 37.31% | 25.58% |
| raw-sensors-1MiB | 16 | 279,574 | 26.66% | 71.47% | 54.11% | 26.66% |
| raw-sensors-16MiB | 16 | 4,472,942 | 26.66% | 71.40% | 53.46% | 26.66% |
| raw-sensors-64MiB | 16 | 17,891,694 | 26.66% | 71.40% | 53.44% | 26.66% |
| independent-noise-16MiB | 1 | 16,777,242 | 100.00% | 100.00% | 100.00% | 100.00% |
| enwik8-16MiB | 1 | 16,777,242 | 100.00% | 41.14% | 35.75% | 35.75% |

## GPU stages and transfer cost

Device times are milliseconds. Codec wall time includes host work, all transfers, synchronization, selection, framing and checksum, but excludes file I/O and initialization. Device columns are not the whole cost. Upload/readback are actual copy commands, not bandwidth estimates. Small parameter/metadata/query transfers are included in wall time but not completely covered by the device copy intervals.

| Input | GPU | Upload ms | Encode tiles ms | Scan + compact ms | Payload readback ms | Codec wall ms | Upload MiB | Payload readback MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| mitbih-original-212 | intel | 0.11 | 1.40 | 0.37 | 0.00 | 7.41 | 3.72 | 0.00 |
| mitbih-original-212 | nvidia | 0.65 | 0.19 | 0.08 | 0.00 | 5.34 | 3.72 | 0.00 |
| mitbih-s16le | intel | 0.17 | 1.97 | 0.75 | 0.07 | 8.82 | 4.96 | 1.42 |
| mitbih-s16le | nvidia | 0.44 | 0.30 | 0.16 | 0.14 | 4.17 | 4.96 | 1.42 |
| eeg-edf-original | intel | 0.42 | 3.46 | 1.34 | 0.23 | 17.56 | 9.83 | 4.38 |
| eeg-edf-original | nvidia | 1.70 | 0.56 | 0.32 | 0.81 | 11.70 | 9.83 | 4.38 |
| ptb-12lead-original | intel | 0.04 | 0.39 | 0.20 | 0.05 | 3.56 | 0.88 | 0.61 |
| ptb-12lead-original | nvidia | 0.09 | 0.06 | 0.06 | 0.07 | 1.57 | 0.88 | 0.61 |
| terrain-hgt-be | intel | 0.81 | 9.99 | 3.72 | 0.71 | 43.78 | 24.73 | 18.07 |
| terrain-hgt-be | nvidia | 4.26 | 1.39 | 0.75 | 3.32 | 31.54 | 24.73 | 18.07 |
| terrain-s16le | intel | 0.68 | 6.94 | 2.39 | 0.22 | 29.88 | 24.73 | 6.33 |
| terrain-s16le | nvidia | 4.30 | 1.70 | 0.75 | 1.17 | 21.59 | 24.73 | 6.33 |
| raw-sensors-1MiB | intel | 0.04 | 0.35 | 0.19 | 0.03 | 2.57 | 1.00 | 0.27 |
| raw-sensors-1MiB | nvidia | 0.18 | 0.07 | 0.07 | 0.05 | 1.72 | 1.00 | 0.27 |
| raw-sensors-16MiB | intel | 0.54 | 5.48 | 2.36 | 0.17 | 19.83 | 16.00 | 4.27 |
| raw-sensors-16MiB | nvidia | 2.77 | 0.89 | 0.46 | 0.74 | 13.59 | 16.00 | 4.27 |
| raw-sensors-64MiB | intel | 1.89 | 16.95 | 6.66 | 0.54 | 70.57 | 64.00 | 17.06 |
| raw-sensors-64MiB | nvidia | 5.51 | 3.56 | 1.82 | 1.60 | 49.75 | 64.00 | 17.06 |
| independent-noise-16MiB | intel | 0.52 | 6.29 | 1.37 | 0.00 | 28.64 | 16.00 | 0.00 |
| independent-noise-16MiB | nvidia | 2.81 | 0.76 | 0.21 | 0.01 | 20.32 | 16.00 | 0.00 |
| enwik8-16MiB | intel | 0.50 | 5.71 | 1.28 | 0.00 | 29.11 | 16.00 | 0.00 |
| enwik8-16MiB | nvidia | 1.38 | 0.76 | 0.20 | 0.01 | 17.89 | 16.00 | 0.00 |

For raw-stored chunks, zero payload bytes are read back: the host already retains the original input. Metadata and query bytes are separate fields in the JSON. Host wait durations overlap device work and must not be added to these device intervals. GPU encode and scan/compact times together are the compute cost.

## Recovery and variability

All recovery uses the original CPU decoder. GPU configurations use eight CPU workers; they do not provide GPU decompression. The complete archive is read back from disk, decoded and written/synced to a new file. The raw JSON also includes decode-only time. Small differences between identical archives are timing variation.

| Input | Config | Cold create ms | Warm create median ms | Warm create range ms | Warm codec median ms | Warm recovery median ms |
|---|---|---:|---:|---:|---:|---:|
| mitbih-original-212 | cpu-numeric-1 | 22.72 | 23.24 | 21.37–23.77 | 13.92 | 21.55 |
| mitbih-original-212 | cpu-numeric-8 | 18.53 | 17.45 | 15.98–20.49 | 6.48 | 17.87 |
| mitbih-original-212 | intel | 1061.26 | 17.09 | 16.68–18.34 | 7.41 | 19.58 |
| mitbih-original-212 | nvidia | 300.19 | 14.71 | 14.11–17.42 | 5.34 | 23.16 |
| mitbih-original-212 | zstd-1 | 26.28 | 22.85 | 18.80–23.23 | 11.17 | 22.81 |
| mitbih-original-212 | zstd-3 | 42.43 | 42.09 | 41.00–43.57 | 33.82 | 25.70 |
| mitbih-original-212 | original-adaptive | 43.73 | 42.62 | 40.57–48.88 | 35.00 | 28.06 |
| mitbih-s16le | cpu-numeric-1 | 18.90 | 19.47 | 18.27–20.64 | 12.24 | 28.26 |
| mitbih-s16le | cpu-numeric-8 | 13.34 | 12.30 | 11.67–13.49 | 4.69 | 22.32 |
| mitbih-s16le | intel | 561.59 | 17.17 | 16.20–18.62 | 8.82 | 27.46 |
| mitbih-s16le | nvidia | 282.34 | 11.96 | 11.65–12.20 | 4.17 | 23.35 |
| mitbih-s16le | zstd-1 | 23.33 | 22.27 | 21.76–38.85 | 13.63 | 35.05 |
| mitbih-s16le | zstd-3 | 52.57 | 52.69 | 49.33–55.27 | 43.92 | 33.26 |
| mitbih-s16le | original-adaptive | 12.36 | 13.35 | 12.91–14.65 | 5.67 | 25.18 |
| eeg-edf-original | cpu-numeric-1 | 36.62 | 37.64 | 34.80–38.93 | 25.39 | 45.71 |
| eeg-edf-original | cpu-numeric-8 | 22.86 | 22.89 | 22.20–23.76 | 10.39 | 31.44 |
| eeg-edf-original | intel | 580.43 | 30.69 | 29.76–34.15 | 17.56 | 37.33 |
| eeg-edf-original | nvidia | 293.61 | 23.83 | 23.42–26.43 | 11.70 | 33.30 |
| eeg-edf-original | zstd-1 | 29.25 | 31.92 | 28.41–37.29 | 16.57 | 40.87 |
| eeg-edf-original | zstd-3 | 85.74 | 86.70 | 78.87–89.31 | 73.08 | 54.84 |
| eeg-edf-original | original-adaptive | 76.87 | 79.03 | 72.03–84.49 | 66.70 | 50.81 |
| ptb-12lead-original | cpu-numeric-1 | 8.04 | 7.20 | 7.06–8.26 | 2.05 | 17.04 |
| ptb-12lead-original | cpu-numeric-8 | 9.47 | 7.80 | 7.39–8.55 | 2.39 | 18.07 |
| ptb-12lead-original | intel | 436.68 | 8.86 | 8.52–9.32 | 3.56 | 14.49 |
| ptb-12lead-original | nvidia | 2155.64 | 7.27 | 6.60–8.60 | 1.57 | 16.73 |
| ptb-12lead-original | zstd-1 | 10.89 | 9.18 | 8.37–9.96 | 3.81 | 17.08 |
| ptb-12lead-original | zstd-3 | 11.88 | 11.35 | 10.51–11.69 | 5.76 | 15.85 |
| ptb-12lead-original | original-adaptive | 12.97 | 12.32 | 11.60–14.67 | 7.30 | 14.63 |
| terrain-hgt-be | cpu-numeric-1 | 106.47 | 104.48 | 93.15–109.69 | 73.46 | 97.90 |
| terrain-hgt-be | cpu-numeric-8 | 67.13 | 63.90 | 62.99–65.16 | 31.10 | 69.42 |
| terrain-hgt-be | intel | 489.24 | 73.97 | 73.24–81.99 | 43.78 | 60.32 |
| terrain-hgt-be | nvidia | 368.15 | 63.69 | 63.37–67.11 | 31.54 | 63.04 |
| terrain-hgt-be | zstd-1 | 78.26 | 54.17 | 52.33–56.22 | 29.36 | 95.52 |
| terrain-hgt-be | zstd-3 | 89.94 | 93.23 | 86.75–108.13 | 70.10 | 98.82 |
| terrain-hgt-be | original-adaptive | 105.87 | 91.70 | 89.23–106.90 | 69.90 | 100.70 |
| terrain-s16le | cpu-numeric-1 | 105.17 | 88.41 | 84.57–104.42 | 65.19 | 105.36 |
| terrain-s16le | cpu-numeric-8 | 39.63 | 39.23 | 37.14–44.15 | 18.27 | 58.23 |
| terrain-s16le | intel | 479.18 | 62.97 | 53.36–73.64 | 29.88 | 57.53 |
| terrain-s16le | nvidia | 340.79 | 42.27 | 41.61–43.85 | 21.59 | 59.98 |
| terrain-s16le | zstd-1 | 51.67 | 55.52 | 51.53–57.22 | 32.23 | 90.71 |
| terrain-s16le | zstd-3 | 89.35 | 87.49 | 84.76–93.03 | 64.89 | 95.90 |
| terrain-s16le | original-adaptive | 37.70 | 38.66 | 36.21–39.65 | 17.73 | 57.06 |
| raw-sensors-1MiB | cpu-numeric-1 | 7.52 | 7.57 | 7.40–7.60 | 2.56 | 13.69 |
| raw-sensors-1MiB | cpu-numeric-8 | 7.84 | 7.39 | 7.02–7.77 | 2.30 | 13.89 |
| raw-sensors-1MiB | intel | 413.93 | 7.58 | 7.54–8.44 | 2.57 | 17.03 |
| raw-sensors-1MiB | nvidia | 280.04 | 6.99 | 6.43–9.40 | 1.72 | 13.71 |
| raw-sensors-1MiB | zstd-1 | 12.69 | 12.84 | 11.72–14.63 | 6.86 | 18.78 |
| raw-sensors-1MiB | zstd-3 | 16.54 | 16.75 | 15.86–18.22 | 11.06 | 14.88 |
| raw-sensors-1MiB | original-adaptive | 7.37 | 7.15 | 6.97–7.27 | 2.27 | 18.66 |
| raw-sensors-16MiB | cpu-numeric-1 | 52.77 | 51.72 | 46.63–61.12 | 35.98 | 56.17 |
| raw-sensors-16MiB | cpu-numeric-8 | 27.99 | 27.46 | 25.61–37.91 | 11.39 | 42.59 |
| raw-sensors-16MiB | intel | 414.16 | 36.79 | 33.25–40.76 | 19.83 | 40.30 |
| raw-sensors-16MiB | nvidia | 306.82 | 28.69 | 28.41–29.31 | 13.59 | 40.93 |
| raw-sensors-16MiB | zstd-1 | 48.28 | 49.64 | 49.03–56.75 | 28.50 | 70.67 |
| raw-sensors-16MiB | zstd-3 | 131.05 | 105.47 | 99.59–117.42 | 81.21 | 81.99 |
| raw-sensors-16MiB | original-adaptive | 27.63 | 27.62 | 27.25–28.82 | 12.78 | 40.20 |
| raw-sensors-64MiB | cpu-numeric-1 | 222.86 | 223.98 | 215.56–286.70 | 171.19 | 222.56 |
| raw-sensors-64MiB | cpu-numeric-8 | 92.52 | 90.12 | 87.94–94.96 | 45.68 | 119.32 |
| raw-sensors-64MiB | intel | 518.81 | 119.55 | 113.35–123.92 | 70.57 | 134.07 |
| raw-sensors-64MiB | nvidia | 1160.24 | 99.35 | 92.24–118.06 | 49.75 | 133.77 |
| raw-sensors-64MiB | zstd-1 | 188.20 | 161.22 | 147.68–179.46 | 81.65 | 252.04 |
| raw-sensors-64MiB | zstd-3 | 221.40 | 238.84 | 214.40–343.52 | 170.58 | 308.24 |
| raw-sensors-64MiB | original-adaptive | 88.83 | 87.68 | 85.69–94.44 | 42.82 | 123.55 |
| independent-noise-16MiB | cpu-numeric-1 | 86.46 | 84.64 | 82.21–91.89 | 58.08 | 40.97 |
| independent-noise-16MiB | cpu-numeric-8 | 46.39 | 47.40 | 45.80–49.35 | 22.07 | 36.14 |
| independent-noise-16MiB | intel | 455.19 | 55.03 | 54.01–55.22 | 28.64 | 42.75 |
| independent-noise-16MiB | nvidia | 326.38 | 44.83 | 43.05–64.97 | 20.32 | 37.89 |
| independent-noise-16MiB | zstd-1 | 59.31 | 59.16 | 55.79–60.51 | 33.62 | 41.16 |
| independent-noise-16MiB | zstd-3 | 79.58 | 72.92 | 67.06–82.66 | 45.76 | 48.48 |
| independent-noise-16MiB | original-adaptive | 58.15 | 56.34 | 54.22–58.67 | 30.99 | 37.05 |
| enwik8-16MiB | cpu-numeric-1 | 92.60 | 81.62 | 78.29–86.35 | 56.71 | 203.34 |
| enwik8-16MiB | cpu-numeric-8 | 52.96 | 49.19 | 46.77–51.07 | 24.30 | 199.56 |
| enwik8-16MiB | intel | 465.50 | 54.41 | 52.09–63.48 | 29.11 | 205.02 |
| enwik8-16MiB | nvidia | 338.63 | 43.55 | 42.21–61.10 | 17.89 | 210.89 |
| enwik8-16MiB | zstd-1 | 40.01 | 41.62 | 39.28–46.52 | 23.76 | 67.41 |
| enwik8-16MiB | zstd-3 | 72.83 | 79.58 | 74.97–86.70 | 64.20 | 68.19 |
| enwik8-16MiB | original-adaptive | 79.66 | 78.23 | 71.26–80.83 | 62.00 | 69.86 |

## What the measurements establish

* **intel:** lower warm creation median than the faster one/eight-worker numeric CPU control on 1/11 inputs; a margin above 10% on 0/11. These are descriptive counts, not statistical significance tests.
* **nvidia:** lower warm creation median than the faster one/eight-worker numeric CPU control on 6/11 inputs; a margin above 10% on 2/11. These are descriptive counts, not statistical significance tests.
* **The two NVIDIA margins above 10% occur on the original packed ECG and text inputs, both stored uncompressed by this codec.** They do not establish a useful compression win over a plain store path. On the real inputs that compress usefully, RTX is approximately tied with CPU on expanded ECG and slower on EEG and little-endian terrain; Arc is slower. There is no compelling result here for promoting GPU numeric compression into production Auto.
* Numeric speed alone is insufficient for Auto: compare output size against direct Zstandard and the original adaptive control. The forced numeric selector does not switch away from numeric on unsuitable real inputs.
* Cold startup is included in the cold table and can outweigh any warm advantage, especially on smaller files. No change to production Auto is justified solely by a favorable kernel timestamp.
* Real inputs cover two ECG sources, native EDF EEG, and one terrain tile; these are useful checks, not a representative sample of all numeric workloads. No GPU float32/float64 codec, neural model, or approximate transform is involved.
* Original versus converted representations, an unsupported 12-channel stride, headers, annotations and high-entropy controls are deliberately retained to expose limitations. Conversion is not automatically performed by the codec.
* Five warm jobs are from one process per configuration/input. Thermal, cache and background effects remain. Do not treat tiny differences as repeatable wins. No peak-memory, energy-use, AMD, Apple or discrete Intel Arc measurement is claimed.

Executable SHA-256: `b10b3835d170f79cdefccf11b6b6ae06c802087c53ece4a9d5a38c0667efb7b3`. Source, corpus and archive hashes are retained in the raw report.
