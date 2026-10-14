"""Validate frozen measurements and report CPU optimization without changing ratio."""
from collections import defaultdict
import hashlib
import json
from pathlib import Path
from statistics import median

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


report = json.loads((HERE/'results.json').read_text())
memory = json.loads((HERE/'memory.json').read_text())
assert report['completed'] and memory['completed']
corpus_path = ROOT/'research/numeric-v2/corpus.json'
corpus = json.loads(corpus_path.read_text())
assert sha(corpus_path) == report['corpus_sha256'] == memory['corpus_sha256']
assert sha(ROOT/'research/numeric-v2/results.json') == report['baseline_report_sha256']
assert memory['binary_sha256'] == report['binaries'][str(ROOT/'target/release/examples/dense_opt.exe')]
for name, digest in report['sources'].items():
    assert sha(ROOT/name) == digest, name
for name, digest in report['binaries'].items():
    assert sha(Path(name)) == digest, name

configs = report['configs']
assert configs == ['original', 'before', 'after', 'zstd-3', 'rar-5', '7z-9', '7z-delta-5']
groups = defaultdict(list)
for row in report['rows']:
    assert row['sha256_verified']
    groups[row['dataset'], row['config']].append(row)
mem = {(g['dataset'], g['config']): g['rows'] for g in memory['groups']}
assert len(mem) == len(memory['groups']) == len(corpus['datasets'])*3
assert len(report['rows']) == len(corpus['datasets'])*len(configs)*3
assert len(groups) == len(corpus['datasets'])*len(configs)
assert memory['iterations'] == 7 and report['repetitions'] == 3
for d in corpus['datasets']:
    name = d['name']
    assert sha(ROOT/d['path']) == d['sha256'], name
    for c in configs:
        rows = groups[name, c]
        assert sorted(r['repeat'] for r in rows) == [0, 1, 2]
        assert all(r['input_sha256'] == d['sha256'] and r['input_bytes'] == d['bytes'] for r in rows)
        assert len({r['output_bytes'] for r in rows}) == 1
        if c in ['before', 'after']:
            assert all(r['archive_sha256'] == report['expected_dense_sha256'][name] for r in rows)
    for c in ['original', 'before', 'after']:
        rows = mem[name, c]
        assert [r['run'] for r in rows] == list(range(7))
        assert all(r['verified'] and r['output_bytes'] == groups[name, c][0]['output_bytes'] for r in rows)
        assert len({r['output_xxh64'] for r in rows}) == 1
    assert mem[name, 'before'][0]['output_xxh64'] == mem[name, 'after'][0]['output_xxh64']


def med(d, c, key):
    return median(r[key] for r in groups[d, c])


def ratio(d, c):
    return 100*med(d, c, 'output_bytes')/groups[d, c][0]['input_bytes']


def codec(d, c):
    return median(r['encode']['codec_ms'] for r in groups[d, c])


def warm(d, c, key='encode_ms'):
    return median(r[key] for r in mem[d, c][1:])


def reduction(before, after):
    return 100*(1-after/before)


def table(headers):
    return ['| '+' | '.join(headers)+' |', '|---|'+'---:|'*(len(headers)-1)]


labels = ['Original numeric', 'Dense before', 'Dense optimized', 'Zstd 3', 'RAR 5', '7z Ultra', '7z Delta 5']
lines = ['# lttl Dense optimization results', '',
         '**Compression is unchanged:** every before/after file is byte-identical to the frozen previous Dense output '
         '(SHA-256 checked). All 294 file creation/extraction pairs and 294 in-memory round trips passed recovery checks. '
         'The release example passed 16 tests.', '',
         'Intel Core Ultra 9 185H; eight requested CPU workers. Actual RAR 7.20.3 and 7-Zip 26.04. '
         'All Rust controls share one executable. This is a CPU research prototype, not a production or GPU rollout. '
         '[Methodology](README.md), [file measurements](results.json), [memory measurements](memory.json).', '',
         '## Outcome', '']
