import json, statistics
from pathlib import Path

root=Path(__file__).resolve().parents[1]
report=json.loads((root/'docs/portable-v2-results.json').read_text())
rows=report['rows']
assert len(rows)==360 and all(r['sha256_verified'] for r in rows)
assert report['gpu_archives_bit_identical_to_v1']
names=sorted({r['dataset'] for r in rows})
def select(dataset,config,warm=False):
    found=[r for r in rows if r['dataset']==dataset and r['config']==config and (not warm or r['run']>0)]
    assert found
    return found
def median(dataset,config,get,warm=False):return statistics.median(get(r) for r in select(dataset,config,warm))
def time(dataset,config,warm=False):return median(dataset,config,lambda r:r['stats']['elapsed_seconds'],warm)
def size(dataset,config):return select(dataset,config)[0]['stats']['output_bytes']
def ratio(dataset,config):return select(dataset,config)[0]['ratio']*100
def metric(dataset,config,key,warm=False):return median(dataset,config,lambda r:r['stats']['portable'][key],warm)
def cold(dataset,vendor):
    return statistics.median(r['stats']['elapsed_seconds'] for r in rows if r['dataset']==dataset and r['config'] in [vendor+'-v2',vendor+'-v2-cold'] and r['run']==0)
def cpu(dataset):return min(time(dataset,c,True) for c in ['cpu-lz4-1','cpu-lz4-8'])
lines=['# Portable GPU optimization results — 2026-10-12','',
       'All **360 archive creation/extraction pairs** passed. Every extracted file was checked against the corpus SHA-256 manifest. Optimized GPU archives are **bit-for-bit identical to v1** across all nine datasets, both GPUs, cold contexts and reused sessions. Compression ratio did not regress.','',
       '[Implementation and reproduction](portable-optimization.md) · [Raw measurements](portable-v2-results.json)','',
       '## Complete archive time, seconds','',
       'These are archive-operation wall times, including transfers, checksums, framing, file reads/writes, sync and publication. They exclude CLI process startup. V1 columns are medians of three fresh-process operations. Warm v2 columns are medians of three subsequent jobs sharing a GPU session. CPU columns use the same three subsequent jobs in one process; CPU pools are still recreated by the existing implementation.','',
       '| Dataset | Faster CPU LZ4 | CPU Zstd Fast | Intel v1 | Intel v2 warm | RTX v1 | RTX v2 warm |',
       '|---|---:|---:|---:|---:|---:|---:|']
for d in names:
    values=[cpu(d),time(d,'cpu-zstd-fast',True),time(d,'intel-v1'),time(d,'intel-v2',True),time(d,'nvidia-v1'),time(d,'nvidia-v2',True)]
    lines.append('| '+d+' | '+' | '.join(f'{v:.4f}' for v in values)+' |')
lines+=['','## Cold contexts and session reuse','',
        'Fresh-process v2 is the median of two first-job measurements (one from each v2 group). Recreated-context is the median of two later jobs that deliberately discard the cache; driver/instance startup is already warm. Cached lookup includes no GPU enumeration, pipeline creation or buffer allocation. Cold statistics here have fewer samples than warm statistics and should not be treated as precise estimates.','',
        '| Dataset | Intel fresh (s) | Intel recreated (s) | Intel acquire warm (µs) | RTX fresh (s) | RTX recreated (s) | RTX acquire warm (µs) |',
        '|---|---:|---:|---:|---:|---:|---:|']
for d in names:
    values=[]
    for v in ['intel','nvidia']:
        values += [f'{cold(d,v):.4f}',f'{time(d,v+"-v2-cold",True):.4f}',f'{metric(d,v+"-v2","session_acquire_seconds",True)*1e6:.2f}']
    lines.append('| '+d+' | '+' | '.join(values)+' |')
lines+=['','## GPU kernel and transfer timing','',
        'Each GPU cell is **v1 kernel → v2 warm kernel / v2 upload / v2 readback**, in milliseconds. Kernel timestamps isolate the shader change; complete archive times above include all host work. V2 uses persistent MAP_WRITE upload buffers. V1 mapped-at-creation allocations could perform staging before the timestamped copy, so upload-copy figures are not an isolated like-for-like transfer comparison.','',
        '| Dataset | Intel (ms) | RTX (ms) |','|---|---:|---:|']
