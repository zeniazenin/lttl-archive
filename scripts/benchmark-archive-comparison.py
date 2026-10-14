"""Production lttl versus ZIP/Deflate and RAR, with exact recovery verification."""
import hashlib, json, os, platform, random, subprocess, tempfile, time
from pathlib import Path
from statistics import median
ROOT=Path(__file__).resolve().parents[1]
EXE=ROOT/'target/release/lttl.exe'
RAR=Path('C:/Program Files/WinRAR/Rar.exe')
DEST=ROOT/'docs/archive-comparison.json'
CONFIGS=['lttl-fast','lttl-dense','zip-1','zip-6','rar-3','rar-5']
NAMES=['mitbih-original-212','eeg-edf-original','terrain-hgt-be','raw-sensors-16MiB','enwik8-16MiB']
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def run(args,output):
    start=time.perf_counter()
    p=subprocess.run(list(map(str,args)),capture_output=True,timeout=300)
    assert p.returncode==0,(args,p.stderr.decode(errors='replace'))
    with output.open('r+b') as f: os.fsync(f.fileno())
    return (time.perf_counter()-start)*1000

def main():
    if DEST.exists(): raise SystemExit('Preserve the existing report before rerunning')
    corpus=ROOT/'research/numeric-v2/corpus.json'
    datasets=[d for d in json.loads(corpus.read_text())['datasets'] if d['name'] in NAMES]
    assert len(datasets)==len(NAMES)
    for d in datasets: assert sha(ROOT/d['path'])==d['sha256']
    report=dict(completed=False,platform=platform.platform(),cpu='Intel Core Ultra 9 185H',repetitions=3,
        binaries={str(p):sha(p) for p in [EXE,RAR]},script_sha256=sha(Path(__file__)),corpus_sha256=sha(corpus),
        lttl_version=subprocess.check_output([str(EXE),'--version'],text=True).strip(),
        rar_version=subprocess.run([str(RAR)],capture_output=True,text=True).stdout.splitlines()[:4],
        datasets=datasets,configs=CONFIGS,rows=[],
        scope='Production single-file archives, serial seeded randomized jobs, three repetitions, eight requested CPU workers (ZIP serial). '
        'Whole-process create/extract times plus common output fsync; includes startup, sampling, checksums and IO. '
        'RAR 3/5 with 32 MiB dictionary; ZIP Deflate 1/6. No GPU. OS cache, thermals, placement and background load uncontrolled. '
        'Development corpus, not a holdout or exhaustive tuning. Original numeric representations, no conversion. Exact SHA256 recovery outside timing.')
    jobs=[(d,c,n) for d in datasets for c in CONFIGS for n in range(3)]
    random.Random(20261013).shuffle(jobs)
    with tempfile.TemporaryDirectory(prefix='release-comparison-',dir=ROOT/'.qa') as tmp:
        for i,(d,c,n) in enumerate(jobs):
            job=Path(tmp)/str(i);job.mkdir(); source=ROOT/d['path']; out=job/'archive'; restored=job/'out'/source.name
            if c.startswith('rar'):
                out=out.with_suffix('.rar'); restored.parent.mkdir()
                create=[RAR,'a','-cfg-','-idq','-y','-ma5','-md32m','-m'+c[-1],'-mt8','-ep1',out,source]
                extract=[RAR,'x','-cfg-','-idq','-y',out,str(restored.parent)+os.sep]
            else:
                zip_mode=c.startswith('zip');out=out.with_suffix('.zip' if zip_mode else '.lttl')
                create=[EXE,'create',source,'-o',out,'--format','zip' if zip_mode else 'lttl','--backend','cpu' if zip_mode else c,
                    '--preset','fast' if c=='zip-1' else 'balanced','--threads','8']
                extract=[EXE,'extract',out,'-d',restored.parent]
            cm=run(create,out); em=run(extract,restored)
            assert sha(restored)==d['sha256']
            report['rows'].append(dict(dataset=d['name'],config=c,repeat=n,input_bytes=d['bytes'],output_bytes=out.stat().st_size,
                create_ms=cm,extract_ms=em,create_command=list(map(str,create)),extract_command=list(map(str,extract)),verified=True))
            print(f'{i+1}/{len(jobs)} {d["name"]} {c}: {cm:.1f} ms',flush=True)
    assert all(sha(Path(p))==h for p,h in report['binaries'].items())
    report['completed']=True
    DEST.write_text(json.dumps(report,indent=2)+'\n')
    lines=['# Production archive comparison','',report['scope'],'',
        'Median of three runs; cells show **creation ms / output as % of input**. Lower is better for both. All 90 archive/extraction pairs passed exact recovery.',
        '', '| Input | lttl Fast | lttl Dense | ZIP level 1 | ZIP level 6 | RAR method 3 | RAR method 5 |', '|---|'+'---:|'*6]
    for d in datasets:
        cells=[]
        for c in CONFIGS:
            rows=[r for r in report['rows'] if r['dataset']==d['name'] and r['config']==c]
            cells.append(f"{median(r['create_ms'] for r in rows):.1f} / {median(r['output_bytes'] for r in rows)/d['bytes']*100:.2f}%")
        lines.append('| '+d['name']+' | '+' | '.join(cells)+' |')
    lines+=['','Complete commands, versions, binary/input hashes and all creation/extraction times: [raw report](archive-comparison.json).',
        'Input attribution and original versus converted representations: [corpus manifest](../research/numeric-v2/corpus.json).',
        'lttl Dense combines independently implemented numeric layout detection/prediction with Zstandard; these measurements are CPU results.',
        'Results are workload-specific. Three samples do not establish statistical significance or performance on arbitrary files.','']
    (ROOT/'docs/archive-comparison.md').write_text('\n'.join(lines))
if __name__=='__main__': main()
