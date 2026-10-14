"""Same-input process-level codec/application comparison plus separate reused-session controls."""
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import subprocess
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
EXE = ROOT / 'target/release/examples/codec_compare.exe'
SESSION = ROOT / 'target/release/examples/numeric_gpu_lab.exe'
SEVEN = ROOT / 'tools/7zip-26.04/extra/x64/7za.exe'
RAR = Path('C:/Program Files/WinRAR/Rar.exe')


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def execute(args, sync=None):
    started = time.perf_counter()
    result = subprocess.run([str(x) for x in args], capture_output=True, timeout=300, cwd=ROOT)
    assert result.returncode == 0, (args, result.stdout.decode(errors='replace'), result.stderr.decode(errors='replace'))
    if sync is not None:
        with open(sync, 'r+b') as file:
            file.flush()
            os.fsync(file.fileno())
    elapsed = (time.perf_counter() - started) * 1000
    try:
        stats = json.loads(result.stdout)
    except ValueError:
        stats = None
    return elapsed, stats, result.stdout.decode(errors='replace')


corpus_path = ROOT / 'research/numeric-gpu/corpus.json'
corpus = json.loads(corpus_path.read_text())
for data in corpus['datasets']:
    assert sha(ROOT / data['path']) == data['sha256']
prior_gpu = json.loads((ROOT / 'research/numeric-gpu/results.json').read_text())
strides = {r['dataset']: r['stride'] for r in prior_gpu['rows'] if r['config'] == 'cpu-numeric-8'}
source_files = [Path(__file__), ROOT / 'examples/codec_compare.rs', ROOT / 'examples/numeric_gpu_lab.rs',
                ROOT / 'research/numeric-gpu/cpu.rs', ROOT / 'research/numeric-gpu/gpu.rs',
                ROOT / 'research/numeric-gpu/numeric.wgsl', ROOT / 'research/codec-lab/codec.rs']
report = {'completed': False, 'platform': platform.platform(), 'processor': platform.processor(),
          'corpus_sha256': sha(corpus_path), 'sources': {str(p.relative_to(ROOT)): sha(p) for p in source_files},
          'binaries': {str(p): sha(p) for p in [EXE, SESSION, SEVEN, RAR]},
          'versions': {'7zip': '26.04 x64 standalone 7za', 'rar': '7.20.3 x64', 'lz4': 'lz4_flex 0.11.6', 'zstd': 'Rust zstd 0.13 (Cargo.lock)'},
          'cold_repeats': 3, 'warm_jobs': 4, 'rows': [], 'warm': [],
          'scope': 'Identical complete corpus byte streams. Cold timings are process launch through exit plus common output fsync; include GPU startup and transfers. '
                   'Warm numeric runs are reported separately, exclude process launch, and include all codec transfers and file IO. '
                   'RAR/7z/ZIP store one source member; research/LZ4 frames store payload only. Numeric stride selection is included for numeric codecs. '
                   '7z Delta distance uses the same previously sampled stride, preconfigured; its selection is excluded. No encryption. '
                   'OS caches, power and scheduling uncontrolled; serial seeded random order; no measured peak memory.'}
destination = HERE / 'results.json'
if destination.exists():
    previous = json.loads(destination.read_text())
    for key in ['corpus_sha256', 'sources', 'binaries']:
        assert previous[key] == report[key], ('Changed experiment', key)
    report = previous
scratch = ROOT / 'bench-output' / ('codec-comparison-' + str(time.time_ns()))
scratch.mkdir()


def save():
    partial = destination.with_suffix('.json.partial')
    for attempt in range(10):
        try:
            partial.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
            os.replace(partial, destination)
            return
        except OSError:
            if attempt == 9:
                raise
            time.sleep(0.1 * (attempt + 1))


configs = ['cpu-numeric', 'intel', 'nvidia', 'adaptive', 'zstd-1', 'zstd-3', 'zstd-9', 'lz4', 'store',
           'zip-deflate-6', 'rar-3', 'rar-5', '7z-1', '7z-5', '7z-9', '7z-delta-5']
