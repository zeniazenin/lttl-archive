"""Verify the experiment and generate size, process-time and warm-session comparisons."""
from collections import defaultdict
import hashlib
import json
from pathlib import Path
from statistics import median

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
report = json.loads((HERE / 'results.json').read_text())
assert report['completed'] and report['numeric_archives_identical']
corpus = json.loads((ROOT / 'research/numeric-gpu/corpus.json').read_text())
assert hashlib.sha256((ROOT / 'research/numeric-gpu/corpus.json').read_bytes()).hexdigest() == report['corpus_sha256']
for name, digest in report['sources'].items():
    assert hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == digest, name
groups = defaultdict(list)
for row in report['rows']:
    assert row['sha256_verified']
    groups[row['dataset'], row['config']].append(row)
configs = ['cpu-numeric', 'intel', 'nvidia', 'adaptive', 'zstd-1', 'zstd-3', 'zstd-9', 'lz4', 'store',
           'zip-deflate-6', 'rar-3', 'rar-5', '7z-1', '7z-5', '7z-9', '7z-delta-5']
for d in corpus['datasets']:
    for c in configs:
        assert sorted(r['repeat'] for r in groups[d['name'], c]) == [0, 1, 2]
        assert all(r['input_sha256'] == d['sha256'] for r in groups[d['name'], c])
    assert len({r['archive_sha256'] for c in ['cpu-numeric', 'intel', 'nvidia'] for r in groups[d['name'], c]}) == 1


def med(name, config, key):
    return median(r[key] for r in groups[name, config])


def ratio(name, config):
    return med(name, config, 'output_bytes') / groups[name, config][0]['input_bytes'] * 100


warm = {(g['dataset'], g['config']): g for g in report['warm']}
for d in corpus['datasets']:
    for c in ['cpu-numeric', 'intel', 'nvidia']:
        rows = warm[d['name'], c]['rows']
        assert sorted(r['run'] for r in rows) == [0, 1, 2, 3]
        assert all(r['sha256_verified'] and r['input_sha256'] == d['sha256'] for r in rows)
        assert {r['archive_sha256'] for r in rows} == {groups[d['name'], 'cpu-numeric'][0]['archive_sha256']}

lines = ['# Codec/application comparison results', '',
         'Core Ultra 9 185H, Intel Arc integrated graphics, RTX 4070 Laptop GPU. '
         'Actual RAR 7.20.3 and official 7-Zip 26.04 x64 standalone console. '
         'Zstandard 1.5.7; LZ4 via `lz4_flex` 0.11.6.', '',
         f"**{len(report['rows'])} fresh-process compression/extraction pairs** and "
         f"**{sum(len(g['rows']) for g in report['warm'])} session jobs** passed exact restored-file SHA-256 verification. "
         'All numeric archives are byte-identical across CPU and both GPUs, including session runs.', '',
         'Read [methodology, configurations and reproduction](README.md), [raw measurements](results.json), '
         'and [dataset provenance](../numeric-gpu/corpus.json). These are specified settings, '
         'not exhaustive tuning or equal-memory comparisons. No production code or Auto policy changed.', '',
         '## Measured conclusion', '',
         'The prototype is competitive as a fast numeric specialization, not as a universal replacement for dense archive codecs. '
         'The CPU version delivers most of the benefit; this experiment does not establish a large GPU advantage.', '',
         '* On original EDF EEG, numeric CPU output is 0.7% smaller than RAR method 5 and creation is 5.48x faster. '
         'It is 3.0% larger than 7z Ultra and creation is 49.22x faster.',
         '* On converted little-endian terrain, numeric output is 13.1% larger than RAR method 5 and 20.5% larger than '
         'delta-filtered 7z, while creation is 8.85x and 78.61x faster respectively. Conversion is excluded from every timing.',
         '* On 64 MiB synthetic sensors, numeric output is 22.9% smaller than RAR method 5 and 5.9% smaller than 7z Ultra, '
         'but 10.0% larger than delta-filtered 7z. This is a synthetic specialization result, not a general corpus win.',
         '* Original packed ECG, original big-endian terrain, 12-channel ECG and text expose important weaknesses. '
         'Numeric-only stores the text unchanged; the adaptive research fallback reaches 35.75% of input through Zstd 3, '
         'versus 28.91% for RAR method 5 and 26.72% for 7z Ultra.',
         '* CPU and GPU compression ratios are identical. In reused sessions, RTX creation takes 81.74 ms versus CPU 89.74 ms '
         'on 64 MiB sensors, and 10.84 ms versus 12.66 ms on converted ECG. Intel is usually slower. '
         'These modest warm-session differences vary by input, and fresh GPU initialization is expensive. '
         'Three repetitions and uncontrolled power/caches do not establish a reliable routing threshold.', '',
         'The useful next algorithm work is representation-aware transforms (endianness, packed samples and channel strides), '
         'a stronger optional entropy stage, and cheap fallback detection. Automatic GPU routing needs larger repeated-job '
         'measurements before these small warm gains justify a policy change.', '',
         '![Measured compression size and creation-time tradeoffs](tradeoffs.png)', '',
         'The chart uses fresh-process times only; [vector version](tradeoffs.svg). Detailed tables below include extraction and reused sessions.', '',
         '## Compressed size', '',
         'Percent of original input bytes; lower is smaller. Full output including framing/metadata is counted. '
         'CPU/GPU numeric outputs have identical size. Converted ECG and terrain representations are separate inputs; '
         'conversion is lossless but outside timing. `7z-delta-5` is a manually configured bytewise Delta + LZMA2 competitor, '
         'not 7-Zip automatic detection. Its filter distance comes from the prototype’s sample-selected stride.', '',
         '| Input | Numeric CPU/GPU | Zstd 3 | Zstd 9 | LZ4 | ZIP/Deflate 6 | RAR 3 | RAR 5 | 7z 5 | 7z 9 | 7z Delta 5 |',
         '|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|']
