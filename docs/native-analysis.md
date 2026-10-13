# WinRAR native compression architecture: bounded static analysis

Date: 2026-10-12. Target: installed `C:\Program Files\WinRAR\Rar.exe`, x86-64 PE, SHA-256 `645b3c027627c17c184bdc39c7eb0694e14c43a1fade7696c979cc1808f25494`. REA used Ghidra 12.1.4. This is the command-line executable supplied with WinRAR; conclusions do not automatically cover `WinRAR.exe` or every codec path.

**Historical scope — pre-GPU implementation pass, 2026-10-12.** The native evidence and its limits below are unchanged. Statements about the Rust app's then-current GPU support describe that earlier pass. Later on the same date, lttl gained an experimental nvCOMP Zstandard backend and actual RTX 4070 Laptop GPU validation. See [current GPU measurements](gpu-performance.md) and [implementation/improvement review](gpu-improvements.md) for that separate work; it does not change what the RAR static evidence proves.

The strongest findings are a CPU batch worker pool, parallel data classification, and an order-dependent hash-position preprocessing routine. These are useful architectural observations, not a recovered complete RAR encoder. No proprietary pseudocode or algorithm was copied into the Rust app; its archive formats and codecs remain the independent implementation described in [performance-research.md](performance-research.md).

## Evidence boundary

**Observation** below means a structure or operation is visible in the retained decompiler output. It does not mean execution was observed. **Inference** gives a proposed semantic interpretation. Names such as “classifier” and “match candidate” are analysis labels, not recovered original symbols. Exact Evidence IDs and retained files are listed at the end.

The overview reports 2,315 procedures and 1,907 defined strings [O1]. This pass selected six callers of a shared task-submission function plus focused worker bodies. The submission dossier lists other callers that were not all traced. Native indirect calls, decompiler types, missing symbols, and unresolved callers limit semantic certainty. The worker-creation pseudocode additionally reports a decompiler restart-limit warning [P4].

## CPU scheduling: observed mechanism

The pool constructor `0x14006e2b8` bounds its requested worker count to 1–64 and creates a critical section, a semaphore, and a completion event [P1]. This bound applies to this pool implementation; it does not prove a process-wide thread limit.

`0x14006e424` lazily creates workers and appends callback/argument pairs into a 64-slot ring. A full outstanding batch triggers `0x14006e948` [P2]. The latter resets the completion event, releases the semaphore for outstanding work, and calls a helper with the event [P5]. It is therefore a **batch dispatch and completion-wait candidate**, not merely a function that waits after immediately dispatched submissions. The event-wait helper itself was not decompiled in this pass.

The worker loop `0x14006e8d0` obtains a callback, invokes it with its argument, decrements the outstanding count under a critical section, and signals the completion event when the count reaches zero [P3]. Worker creation uses Windows `CreateThread`; it also resolves processor-group affinity APIs and can set thread priority [P4]. Together these observations strongly support reusable CPU workers with explicit batch barriers. They do not establish achieved CPU utilization or speedup.

## Compression-related preprocessing

### Position-history hashing: strongest matcher evidence

The two-task wrapper `0x1400495a4` submits `0x1400492e0` and `0x1400492d0`, then dispatches/joins [C4]. The first callback forwards to `0x140049b0c` [H1].

**Observed:** `0x140049b0c` walks input positions in order, derives separate hash indexes from overlapping two-, three-, and four-byte prefixes, reads previous table entries, replaces those entries with the current position truncated to different widths, and writes the old values into compact seven-byte records [H2].

**Inference, high confidence:** this is position-history preprocessing for a match finder or related repeated-string analysis. Its output contains previous-position fragments, not demonstrated final back-reference lengths or distances. Repeated hash values can collide; this function alone does not validate actual string matches.

**Unknown:** how the packed records are consumed; candidate validation, match extension, dictionary wrap semantics, match selection, entropy coding, and exact format/version association. The second submitted callback `0x1400492d0` was not resolved here. It would be incorrect to describe this routine as a complete compression algorithm.

