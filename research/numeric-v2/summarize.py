"""Validate raw observations and generate the numeric-v2 comparison report."""
from collections import defaultdict
import hashlib
import json
from pathlib import Path
from statistics import median

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
report = json.loads((HERE/'results.json').read_text())
assert report['completed']
corpus_path = HERE/'corpus.json'
corpus = json.loads(corpus_path.read_text())
assert hashlib.sha256(corpus_path.read_bytes()).hexdigest() == report['corpus_sha256']
for name, digest in report['sources'].items():
    assert hashlib.sha256((ROOT/name).read_bytes()).hexdigest() == digest, name
for name, digest in report['binaries'].items():
    assert hashlib.sha256(Path(name).read_bytes()).hexdigest() == digest, name
groups = defaultdict(list)
for row in report['rows']:
    assert row['sha256_verified']
    groups[row['dataset'],row['config']].append(row)
configs = report['configs']
for d in corpus['datasets']:
    for config in configs:
        rows = groups[d['name'],config]
        assert sorted(r['repeat'] for r in rows) == [0,1,2]
        assert all(r['input_sha256'] == d['sha256'] and r['input_bytes'] == d['bytes'] for r in rows)
        assert len({r['output_bytes'] for r in rows}) == 1
        if config.startswith('v2-'):
            assert len({r['archive_sha256'] for r in rows}) == 1

def med(d,c,key):
    return median(r[key] for r in groups[d,c])

def ratio(d,c):
    return med(d,c,'output_bytes')/groups[d,c][0]['input_bytes']*100

def codec(d,c):
    return median(r['encode'].get('codec_ms',r['encode'].get('codec_including_init_ms')) for r in groups[d,c])

def delta(d,a,b):
    return (med(d,a,'output_bytes')/med(d,b,'output_bytes')-1)*100

def speed(d,a,b):
    return med(d,b,'create_ms')/med(d,a,'create_ms')

def plan_name(p):
    l = p['layout']
    width = 'packed-212' if l['width']==3 else str(l['width']*8)+('BE' if l['big_endian'] else 'LE')
    stage = ['store','tile pack','Zstd planes','ordinary Zstd','Zstd packed tiles'][p['stage']]
    return f"{width}, {'XOR' if l['xor'] else 'delta'}, stride {l['stride']}; {stage}"

lines = ['# lttl compression benchmark results','',
         '**lttl Fast** and **lttl Dense** are the new algorithm presets. This is the numeric-v2 research implementation; '
         'raw configuration IDs remain `v2-fast` and `v2-dense` for reproducibility.', '',
         f"**{len(report['rows'])} compression/extraction pairs passed exact SHA-256 recovery verification** across "
         f"{len(corpus['datasets'])} inputs and {len(configs)} configurations. Three serial randomized repetitions per case. "
         'V2 archive bytes are deterministic across repetitions. Source, binary and corpus hashes were verified.', '',
         'Core Ultra 9 185H; eight requested CPU workers. Actual RAR 7.20.3 x64 and 7-Zip 26.04 x64. '
         'No GPU v2 implementation is measured. [Methodology and reproduction](README.md), '
         '[raw measurements](results.json), [input manifest](corpus.json).', '',
         '## Measured outcome', '',
         'The dense stage substantially closes the numeric compression gap. Its CPU cost is visible below; '
         'it is not free compression. Fast remains available when packing alone is preferable.', '']
for d,label in [('mitbih-original-212','Original packed ECG'),('eeg-edf-original','Original EDF EEG'),
                ('ptb-12lead-original','Original 12-channel ECG'),('terrain-hgt-be','Original big-endian terrain'),
                ('raw-sensors-64MiB','64 MiB synthetic sensors')]:
    lines.append(f"* **{label}:** Dense produces {ratio(d,'v2-dense'):.2f}% of input, versus "
                 f"{ratio(d,'cpu-numeric'):.2f}% for v1 numeric. Its output is {delta(d,'v2-dense','rar-5'):+.1f}% "
                 f"relative to RAR method 5 and {delta(d,'v2-dense','7z-delta-5'):+.1f}% relative to delta-filtered 7z "
                 f"(negative means smaller). Creation is {speed(d,'v2-dense','rar-5'):.2f}x faster than RAR and "
                 f"{speed(d,'v2-dense','7z-delta-5'):.2f}x faster than filtered 7z at these settings.")
lines += ['', 'These are specified settings, not an exhaustive tuning contest. Delta-filtered 7z receives the newly detected '
          'stride outside its timer, including improved distances for original packed and 12-channel ECG. '
          'Converted ECG/terrain remain separate reference inputs, not substitutes for originals.', '',
          f"**The speed goal is only partly met.** On 64 MiB sensors, codec-only time changes from "
          f"{codec('raw-sensors-64MiB','cpu-numeric'):.2f} ms for v1 to {codec('raw-sensors-64MiB','v2-fast'):.2f} ms for lttl Fast "
          f"and {codec('raw-sensors-64MiB','v2-dense'):.2f} ms for lttl Dense. Dense output is "
          f"{-delta('raw-sensors-64MiB','v2-dense','cpu-numeric'):.1f}% smaller than v1. "
          'Full-process timings also reflect different executable launch costs; they must not be used alone to claim faster numeric math. '
          'Small-input sampling overhead and slower generic decoding remain optimization targets.', '',
          '## Compression ratio', '',
          'Full output bytes / input bytes, percent; lower is smaller. Headers are included. '+
          'Text uses ordinary Zstd fallback; noise may be stored. No lossy representation is used.', '',
          '| Input | v1 numeric | v1 adaptive | lttl Fast | lttl Dense | Zstd 3 | RAR 5 | 7z 5 | 7z 9 | 7z Delta 5 |',
          '|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|']
