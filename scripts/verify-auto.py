"""Independent format and missing-runtime checks; run after a release build."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

root = Path(__file__).resolve().parent.parent
qa = root / ".qa"
qa.mkdir(exist_ok=True)
run = Path(tempfile.mkdtemp(prefix="auto-validation-", dir=qa))
env = {**os.environ, "TEMP": str(run), "TMP": str(run)}
exe = root / "target/release/lttl.exe"
corpus = root / "bench-output/auto-corpus"


def invoke(binary, *args, success=True):
    p = subprocess.run([str(binary), *map(str, args)], env=env, capture_output=True, text=True, timeout=120)
    assert (p.returncode == 0) == success, p.stderr
    return json.loads(p.stdout) if success else p.stderr


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


checks = []
for name, goal, backend, stored in [
    ("independent-noise", "balanced", "Cpu", True),
    ("structured-csv", "small", "Gpu", False),
]:
    archive = run / f"{name}.tar.zst"
    stats = invoke(exe, "create", corpus / name, "-o", archive, "--preset", goal)
    choice = stats["selection"]
    assert choice["backend"] == backend and choice["stored"] == stored
    out = run / f"unpacked-{name}"
    out.mkdir()
    p = subprocess.run(["tar", "-xf", str(archive), "-C", str(out)], capture_output=True, text=True, timeout=120)
    assert p.returncode == 0, p.stderr
    sources = list((corpus / name).rglob("*"))
    hashes = 0
    for source in sources:
        if source.is_file():
            assert digest(source) == digest(out / source.relative_to(corpus))
            hashes += 1
    checks.append({"check": "independent-bsdtar", "dataset": name, "sha256_matches": hashes, "stats": stats})

isolated = run / "without-nvidia"
isolated.mkdir()
cpu_exe = isolated / "lttl.exe"
shutil.copy2(exe, cpu_exe)
assert not (isolated / "gpu").exists()
archive = run / "fallback.tar.zst"
stats = invoke(cpu_exe, "create", corpus / "structured-csv", "-o", archive, "--preset", "small")
assert stats["selection"]["backend"] == "Cpu" and stats["selection"]["gpu_unavailable"]
assert "gpu" not in stats
invoke(cpu_exe, "test", archive)
dest = run / "fallback-extracted"
invoke(cpu_exe, "extract", archive, "-d", dest)
count = 0
for source in (corpus / "structured-csv").rglob("*"):
    if source.is_file():
        assert digest(source) == digest(dest / source.relative_to(corpus))
        count += 1
checks.append({"check": "missing-runtime-auto-fallback", "sha256_matches": count, "stats": stats})
failed = run / "explicit-gpu-must-fail.tar.zst"
error = invoke(cpu_exe, "create", corpus / "structured-csv", "-o", failed, "--backend", "gpu", success=False)
assert not failed.exists()
checks.append({"check": "explicit-gpu-error-no-output", "error": error})
report = {"executable_sha256": digest(exe), "tar_version": subprocess.check_output(["tar", "--version"], text=True).strip(), "checks": checks}
(run / "results.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
print(json.dumps({"passed": len(checks), "report": str(run / "results.json")}, indent=2))
