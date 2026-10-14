"""Plot measured size/time tradeoffs; cold process timings only, never mixed with warm kernels."""
import json
from pathlib import Path
from statistics import median
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

HERE = Path(__file__).resolve().parent
report = json.loads((HERE / 'results.json').read_text())
assert report['completed']
panels = [('mitbih-s16le', 'Real ECG · converted int16'), ('eeg-edf-original', 'Real EEG · original EDF'),
          ('terrain-s16le', 'Terrain · converted little endian'), ('raw-sensors-64MiB', 'Synthetic sensors · 64 MiB'),
          ('terrain-hgt-be', 'Terrain · original big endian'), ('enwik8-16MiB', 'Real text · 16 MiB')]
styles = [('cpu-numeric', 'Numeric CPU', '#0072b2', 'o'), ('intel', 'Intel GPU (cold)', '#56b4e9', '^'),
          ('nvidia', 'RTX GPU (cold)', '#009e73', 'v'), ('zstd-3', 'Zstd 3', '#444444', 's'),
          ('rar-5', 'RAR 5', '#d55e00', 'D'), ('7z-9', '7z 9', '#cc79a7', 'P'),
          ('7z-delta-5', '7z Delta 5', '#b58100', 'X')]
plt.rcParams.update({'font.family': 'DejaVu Sans', 'font.size': 10})
fig, axes = plt.subplots(3, 2, figsize=(12.5, 12.2))
fig.subplots_adjust(left=0.065, right=0.985, top=0.93, bottom=0.15, hspace=0.40, wspace=0.17)
for ax, (dataset, title) in zip(axes.flat, panels):
    for config, label, color, marker in styles:
        rows = [r for r in report['rows'] if r['dataset'] == dataset and r['config'] == config]
        seconds = [r['create_ms'] / 1000 for r in rows]
        x = median(seconds)
        y = median(r['output_bytes'] / r['input_bytes'] * 100 for r in rows)
        ax.errorbar(x, y, xerr=[[x - min(seconds)], [max(seconds) - x]],
                    color=color, marker=marker, markersize=7, linestyle='none', capsize=3,
                    alpha=0.9, label=label)
    ax.set_xscale('log')
    ax.set_ylim(0, 105)
    ax.set_title(title, loc='left', fontweight='bold', pad=10)
    ax.set_xlabel('Creation time (seconds, log scale)')
    ax.set_ylabel('Output / input (%)')
    ax.grid(True, which='major', color='#dddddd', linewidth=0.6)
    ax.spines[['top', 'right']].set_visible(False)
fig.suptitle('Compression tradeoffs: smaller and faster is toward the lower left', fontsize=16, fontweight='bold')
handles, labels = axes.flat[0].get_legend_handles_labels()
fig.legend(handles, labels, loc='lower center', bbox_to_anchor=(0.5, 0.065), ncol=4, frameon=False, fontsize=10)
fig.text(0.5, 0.025, 'Three-run medians; bars show min–max. Includes process startup, file I/O, sync, and GPU initialization/transfers.\n'
         'Same input within each panel. Lossless ECG/terrain preparation is excluded. Cold and warm timing boundaries are not mixed.',
         fontsize=9, ha='center', va='bottom')
fig.savefig(HERE / 'tradeoffs.svg')
fig.savefig(HERE / 'tradeoffs.png', dpi=140)
print('Saved tradeoffs.svg and tradeoffs.png')