size_configs = ['cpu-numeric', 'zstd-3', 'zstd-9', 'lz4', 'zip-deflate-6', 'rar-3', 'rar-5', '7z-5', '7z-9', '7z-delta-5']
for d in corpus['datasets']:
    name = d['name']
    lines.append('| ' + name + ' | ' + ' | '.join(f'{ratio(name, c):.2f}%' for c in size_configs) + ' |')

lines += ['', '## Fresh-process compression time', '',
          'Milliseconds, medians of three randomized repetitions. Every column includes process startup, '
          'input/output, an explicit durability flush, checksums and codec work. GPU columns also include '
          'initializing a new device/session plus all transfers and synchronization. These are comparable operation '
          'boundaries; they are not kernel-only or reused-session timings.', '',
          '| Input | Numeric CPU | Intel cold | RTX cold | Zstd 3 | LZ4 | ZIP 6 | RAR 5 | 7z 5 | 7z 9 | 7z Delta 5 |',
          '|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|']
time_configs = ['cpu-numeric', 'intel', 'nvidia', 'zstd-3', 'lz4', 'zip-deflate-6', 'rar-5', '7z-5', '7z-9', '7z-delta-5']
for d in corpus['datasets']:
    name = d['name']
    lines.append('| ' + name + ' | ' + ' | '.join(f"{med(name, c, 'create_ms'):.1f}" for c in time_configs) + ' |')

lines += ['', '## Fresh-process extraction time', '',
          'Milliseconds, medians of three runs, including process startup, reading the archive, decoding, '
          'writing and syncing the restored stream. GPU-created numeric archives use the same CPU decoder '
          'and identical bytes as the CPU numeric column. No GPU decoding is claimed.', '',
          '| Input | Numeric CPU | Zstd 3 | LZ4 | ZIP 6 | RAR 5 | 7z 5 | 7z 9 | 7z Delta 5 |',
          '|---|---:|---:|---:|---:|---:|---:|---:|---:|']
extract_configs = ['cpu-numeric', 'zstd-3', 'lz4', 'zip-deflate-6', 'rar-5', '7z-5', '7z-9', '7z-delta-5']
for d in corpus['datasets']:
    name = d['name']
    lines.append('| ' + name + ' | ' + ' | '.join(f"{med(name, c, 'extract_ms'):.1f}" for c in extract_configs) + ' |')

