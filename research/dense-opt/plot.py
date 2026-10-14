"""Plot observed warm codec latency, separately from archive-tool process timings."""
import json
from pathlib import Path
from statistics import median
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

HERE = Path(__file__).resolve().parent
memory = json.loads((HERE/'memory.json').read_text())
report = json.loads((HERE/'results.json').read_text())
assert memory['completed'] and report['completed']
groups = {(g['dataset'],g['config']):g['rows'][1:] for g in memory['groups']}
inputs = [('mitbih-original-212','Packed ECG · original'), ('mitbih-s16le','ECG · converted 16-bit LE'),
          ('eeg-edf-original','EDF EEG · original'), ('ptb-12lead-original','12-channel ECG · original'),
          ('terrain-hgt-be','Terrain · original BE'), ('terrain-s16le','Terrain · converted LE'),
          ('raw-sensors-1MiB','Synthetic sensors · 1 MiB'), ('raw-sensors-16MiB','Synthetic sensors · 16 MiB'),
          ('raw-sensors-64MiB','Synthetic sensors · 64 MiB'), ('independent-noise-16MiB','Random noise · stored'),
          ('enwik8-16MiB','Text · Zstd fallback'), ('synthetic-u32be-12ch','Synthetic u32 BE · 12 channels'),
          ('synthetic-u64le-3ch','Synthetic u64 LE · 3 channels'), ('synthetic-f64be-3ch','Synthetic f64 BE · 3 channels')]
plt.rcParams.update({'font.family':'DejaVu Sans','font.size':10,'svg.fonttype':'none'})
fig, ax = plt.subplots(figsize=(12,10),facecolor='#fafbfc')
ax.set_facecolor('#fafbfc')
y = np.arange(len(inputs))
colors = ['#8a96a3','#e29b32','#087f8c']
for offset, (method,label,color) in enumerate(zip(['original','before','after'],
                                               ['Original numeric','Dense before','Dense optimized'],colors)):
    times = [[r['encode_ms'] for r in groups[name,method]] for name,_ in inputs]
    med = np.array([median(t) for t in times])
    errors = [med-np.array([min(t) for t in times]), np.array([max(t) for t in times])-med]
    ax.barh(y+(offset-1)*.22, med, height=.19, label=label, color=color, xerr=errors,
            error_kw={'elinewidth':.7,'capsize':2,'ecolor':'#334155'})
ax.set_yticks(y,[label for _,label in inputs]); ax.invert_yaxis()
ax.set_xlim(0,105); ax.set_xticks(np.arange(0,101,20))
ax.set_xlabel('CPU codec encode time (ms) · lower is faster',labelpad=12)
ax.grid(axis='x',alpha=.2); ax.set_axisbelow(True)
ax.tick_params(axis='y',length=0,pad=10)
for spine in ax.spines.values(): spine.set_visible(False)
for i,(name,_) in enumerate(inputs):
    before = median(r['encode_ms'] for r in groups[name,'before'])
    after = median(r['encode_ms'] for r in groups[name,'after'])
    ax.text(103,i,f'{(1-after/before)*100:.0f}% less',va='center',ha='right',color='#086975',fontsize=9)
fig.subplots_adjust(left=.29,right=.97,top=.85,bottom=.18)
fig.text(.035,.955,'lttl Dense: less CPU latency, identical compressed bytes',fontsize=19,weight='bold',color='#152a3b')
fig.text(.035,.922,'Warm worker pool · median of 6 jobs after one first job · error bars show observed min/max',fontsize=11,color='#475569')
fig.legend(loc='upper left',bbox_to_anchor=(.282,.898),ncol=3,frameon=False)
ax.text(103,-.72,'vs Dense before',ha='right',va='bottom',fontsize=8,color='#086975')
fig.text(.035,.045,'Core Ultra 9 185H · 8 workers · sampling, transforms and checksum included; file IO excluded.\n'
         'Before/after outputs match frozen Dense SHA-256 in the file benchmark. Noise is stored; text uses Zstd.\n'
         'Original numeric can be faster with larger output. RAR/7z process-time and ratio comparisons are in measurements.md.',
         fontsize=9,color='#475569',linespacing=1.55)
fig.savefig(HERE/'codec-time.svg')
fig.savefig(HERE/'codec-time.png',dpi=160)
print('Saved codec-time.svg and codec-time.png')
