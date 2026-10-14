"""Check evidence and generate hardware, ratio, transfer, and recovery tables."""
from collections import defaultdict
import hashlib
import json
from pathlib import Path
from statistics import median

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
report = json.loads((HERE / 'results.json').read_text())
corpus = json.loads((HERE / 'corpus.json').read_text())
assert report['completed'] and report['all_numeric_outputs_identical']
for name, expected in report['source_sha256'].items():
    assert hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == expected, name
assert hashlib.sha256((HERE / 'corpus.json').read_bytes()).hexdigest() == report['corpus_manifest_sha256']
groups = defaultdict(list)
for row in report['rows']:
    assert row['sha256_verified']
    groups[row['dataset'], row['config']].append(row)
configs = ['cpu-numeric-1', 'cpu-numeric-8', 'intel', 'nvidia', 'zstd-1', 'zstd-3', 'original-adaptive']
for dataset in corpus['datasets']:
    for config in configs:
        rows = groups[dataset['name'], config]
        assert sorted(r['run'] for r in rows) == list(range(6))
        assert {r['input_sha256'] for r in rows} == {dataset['sha256']}
        assert len({r['archive_sha256'] for r in rows}) == 1
    assert len({r['archive_sha256'] for c in configs[:4] for r in groups[dataset['name'], c]}) == 1


def rows(name, config):
    return [r for r in groups[name, config] if r['run'] > 0]


def med(name, config, key):
    return median(r[key] for r in rows(name, config))


def size(name, config):
    return rows(name, config)[0]['output_bytes']


def metric(name, config, key):
    values = [r['metrics'][key] for r in rows(name, config)]
    return median(values) if all(v is not None for v in values) else None


def fmt(value):
    return 'unavailable' if value is None else f'{value:.2f}'


names = [d['name'] for d in corpus['datasets']]
lines = ['# GPU numeric benchmark results', '',
         'Measured on the Intel Core Ultra 9 185H host. See [methodology, implementation and data attribution](README.md), '
         '[raw results](results.json), and [input provenance](corpus.json).', '',
         f"All **{len(report['rows'])} benchmark creation/recovery pairs** passed restored-file SHA-256 checks. "
         'All numeric CPU/GPU archive hashes are identical per dataset. '
         'Both GPUs also passed 316 additional edge-case pairs each, decoded by the original CPU decoder. '
         'The three original Rust framing/round-trip/corruption tests passed.', '',
         '## Devices and session cost', '',
         '| Device | Driver | Backend | Initialization median ms | Initialization range ms |',
         '|---|---|---|---:|---:|']
for vendor in ['intel', 'nvidia']:
    matches = [g for g in report['groups'] if g['config'] == vendor]
    adapter = matches[0]['adapter']
    times = [g['initialization_ms'] for g in matches]
    lines.append(f"| {adapter['name']} | {adapter['driver_info']} | {adapter['backend']} | {median(times):.2f} | {min(times):.2f}–{max(times):.2f} |")
lines += ['', 'These are new-session startup costs with uncontrolled OS/driver caches, not cold-machine costs. '
          'Both devices support timestamp queries. Explicit per-session GPU/staging/readback buffer capacity is '
          '43,518,184 bytes (about 41.5 MiB); process peak RAM and driver allocations were not measured. '
          'Sessions reuse resources and process 8 MiB batches serially.', '',
          '## Complete warm compression time', '',
          'Milliseconds, medians of five jobs after one cold job. Includes file reads/writes, output sync, '
          'selection, host staging, upload, GPU compute, readback, synchronization, framing and checksum. '
          'Initialization and process startup are excluded. CPU uses the same operation boundary. '
          'The first four configurations produce exactly the same compressed bytes.', '',
          '| Input | MiB | CPU numeric 1 | CPU numeric 8 | Intel Arc | RTX 4070 | CPU Zstd 1 | CPU Zstd 3 | Original adaptive |',
          '|---|---:|---:|---:|---:|---:|---:|---:|---:|']
for dataset in corpus['datasets']:
    name = dataset['name']
    lines.append(f"| {name} | {dataset['bytes'] / 1048576:.2f} | " + ' | '.join(fmt(med(name, c, 'create_ms')) for c in configs) + ' |')
lines += ['', '## Compression ratio and selected stride', '',
          'Output bytes / input bytes, in percent; lower is smaller. GPU and CPU numeric columns are identical. '
          'Converted inputs have their own denominator; do not compare their percentages with the original representations '
          'as if conversion were free or input sizes were equal. Input conversion is outside timing.', '',
          '| Input | Stride (16-bit words) | Numeric output bytes | CPU/GPU numeric | Zstd 1 | Zstd 3 | Original adaptive |',
          '|---|---:|---:|---:|---:|---:|---:|']
for dataset in corpus['datasets']:
    name = dataset['name']
    lines.append(f"| {name} | {rows(name, 'intel')[0]['stride']} | {size(name, 'intel'):,} | " +
                 ' | '.join(f"{size(name, c) / dataset['bytes'] * 100:.2f}%" for c in ['intel', 'zstd-1', 'zstd-3', 'original-adaptive']) + ' |')
