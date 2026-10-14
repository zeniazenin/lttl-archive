"""Same-executable comparison of original numeric, frozen Dense and optimized Dense with real archive applications."""
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
EXE = ROOT/'target/release/examples/dense_opt.exe'
SEVEN = ROOT/'tools/7zip-26.04/extra/x64/7za.exe'
RAR = Path('C:/Program Files/WinRAR/Rar.exe')

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def execute(args, sync=None):
    start = time.perf_counter()
    result = subprocess.run([str(x) for x in args], capture_output=True, cwd=ROOT, timeout=300)
    assert result.returncode == 0, (args, result.stdout.decode(errors='replace'), result.stderr.decode(errors='replace'))
    if sync is not None:
        with open(sync, 'r+b') as f:
            f.flush(); os.fsync(f.fileno())
    elapsed = (time.perf_counter()-start)*1000
    try:
        stats = json.loads(result.stdout)
    except ValueError:
        stats = None
    return elapsed, stats, result.stdout.decode(errors='replace')

corpus_path = ROOT/'research/numeric-v2/corpus.json'
corpus = json.loads(corpus_path.read_text())
for d in corpus['datasets']:
    assert sha(ROOT/d['path']) == d['sha256']
baseline_path = ROOT/'research/numeric-v2/results.json'
baseline = json.loads(baseline_path.read_text())
assert baseline['completed']
for filename, digest in baseline['sources'].items():
    assert sha(ROOT/filename) == digest, filename
expected = {r['dataset']:r['archive_sha256'] for r in baseline['rows'] if r['config']=='v2-dense'}
source_files = [Path(__file__), HERE/'memory_benchmark.py', HERE/'codec.rs', ROOT/'examples/dense_opt.rs',
                ROOT/'research/numeric-v2/codec.rs', ROOT/'research/numeric-gpu/cpu.rs', ROOT/'research/codec-lab/codec.rs', ROOT/'Cargo.lock']
configs = ['original', 'before', 'after', 'zstd-3', 'rar-5', '7z-9', '7z-delta-5']
report = dict(completed=False, platform=platform.platform(), configs=configs, repetitions=3,
              corpus_sha256=sha(corpus_path), sources={str(p.relative_to(ROOT)):sha(p) for p in source_files},
              binaries={str(p):sha(p) for p in [EXE,SEVEN,RAR]}, rows=[], delta_plans=baseline['delta_plans'],
              baseline_report_sha256=sha(baseline_path), expected_dense_sha256=expected,
              scope='Fresh process through exit and common output fsync; codec selection, transformations, IO, checksums included. '
              'Same exact bytes within each dataset. Eight requested CPU workers. RAR/7z store one member; research frames payload only. '
              'One executable hosts original, before, after and Zstd controls. No GPU timing. Power, OS cache, CPU placement uncontrolled. No peak memory measurement. '
              '7z Delta distance retained from prior v2 sampling, outside its timed operation. Delta is a byte transform, not the same as word prediction.')
destination = HERE/'results.json'
if destination.exists():
    previous = json.loads(destination.read_text())
    for key in ['sources','binaries','corpus_sha256','configs','repetitions','baseline_report_sha256']:
        assert previous[key] == report[key], ('Experiment changed',key)
    report = previous
scratch = ROOT/'bench-output'/('dense-opt-bench-'+str(time.time_ns()))
scratch.mkdir()

def save():
    partial = destination.with_suffix('.json.partial')
    for attempt in range(10):
        try:
            partial.write_text(json.dumps(report, indent=2)+'\n',encoding='utf-8')
            os.replace(partial,destination); return
        except OSError:
            if attempt == 9: raise
            time.sleep(.1*(attempt+1))

work = [(d,c,n) for d in corpus['datasets'] for c in configs for n in range(3)]
random.Random(20261015).shuffle(work)
done = {(r['dataset'],r['config'],r['repeat']) for r in report['rows']}
for i,(d,config,repeat) in enumerate(work):
    if (d['name'],config,repeat) in done: continue
    path = ROOT/d['path']; job = scratch/str(i); job.mkdir()
    output, restored = job/'output.bin',job/'restored.bin'
    if config == 'rar-5':
        output = output.with_suffix('.rar'); restored = job/path.name
        create = [RAR,'a','-cfg-','-idq','-y','-ma5','-md32m','-m5','-mt8','-s','-ep1',output,path]
        extract = [RAR,'x','-cfg-','-idq','-y',output,str(job)+os.sep]
    elif config.startswith('7z-'):
        level = config[-1]; options = ['-t7z','-mx'+level,'-mmt8','-ms=on']
        if config == '7z-delta-5':
            options += ['-m0=Delta:'+str(report['delta_plans'][d['name']]['distance']),'-m1=LZMA2:d32m']
        else:
            options += ['-m0=LZMA2:d'+('64m' if level == '9' else '32m')]
        output = output.with_suffix('.7z'); restored = job/path.name
        create = [SEVEN,'a',*options,'-y','-bd','-bso0','-bsp0',output,path]
        extract = [SEVEN,'x','-y','-bd','-bso0','-bsp0',output,'-o'+str(job)]
    else:
        exe,method = EXE,config
        create = [exe,'encode',method,path,output]
        extract = [exe,'decode',method,output,restored]
    create_ms,encode,_ = execute(create,output)
    extract_ms,decode,_ = execute(extract,restored)
    assert sha(restored) == d['sha256'], (d['name'],config)
    archive_hash = sha(output)
    if config in ['before','after']:
        assert archive_hash == expected[d['name']], ('Changed Dense bytes',d['name'],config)
    listing = None
    if config.startswith('7z-') and repeat == 0:
        _,_,listing = execute([SEVEN,'l','-slt',output])
        assert 'LZMA2' in listing
        if config == '7z-delta-5': assert 'Delta:'+str(report['delta_plans'][d['name']]['distance']) in listing
    report['rows'].append(dict(dataset=d['name'],config=config,repeat=repeat,input_bytes=d['bytes'],input_sha256=d['sha256'],
        output_bytes=output.stat().st_size,archive_sha256=archive_hash,create_ms=create_ms,extract_ms=extract_ms,
        encode=encode,decode=decode,sha256_verified=True,create_arguments=[str(x) for x in create],
        extract_arguments=[str(x) for x in extract],seven_listing=listing))
    save()
    for generated in [output,restored]:
        assert generated.resolve().is_relative_to(scratch.resolve()); generated.unlink()
    job.rmdir()
    print(f"{i+1}/{len(work)} {d['name']} {config} #{repeat} {create_ms:.1f} ms",flush=True)
report['completed'] = True; save(); scratch.rmdir()
print('Completed '+str(destination),flush=True)
