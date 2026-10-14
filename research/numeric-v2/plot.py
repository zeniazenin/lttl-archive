"""Static publication-style comparison using one timing boundary throughout."""
import json
from pathlib import Path
from statistics import median
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

HERE = Path(__file__).resolve().parent
r=json.loads((HERE/'results.json').read_text())
assert r['completed']
panels=[('mitbih-original-212','Original packed ECG'),('eeg-edf-original','Original EDF EEG'),
        ('ptb-12lead-original','Original 12-channel ECG'),('terrain-hgt-be','Original big-endian terrain'),
        ('raw-sensors-64MiB','Synthetic sensors · 64 MiB'),('enwik8-16MiB','Real text · 16 MiB')]
styles=[('cpu-numeric','v1 numeric','#999999','x'),('v2-fast','lttl Fast','#0072b2','o'),
        ('v2-dense','lttl Dense','#009e73','*'),('zstd-3','Zstd 3','#444444','s'),
        ('rar-5','RAR 5','#d55e00','D'),('7z-9','7z Ultra','#cc79a7','P'),('7z-delta-5','7z Delta 5','#b58100','X')]
plt.rcParams.update({'font.family':'DejaVu Sans','font.size':10})
fig,axes=plt.subplots(3,2,figsize=(12.5,12.2))
fig.subplots_adjust(left=.065,right=.985,top=.93,bottom=.15,hspace=.40,wspace=.17)
for ax,(dataset,title) in zip(axes.flat,panels):
    for config,label,color,marker in styles:
        rows=[x for x in r['rows'] if x['dataset']==dataset and x['config']==config]
        t=[x['create_ms']/1000 for x in rows]; x=median(t)
        y=median(v['output_bytes']/v['input_bytes']*100 for v in rows)
        ax.errorbar(x,y,xerr=[[x-min(t)],[max(t)-x]],color=color,marker=marker,
                    markersize=10 if config=='v2-dense' else 7,linestyle='none',capsize=3,alpha=.9,label=label)
    ax.set_xscale('log'); ax.set_ylim(0,105)
    ax.set_title(title,loc='left',fontweight='bold',pad=10)
    ax.set_xlabel('Creation time (seconds, log scale)'); ax.set_ylabel('Output / input (%)')
    ax.grid(True,which='major',color='#dddddd',linewidth=.6); ax.spines[['top','right']].set_visible(False)
fig.suptitle('lttl compression: layout support and optional dense stage',fontsize=16,fontweight='bold')
handles,labels=axes.flat[0].get_legend_handles_labels()
fig.legend(handles,labels,loc='lower center',bbox_to_anchor=(.5,.065),ncol=4,frameon=False,fontsize=10)
fig.text(.5,.025,'Smaller and faster is toward the lower left. Three-run medians; bars show observed min–max.\n'
         'Times include process startup, selection, transforms, file I/O and flush. Identical original inputs within each panel.',
         fontsize=9,ha='center',va='bottom')
fig.savefig(HERE/'tradeoffs.svg'); fig.savefig(HERE/'tradeoffs.png',dpi=140)
print('Wrote comparison charts')
