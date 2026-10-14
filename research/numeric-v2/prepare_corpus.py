"""Retain all prior real inputs and add explicitly synthetic wider-layout controls."""
import hashlib
import json
from pathlib import Path
import struct

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
old = ROOT/'research/numeric-gpu/corpus.json'
datasets = json.loads(old.read_text())['datasets']
directory = ROOT/'bench-output/numeric-v2-corpus'
directory.mkdir(exist_ok=True)
for name, description in [
    ('synthetic-u32be-12ch', 'Synthetic big-endian 32-bit, 12 interleaved channels, wrapping counters plus deterministic small noise; 2 MiB.'),
    ('synthetic-u64le-3ch', 'Synthetic little-endian 64-bit, three interleaved counters above 2^53, with wrapping and small deterministic noise; 2 MiB.'),
    ('synthetic-f64be-3ch', 'Synthetic big-endian binary64, three channels, exactly represented increments plus sparse NaN payloads, signed zero and infinities; 2 MiB. No floating point arithmetic in codec.'),
]:
    data = bytearray(); state = 0x123456789abcdef
    width = 4 if 'u32' in name else 8
    for i in range(2*1024*1024//width):
        state ^= (state << 13) & ((1<<64)-1); state ^= state >> 7; state ^= (state << 17) & ((1<<64)-1)
        if 'u32' in name:
            value = ((i//12)*3 + (i%12)*400000003 + (state & 3)) & 0xffffffff
            data.extend(struct.pack('>I', value))
        elif 'u64' in name:
            value = ((1<<64)-100000 + (i//3)*5 + (i%3)*(1<<57) + (state & 3)) & ((1<<64)-1)
            data.extend(struct.pack('<Q', value))
        else:
            if i % 8191 == 0:
                value = [0x7ff8000000000001, 0x8000000000000000, 0x7ff0000000000000, 0xfff0000000000000][(i//8191)%4]
                data.extend(struct.pack('>Q', value))
            else:
                data.extend(struct.pack('>d', (i%3)*1000.0 + (i//3)/256.0 + (state&3)/1024.0))
    path = directory/(name+'.bin')
    if path.exists():
        assert path.read_bytes() == data
    else:
        path.write_bytes(data)
    datasets.append(dict(name=name, path=str(path.relative_to(ROOT)), bytes=len(data),
                         sha256=hashlib.sha256(data).hexdigest(), description=description, provenance='Synthetic; prepare_corpus.py'))
for d in datasets:
    assert hashlib.sha256((ROOT/d['path']).read_bytes()).hexdigest() == d['sha256']
result = dict(parent_manifest_sha256=hashlib.sha256(old.read_bytes()).hexdigest(), datasets=datasets)
text = json.dumps(result, indent=2)+'\n'
destination = HERE/'corpus.json'
if destination.exists():
    assert destination.read_text() == text
else:
    destination.write_text(text)
print('Verified', len(datasets), 'datasets')
