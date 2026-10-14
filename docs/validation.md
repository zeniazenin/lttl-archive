# Validation — 2026-10-12

This records early lttl validation. The original CPU-only pass below is superseded where noted by the GPU implementation and benchmark pass.

## Auto sampling optimization pass

- Policy version 2 removes Fast sampling, reduces Balanced regions to 64 KiB, returns related-file choices without codec probes, and reuses independent CPU compression contexts/output buffers. Small's sample sizes and GPU thresholds are unchanged.
- All **26 ordinary tests pass**. New checks prove Fast bypasses sampling and produces bytes identical to explicit CPU Fast, reduced Balanced sampling detects a compressible tail despite a misleading extension, and reused probe contexts match independent fresh-context ratios at levels 1/3/9. The related-file test also checks that no redundant CPU probes run.
- Release build succeeds. Current executable and benchmark SHA256: `BFA3ECC1F79BD550D7C9091D591A08819717FCABACCD538377CACAAE19CC2A5B`. Preserved v1 executable SHA256: `59887D30DFDB846828EAA29E22387B49B3D86D0E356793B93749FB2904F5A929`; both binaries used the same NVIDIA DLL folder.
- Nine datasets × three goals × three implementations × four repetitions completed **324 creation/integrity checks, 324 timed extractions, and 2,988 matching SHA256 hashes**. Repetitions 1–3 and an additional reversed-order fourth pass were merged only after verifying build/runtime/corpus identity and complete, nonduplicate cases. Original source-result paths/hashes and every timing remain in the [raw report](../research/legacy/auto-v2-benchmark-results.json).
- All 18 Balanced/Small dataset-goal combinations retained their v1 archive byte counts. Fast uses zero sample bytes; Balanced selection time fell 62.7–87.4% across sampled datasets. The [full results](auto-optimization.md) retain regressions, extraction times, and GPU startup variability instead of claiming universal speedups.
- The final v2 release passed all four independent format/fallback checks in `scripts/verify-auto.py`: bsdtar extraction/hash equality for raw and GPU output, CPU fallback/test/extraction without NVIDIA DLLs, and explicit-GPU failure without output. Evidence: [auto-v2-validation-results.json](auto-v2-validation-results.json).
- No archive-format, GPU-kernel, decoder, UI-layout, or registry change was needed. The GUI-enabled release uses the updated selector.

## Content-aware implementation pass

- Default CLI/GUI TAR.ZST creation now uses Auto. Explicit CPU/GPU operation and the CPU-only `create()` library convenience function remain available. ZIP retains its prior CPU policy.
- All 24 ordinary tests pass with the GUI-enabled build. Three new tests cover raw-frame round trips and corruption, misleading extensions, related-content selection, small-input behavior, and cancellation during sampling without publication. The optional hardware test remains separately opt-in; actual Auto GPU encoding was exercised by the full matrix.
- Release build succeeds. V1 final executable SHA256: `59887D30DFDB846828EAA29E22387B49B3D86D0E356793B93749FB2904F5A929`. V1 benchmark executable SHA256: `A23BF354EF63084D93A8FDFBF1F90A563D4AC15A92B8AF09BA901315A0F31B20`; only CLI help text changed between those builds, with no compression-policy/encoder change.
- The completed Auto comparison covers nine datasets, seven settings, and three repetitions: **189 creation/integrity checks, 189 timed extractions, 1,743 matching extracted SHA256 hashes**. Selection/trial costs and GPU transfers are included in creation time. Median/range results and regressions are in [Auto performance](auto-performance.md); raw arguments/hashes/component timings are retained.
- `python scripts/verify-auto.py` passed four checks on the final release: independent Windows bsdtar 3.8.8 / libzstd 1.5.7 extraction and SHA256 equality for CPU raw frames and Auto-selected GPU CSV; actual Auto Small fallback with recorded GPU unavailability when the executable has no NVIDIA DLL folder, followed by CPU test/extraction/hash verification; and explicit-GPU failure with no output published. The retained check results are in [auto-validation-results.json](auto-validation-results.json).
- Tests used project-local TEMP/TMP because the workspace sandbox cannot publish files from the default user temporary directory. No registry installation or GUI visual acceptance was performed for this change. The new selector and completion UI compile successfully.

## GPU implementation pass