### Data classification and executable-filter candidates

`0x140040e7c` divides an input range into at-most-64-KiB tasks, runs `0x140040cf4` directly for small/single-worker cases or via callback `0x140040e70`, then merges per-task region descriptors in input order [C1, F1].

The worker samples local data in small windows, invokes the statistical routine `0x1400423bc`, chooses among classification routines depending on settings, and calls another helper to populate region records [F1]. The statistical routine updates a rolling-hash recency table and reports a normalized recent-hit count [F4]. This is **a local repetition/similarity proxy**, not proven Shannon entropy, checksum validation, or an exact estimate of compressed size. Hash collisions affect its interpretation.

Two focused routines clarify the classifier:

- `0x1400426d0` scans byte patterns corresponding to `E8`/`E9`, reconstructs the following displacement, combines it with input position, and tests recurrence/density conditions [F2]. **Strong inference:** selecting x86 branch-address preprocessing. This function classifies; the actual forward transform was not traced.
- `0x140042464` scans aligned four-byte words, checks a high byte of `EB`, combines an immediate-like value with the word position, and tests repeated targets [F3]. **Qualified inference:** an ARM32 branch-filter classifier. Its exact ISA/version and subsequent transform remain unverified.

`0x140048664` is a related region-analysis candidate: it also divides input into 64-KiB pieces, combines worker results, compares 256-bin distributions in one mode, and either emits region descriptors or records transition decisions [C3]. Its masked input positions suggest a circular-buffer representation. It is not enough to label it definitively as an older RAR format or a decompressor.

### Multistage parallel preparation

`0x140041e1c` is the most substantial parallel orchestration candidate in the six-call sample [C2]. **Observed:** it processes bounded batches up to 256 KiB; optionally collects per-worker 256-bin statistics and reduces them; submits another pass over partitioned input and auxiliary arrays; submits tasks selected by category/length-like integers; and optionally submits a final pass. Dispatch/join calls separate these phases.

**Inference, medium confidence:** staged compression preparation, potentially histogram-guided hash/match preprocessing. The arrays and task boundaries are visible, but their final meaning cannot be established from this coordinator alone. Its relationship to `0x140049b0c` is not established merely because both appear compression-related.

Useful future callees, if a deeper investigation is requested: `0x1400415c0` / `0x140041240` for the partition pass; `0x140042330` / `0x140041340` for the category-dependent pass; `0x140043f40` for the histogram decision; and `0x140042cc0` for the optional final pass. These are follow-up candidates, not claims that each is a confirmed matcher.

## Distinguishing other threaded work

| Sampled caller | What the retained evidence establishes | Classification and remaining gap |
| --- | --- | --- |
| `0x140040e7c` [C1] | Chunk tasks and ordered region-descriptor gathering; worker classifiers inspected. | Strong compression/filter-preprocessing candidate; transform and encoder integration untraced. |
| `0x140041e1c` [C2] | Multiple synchronized passes, per-worker statistics, auxiliary arrays. | Strong staged-preparation candidate; detailed match/encoding semantics unresolved. |
| `0x140048664` [C3] | Chunk analysis, histogram comparisons, region or transition records. | Related classifier candidate; codec generation and exact callers unresolved. |
| `0x1400495a4` [C4] | Two callbacks plus a barrier; one callback reaches position-history hashing. | Confirmed connection to match-candidate preprocessing; second branch unresolved. |
| `0x140054088` [C5] | Independent 64-KiB chunks submitted to `0x140054b30`, which calls `0x140053fe4`; the body invokes `0x140026bb4` and stores one 64-bit result per indexed chunk [A1, A2]. | Digest/checksum-like block summary, **not demonstrated compression**. The digest algorithm and caller purpose require the remaining callee. |
| `0x140059154` [C6] | Constructs `.rev` filenames, opens multiple inputs/outputs, bounds their combined count, partitions ranges among workers, and writes resulting output blocks. | Strong recovery-volume generation inference. Worker `0x140058fcc` and initializer `0x140066844` remain candidates for validating the redundancy coding algorithm; Reed–Solomon is not established by this pass alone. |