order = ['cpu-numeric','adaptive','v2-fast','v2-dense','zstd-3','rar-5','7z-5','7z-9','7z-delta-5']
for d in corpus['datasets']:
    lines.append('| '+d['name']+' | '+' | '.join(f"{ratio(d['name'],c):.2f}%" for c in order)+' |')
for key,title in [('create_ms','Creation'),('extract_ms','Extraction')]:
    lines += ['',f'## {title}: complete process time','',
              'Milliseconds, medians of three runs. Includes process launch, file IO and common output fsync. '
              'Creation includes layout detection and transforms; extraction uses the recorded layout and inverts those transforms. '
              'No mixing with kernel-only times.', '',
              '| Input | v1 numeric | v1 adaptive | lttl Fast | lttl Dense | Zstd 3 | RAR 5 | 7z 5 | 7z 9 | 7z Delta 5 |',
              '|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|']
    for d in corpus['datasets']:
        lines.append('| '+d['name']+' | '+' | '.join(f"{med(d['name'],c,key):.1f}" for c in order)+' |')
lines += ['', '## Algorithm cost without executable/file startup', '',
          'Encode-only milliseconds inside the Rust workers. Includes layout selection, transforms, compression and checksum; '
          'excludes input/output and thread-pool initialization. Fresh worker per run. These timings cannot be substituted '
          'for RAR/7z process times. They show whether density improvements cost CPU time even when process overhead masks it.', '',
          '| Input | v1 numeric codec | lttl Fast codec | lttl Dense codec | Dense / v1 time | Fast selection | Dense selection |',
          '|---|---:|---:|---:|---:|---:|---:|']
for d in corpus['datasets']:
    name=d['name']; old=codec(name,'cpu-numeric'); fast=codec(name,'v2-fast'); dense=codec(name,'v2-dense')
    select=[median(r['encode']['plan']['selection_ms'] for r in groups[name,c]) for c in ['v2-fast','v2-dense']]
    lines.append(f'| {name} | {old:.2f} | {fast:.2f} | {dense:.2f} | {dense/old:.2f}x | {select[0]:.2f} | {select[1]:.2f} |')
lines += ['', '### Decoder-only cost', '',
          'Milliseconds inside each Rust worker, including content checksum and excluding file IO and pool startup. '
          'This separates decoder work from the different executable launch costs.', '',
          '| Input | v1 numeric decode | lttl Fast decode | lttl Dense decode |', '|---|---:|---:|---:|']
for d in corpus['datasets']:
    times = [median(row['decode'].get('codec_ms',row['decode'].get('codec_including_init_ms'))
                    for row in groups[d['name'],c]) for c in ['cpu-numeric','v2-fast','v2-dense']]
    lines.append('| '+d['name']+' | '+' | '.join(f'{v:.2f}' for v in times)+' |')
lines += ['', '## Detected plans', '',
          'These are compression hypotheses, not guaranteed semantic types. A floating-point control may favor generic Zstd '
          'over an XOR transform. Selection examines bytes, not filenames.', '',
          '| Input | Fast | Dense | 7z Delta distance (bytes) |', '|---|---|---|---:|']
for d in corpus['datasets']:
    name=d['name']
    plans=[groups[name,c][0]['encode']['plan'] for c in ['v2-fast','v2-dense']]
    lines.append('| '+name+' | '+' | '.join(plan_name(p) for p in plans)+' | '+str(report['delta_plans'][name]['distance'])+' |')
lines += ['', '## Chart', '', '![Measured size and creation-time tradeoffs](tradeoffs.png)', '',
          '[Vector chart](tradeoffs.svg). Error bars are observed min/max across three runs. All chart timings use the same complete-process boundary.', '',
          '## Every configuration and observed variability', '']
for d in corpus['datasets']:
    name=d['name']
    lines += [f'### {name}', '', f"{d['bytes']:,} input bytes. {d['description']}", '',
              '| Configuration | Output bytes | Create median ms | Create min–max | Extract median ms | Extract min–max |',
              '|---|---:|---:|---:|---:|---:|']
    for c in order:
        rows=groups[name,c]; create=[r['create_ms'] for r in rows]; extract=[r['extract_ms'] for r in rows]
        lines.append(f"| {c} | {med(name,c,'output_bytes'):,.0f} | {median(create):.1f} | {min(create):.1f}–{max(create):.1f} "
                     f"| {median(extract):.1f} | {min(extract):.1f}–{max(extract):.1f} |")
    lines.append('')
lines += ['## Limits and next decision', '',
          '* Development and selection adjustments used this corpus. These are exploratory in-corpus results, not holdout validation.',
          '* Fast/Dense are CPU research presets. V1 GPU code and production Auto are unchanged. A GPU port would need separate ratio/correctness/transfer measurements.',
          '* Generic text compression still trails the dense archive presets; the new numeric stage is not a universal codec replacement.',
          '* Native layout support removes the need for external endian swapping or packed-212 expansion on the tested originals. '
          'It does not parse arbitrary scientific containers, mixed schemas, arbitrary channel counts or unaligned data prefixes.',
          '* Power, thermal state, OS caches, CPU placement and background activity are uncontrolled. Three repetitions do not establish statistical significance.',
          '* RAR/7z contain a filename and metadata; prototype frames contain payload only. Different dictionaries and memory usage are not normalized.',
          '* Keep the dense stage optional: internal codec timings and small-input costs matter, even when it remains far faster than RAR/7z.', '']
(HERE/'measurements.md').write_text('\n'.join(lines),encoding='utf-8')
print('Verified and summarized',len(report['rows']),'round trips')
