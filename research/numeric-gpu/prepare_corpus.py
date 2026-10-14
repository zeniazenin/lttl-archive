"""Download public numeric data; preserve originals and record all reversible conversions."""
from array import array
from datetime import datetime, timezone
import gzip
import hashlib
import json
from pathlib import Path
import sys
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
OUT = ROOT / 'bench-output/numeric-real-corpus'
OUT.mkdir(parents=True, exist_ok=True)
SOURCES = OUT / 'sources'
SOURCES.mkdir(exist_ok=True)
report = {'retrieved_utc': datetime.now(timezone.utc).isoformat(), 'sources': [], 'datasets': []}
lock_path = HERE / 'corpus.json'
old = json.loads(lock_path.read_text()) if lock_path.exists() else None
locked = {s['url']: s['sha256'] for s in old['sources']} if old else {}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def download(url, name):
    dest = SOURCES / name
    if dest.exists():
        data = dest.read_bytes()
    else:
        print('Downloading ' + url, flush=True)
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read(64 * 1024 * 1024 + 1)
        assert len(data) <= 64 * 1024 * 1024
        dest.write_bytes(data)
    if url in locked:
        assert sha(data) == locked[url], ('Source changed', url)
    report['sources'].append({'url': url, 'local_path': str(dest.relative_to(ROOT)), 'bytes': len(data), 'sha256': sha(data)})
    return data


def add(name, data, description, provenance):
    dest = OUT / (name + '.bin')
    dest.write_bytes(data)
    report['datasets'].append({'name': name, 'path': str(dest.relative_to(ROOT)), 'bytes': len(data),
                               'sha256': sha(data), 'description': description, 'provenance': provenance})
    print(name, len(data), sha(data), flush=True)


mit_original = bytearray()
mit_words = array('H')
for record in ['100', '101']:
    base = 'https://physionet.org/files/mitdb/1.0.0/' + record
    header = download(base + '.hea', 'mitdb-' + record + '.hea').decode('ascii')
    assert header.splitlines()[0].split()[1:4] == ['2', '360', '650000']
    assert all(line.split()[1] == '212' for line in header.splitlines()[1:3])
    data = download(base + '.dat', 'mitdb-' + record + '.dat')
    assert len(data) == 650000 * 3
    words = array('H')
    for a, b, c in zip(data[0::3], data[1::3], data[2::3]):
        x, y = a | ((b & 15) << 8), c | ((b >> 4) << 8)
        words.append(x | (0xf000 if x & 0x800 else 0))
        words.append(y | (0xf000 if y & 0x800 else 0))
    rebuilt = bytearray()
    for x, y in zip(words[0::2], words[1::2]):
        rebuilt.extend((x & 255, ((x >> 8) & 15) | ((y >> 4) & 240), y & 255))
    assert rebuilt == data, 'WFDB 212 conversion must be bit-exactly reversible'
    mit_words.extend(words)
    mit_original.extend(data)
if sys.byteorder != 'little':
    mit_words.byteswap()
add('mitbih-original-212', mit_original, 'Original packed ECG records 100 and 101 concatenated in order, no sample conversion.',
    'https://physionet.org/content/mitdb/1.0.0/')
add('mitbih-s16le', mit_words.tobytes(), 'Same records unpacked losslessly from WFDB 212 to interleaved signed 16-bit little-endian samples. Exact 212 repacking verified. Conversion is outside compression timing.',
    'https://physionet.org/content/mitdb/1.0.0/')

eeg = bytearray()
for subject in ['S001', 'S002', 'S003', 'S004']:
    name = subject + 'R03.edf'
    data = download(f'https://physionet.org/files/eegmmidb/1.0.0/{subject}/{name}', name)
    assert data[:8].strip() == b'0' and len(data) > 256
    header_bytes = int(data[184:192]); records = int(data[236:244]); signals = int(data[252:256])
    assert header_bytes == 256 + signals * 256
    samples_at = 256 + signals * 216
    samples = [int(data[samples_at + i * 8:samples_at + (i + 1) * 8]) for i in range(signals)]
    assert len(data) == header_bytes + records * sum(samples) * 2
    eeg.extend(data)
add('eeg-edf-original', eeg, 'Four complete original EDF+ files S001R03 through S004R03 concatenated in order, including headers and annotation channels. No conversion, scaling, duplication or quantization.',
    'https://physionet.org/content/eegmmidb/1.0.0/')

base = 'https://physionet.org/files/ptbdb/1.0.0/patient001/s0010_re'
header = download(base + '.hea', 'ptb-s0010_re.hea').decode('ascii')
assert header.splitlines()[0].split()[1:4] == ['15', '1000', '38400']
data = download(base + '.dat', 'ptb-s0010_re.dat')
assert len(data) == 38400 * 12 * 2
add('ptb-12lead-original', data, 'Original 12-channel signed 16-bit little-endian ECG .dat file; 12-channel stride is deliberately outside the original codec candidate set. No conversion.',
    'https://physionet.org/content/ptbdb/1.0.0/')

url = 'https://s3.amazonaws.com/elevation-tiles-prod/skadi/N37/N37W122.hgt.gz'
packed = download(url, 'N37W122.hgt.gz')
terrain = gzip.decompress(packed)
assert len(terrain) == 3601 * 3601 * 2
add('terrain-hgt-be', terrain, 'Complete N37W122 elevation tile, gzip transport removed. Original big-endian signed 16-bit HGT samples.',
    'https://registry.opendata.aws/terrain-tiles/')
little = bytearray(len(terrain))
little[0::2], little[1::2] = terrain[1::2], terrain[0::2]
reverse = bytearray(len(terrain)); reverse[0::2], reverse[1::2] = little[1::2], little[0::2]
assert reverse == terrain
add('terrain-s16le', little, 'Same complete elevation tile with reversible byte swap to little endian, no quantization. Byte-swap conversion is outside compression timing.',
    'https://registry.opendata.aws/terrain-tiles/')

auto = ROOT / 'bench-output/auto-corpus'
saved = json.loads((auto / 'manifest.json').read_text(encoding='utf-8-sig'))
for dataset, sizes in [('raw-sensors', [1, 16, 64]), ('independent-noise', [16]), ('enwik8', [16])]:
    entry = next(d for d in saved['Datasets'] if d['Name'] == dataset)['Files'][0]
    data = (auto / entry['Path']).read_bytes()
    assert sha(data).lower() == entry['SHA256'].lower()
    for mib in sizes:
        add(dataset + '-' + str(mib) + 'MiB', data[:mib * 1048576],
            f'Existing corpus control: first {mib} MiB. Sensor/noise controls are synthetic; enwik8 is real text.', entry['Path'])

lock_path.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
print('Saved ' + str(lock_path), flush=True)