work = [(d, c, repeat) for d in corpus['datasets'] for c in configs for repeat in range(3)]
random.Random(7134070).shuffle(work)
done = {(r['dataset'], r['config'], r['repeat']) for r in report['rows']}
for index, (data, config, repeat) in enumerate(work):
    if (data['name'], config, repeat) in done:
        continue
    path = ROOT / data['path']
    job = scratch / str(index); job.mkdir()
    output = job / 'output.bin'
    restored = job / 'restored.bin'
    commands = []
    if config.startswith('rar-'):
        output = output.with_suffix('.rar'); restored = job / path.name
        create = [RAR, 'a', '-cfg-', '-idq', '-y', '-ma5', '-md32m', '-m' + config[-1], '-mt8', '-s', '-ep1', output, path]
        extract = [RAR, 'x', '-cfg-', '-idq', '-y', output, str(job) + os.sep]
    elif config.startswith('7z-') or config == 'zip-deflate-6':
        if config == 'zip-deflate-6':
            options = ['-tzip', '-mm=Deflate', '-mx6', '-mmt8']
            output = output.with_suffix('.zip')
        else:
            level = config[-1]
            options = ['-t7z', '-mx' + level, '-mmt8', '-ms=on']
            if config == '7z-delta-5':
                options += ['-m0=Delta:' + str(strides[data['name']] * 2), '-m1=LZMA2:d32m']
            else:
                dictionary = {'1': '4m', '5': '32m', '9': '64m'}[level]
                options += ['-m0=LZMA2:d' + dictionary]
            output = output.with_suffix('.7z')
        restored = job / path.name
        create = [SEVEN, 'a', *options, '-y', '-bd', '-bso0', '-bsp0', output, path]
        extract = [SEVEN, 'x', '-y', '-bd', '-bso0', '-bsp0', output, '-o' + str(job)]
    else:
        create = [EXE, 'encode', config, path, output]
        extract = [EXE, 'decode', config, output, restored]
    create_ms, encode, _ = execute(create, output)
    extract_ms, decode, _ = execute(extract, restored)
    assert sha(restored) == data['sha256'], (data['name'], config)
    listing = None
    if config.startswith('7z-') and repeat == 0:
        _, _, listing = execute([SEVEN, 'l', '-slt', output])
        assert 'LZMA2' in listing
        if config == '7z-delta-5':
            assert 'Delta:' + str(strides[data['name']] * 2) in listing
    report['rows'].append({'dataset': data['name'], 'config': config, 'repeat': repeat,
                          'input_bytes': data['bytes'], 'input_sha256': data['sha256'], 'output_bytes': output.stat().st_size,
                          'archive_sha256': sha(output), 'create_ms': create_ms, 'extract_ms': extract_ms,
                          'encode': encode, 'decode': decode, 'sha256_verified': True,
                          'create_arguments': [str(x) for x in create], 'extract_arguments': [str(x) for x in extract],
                          '7z_listing': listing})
    save()
    for path_to_remove in [output, restored]:
        assert path_to_remove.resolve().is_relative_to(scratch.resolve())
        path_to_remove.unlink()
    job.rmdir()
    print(f"{index + 1}/{len(work)} {data['name']} {config} #{repeat} {create_ms:.1f} ms", flush=True)

warm_work = [(d, c) for d in corpus['datasets'] for c in ['cpu-numeric', 'intel', 'nvidia']]
random.Random(4070).shuffle(warm_work)
done_warm = {(r['dataset'], r['config']) for r in report['warm']}
for data, config in warm_work:
    if (data['name'], config) in done_warm:
        continue
    job = scratch / ('warm-' + data['name'] + '-' + config)
    command = [SESSION, 'bench', ROOT / data['path'], config, '8', job, '4']
    process_ms, stats, _ = execute(command)
    assert len(stats['rows']) == 4 and all(r['sha256_verified'] for r in stats['rows'])
    assert all(r['input_sha256'] == data['sha256'] for r in stats['rows'])
    report['warm'].append({'dataset': data['name'], 'config': config, 'process_ms': process_ms,
                           'command': [str(x) for x in command], **stats})
    save(); job.rmdir()
    print('Warm ' + data['name'] + ' ' + config, flush=True)
for data in corpus['datasets']:
    numeric = [r for r in report['rows'] if r['dataset'] == data['name'] and r['config'] in ['cpu-numeric', 'intel', 'nvidia']]
    assert len({r['archive_sha256'] for r in numeric}) == 1
    expected = numeric[0]['archive_sha256']
    assert all(r['archive_sha256'] == expected for g in report['warm'] if g['dataset'] == data['name'] for r in g['rows'])
report['numeric_archives_identical'] = True
report['completed'] = True
save()
print('Completed ' + str(destination), flush=True)
