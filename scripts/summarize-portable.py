"""Generate tables directly from the completed portable benchmark."""
import json, statistics
from pathlib import Path

root=Path(__file__).resolve().parents[1]
report=json.loads((root/'docs/portable-benchmark-results.json').read_text())
rows=report['rows']
assert len(rows)==243 and all(row['sha256_verified'] for row in rows)
names=sorted({row['dataset'] for row in rows})
def group(dataset,config):
    selected=[row for row in rows if row['dataset']==dataset and row['config']==config]
    assert len(selected)==3
    return selected
def median(dataset,config,field):
    return statistics.median(field(row) for row in group(dataset,config))
def wall(dataset,config):return median(dataset,config,lambda row:row['create_process_seconds'])
def ratio(dataset,config):return median(dataset,config,lambda row:row['ratio'])*100
def metric(dataset,config,name):return median(dataset,config,lambda row:row['stats']['portable'][name])

lines=['# Portable GPU benchmark — 2026-10-12','',
       '243/243 complete creation and extraction runs passed SHA-256 verification: nine datasets, nine configurations, three repetitions. All times below are medians. Smaller output percentage means better compression. The CPU is the Intel Core Ultra 9 185H; GPUs are Intel Arc Graphics and NVIDIA RTX 4070 Laptop GPU. Both portable GPU paths used Vulkan.','',
       '[Method, implementation and limitations](portable-gpu.md) · [Raw measurements](portable-benchmark-results.json)','',
       '## Complete archive creation','',
       'Each cell is **fresh-process seconds / output as % of original input**. Includes startup, GPU transfers and filesystem work. LZ4 uses 16 KiB independent blocks and 16 MiB batches; CPU entries here use 8 workers.','',
       '| Dataset | CPU Zstd Fast | CPU LZ4 | Intel LZ4 | RTX LZ4 |','|---|---:|---:|---:|---:|']
for dataset in names:
    cells=[f'{wall(dataset,c):.3f} s / {ratio(dataset,c):.2f}%' for c in ['cpu-zstd-fast','cpu-lz4-8','intel-lz4','nvidia-lz4']]
    lines.append('| '+dataset+' | '+' | '.join(cells)+' |')
lines+=['','## CPU presets, worker count and Auto','',
        'Zstd Balanced uses level 3; Fast uses level 1. CPU LZ4 worker count does not change output. Auto LZ4 performs no sampling or GPU initialization. Its output matches explicit CPU LZ4 in every case.','',
        '| Dataset | Zstd Balanced seconds / output | CPU LZ4 1 worker (s) | CPU LZ4 8 workers (s) | Auto LZ4 (s) |',
        '|---|---:|---:|---:|---:|']
for dataset in names:
    assert {r['stats']['output_bytes'] for r in group(dataset,'auto-lz4')}=={r['stats']['output_bytes'] for r in group(dataset,'cpu-lz4-8')}
    lines.append(f'| {dataset} | {wall(dataset,"cpu-zstd-balanced"):.3f} / {ratio(dataset,"cpu-zstd-balanced"):.2f}% | {wall(dataset,"cpu-lz4-1"):.3f} | {wall(dataset,"cpu-lz4-8"):.3f} | {wall(dataset,"auto-lz4"):.3f} |')
for config,title in [('intel-lz4','Intel Arc'),('nvidia-lz4','RTX 4070')]:
    lines+=['',f'## {title}: device timing breakdown','',
            'Initialization is host wall time. Copy and kernel intervals are GPU timestamps. These are components, not a replacement for complete creation time; CPU staging, synchronization, checksums, framing and disk work account for additional time. Full worst-case output slots are copied back.','',
            '| Dataset | Init (ms) | Upload (ms) | Kernel (ms) | Readback (ms) | Uploaded / read back (MiB) |',
            '|---|---:|---:|---:|---:|---:|']
    for dataset in names:
        values=[metric(dataset,config,'initialize_seconds')*1000]+[metric(dataset,config,k) for k in ['host_to_device_ms','compression_ms','device_to_host_ms']]
        lines.append('| '+dataset+' | '+' | '.join(f'{v:.2f}' for v in values)+f' | {metric(dataset,config,"h2d_bytes")/1048576:.2f} / {metric(dataset,config,"d2h_bytes")/1048576:.2f} |')
lines+=['','## Smaller GPU blocks','',
        'Each cell is seconds / output percentage. Smaller blocks expose more independent work but sacrifice history and add framing/table overhead. Neither setting is assumed best without measurement.','',
        '| Dataset | Intel 4 KiB | Intel 16 KiB | RTX 4 KiB | RTX 16 KiB |','|---|---:|---:|---:|---:|']
for dataset in names:
    cells=[f'{wall(dataset,c):.3f} / {ratio(dataset,c):.2f}%' for c in ['intel-lz4-4k','intel-lz4','nvidia-lz4-4k','nvidia-lz4']]
    lines.append('| '+dataset+' | '+' | '.join(cells)+' |')
lines+=['','## CPU extraction time','',
        'Fresh-process seconds, including staging and publication. All configurations use CPU extraction. SHA-256 verification occurs after the timed extraction command.','',
        '| Dataset | CPU Zstd Fast archive | CPU LZ4 archive | Intel LZ4 archive | RTX LZ4 archive |','|---|---:|---:|---:|---:|']
for dataset in names:
    cells=[f'{median(dataset,c,lambda row:row["extract_process_seconds"]):.3f}' for c in ['cpu-zstd-fast','cpu-lz4-8','intel-lz4','nvidia-lz4']]
    lines.append('| '+dataset+' | '+' | '.join(cells)+' |')
wins=[]
for dataset in names:
    for config in ['intel-lz4','intel-lz4-4k','nvidia-lz4','nvidia-lz4-4k']:
        if wall(dataset,config)<min(wall(dataset,'cpu-lz4-1'),wall(dataset,'cpu-lz4-8')):wins.append((dataset,config))
lines+=['','## Auto decision','',f'Portable GPU configurations beat the faster CPU LZ4 baseline in **{len(wins)} of 36** dataset/configuration median comparisons. Auto therefore retains CPU LZ4 and avoids GPU enumeration, compilation and sampling. This is a measured exclusion, not a GPU performance claim. TAR.ZST Auto remains unchanged.','',
        'The dominant GPU cost is the current match/token kernel and initialization, not the measured bulk copies. Smaller blocks alone do not solve this. A faster implementation needs parallel token construction and better match finding; warm session reuse and output compaction are additional opportunities. The present results do not establish performance on AMD or Apple silicon.','',
        f'Executable SHA-256: `{report["executable_sha256"]}`.']
(root/'docs/portable-results.md').write_text('\n'.join(lines)+'\n',encoding='utf-8')
print('GPU wins:',wins)
print('Verified files:',sum(row['extraction']['files'] for row in rows))
