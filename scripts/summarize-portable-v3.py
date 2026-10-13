import json, statistics
from pathlib import Path
root=Path(__file__).resolve().parents[1]
report=json.loads((root/'docs/portable-v3-results.json').read_text(encoding='utf-8'))
rows=report['rows'];assert len(rows)==468 and all(r['sha256_verified'] for r in rows)
assert report['gpu_archives_bit_identical_to_v2']
names=sorted({r['dataset'] for r in rows})
def sel(d,c,warm=True):return [r for r in rows if r['dataset']==d and r['config']==c and (not warm or r['run']>0)]
def med(d,c,get,warm=True):return statistics.median(get(r) for r in sel(d,c,warm))
def sec(d,c):return med(d,c,lambda r:r['stats']['elapsed_seconds'])
def size(d,c):return sel(d,c)[0]['stats']['output_bytes']
def ratio(d,c):return sel(d,c)[0]['ratio']*100
def met(d,c,k):return med(d,c,lambda r:r['stats']['portable'][k])
def cpu(d):return min(sec(d,c) for c in ['cpu-lz4-1','cpu-lz4-8'])
lines=['# GPU transfer and pipeline results — 2026-10-12','',
    f'All **{len(rows)} archive creation/extraction pairs** passed, including **{sum(r["extraction"]["files"] for r in rows):,} extracted-file SHA-256 checks**. GPU archive hashes are identical across the previous implementation, all four transfer modes, both GPUs, and cold/warm jobs.','',
    '[Implementation and reproduction](portable-transfers.md) · [Raw measurements](portable-v3-results.json)','',
    '## Complete archive creation','',
    'Seconds; medians of three warm jobs following a cold first job in each process. Includes host staging, GPU transfers, compression, checksums, framing, file I/O, synchronization and publication. Excludes CLI process startup. CPU uses the same boundary. Faster CPU LZ4 is the better of one/eight workers. V2 controls were rerun in this experiment, not copied from the previous report.','',
    '| Dataset | CPU LZ4 | CPU Zstd Fast | Intel v2 | Intel compact pipeline | RTX v2 | RTX compact pipeline |',
    '|---|---:|---:|---:|---:|---:|---:|']
for d in names:
    values=[cpu(d),sec(d,'cpu-zstd-fast'),sec(d,'intel-v2'),sec(d,'intel-compact-pipeline'),sec(d,'nvidia-v2'),sec(d,'nvidia-compact-pipeline')]
    lines.append('| '+d+' | '+' | '.join(f'{x:.4f}' for x in values)+' |')
lines+=['','## Isolating compaction and pipelining','',
    'All four v3 modes write directly from mapped readback memory, avoiding per-block host allocations. Fixed modes transfer complete output slots. Compact modes pack only useful compressed blocks; raw blocks use retained host bytes. Pipeline modes allow two pending batches; serial modes finish each batch before accepting the next. Values are warm archive seconds, not kernel-only times.','',
    '| Dataset | GPU | Fixed serial | Fixed pipeline | Compact serial | Compact pipeline |','|---|---|---:|---:|---:|---:|']
for d in names:
    for v in ['intel','nvidia']:
        values=[sec(d,v+'-'+mode) for mode in ['fixed-serial','fixed-pipeline','compact-serial','compact-pipeline']]
        lines.append('| '+d+' | '+v+' | '+' | '.join(f'{x:.4f}' for x in values)+' |')
lines+=['','## Transfer volume and GPU timing','',
    'Transfer bytes include block-size metadata and padded compressed payload, excluding timing instrumentation (48 bytes per batch in v3, 32 in v2). Incompressible blocks need no payload readback. Compaction, upload and payload-copy timestamps exclude host mapping/waits and metadata-copy time; complete archive times include them. Readback wait is host blocking time, which may include queued compression. Two pending batches establishes bounded pipelining, not proof of simultaneous GPU copy-engine and shader execution.','',
    '| Dataset | GPU | V2 readback MiB | V3 readback MiB | Reduction | Upload ms | Compression ms | Compact ms | Payload copy ms | Host wait ms | Pending peak |',
    '|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|']