None of these six is conclusively identified as a decompressor. Treating every caller of the common pool as a compression kernel would overstate the evidence.

## What this means for GPU acceleration

These are engineering inferences from the observed dependencies, not measured GPU results:

1. **Do not directly parallelize the position-update loop.** At `0x140049b0c`, each input position reads the prior value before replacing it. Concurrent updates to the same hash slot can change which predecessor is reported. An atomic exchange alone does not guarantee input-order semantics. A GPU design would need explicit ordering or a reformulated grouping/index-building algorithm, followed by equivalence tests.
2. **Coarse independent analysis is a more plausible experiment.** Per-block byte statistics, some classifier work, and block-summary calculation expose task boundaries. The classifiers and rolling recency statistic still contain local state and boundary dependencies; copying each inner loop to a GPU is not automatically correct. Preserve overlap/context and compare outputs to a reference implementation of the chosen open algorithm.
3. **Task parallelism is not a speedup measurement.** The evidence does not identify hot functions, their percentage of archive time, memory bandwidth saturation, or the relative cost of matching and entropy coding. Offloading a small preparation stage may add transfers while leaving the dominant CPU work unchanged.
4. **Prefer an independently supported open codec backend for a product experiment.** Evaluate a real GPU Zstandard/Deflate implementation through the versioned API and container/round-trip plan in [performance-research.md](performance-research.md). This avoids confusing a proprietary internal stage with a complete interoperable codec. At the time of this static pass, the Rust app had no functioning GPU compression backend; the later implementation is documented separately above.
5. **Measure the full pipeline.** Include disk I/O, initialization, host/device copies, workspace allocations, container construction, checksums, compressed size, CPU decoding, and cancellation. A GPU checkbox or kernel-only throughput number is insufficient evidence of faster archive creation.

Two bounded REA searches found no matching procedure names for `cuda|nvcomp|OpenCL|D3D12`, and no matching defined strings for `cuda|nvcomp|OpenCL|DirectCompute` [G1, G2]. **This does not prove GPU code is absent:** stripped names, dynamic loading, other APIs, or strings outside Ghidra's defined-data inventory may evade those searches. The inspected pool and workers do establish CPU mechanisms in the selected paths.

## Completion and open questions

| Investigation question | Status |
| --- | --- |
| Does the shipped executable contain a reusable CPU task pool? | Answered for the inspected implementation. |
| Are useful compression preparation structures visible? | Partially answered: position-history hashing, filter classification, statistics, and staged task dispatch. |
| Is the full RAR encoder reconstructed and its format mapping verified? | Unresolved; not claimed. |
| Are exact match lengths, parse decisions, probability tables, and entropy bitstream generation recovered? | Unresolved. |
| Is GPU use absent throughout WinRAR? | Unresolved; only bounded negative searches and CPU-path observations. |
| Would GPU offload beat the current CPU app on this machine? | Unresolved without a real backend and end-to-end measurements. |

No further static calls are required to support the bounded findings above. Deeper encoder recovery and runtime profiling would be a separate expansion of the evidence boundary.

## Evidence ledger

Each retained JSON carries artifact identity, provider/profile metadata, and limitations. Pseudocode is decompiler output rather than original source. IDs below refer to the complete retained records, not abbreviated console previews.