for d, label in [('mitbih-original-212', 'Original packed ECG'), ('eeg-edf-original', 'Original EDF EEG'),
                 ('ptb-12lead-original', 'Original 12-channel ECG'), ('terrain-hgt-be', 'Original big-endian terrain'),
                 ('raw-sensors-64MiB', '64 MiB synthetic sensors')]:
    lines.append(f"* **{label}:** warm codec time {warm(d,'before'):.2f} → {warm(d,'after'):.2f} ms "
                 f"({reduction(warm(d,'before'),warm(d,'after')):.1f}% lower). Original numeric: {warm(d,'original'):.2f} ms. "
                 f"Output remains {ratio(d,'after'):.2f}% of input, versus {ratio(d,'original'):.2f}% original, "
                 f"{ratio(d,'rar-5'):.2f}% RAR, {ratio(d,'7z-9'):.2f}% 7z Ultra and {ratio(d,'7z-delta-5'):.2f}% filtered 7z.")
lines += ['', f"On the 64 MiB sensor file, complete creation takes {med('raw-sensors-64MiB','after','create_ms'):.1f} ms "
          f"for optimized Dense, {med('raw-sensors-64MiB','rar-5','create_ms'):.1f} ms for RAR, "
          f"{med('raw-sensors-64MiB','7z-9','create_ms'):.1f} ms for 7z Ultra and "
          f"{med('raw-sensors-64MiB','7z-delta-5','create_ms'):.1f} ms for filtered 7z. "
          'The filtered 7z output remains slightly smaller. These compare complete process times at the recorded settings.', '',
          'The optimization removes a substantial part of Dense overhead, but it does not eliminate the cost of entropy '
          'compression. Several inputs still encode more slowly than the original numeric codec. Stored noise benefits from '
          'avoiding redundant copies; that is not a compression improvement. Text selects ordinary Zstd, while the forced '
          'original numeric codec stores it, so their speed comparison must be read alongside size. '
          'File-to-file savings are smaller and less consistent than codec savings: startup and IO can dominate, '
          'and some small-file process medians barely improve or regress despite lower codec time.', '',
          '## Codec time: fresh process and warm worker pool', '',
          'Milliseconds; lower is faster. Fresh values are medians of three independent process jobs, excluding IO and pool startup. '
          'Warm values are medians of six consecutive jobs after one first job, also excluding IO. Both include selection and checksum. '
          'Reduction compares optimized Dense to frozen Dense, not to original numeric. These are elapsed times, not total CPU usage.', '']
lines += table(['Input', 'Fresh original', 'Fresh before', 'Fresh optimized', 'Reduction',
                'Warm original', 'Warm before', 'Warm optimized', 'Reduction'])
for d in corpus['datasets']:
    n = d['name']
    lines.append('| '+n+' | '+' | '.join(f'{codec(n,c):.2f}' for c in configs[:3])+
                 f" | {reduction(codec(n,'before'),codec(n,'after')):.1f}% | "+
                 ' | '.join(f'{warm(n,c):.2f}' for c in configs[:3])+
                 f" | {reduction(warm(n,'before'),warm(n,'after')):.1f}% |")
lines += ['', '![Warm CPU codec times and observed ranges](codec-time.png)', '',
          '[Vector chart](codec-time.svg). Error bars are observed minimum and maximum, not confidence intervals.', '',
          '## Compression ratio', '', 'Output bytes / input bytes, percent. Headers included; lower is smaller. '
          'Frozen and optimized Dense have exactly the same bytes, not just rounded ratios.', '']
lines += table(['Input']+labels)
for d in corpus['datasets']:
    lines.append('| '+d['name']+' | '+' | '.join(f"{ratio(d['name'],c):.2f}%" for c in configs)+' |')
for key, title in [('create_ms', 'Creation'), ('extract_ms', 'Extraction')]:
    lines += ['', f'## {title}: complete process time', '',
              'Milliseconds, medians of three runs. Common boundary: process launch through exit plus output fsync. '
              'Includes IO. RAR/7z and Rust are compared only at this common boundary. Decoder logic was not optimized.', '']
    lines += table(['Input']+labels)
    for d in corpus['datasets']:
        lines.append('| '+d['name']+' | '+' | '.join(f"{med(d['name'],c,key):.1f}" for c in configs)+' |')
