"""Generate the same-run CPU / original Auto / optimized Auto comparison."""
import json
from pathlib import Path
from statistics import median

root = Path(__file__).resolve().parent.parent
data = json.loads((root / "research/legacy/auto-v2-benchmark-results.json").read_text(encoding="utf-8-sig"))
assert data["Completed"]
rows = data["Results"]
assert len(rows) == 9 * 9 * data["Repeats"]
assert all(r["Succeeded"] and r["IntegrityVerified"] and r["ExtractionWallSeconds"] for r in rows)
names = sorted({r["Dataset"] for r in rows})


def group(name, goal, backend):
    return [r for r in rows if (r["Dataset"], r["Preset"], r["Backend"]) == (name, goal, backend)]


def mid(values, key="WallSeconds"):
    return median(r[key] for r in values)


def select_ms(values):
    return median(r["Stats"]["selection"]["selection_seconds"] for r in values) * 1000


def choice(row):
    c = row["Stats"]["selection"]
    return "CPU stored" if c["stored"] else "GPU 64 KiB" if c["backend"] == "Gpu" else f"CPU {c['preset']}"


retained = sum(mid(group(n, g, "auto"), "ArchiveBytes") == mid(group(n, g, "auto-v1"), "ArchiveBytes") for n in names for g in ["balanced", "small"])
out = ["# Auto sampling optimization — same-run results", "",
       "Policy version 2 removes all Fast-mode sampling, reduces Balanced regions to 64 KiB, skips codec probes when repeated prefixes already determine the choice, and reuses CPU probe context/output buffers. Small retains version 1's sample sizes and GPU decision thresholds. Raw-storage region checks now run only when aggregate compression indicates that storage might be useful, and stop at the first compressible region. See [current policy](auto-compression.md).", "",
       f"**{retained}/18 Balanced/Small dataset-goal combinations retained exactly the same archive byte count as original Auto.** The table below includes every tested dataset, including regressions. File content was hash-verified after extraction; byte-count equality is not a claim that archive headers are byte-identical.", "",
       f"Completed {data['TotalIntegrityChecks']} creation/integrity checks, {data['TotalRoundTrips']} timed extractions, and {int(data['TotalComparedFileHashes']):,} matching file hashes. {data['Repeats']} fresh-process repetitions per case; medians below. All three versions were interleaved within each repetition on the same source hashes, with rotated order and reversed order on even repetitions. Original Auto was preserved before rebuilding, and both binaries used the same adjacent NVIDIA DLLs.", "",
       "RTX 4070 Laptop GPU, Core Ultra 9 185H, Windows x64, eight requested CPU workers. Full process times include file reads/writes, selection, any GPU trials/initialization/transfers, checksums, and publication. Extraction includes staging/copy publication; verification and SHA256 time are outside the timed creation/extraction. Warm filesystem cache, no controlled thermal/power/background-load isolation. Small differences and overlapping ranges do not prove a speed advantage. The corpus is reused for this optimization, not a new held-out evaluation.", "",
       "[Raw results](../research/legacy/auto-v2-benchmark-results.json) · [Reproduction script](../scripts/benchmark-auto-v2.ps1) · [Original policy measurements](auto-performance.md)", ""]
if "MergeNote" in data:
    out += [data["MergeNote"], ""]
for goal in ["fast", "balanced", "small"]:
    out += [f"## {goal.title()} creation", "",
            "Negative time change is less elapsed time; size is the percentage of original file bytes retained, including archive overhead. Lower size is better.", "",
            "| Dataset | CPU ms | Auto v1 ms | Auto v2 ms | v2 vs v1 time | v2 vs CPU time | CPU / v1 / v2 size % | v2 choice |",
            "|---|---:|---:|---:|---:|---:|---:|---|"]
    for name in names:
        cpu, old, new = [group(name, goal, b) for b in ["cpu", "auto-v1", "auto"]]
        ct, ot, nt = map(mid, [cpu, old, new])
        out.append(f"| {name} | {ct*1000:.1f} | {ot*1000:.1f} | {nt*1000:.1f} | {(nt/ot-1)*100:+.1f}% | {(nt/ct-1)*100:+.1f}% | {mid(cpu,'Ratio')*100:.4f} / {mid(old,'Ratio')*100:.4f} / {mid(new,'Ratio')*100:.4f} | {choice(new[0])} |")
    out += ["", "| Dataset | v1 / v2 selection ms | v1 / v2 sampled KiB | CPU / v1 / v2 extraction ms | CPU create range ms | v2 create range ms |", "|---|---:|---:|---:|---:|---:|"]
    for name in names:
        cpu, old, new = [group(name, goal, b) for b in ["cpu", "auto-v1", "auto"]]
        def span(v):
            return f"{min(r['WallSeconds'] for r in v)*1000:.1f}–{max(r['WallSeconds'] for r in v)*1000:.1f}"
        out.append(f"| {name} | {select_ms(old):.3f} / {select_ms(new):.3f} | {old[0]['Stats']['selection']['sampled_bytes']/1024:g} / {new[0]['Stats']['selection']['sampled_bytes']/1024:g} | {mid(cpu,'ExtractionWallSeconds')*1000:.1f} / {mid(old,'ExtractionWallSeconds')*1000:.1f} / {mid(new,'ExtractionWallSeconds')*1000:.1f} | {span(cpu)} | {span(new)} |")
    out.append("")
