"""Serial, randomized configuration groups: one cold and five reused-session jobs each."""
import hashlib
import json
from pathlib import Path
import platform
import random
import subprocess
import time
import os

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
EXE = ROOT / 'target/release/examples/numeric_gpu_lab.exe'
report = {'completed': False, 'platform': platform.platform(), 'processor': platform.processor(),
          'method': 'One cold job plus five warm jobs per process, serial randomized configuration/dataset groups. '
                    'Compression/recovery include file I/O, checksums and output sync. Cold includes session/pool initialization; '
                    'all exclude executable startup. Codec time includes numeric selection, transfers, synchronization and framing; '
                    'verification and reference recompression are outside timers. Recovery uses the original CPU decoder. '
                    'OS/driver caches are uncontrolled. Original versus converted real samples are separate datasets.',
          'gpu_decompression': False, 'peak_memory_measured': False, 'rows': [], 'groups': [], 'source_sha256': {}}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


report['binary_sha256'] = sha(EXE)
for path in [HERE / 'cpu.rs', HERE / 'gpu.rs', HERE / 'numeric.wgsl', ROOT / 'examples/numeric_gpu_lab.rs', HERE / 'benchmark.py', HERE / 'prepare_corpus.py', ROOT / 'research/codec-lab/codec.rs']:
    report['source_sha256'][str(path.relative_to(ROOT))] = sha(path)
corpus = json.loads((HERE / 'corpus.json').read_text())
report['corpus_manifest_sha256'] = sha(HERE / 'corpus.json')
for dataset in corpus['datasets']:
    assert sha(ROOT / dataset['path']) == dataset['sha256'], dataset['name']
scratch = ROOT / 'bench-output' / ('numeric-gpu-' + str(time.time_ns()))
scratch.mkdir()
report['validation'] = {}
for adapter in ['intel', 'nvidia']:
    validation = json.loads((ROOT / f'.qa/numeric-{adapter}-validation.log').read_text())
    assert validation['gpu_cpu_exact_cases'] == 316 and validation['original_decoder_verified']
    report['validation'][adapter] = validation
configs = [('cpu-numeric-1', 'cpu-numeric', 1), ('cpu-numeric-8', 'cpu-numeric', 8),
           ('zstd-1', 'zstd-1', 8), ('zstd-3', 'zstd-3', 8), ('original-adaptive', 'adaptive', 8),
           ('intel', 'intel', 8), ('nvidia', 'nvidia', 8)]
work = [(dataset, config) for dataset in corpus['datasets'] for config in configs]
random.Random(407013).shuffle(work)
destination = HERE / 'results.json'
if destination.exists():
    previous = json.loads(destination.read_text(encoding='utf-8'))
    assert previous['binary_sha256'] == report['binary_sha256'], 'Binary changed; archive old results before a new experiment'
    assert previous['corpus_manifest_sha256'] == report['corpus_manifest_sha256'], 'Corpus changed'
    for path, digest in previous['source_sha256'].items():
        if path != str(Path(__file__).resolve().relative_to(ROOT)):
            assert report['source_sha256'][path] == digest, ('Source changed', path)
    report['rows'], report['groups'] = previous['rows'], previous['groups']
    report['harness_history_sha256'] = list(dict.fromkeys(previous.get('harness_history_sha256', []) +
        [previous['source_sha256'][str(Path(__file__).resolve().relative_to(ROOT))], sha(Path(__file__))]))
    report['resume_note'] = 'Resumed verified completed groups after a checkpoint-write failure. Only persistence/resume logic changed; codec binary, corpus, configurations and timing boundaries are unchanged.'
done = {(g['dataset'], g['config']) for g in report['groups']}


def save():
    temporary = destination.with_suffix('.json.partial')
    payload = json.dumps(report, indent=2) + '\n'
    for attempt in range(10):
        try:
            temporary.write_text(payload, encoding='utf-8')
            os.replace(temporary, destination)
            return
        except OSError:
            if attempt == 9:
                raise
            time.sleep(0.1 * (attempt + 1))


for index, (dataset, (label, backend, threads)) in enumerate(work):
    if (dataset['name'], label) in done:
        continue
    command = [str(EXE), 'bench', str(ROOT / dataset['path']), backend, str(threads), str(scratch / str(index)), '6']
    started = time.perf_counter()
    result = subprocess.run(command, capture_output=True, text=True, timeout=180, cwd=ROOT)
    assert result.returncode == 0, (dataset['name'], label, result.stderr, result.stdout)
    group = json.loads(result.stdout)
    report['groups'].append({'dataset': dataset['name'], 'config': label, 'process_ms': (time.perf_counter() - started) * 1000,
                            'initialization_ms': group['initialization_ms'], 'adapter': group['adapter'], 'command': command})
    for row in group['rows']:
        assert row['input_sha256'] == dataset['sha256'] and row['sha256_verified']
        assert label not in ['intel', 'nvidia'] or row['gpu_cpu_identical']
        report['rows'].append({'dataset': dataset['name'], 'config': label, **row})
    save()
    print(f"{index + 1}/{len(work)} {dataset['name']} {label} cold={group['rows'][0]['create_ms']:.2f} "
          + ' warm=' + ','.join(f"{r['create_ms']:.2f}" for r in group['rows'][1:]), flush=True)
for dataset in corpus['datasets']:
    rows = [r for r in report['rows'] if r['dataset'] == dataset['name'] and r['config'] in ['cpu-numeric-1', 'cpu-numeric-8', 'intel', 'nvidia']]
    assert len({r['archive_sha256'] for r in rows}) == 1, ('Different numeric formats', dataset['name'])
report['all_numeric_outputs_identical'] = True
report['completed'] = True
save()
print('Completed ' + str(destination), flush=True)
