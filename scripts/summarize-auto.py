"""Rebuild the Markdown tables from retained raw measurements; no dependencies."""
import json
from pathlib import Path
from statistics import median

root = Path(__file__).resolve().parent.parent
data = json.loads((root / "docs/auto-benchmark-results.json").read_text(encoding="utf-8-sig"))
assert data["Completed"] and all(r["Succeeded"] and r["IntegrityVerified"] for r in data["Results"])
rows = data["Results"]
names = sorted({r["Dataset"] for r in rows})


def cases(name, backend, goal):
    return [r for r in rows if r["Dataset"] == name and r["Backend"] == backend and r["Preset"] == goal]


def mid(values, key):
    return median(v[key] for v in values)


out = ["# Auto compression measurements — 2026-10-12", "",
       "Policy version 1 on NVIDIA RTX 4070 Laptop GPU (8 GiB, driver 592.82), Core Ultra 9 185H (22 logical processors), Windows x64. CPU comparisons explicitly requested eight Zstandard workers. Three repetitions per configuration; medians below. Fresh processes include selection, GPU trials/initialization/transfers, checksums, reads/writes/sync/publication, and process launch/exit. Input/output cache is warm; no thermal, power, cold-cache, or background-load isolation. Small timing differences are not statistical proof of improvement.", "",
       f"Completed **{data['TotalIntegrityChecks']} creation/integrity checks, {data['TotalRoundTrips']} timed extractions, and {int(data['TotalComparedFileHashes']):,} matching extracted SHA256 hashes**. Verification and hash time are outside creation/extraction timing. Extraction includes lttl's staging/copy publication. Negative time/byte change is an improvement. Size is archive bytes as a percentage of original file bytes, including framing and checksums; a lower percentage is better.", "",
       "The previous CLI default was CPU Balanced. Compare Auto Balanced with that baseline first. Small and Fast comparisons use the corresponding explicit CPU preset, so goals are not conflated. The original GPU default uses a fixed nvCOMP setting with 1 MiB chunks / 32 MiB batches.", "",
       "[Selection policy and limitations](auto-compression.md) · [Raw results](auto-benchmark-results.json) · [Harness](../scripts/benchmark-auto.ps1)", ""]
for goal in ["balanced", "small", "fast"]:
    out += [f"## {goal.title()}: Auto versus explicit CPU {goal.title()}", "",
            "| Dataset | Auto choice | CPU / Auto create ms | Time change | CPU / Auto size % | Archive byte change | CPU / Auto extract ms | Selection ms |",
            "|---|---|---:|---:|---:|---:|---:|---:|"]
    for name in names:
        cpu, auto = cases(name, "cpu", goal), cases(name, "auto", goal)
        a = auto[0]["Stats"]["selection"]
        choice = "CPU stored" if a["stored"] else "GPU 64 KiB" if a["backend"] == "Gpu" else f"CPU {a['preset']}"
        ct, at, cr, ar = mid(cpu, "WallSeconds"), mid(auto, "WallSeconds"), mid(cpu, "Ratio"), mid(auto, "Ratio")
        selection = median(r["Stats"]["selection"]["selection_seconds"] for r in auto)
        out.append(f"| {name} | {choice} | {ct*1000:.1f} / {at*1000:.1f} | {(at/ct-1)*100:+.1f}% | {cr*100:.4f} / {ar*100:.4f} | {(ar/cr-1)*100:+.3f}% | {mid(cpu,'ExtractionWallSeconds')*1000:.1f} / {mid(auto,'ExtractionWallSeconds')*1000:.1f} | {selection*1000:.2f} |")
    out.append("")