summary={}
for d in names:
    for v in ['intel','nvidia']:
        c=v+'-compact-pipeline';before=met(d,v+'-v2','d2h_bytes');after=met(d,c,'d2h_bytes')
        vals=[before/1048576,after/1048576,100*(1-after/before),met(d,c,'host_to_device_ms'),met(d,c,'compression_ms'),met(d,c,'compaction_ms'),met(d,c,'device_to_host_ms'),met(d,c,'readback_wait_seconds')*1000,met(d,c,'max_in_flight')]
        lines.append('| '+d+' | '+v+' | '+' | '.join(f'{x:.3f}' for x in vals)+' |')
lines+=['','## Compression ratio and extraction','',
    'Archive size as percentage of source file bytes (including archive framing/metadata overhead). GPU ratio is unchanged from v2, but that does not imply parity with CPU. Extraction uses the same CPU decoder; values are warm medians in seconds for the default compact pipeline archives.','',
    '| Dataset | CPU LZ4 | GPU LZ4 | Extra GPU bytes | CPU Zstd Fast | Extract Intel archive | Extract RTX archive |','|---|---:|---:|---:|---:|---:|---:|']
for d in names:
    lines.append(f'| {d} | {ratio(d,"cpu-lz4-8"):.4f}% | {ratio(d,"intel-compact-pipeline"):.4f}% | {size(d,"intel-compact-pipeline")-size(d,"cpu-lz4-8"):,} | {ratio(d,"cpu-zstd-fast"):.4f}% | {med(d,"intel-compact-pipeline",lambda r:r["extraction"]["elapsed_seconds"]):.4f} | {med(d,"nvidia-compact-pipeline",lambda r:r["extraction"]["elapsed_seconds"]):.4f} |')
lines+=['','## Cold first jobs','',
    'One first-job sample per configuration; these are descriptive, not stable cold-start estimates. Session caching applies only within the same process. OS/shader caches were not flushed.','',
    '| Dataset | Intel v2 | Intel v3 | RTX v2 | RTX v3 |','|---|---:|---:|---:|---:|']
for d in names:
    vals=[next(r['stats']['elapsed_seconds'] for r in sel(d,c,False) if r['run']==0) for c in ['intel-v2','intel-compact-pipeline','nvidia-v2','nvidia-compact-pipeline']]
    lines.append('| '+d+' | '+' | '.join(f'{x:.4f}' for x in vals)+' |')
lines+=['','## Decision','']
for v in ['intel','nvidia']:
    c=v+'-compact-pipeline';faster=[d for d in names if sec(d,c)<cpu(d)];robust=[d for d in names if sec(d,c)<=.95*cpu(d)]
    joint=[d for d in robust if size(d,c)<=size(d,'cpu-lz4-8')]
    vsold=[sec(d,v+'-v2')/sec(d,c) for d in names]
    summary[v]={'warm_faster_than_cpu_lz4':faster,'warm_at_least_5_percent_faster':robust,'strict_speed_and_ratio_wins':joint,'archive_speedup_vs_v2_min':min(vsold),'archive_speedup_vs_v2_max':max(vsold),
        'readback_reduction_percent_min':min(100*(1-met(d,c,'d2h_bytes')/met(d,v+'-v2','d2h_bytes')) for d in names),'readback_reduction_percent_max':max(100*(1-met(d,c,'d2h_bytes')/met(d,v+'-v2','d2h_bytes')) for d in names)}
    lines.append(f'- {v}: complete warm archive speedup over v2 **{min(vsold):.2f}–{max(vsold):.2f}×**; faster than CPU LZ4 on **{len(faster)}/9** datasets, at least 5% faster on **{len(robust)}/9**, and at least 5% faster with no larger archive on **{len(joint)}/9**.')
lines+=['','The strict joint criterion uses no archive-size tolerance. Auto remains on CPU if that criterion is unmet. Laptop power/thermal state and background activity affect small differences; groups were sequential and randomized, with only three warm observations per group. Raw samples are retained. AMD/Apple hardware was not tested.','',
    f'V2 executable SHA-256: `{report["v2_sha256"]}`. V3: `{report["v3_sha256"]}`.']
(root/'docs/portable-v3-results.md').write_text('\n'.join(lines)+'\n',encoding='utf-8')
(root/'docs/portable-v3-summary.json').write_text(json.dumps(summary,indent=2)+'\n',encoding='utf-8')
print(json.dumps(summary,indent=2))