| Key | Retained file | Exact Evidence ID |
| --- | --- | --- |
| O1 | [rar-native-overview.json](rar-native-overview.json) | `ev_62a70e63602c70f74cc635611f498f9a8dadca2e43244a71e1366711cad57a8c` |
| P1 | [threadpool-function.json](native/threadpool-function.json) | `ev_3018635fd298c23605a609efd39ac004426765bf9513ae83698359e7cbf4fd8a` |
| P2 | [thread-submit-function.json](native/thread-submit-function.json) | `ev_c489804535617fd08a734b27e59794d7d618a4f2206b6553e85be6707fc7982e` |
| P3 | [thread-work-loop.json](native/thread-work-loop.json) | `ev_d164554fbb6411b3d92ed21f5f3bcc76e625c222cdc6fe82b98dd72d8f49b502` |
| P4 | [worker-create-function.json](native/worker-create-function.json) | `ev_ae03ce61fcc900ed7ad0531dc2de5576db7e03aad77b0b67f621d3135595f397` |
| P5 | [batch-dispatch-join.json](native/batch-dispatch-join.json) | `ev_2943dcf0c00a9cc4ab3f83d5ce27672dcd2c9e36329e1ac79447063a2fb6c753` |
| C1 | [caller-40e7c.json](native/caller-40e7c.json) | `ev_f9da6858f7b57c4470c3c04487d78663d14654eae698377f62f3299de36242e2` |
| C2 | [caller-41e1c.json](native/caller-41e1c.json) | `ev_d1196169f553cdad5870773d5595bcdb302ba2c379d9bce96a422ccae14d6adb` |
| C3 | [caller-48664.json](native/caller-48664.json) | `ev_2d4a31a5057361936e2ab71a6659843bf6986c0cebf54deb93acea2937198d68` |
| C4 | [caller-495a4.json](native/caller-495a4.json) | `ev_20e5fd6719f9c307e4861e12c7bc93277d7cc950420d77cb7c6e7a1490641319` |
| C5 | [caller-54088.json](native/caller-54088.json) | `ev_60c968de13bcb3fff2fb2031aa6b794cad5cae920c6f921486689f4d7e8ed93b` |
| C6 | [caller-59154.json](native/caller-59154.json) | `ev_6ec6a37e4157fc525df7acbc3fbdf2c498a62c099d5e3084e68f398e2cc76f2e` |
| H1 | [job-492e0.json](native/job-492e0.json) | `ev_9d0eebde78e37b16947c37fb5fc82429e812de3f0f3e2fc8e3459af3eb69db05` |
| H2 | [job-body-49b0c.json](native/job-body-49b0c.json) | `ev_08f7ddcc68d2b730b3f7e73bad4aac9e8144c81a487e46a6d05e602d885aba5b` |
| F1 | [job-body-40cf4.json](native/job-body-40cf4.json) | `ev_2608803bbbc202af65184829e7c56c966b2388a0b8d598c4d3363063fba57026` |
| F2 | [classifier-426d0.json](native/classifier-426d0.json) | `ev_e9714f01052c1d647e005b10e630f79400d981b00bbeeb9ad9ca9d95584fe069` |
| F3 | [classifier-42464.json](native/classifier-42464.json) | `ev_d58f8161ccb3eda99b9e7c1889404ab841286294f2dc85d556c8aaca2a306a37` |
| F4 | [statistics-423bc.json](native/statistics-423bc.json) | `ev_ce265c326594b07800bf1888eec0058ab0c92e2fa96cae395266af34444c16ed` |
| A1 | [job-54b30.json](native/job-54b30.json) | `ev_c118805821b28afd0066007bda95376c444dfaba6314898bd457eba3c3b64ead` |
| A2 | [job-body-53fe4.json](native/job-body-53fe4.json) | `ev_ce0bc9227ed03b82e0e76c4f1a4c4d88c420d74ec53a2548044623fcd82d3c75` |
| G1 | [gpu-procedure-search.json](native/gpu-procedure-search.json) | `ev_42edf658a16d2ce063479dd990dff24d2b1f039523f39cb0f6c69cba3b9f07e6` |
| G2 | [gpu-string-search.json](native/gpu-string-search.json) | `ev_da57c52c760de2fe3150a4c50f2dedd61b1b32ddc71b21850a501f3088578236` |