lines += ['', '## Warm phase observations', '',
          'Medians in milliseconds. Selection includes the parallel candidate trials. Chunk wall time includes transform and entropy '
          'work. Worker sums overlap and are **not** additive to wall time or measurements of CPU utilization. Framing includes '
          'checksum and output assembly. Zero detailed phases mean the stored/plain-Zstd fallback is uninstrumented.', '']
lines += table(['Input', 'Before selection', 'Optimized selection', 'Chunk wall', 'Transform worker sum', 'Entropy worker sum', 'Framing'])
for d in corpus['datasets']:
    n = d['name']
    values = [median(r['plan']['selection_ms'] for r in mem[n,c][1:]) for c in ['before','after']]
    values += [median(r['plan']['profile'][key] for r in mem[n,'after'][1:])
               for key in ['chunks_wall_ms','transform_worker_ms','entropy_worker_ms','framing_ms']]
    lines.append('| '+n+' | '+' | '.join(f'{v:.2f}' for v in values)+' |')
lines += ['', '## Warm-job variability and first-job cost', '',
          'First job is reported separately; it is not a guaranteed cold hardware/OS cache measurement. '
          'Warm ranges cover jobs 1–6. All values are codec milliseconds.', '']
lines += table(['Input / method', 'First encode', 'Warm encode median', 'Warm encode range', 'Warm decode median', 'Warm decode range'])
for d in corpus['datasets']:
    for c in configs[:3]:
        rows = mem[d['name'],c]; enc = [r['encode_ms'] for r in rows[1:]]; dec = [r['decode_ms'] for r in rows[1:]]
        lines.append(f"| {d['name']} / {c} | {rows[0]['encode_ms']:.2f} | {median(enc):.2f} | {min(enc):.2f}–{max(enc):.2f} "
                     f"| {median(dec):.2f} | {min(dec):.2f}–{max(dec):.2f} |")
lines += ['', '## Every file configuration and observed variability', '']
for d in corpus['datasets']:
    n = d['name']
    lines += [f'### {n}', '', f"{d['bytes']:,} input bytes. {d['description']}", '']
    lines += table(['Method', 'Output bytes', 'Create median', 'Create range', 'Extract median', 'Extract range'])
    for c in configs:
        rows = groups[n,c]; enc = [r['create_ms'] for r in rows]; dec = [r['extract_ms'] for r in rows]
        lines.append(f"| {c} | {med(n,c,'output_bytes'):,.0f} | {median(enc):.1f} | {min(enc):.1f}–{max(enc):.1f} "
                     f"| {median(dec):.1f} | {min(dec):.1f}–{max(dec):.1f} |")
    lines.append('')
lines += ['## Limits', '',
          '* These are development-corpus observations; no holdout or multi-machine confirmation. Power, thermal state, '
          'OS cache, CPU placement and background activity were uncontrolled. The sample counts do not establish statistical significance.',
          '* AVX2 targets 16-bit byte-plane transforms; wider and packed representations retain their existing transform paths. '
          'Other architectures use the scalar fallback and were not measured.',
          '* Context reuse retains worker memory. Peak memory, energy and aggregate CPU usage were not measured.',
          '* Competitor dictionaries/memory are not normalized; 7z Delta uses preconfigured distances outside its timer. '
          'No exhaustive competitor tuning, multi-file solid archive, encryption or GUI is measured.',
          '* The original codec has less layout flexibility and no Dense entropy stage. Better speed alone does not imply a better size/speed tradeoff.',
          '* This improves CPU Dense only. Production Auto, Windows UI, GPU code and installer are unchanged.', '']
(HERE/'measurements.md').write_text('\n'.join(lines), encoding='utf-8')
print('Validated and summarized', len(report['rows']), 'file pairs and', sum(map(len,mem.values())), 'memory round trips')
