"""Same-host codec-only comparison; first job cold, subsequent jobs reuse the pool/contexts."""
import hashlib
import json
from pathlib import Path
import random
import subprocess
import sys

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]
exe=ROOT/'target/release/examples/dense_opt.exe'
corpus=ROOT/'research/numeric-v2/corpus.json'
datasets=json.loads(corpus.read_text())['datasets']
output=Path(sys.argv[1]) if len(sys.argv)>1 else HERE/'memory.json'
iterations=int(sys.argv[2]) if len(sys.argv)>2 else 7
work=[(d,c) for d in datasets for c in ['original','before','after']]
random.Random(407014).shuffle(work)
rows=[]
for d,c in work:
    data=ROOT/d['path']
    assert hashlib.sha256(data.read_bytes()).hexdigest()==d['sha256']
    r=subprocess.run([str(exe),'memory',c,str(data),str(iterations)],capture_output=True,check=True,timeout=120)
    result=json.loads(r.stdout)
    assert len(result['rows'])==iterations and all(x['verified'] for x in result['rows'])
    rows.append(dict(dataset=d['name'],config=c,**result))
    from statistics import median
    print(d['name'],c,round(median(x['encode_ms'] for x in result['rows'][1:]),2),flush=True)
for d in datasets:
    a=[x for g in rows if g['dataset']==d['name'] and g['config']=='after' for x in g['rows']]
    b=[x for g in rows if g['dataset']==d['name'] and g['config']=='before' for x in g['rows']]
    assert len({x['output_xxh64'] for x in a+b})==1,d['name']
    assert len({x['output_bytes'] for x in a+b})==1,d['name']
result=dict(completed=True,iterations=iterations,corpus_sha256=hashlib.sha256(corpus.read_bytes()).hexdigest(),
            binary_sha256=hashlib.sha256(exe.read_bytes()).hexdigest(),groups=rows,
            scope='Same executable, eight-worker pool, memory resident original input, codec including selection/checksum; no IO or process startup. '
                  'Run 0 cold; remaining runs reuse pool and compressor contexts. Exact recovered-byte equality checked outside timing. '
                  'Before/after identical output length and XXH64; separate process benchmark verifies archive SHA256.')
output.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
print('Saved',output)
