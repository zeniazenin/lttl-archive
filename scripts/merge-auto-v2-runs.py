"""Merge disjoint repetitions only after checking build, runtime, and corpus identity."""
import copy
import hashlib
import json
from pathlib import Path
import sys

assert len(sys.argv) >= 3, "Pass all completed source results.json files (in execution order)."
paths = [Path(p).resolve() for p in sys.argv[1:]]
parts = [json.loads(p.read_text(encoding="utf-8-sig")) for p in paths]
first = parts[0]
identity = ["ExecutableSHA256", "ReferenceExecutableSHA256", "Manifest", "Threads", "ChunkMiB", "BatchMiB", "GpuRuntimeFiles", "TimeExtractionEveryRun"]
for part in parts:
    assert part["Completed"]
    assert all(part[k] == first[k] for k in identity), "Cannot combine different builds, runtime files, inputs or settings"
rows = [r for part in parts for r in part["Results"]]
trips = [r for part in parts for r in part["RoundTrips"]]
key = lambda r: (r["Dataset"], r["Backend"], r["Preset"], r["Repeat"])
assert len(set(map(key, rows))) == len(rows), "Duplicate repetitions"
assert set(map(key, rows)) == set(map(key, trips))
assert all(r["Succeeded"] and r["IntegrityVerified"] and r["ExtractionWallSeconds"] for r in rows)
assert all(r["SHA256Matches"] for r in trips)
repeats = sorted({r["Repeat"] for r in rows})
assert repeats == list(range(1, max(repeats) + 1))
datasets = {d["Name"] for d in first["Manifest"]["Datasets"]}
assert set(map(key, rows)) == {(d,b,g,r) for d in datasets for b in ["cpu","auto-v1","auto"] for g in ["fast","balanced","small"] for r in repeats}
for row in rows:
    if row["Backend"] in ["auto", "auto-v1"]:
        assert row["Stats"]["selection"]["policy_version"] == (2 if row["Backend"] == "auto" else 1)
merged = copy.deepcopy(first)
merged.update(Results=rows, RoundTrips=trips, Repeats=max(repeats), StartRepeat=1,
              TotalIntegrityChecks=len(rows), TotalRoundTrips=len(trips),
              TotalComparedFileHashes=sum(r["Files"] for r in trips))
merged["SourceRuns"] = [{"path": str(p), "sha256": hashlib.sha256(p.read_bytes()).hexdigest(),
                        "date_utc": part["DateUTC"], "repetitions": sorted({r["Repeat"] for r in part["Results"]})}
                       for p,part in zip(paths,parts)]
merged["MergeNote"] = "Repetitions 1–3 followed by a separate fourth reversed-order pass, so each Auto version runs first equally often. No observations removed or retimed; raw source paths and hashes retained."
target = Path(__file__).resolve().parent.parent / "docs/auto-v2-benchmark-results.json"
target.write_text(json.dumps(merged, indent=2), encoding="utf-8")
print(f"Merged {len(rows)} verified cases into {target}")
