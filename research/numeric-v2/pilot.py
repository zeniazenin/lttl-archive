"""Development-only one-pass sweep. Not the final randomized benchmark."""
import hashlib
import json
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
corpus = json.loads((ROOT/'research/numeric-gpu/corpus.json').read_text())
work = ROOT/'bench-output'/('numeric-v2-pilot-'+str(time.time_ns()))
work.mkdir()
results = []
for dataset in corpus['datasets']:
    for mode in ['fast', 'dense']:
        encoded, restored = work/'encoded.bin', work/'restored.bin'
        command = [str(ROOT/'target/release/examples/numeric_v2.exe'), 'encode', mode,
                   str(ROOT/dataset['path']), str(encoded)]
        r = subprocess.run(command, capture_output=True, check=True)
        stats = json.loads(r.stdout)
        subprocess.run([command[0], 'decode', mode, str(encoded), str(restored)], capture_output=True, check=True)
        assert hashlib.sha256(restored.read_bytes()).hexdigest() == dataset['sha256']
        stats.update(dataset=dataset['name'], mode=mode, ratio=100*encoded.stat().st_size/dataset['bytes'])
        results.append(stats)
        print(json.dumps(stats), flush=True)
        encoded.unlink(); restored.unlink()
(work/'results.json').write_text(json.dumps(results, indent=2)+'\n')
print(work, flush=True)