out += ["Balanced: CSV improved both time and size; related files became much smaller at an 8.7% median time cost. JSON and mixed content saved space but took longer. enwik8 and raw sensors retained the same encoder and paid sampling overhead. Canterbury skips sampling; its small time changes reflect process/system variation. Fast showed no reliable speed benefit and mostly regressed: explicit CPU Fast avoids this selection work. Small accepted slightly larger mixed/precompressed outputs in exchange for less CPU search. Stored noise has slightly higher framing overhead.", "",
        "## Actual GPU selections versus the original GPU defaults", "",
        "| Dataset | Original GPU / Auto Small create ms | Time change | Original GPU / Auto size % | Archive byte change |", "|---|---:|---:|---:|---:|"]
for name in ["heldout-json", "structured-csv"]:
    gpu, auto = cases(name, "gpu", "nvcomp-fixed"), cases(name, "auto", "small")
    gt, at, gr, ar = mid(gpu,"WallSeconds"), mid(auto,"WallSeconds"), mid(gpu,"Ratio"), mid(auto,"Ratio")
    out.append(f"| {name} | {gt*1000:.1f} / {at*1000:.1f} | {(at/gt-1)*100:+.1f}% | {gr*100:.4f} / {ar*100:.4f} | {(ar/gr-1)*100:+.2f}% |")
out += ["", "GPU trial and archive event costs below are combined per run, then their medians are taken. These intervals overlap native host-call timing; they must not be added to total wall time. Initialization is included once, under selection. These values do not imply a warm-context-only benchmark.", "",
        "| Dataset | H2D ms | GPU compression ms | D2H ms | Initialization ms | CPU Small create range ms | Auto Small create range ms |", "|---|---:|---:|---:|---:|---:|---:|"]
for name in ["heldout-json", "structured-csv"]:
    cpu, auto = cases(name, "cpu", "small"), cases(name, "auto", "small")
    events = [median(r["Stats"]["gpu"][k]+r["Stats"]["selection"]["gpu_trial"][k] for r in auto) for k in ["host_to_device_ms", "compression_ms", "device_to_host_ms"]]
    init = median(r["Stats"]["selection"]["gpu_trial"]["initialize_seconds"] for r in auto)*1000
    out.append(f"| {name} | {events[0]:.2f} | {events[1]:.2f} | {events[2]:.2f} | {init:.2f} | {min(r['WallSeconds'] for r in cpu)*1000:.1f}–{max(r['WallSeconds'] for r in cpu)*1000:.1f} | {min(r['WallSeconds'] for r in auto)*1000:.1f}–{max(r['WallSeconds'] for r in auto)*1000:.1f} |")
out += ["", "CSV Auto Small was 13.8% faster and 4.42% smaller than CPU Small in this run, with nonoverlapping observed creation ranges. JSON Auto Small was 6.46% smaller; its 3.8% median time improvement had overlapping ranges and is not a reliable demonstrated speed win. CPU Fast/Balanced remained faster than either GPU path for these datasets. A separate first CSV pilot took 2.068 seconds including 1.885 seconds initialization; this matrix is not a driver-cold startup study.", "",
        "## Corpus and provenance", "", "| Dataset | Input bytes | Files |", "|---|---:|---:|"]
for d in data["Manifest"]["Datasets"]:
    out.append(f"| {d['Name']} | {d['InputBytes']:,} | {len(d['Files'])} |")
out += ["", "Canterbury and enwik8 are downloaded public corpora with provenance in [downloaded-file results](downloaded-performance.md). Five synthetic datasets were retained from the earlier GPU baseline. The new heldout JSON and mixed fixtures were generated after policy thresholds were frozen; misleading `.jpg`, `.csv`, and `.bin` extensions exercise byte-based selection. These are bounded experiments, not a representative claim about all file types. Full hashes, arguments, repeat-level selection reports, and component timings are retained in the raw JSON.", "",
        f"Benchmark executable SHA256: `{data['ExecutableSHA256']}`. The final build subsequently changed CLI help text only; its hash is recorded in [validation](validation.md). Generate these tables with `python scripts/summarize-auto.py`. Historical benchmark reports retain their own executable hashes.", ""]
(root / "docs/auto-performance.md").write_text("\n".join(out), encoding="utf-8")
print("Generated docs/auto-performance.md")