lines += ['', '## GPU stages and transfer cost', '',
          'Device times are milliseconds. Codec wall time includes host work, all transfers, synchronization, selection, '
          'framing and checksum, but excludes file I/O and initialization. Device columns are not the whole cost. '
          'Upload/readback are actual copy commands, not bandwidth estimates. Small parameter/metadata/query transfers '
          'are included in wall time but not completely covered by the device copy intervals.', '',
          '| Input | GPU | Upload ms | Encode tiles ms | Scan + compact ms | Payload readback ms | Codec wall ms | Upload MiB | Payload readback MiB |',
          '|---|---|---:|---:|---:|---:|---:|---:|---:|']
for name in names:
    for gpu in ['intel', 'nvidia']:
        lines.append(f'| {name} | {gpu} | ' + ' | '.join(fmt(metric(name, gpu, key)) for key in ['upload_ms', 'encode_tiles_ms', 'scan_compact_ms', 'readback_ms']) +
                     f" | {med(name, gpu, 'codec_ms'):.2f} | {metric(name, gpu, 'uploaded_bytes') / 1048576:.2f} | {metric(name, gpu, 'payload_readback_bytes') / 1048576:.2f} |")
lines += ['', 'For raw-stored chunks, zero payload bytes are read back: the host already retains the original input. '
          'Metadata and query bytes are separate fields in the JSON. Host wait durations overlap device work and '
          'must not be added to these device intervals. GPU encode and scan/compact times together are the compute cost.', '',
          '## Recovery and variability', '',
          'All recovery uses the original CPU decoder. GPU configurations use eight CPU workers; they do not provide '
          'GPU decompression. The complete archive is read back from disk, decoded and written/synced to a new file. '
          'The raw JSON also includes decode-only time. Small differences between identical archives are timing variation.', '',
          '| Input | Config | Cold create ms | Warm create median ms | Warm create range ms | Warm codec median ms | Warm recovery median ms |',
          '|---|---|---:|---:|---:|---:|---:|']
for name in names:
    for config in configs:
        warm = rows(name, config)
        cold = next(r for r in groups[name, config] if r['run'] == 0)
        values = [r['create_ms'] for r in warm]
        lines.append(f"| {name} | {config} | {cold['create_ms']:.2f} | {median(values):.2f} | "
                     f"{min(values):.2f}–{max(values):.2f} | {med(name, config, 'codec_ms'):.2f} | {med(name, config, 'recover_ms'):.2f} |")
lines += ['', '## What the measurements establish', '']
for gpu in ['intel', 'nvidia']:
    wins = [n for n in names if med(n, gpu, 'create_ms') < min(med(n, c, 'create_ms') for c in ['cpu-numeric-1', 'cpu-numeric-8'])]
    strong = [n for n in wins if med(n, gpu, 'create_ms') < 0.90 * min(med(n, c, 'create_ms') for c in ['cpu-numeric-1', 'cpu-numeric-8'])]
    lines.append(f"* **{gpu}:** lower warm creation median than the faster one/eight-worker numeric CPU control on "
                 f"{len(wins)}/{len(names)} inputs; a margin above 10% on {len(strong)}/{len(names)}. "
                 'These are descriptive counts, not statistical significance tests.')
lines += ['* **The two NVIDIA margins above 10% occur on the original packed ECG and text inputs, both stored uncompressed by this codec.** '
          'They do not establish a useful compression win over a plain store path. On the real inputs that compress usefully, '
          'RTX is approximately tied with CPU on expanded ECG and slower on EEG and little-endian terrain; Arc is slower. '
          'There is no compelling result here for promoting GPU numeric compression into production Auto.',
          '* Numeric speed alone is insufficient for Auto: compare output size against direct Zstandard and the original adaptive control. '
          'The forced numeric selector does not switch away from numeric on unsuitable real inputs.',
          '* Cold startup is included in the cold table and can outweigh any warm advantage, especially on smaller files. '
          'No change to production Auto is justified solely by a favorable kernel timestamp.',
          '* Real inputs cover two ECG sources, native EDF EEG, and one terrain tile; these are useful checks, not a representative sample '
          'of all numeric workloads. No GPU float32/float64 codec, neural model, or approximate transform is involved.',
          '* Original versus converted representations, an unsupported 12-channel stride, headers, annotations and high-entropy controls '
          'are deliberately retained to expose limitations. Conversion is not automatically performed by the codec.',
          '* Five warm jobs are from one process per configuration/input. Thermal, cache and background effects remain. '
          'Do not treat tiny differences as repeatable wins. No peak-memory, energy-use, AMD, Apple or discrete Intel Arc measurement is claimed.', '',
          f"Executable SHA-256: `{report['binary_sha256']}`. Source, corpus and archive hashes are retained in the raw report.", '']
(HERE / 'measurements.md').write_text('\n'.join(lines), encoding='utf-8')
print(f"Validated {len(report['rows'])} benchmark pairs and wrote {HERE / 'measurements.md'}")