out += ["## Interpretation", "",
        "Fast selection fell from 9.1–24.1 ms on sampled inputs to about 0.001 ms. Its full creation medians ranged from 6.5% faster to 2.5% slower than explicit CPU Fast, with overlapping observed ranges; this supports removal of the sampling penalty, not a claim that Auto's identical encoder is inherently faster. Relative to original Auto, Fast enwik8 improved 13.9%, raw sensors 12.6%, and CSV 9.0%. Noise was 3.2% slower than original Auto but 2.0% faster than CPU Fast; the old raw-storage shortcut is gone.", "",
        "Balanced selection fell 62.7–87.4% on the eight sampled datasets. CSV kept its 10.4% archive-byte saving over CPU Balanced and took 11.3% less time; JSON kept its 4.3% saving and took 11.9% less time. enwik8 matched CPU Balanced's size and was within 1.2% of its median time. However, raw sensors remained 6.1% slower, precompressed ZIP 2.8% slower, and related files 7.3% slower (while retaining the 83.3% archive-byte saving). Reduced selection cost does not guarantee a full-run speedup in noisy measurements.", "",
        "Small retained all output sizes but not a universal timing improvement: enwik8 was 4.5% slower than original Auto and 7.9% slower than CPU Small in this comparison. GPU JSON had roughly 80% greater median latency than CPU Small while saving 6.5% archive bytes; initialization varied from about 0.1 to 1.9 seconds across GPU observations. These startup spikes affected both Auto versions and are not removed from the results. Small continues to favor size rather than promise low latency.", "",
        "Fast now executes the same CPU encoder/settings as explicit CPU Fast with no content reads or compression trials before encoding. No-sampling does not guarantee identical measured wall time: process launch, cache state, clocks and background work still vary. Version 1's raw-storage alternative for Fast is removed; on noise, the normal CPU frame was slightly smaller in this corpus. Balanced/Small retain raw-storage selection.", "",
        "Balanced screening still has a nonzero cost. A smaller sample can miss content that a larger one would find, and preserving decisions on this corpus is not proof of equal decisions on other files. Related-file routing retains 64 KiB prefix fingerprints and shared CPU history. Small retains expensive size/GPU trials where those can preserve its compression benefit; it is not a promise of CPU-like latency.", "",
        "The largest Auto GPU startup observations are retained in the ranges and raw report. Fresh process does not imply a driver-cold GPU each time. CUDA trial and archive event totals are listed below; both are included in wall time and must not be added to overlapping host timings.", "",
        "| Dataset / version | GPU H2D ms | GPU compression ms | GPU D2H ms | Initialization ms |", "|---|---:|---:|---:|---:|"]
for name in names:
    for backend in ["auto-v1", "auto"]:
        values = group(name, "small", backend)
        if all(r["Stats"].get("gpu") for r in values):
            events = [median(r["Stats"]["gpu"][k] + r["Stats"]["selection"]["gpu_trial"][k] for r in values) for k in ["host_to_device_ms", "compression_ms", "device_to_host_ms"]]
            init = median(r["Stats"]["selection"]["gpu_trial"]["initialize_seconds"] for r in values)*1000
            out.append(f"| {name} / {backend} | {events[0]:.2f} | {events[1]:.2f} | {events[2]:.2f} | {init:.2f} |")
out += ["", "## Reproduce", "", "Preserve the policy-v1 release as `target/release/lttl-auto-v1.exe` before rebuilding version 2. Both use the existing `target/release/gpu` runtime folder. Run:", "", "```powershell", "pwsh -NoProfile -File scripts/benchmark-auto-v2.ps1", "```", "", "The harness validates the retained `bench-output/auto-corpus/manifest.json` and every source hash. Copy the completed run's `results.json` to `research/legacy/auto-v2-benchmark-results.json`, then regenerate this report with `python scripts/summarize-auto-v2.py`. No sources are downloaded or changed by the comparison.", "",
        f"Original Auto executable SHA256: `{data['ReferenceExecutableSHA256']}`.", "",
        f"Optimized executable SHA256: `{data['ExecutableSHA256']}`.", ""]
(root / "docs/auto-optimization.md").write_text("\n".join(out), encoding="utf-8")
print(json.dumps({"cases": len(rows), "retained_balanced_small_sizes": retained, "report": "docs/auto-optimization.md"}))
