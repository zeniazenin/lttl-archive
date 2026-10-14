"""Fetch the official standalone 7-Zip package into ignored local tools, no system install."""
import hashlib
import json
from pathlib import Path
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'tools/7zip-26.04'
DEST.mkdir(parents=True, exist_ok=True)
rows = []
for name in ['7zr.exe', '7z2604-extra.7z']:
    url = 'https://github.com/ip7z/7zip/releases/download/26.04/' + name
    path = DEST / name
    if not path.exists():
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read(20 * 1048576 + 1)
        assert len(data) <= 20 * 1048576
        path.write_bytes(data)
    rows.append({'url': url, 'bytes': path.stat().st_size,
                 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})
(Path(__file__).parent / 'tool-downloads.json').write_text(json.dumps(rows, indent=2) + '\n')
print(json.dumps(rows))