- `lttl.exe` release build succeeds with the optional NVIDIA bridge and local runtime DLLs. `doctor` recognizes the RTX 4070 Laptop GPU.
- The 21 ordinary unit/integration tests pass. The additional explicitly enabled hardware test passes at both 1 MiB and 64 KiB GPU chunks, covering multiple batches, empty/Unicode entries, CPU extraction and byte comparison, deliberate checksum corruption, and cancellation cleanup.
- GPU-created Canterbury TAR.ZST independently extracted with Windows bsdtar 3.8.8 / libzstd 1.5.7: all 11 files matched SHA256. Native GPU frames also decoded independently with Python 3.14 Zstandard.
- The synthetic/local-file comparison completed 147 creation/integrity cases, 31 extraction round trips, and 493 matching file hashes. The downloaded Canterbury/enwik8 comparison completed 30 creation/test/timed-extraction cases with 180 matching hashes.
- The subsequent chunk/batch tuning pass completed 63 more creation/integrity cases and 21 extracted SHA256 comparisons. CPU operation was separately verified using a copy of the release executable with no NVIDIA DLL folder present; creation and integrity testing succeeded, while `doctor` correctly reported GPU unavailable.
- The isolated codec probe completed 28 configurations and 84 timed passes, with 112 exact GPU corpus round trips and 16 independent CPU LZ4 corpus round trips including warmups. It validates experiments; it does not add LZ4/GDeflate archive formats or GPU extraction to the app.
- GPU-pass release SHA256: `28A5AC838A71AC9039B5F2B4F25E53F89921BE93E3AB77C5A23123B3717AF612`. Earlier benchmark reports intentionally retain their earlier executable hashes. The renamed Explorer script passed a fresh six-command `-WhatIf` check; no registry installation was performed.
- Reproducible GPU setup verifies official SHA256-pinned packages; the setup script was rerun successfully from cached downloads. Native ABI loading uses an absolute Windows path and safe DLL search flags. CPU operation has no mandatory NVIDIA dependency.
- See [GPU results](gpu-performance.md), [downloaded-file unpacking results](downloaded-performance.md), and [backend details](gpu-backend.md). Finer chunk/batch tuning and alternative-codec experiments are separately reported so they do not overwrite the baseline evidence.

## Completed

- `scripts/cargo.ps1 test --locked`: GUI-enabled build and all 18 integration tests passed. Core-only tests also passed.
- Release build succeeded with Rust 1.92/MSVC. The executable is `target/release/archiver.exe`.
- Tests cover ZIP/TAR.ZST round trips, Unicode and empty entries, unsafe paths and case collisions, no overwrite, cancellation cleanup, source size changes, ZIP CRC and Zstd frame corruption, truncated input, large metadata, decoder window limits, and content-sampling behavior.
- The Windows GUI was opened and visually checked in Create, Extract, and Inspect views. A real ZIP was listed and tested successfully from the GUI. This found a Windows extended-path prefix bug, which was fixed and rebuilt.
- Final release TAR.ZST creation/extraction of `docs/api-contract.md` passed an independent SHA-256 comparison.
- Rust release comparison: 84 creation and integrity-test cases, plus eight extractions and 70 matching file hashes. Raw results and executable SHA-256 are in the [historical benchmark record](../research/legacy/rust-benchmark-results.json).
- Explorer installer/uninstaller PowerShell syntax and dry runs passed. The final installer dry run resolves all six commands to the release executable. **Registry installation and actual Explorer menu dispatch have not been performed.**

## Coverage limits

- The GPU compression backend and bounded Auto selection are implemented and measured. GPU decoding in the app, cross-batch overlap, persistent contexts across archive jobs, per-file mixed-codec routing, and learned device calibration are not implemented.
- Synthetic warm-cache benchmarks are not representative of every workload. No equal-memory, cold-cache, energy, sustained thermal, or controlled real-world corpus comparison was performed.
- The app handles ZIP Store/Deflate and a restricted TAR.ZST subset. Encryption, volumes, recovery records, updates, and full filesystem metadata are not implemented.
- Creation/extraction are not backed by a filesystem snapshot. See README for source-mutation, staging-space, and partial-publication limits.

## Investigation checklist

| Question | State |
|---|---|
| Obtain and configure native decompiler | Answered: Ghidra/JDK installed and exercised |
| Examine thread scheduling and selected compression-related paths | Partially answered: bounded native evidence; full codec and runtime hot paths remain open |
| Compare file-content and CPU tuning behavior | Answered for the documented synthetic corpora |
| Build independent Rust archive engine and desktop interface | Working prototype built and tested |
| Provide requested Explorer commands for review | Scripts ready; live registration/dispatch pending |
| Establish actual GPU archive-creation benefit | Measured: working GPU backend; CPU Fast/Balanced faster across baseline inputs, GPU not a general default improvement |
