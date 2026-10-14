"""Measure installed-engine selection and round trips on the preserved numeric corpus."""
import hashlib
import json
import random
from pathlib import Path
from statistics import median
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
EXE = ROOT/'target/release/lttl.exe'
CORPUS = ROOT/'research/numeric-v2/corpus.json'
CONFIGS = {'cpu-fast':('cpu','fast'), 'auto-fast':('auto','fast'),
           'cpu-balanced':('cpu','balanced'), 'auto-balanced':('auto','balanced'),
           'lttl-fast':('lttl-fast','balanced'), 'lttl-dense':('lttl-dense','balanced'),
           'cpu-small':('cpu','small'), 'auto-small':('auto','small')}
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def cli(*args):
    start = time.perf_counter()
    result = subprocess.run([str(EXE),*map(str,args)],capture_output=True,timeout=120)
    assert result.returncode == 0, result.stderr.decode(errors='replace')
    return json.loads(result.stdout), (time.perf_counter()-start)*1000

def main():
    corpus = json.loads(CORPUS.read_text(encoding='utf-8'))
    for d in corpus['datasets']: assert sha(ROOT/d['path']) == d['sha256']
    sources = ['src/numeric.rs','src/native_archive.rs','src/lib.rs','src/auto.rs','src/main.rs',
               'src/gpu.rs','Cargo.toml','Cargo.lock','scripts/benchmark-native.py']
    report = dict(completed=False, binary_sha256=sha(EXE), sources={p:sha(ROOT/p) for p in sources},
                  corpus_sha256=sha(CORPUS), configurations=CONFIGS, repetitions=3, rows=[],
                  scope='Production LTTLAR01 container, each original file separately; eight CPU workers, serial randomized jobs. '
                        'All creation timers include sampling, input/output and publication; process_ms includes executable startup. '
                        'Recovered SHA256 checked outside timing. OS cache, thermal state, background activity uncontrolled. '
                        'Auto uses bounded samples and is not guaranteed optimal. These are development-corpus results.')
    work = [(d,c,n) for d in corpus['datasets'] for c in CONFIGS for n in range(3)]
    random.Random(20261016).shuffle(work)
    target = ROOT/'docs/native-codec-benchmark.json'
    with tempfile.TemporaryDirectory(prefix='native-bench-',dir=ROOT/'.qa') as temp:
        scratch = Path(temp)
        for i,(d,c,n) in enumerate(work):
            source = ROOT/d['path']; output = scratch/'archive.lttl'; dest = scratch/'out'
            backend,preset = CONFIGS[c]
            stats, process_ms = cli('create',source,'-o',output,'--format','lttl','--backend',backend,'--preset',preset,'--threads','8')
            decoded, extract_ms = cli('extract',output,'-d',dest)
            assert sha(dest/source.name) == d['sha256']
            report['rows'].append(dict(dataset=d['name'],config=c,repeat=n,input_bytes=d['bytes'],
                output_bytes=output.stat().st_size,archive_sha256=sha(output),process_ms=process_ms,
                extract_process_ms=extract_ms,stats=stats,extract=decoded,verified=True))
            (dest/source.name).unlink(); dest.rmdir(); output.unlink()
            target.write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
            print(f'{i+1}/{len(work)} {d["name"]} {c} {process_ms:.1f} ms {stats["numeric"]["blocks"]}',flush=True)
    report['completed'] = True
    assert sha(EXE) == report['binary_sha256']
    assert all(sha(ROOT/p)==h for p,h in report['sources'].items())
    target.write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    lines = ['# Production lttl codec benchmark','',report['scope'],'',
             f'{len(work)} creation/extraction pairs passed exact SHA-256 recovery checks. Medians of three runs. '
             'Creation times below include sampling and file IO inside the app; process startup is excluded from these columns. '
             'Raw results also contain complete process timings and extraction timings. Lower output/input is smaller.', '',
             '| Input | CPU Fast ms / % | Auto Fast ms / % | CPU Balanced ms / % | Auto Balanced ms / % | lttl Fast ms / % | lttl Dense ms / % | CPU Small ms / % | Auto Small ms / % |',
             '|---|'+'---:|'*8]
    for d in corpus['datasets']:
        cells=[]
        for c in CONFIGS:
            rows=[r for r in report['rows'] if r['dataset']==d['name'] and r['config']==c]
            assert len(rows)==3
            cells.append(f"{median(r['stats']['elapsed_seconds'] for r in rows)*1000:.1f} / {median(r['output_bytes'] for r in rows)/d['bytes']*100:.2f}%")
        lines.append('| '+d['name']+' | '+' | '.join(cells)+' |')
    lines += ['', '## Observed Auto decisions', '', '| Input / goal | Block choices across three runs |', '|---|---|']
    for d in corpus['datasets']:
        for c in ['auto-fast','auto-balanced','auto-small']:
            rows=[r for r in report['rows'] if r['dataset']==d['name'] and r['config']==c]
            choices=sorted({json.dumps(r['stats']['numeric']['blocks'],sort_keys=True) for r in rows})
            lines.append('| '+d['name']+' / '+c+' | '+'; '.join(choices)+' |')
    lines += ['', 'Auto speed decisions use timed probes and can vary with load. Three runs do not establish statistical significance. '
              'Small inputs/text skip numeric trials; block resets can lose dictionary history compared with TAR.ZST. '
              'Prior prototype ratios/timings should not be substituted for these production-container measurements.', '']
    (ROOT/'docs/native-codec-benchmark.md').write_text('\n'.join(lines),encoding='utf-8')
    print('Completed production benchmark',flush=True)

if __name__ == '__main__': main()