speedups={v:[] for v in ['intel','nvidia']}
for d in names:
    cells=[]
    for v in ['intel','nvidia']:
        before=metric(d,v+'-v1','compression_ms');after=metric(d,v+'-v2','compression_ms',True)
        speedups[v].append(before/after)
        cells.append(f'{before:.2f} → {after:.2f} / {metric(d,v+"-v2","host_to_device_ms",True):.2f} / {metric(d,v+"-v2","device_to_host_ms",True):.2f}')
    lines.append('| '+d+' | '+' | '.join(cells)+' |')
lines+=['','## Intel execution selection','',
        'The bounded host match-density probe is included in full archive wall time. Dense batches retain the v1 kernel on Intel; sparse batches use the optimized cooperative kernel. Counts below are medians per archive. NVIDIA uses the cooperative kernel without this data probe.','',
        '| Dataset | Serial batches | Cooperative batches | Host selection (ms) |','|---|---:|---:|---:|']
for d in names:
    lines.append(f'| {d} | {metric(d,"intel-v2","serial_batches",True):.0f} | {metric(d,"intel-v2","cooperative_batches",True):.0f} | {metric(d,"intel-v2","kernel_selection_seconds",True)*1000:.3f} |')
lines+=['','## Compression ratio','',
        'Output as a percentage of source-file bytes. GPU v1 and v2 output hashes match exactly. Keeping v1 ratio is distinct from matching the CPU encoder: GPU output remains larger on these fixtures. No tolerance is used for the strict no-ratio-sacrifice comparison.','',
        '| Dataset | CPU LZ4 | GPU LZ4 v1 = v2 | Extra GPU bytes vs CPU LZ4 | CPU Zstd Fast | CPU Zstd Balanced |','|---|---:|---:|---:|---:|---:|']
for d in names:
    lines.append(f'| {d} | {ratio(d,"cpu-lz4-8"):.4f}% | {ratio(d,"nvidia-v2"):.4f}% | {size(d,"nvidia-v2")-size(d,"cpu-lz4-8"):,} | {ratio(d,"cpu-zstd-fast"):.4f}% | {ratio(d,"cpu-zstd-balanced"):.4f}% |')
lines+=['','## Decision','']
summary={}
for v in ['intel','nvidia']:
    faster=[d for d in names if time(d,v+'-v2',True)<cpu(d)]
    robust=[d for d in names if time(d,v+'-v2',True)<=cpu(d)*.95]
    joint=[d for d in robust if size(d,v+'-v2')<=size(d,'cpu-lz4-8')]
    summary[v]={'warm_faster_than_cpu_lz4':faster,'warm_at_least_5_percent_faster':robust,'strict_speed_and_ratio_wins':joint,
                'kernel_speedup_min':min(speedups[v]),'kernel_speedup_max':max(speedups[v])}
    lines.append(f'- {v}: kernel speedup **{min(speedups[v]):.2f}–{max(speedups[v]):.2f}×**. Warm operation faster than the better 1/8-worker CPU LZ4 median on **{len(faster)}/9** datasets; at least 5% faster on **{len(robust)}/9**; at least 5% faster with no larger archive on **{len(joint)}/9**.')
lines+=['','Auto eligibility requires an end-to-end speed improvement without larger output. Preserving the previous GPU ratio alone does not establish that. CPU remains the conservative default when this joint condition is unmet. The original TAR.ZST Auto policy is unchanged.','',
        'Warm-session benefits apply to repeated jobs in the desktop process, library callers, and `benchmark-session`. Separate `lttl create` invocations each start a new process and cannot share this cache. No daemon or background service was installed.','',
        f'V1 executable: `{report["v1_sha256"]}`. V2 executable: `{report["v2_sha256"]}`.',
        f'Verified extracted files: **{sum(r["extraction"]["files"] for r in rows):,}**.']
(root/'docs/portable-v2-summary.json').write_text(json.dumps(summary,indent=2),encoding='utf-8')
(root/'docs/portable-v2-results.md').write_text('\n'.join(lines)+'\n',encoding='utf-8')
print(json.dumps(summary,indent=2))