lines += ['', '## Reused sessions: a separate timing boundary', '',
          'Milliseconds, medians of three warm jobs after one initial job in an already-running process. '
          'Includes reads/writes, sync, selection, framing/checksums, GPU staging/transfers and synchronization, '
          'but excludes process/session initialization. These values must not be directly substituted into the '
          'RAR/7-Zip fresh-process comparison above. All outputs match the fresh-process numeric archives exactly.', '',
          '| Input | CPU warm create | Intel warm create | RTX warm create | CPU warm recovery | Intel init | RTX init |',
          '|---|---:|---:|---:|---:|---:|---:|']
for d in corpus['datasets']:
    name = d['name']
    times = [median(r['create_ms'] for r in warm[name, c]['rows'] if r['run'] > 0) for c in ['cpu-numeric', 'intel', 'nvidia']]
    recover = median(r['recover_ms'] for r in warm[name, 'cpu-numeric']['rows'] if r['run'] > 0)
    lines.append('| ' + name + ' | ' + ' | '.join(f'{x:.2f}' for x in times + [recover, warm[name, 'intel']['initialization_ms'], warm[name, 'nvidia']['initialization_ms']]) + ' |')

lines += ['', '## Numeric tradeoffs against dense encoders', '',
          'Positive size delta means numeric output is larger; negative means smaller. Speed factors use '
          'fresh-process creation medians (competitor / numeric), not mixed cold/warm timings. '
          'These are per-input tradeoffs, not a universal ranking.', '',
          '| Input | Numeric vs RAR 5 bytes | Numeric vs 7z 9 bytes | Numeric vs Delta 5 bytes | Speed vs RAR 5 | Speed vs 7z 9 | Speed vs Delta 5 |',
          '|---|---:|---:|---:|---:|---:|---:|']
for d in corpus['datasets']:
    name = d['name']; n = med(name, 'cpu-numeric', 'output_bytes'); t = med(name, 'cpu-numeric', 'create_ms')
    comparisons = ['rar-5', '7z-9', '7z-delta-5']
    lines.append('| ' + name + ' | ' + ' | '.join(f"{(n / med(name, c, 'output_bytes') - 1) * 100:+.1f}%" for c in comparisons) +
                 ' | ' + ' | '.join(f"{med(name, c, 'create_ms') / t:.2f}x" for c in comparisons) + ' |')

lines += ['', '## All configurations and variability', '',
          'All times in milliseconds. Caches, temperature, power and background activity are uncontrolled. '
          'Min/max ranges expose variability; three repetitions do not establish statistical significance. '
          'Dictionary settings differ, and peak memory was not measured.', '']
for d in corpus['datasets']:
    name = d['name']
    lines += [f'### {name}', '', f"Input: {d['bytes']:,} bytes. {d['description']}", '',
              '| Configuration | Output bytes | Output/input | Create median | Create range | Extract median | Extract range |',
              '|---|---:|---:|---:|---:|---:|---:|']
    for c in configs:
        rows = groups[name, c]
        creation = [r['create_ms'] for r in rows]; extraction = [r['extract_ms'] for r in rows]
        lines.append(f"| {c} | {med(name, c, 'output_bytes'):,.0f} | {ratio(name, c):.2f}% | "
                     f"{median(creation):.1f} | {min(creation):.1f}–{max(creation):.1f} | "
                     f"{median(extraction):.1f} | {min(extraction):.1f}–{max(extraction):.1f} |")
    lines.append('')
lines += ['## Interpretation limits', '',
          '* A specialized numeric encoder is not a general archiver replacement. Text, original packed ECG and big-endian samples '
          'can favor established encoders strongly. The adaptive research fallback is a separate row.',
          '* Delta + LZMA2 demonstrates that relevant numeric transforms already exist in a standard tool. '
          'A win against generic settings alone is insufficient evidence of a new compression-density breakthrough.',
          '* The 7z normal/ultra and RAR settings are fully specified and decoded by their real tools. '
          'This run does not substitute Python/liblzma for the 7-Zip application.',
          '* No mixing of numerical warm timings and application cold timings is used to calculate the speed factors. '
          'GPU session reuse may help interactive use, but standard tools would also benefit from avoiding startup.',
          '* This corpus emphasizes numeric workloads. It lacks broad source trees, executables, office documents, images/video and '
          'multi-file solid grouping. These results are not a general market-wide ranking.', '']
(HERE / 'measurements.md').write_text('\n'.join(lines), encoding='utf-8')
print('Validated measurements and wrote ' + str(HERE / 'measurements.md'))
