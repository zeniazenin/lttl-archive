# RAR compressor review and Rust archiver recommendations

Reviewed 2026-10-12. **The strongest near-term improvements are choosing how much compression work to do, grouping similar content, and using CPU workers appropriately. GPU compression is a separate experimental backend, with no measured speedup established here.**

## What was actually inspected

- **Shipped documentation:** `C:\Program Files\WinRAR\Rar.txt`, the console RAR 7.20 manual, especially compression method, `-mc`, `-md`, `-ms`, `-mt`, and solid archive sections.
- **Runtime evidence:** the installed `Rar.exe` 7.20.3 executed on generated test inputs, with every resulting archive checked by `rar t`. SHA-256: `645B3C027627C17C184BDC39C7EB0694E14C43A1FADE7696C979CC1808F25494`.
- **Public primary documentation:** RARLAB format and release documentation, plus NVIDIA's current nvCOMP documentation, linked below.
- **Separate static investigation:** the first benchmark pass ran while REA had no configured native decompiler. Ghidra/JDK were subsequently installed at the user's request, and native analysis now works. The retained overview reports 2,315 functions and 1,907 strings (`docs/rar-native-overview.json`, evidence `ev_62a70e63602c70f74cc635611f498f9a8dadca2e43244a71e1366711cad57a8c`). See [native analysis](native-analysis.md) for the focused findings and their coverage limits. This performance review's algorithm descriptions remain documented behavior and architectural inference; it does not claim recovered compressor logic or CPU-profiled bottlenecks. No UnRAR source was used to reconstruct an encoder.

## What RAR already does

The local manual describes a sliding dictionary that finds repeated byte patterns, adjustable search effort, automatic delta and x86 executable transformations, and reuse of compression history across files in solid mode. These are already content-sensitive mechanisms; adding an extension-based selector alone would not be a new compression algorithm. Delta transformation supports 1–31 byte channels; forcing transforms globally can worsen results. Sources: local `Rar.txt` lines 1372–1508 and 2050–2088.

Search effort is distinct from the archive's decoding format. RARLAB documents long-range matching for distant repeats, an additional expensive exhaustive search, and compatibility of those search options with existing decoders at compatible dictionary sizes. Large dictionaries target distant repeats in large images or related files, at memory and speed cost. [RARLAB release notes](https://www.rarlab.com/rarnew.htm)

The format records the method, required dictionary size, and whether a file continues the previous dictionary. That header specification does **not** reveal the encoder's match-finder implementation, parsing cost model, filter-selection heuristics, or threading bottleneck. [RAR5 format specification](https://www.rarlab.com/technote.htm)

The local manual also provides a built-in list of already compressed extensions to store with `-ms`, and a thread recommendation `-mt1` through `-mt64`. Changing thread count can change archive bytes and ratio. Solid groups trade better reuse for slower access to individual files, updates, and worse damage isolation. Sources: `Rar.txt` lines 1549–1594 and 2050–2088.

## Reproducible experiment

Run from the project directory in PowerShell 7:

```powershell
pwsh -NoProfile -File scripts/benchmark-rar.ps1 -SizeMiB 32 -Repeats 3
```

The script creates a fresh timestamped directory beneath `bench-output/rar-review`, then retains corpus hashes, environment, full argument arrays, archive bytes, wall time, CPU time, peak working set, and integrity results. It never reads user documents. The executable is launched with an argument list, not a shell command string. Configuration files and the `RARINISWITCHES` environment override are disabled for consistency.

Creation command shape (each source is relative to the generated corpus directory):

```text
Rar.exe a -cfg- -idq -y -md32m -m3 -mt8 -s- -ep1 <fresh-archive.rar> records.txt
Rar.exe a -cfg- -idq -y -md32m -m3 -mt8 -s  -ep1 <fresh-archive.rar> similar\*
Rar.exe t -cfg- -idq -y <fresh-archive.rar>
```

The run compares methods 1, 3, 5 with thread recommendations 1 and 8 on each single-file corpus, plus stored random data and solid/non-solid related files. Dictionary size is fixed at 32 MiB. Integrity testing is outside the creation timer. Each case runs three times; order rotates between rounds.

The four synthetic corpora are each exactly 32 MiB:

| Corpus | Deliberately modeled property | What it does not represent |
|---|---|---|
| `records.txt` | Generated text records with shared fields, variable counters and pseudorandom IDs | A representative real log or source-code collection |
| `repeating.bin` | The same pseudorandom 64 KiB block repeated 512 times | A real executable, VM image, or multimedia file |
| `random.bin` | Seeded pseudorandom bytes with little useful repetition | Actual encrypted files or a sampled real compressed-media collection |
| `similar/` | 32 files sharing almost all of one 1 MiB pseudorandom template, each with a different 1 KiB tail | Typical everyday documents or typical deduplication ratios |

The machine reports 22 logical processors and Windows build 26300; registry CPU identity is Intel Core Ultra 9 185H. PowerShell 7.6.5/.NET 10.0.11 generated the data. CIM queries for GPU, physical memory, and CPU topology were unavailable. The benchmark runs in a live development session with warm filesystem cache, no cache flushing, CPU pinning, thermal control, or isolation from other work. Wall time includes process launch and file I/O. Short cases are especially sensitive to fixed overhead. These measurements demonstrate tradeoffs on this corpus, not industry performance rankings.

Measured run: `bench-output/rar-review/20261012T205958852Z`. All 63 creation operations and all 63 integrity checks passed. Ratios below are **archive size / original size**, so smaller is better. Times are medians of three runs; ranges show the observed minimum–maximum, not confidence intervals.

| Corpus | Method | Threads | Solid | Archive bytes | Original size retained | Median seconds | Range seconds |
|---|---:|---:|---|---:|---:|---:|---:|
| random.bin | 0 | 1 | False | 33554598 | 100.0005% | 0.123 | 0.123–0.133 |
| random.bin | 1 | 1 | False | 33554598 | 100.0005% | 0.815 | 0.693–0.996 |
| random.bin | 1 | 8 | False | 33554598 | 100.0005% | 0.460 | 0.446–0.511 |
| random.bin | 3 | 1 | False | 33554598 | 100.0005% | 6.856 | 5.730–6.900 |
| random.bin | 3 | 8 | False | 33554598 | 100.0005% | 1.254 | 1.226–1.372 |
| random.bin | 5 | 1 | False | 33554598 | 100.0005% | 5.654 | 5.415–6.030 |
| random.bin | 5 | 8 | False | 33554598 | 100.0005% | 1.284 | 1.283–1.446 |
| records.txt | 1 | 1 | False | 3521857 | 10.4960% | 0.458 | 0.447–0.477 |
| records.txt | 1 | 8 | False | 3521857 | 10.4960% | 0.293 | 0.288–0.323 |
| records.txt | 3 | 1 | False | 3419813 | 10.1918% | 3.536 | 3.398–3.912 |
| records.txt | 3 | 8 | False | 3419813 | 10.1918% | 0.806 | 0.743–0.841 |
| records.txt | 5 | 1 | False | 3403925 | 10.1445% | 11.683 | 9.838–22.454 |
| records.txt | 5 | 8 | False | 3403925 | 10.1445% | 1.887 | 1.688–1.948 |
| repeating.bin | 1 | 1 | False | 67512 | 0.2012% | 0.296 | 0.282–0.321 |
| repeating.bin | 1 | 8 | False | 67512 | 0.2012% | 0.276 | 0.275–0.290 |
| repeating.bin | 3 | 1 | False | 67605 | 0.2015% | 0.960 | 0.941–1.062 |
| repeating.bin | 3 | 8 | False | 67605 | 0.2015% | 0.382 | 0.333–0.434 |
| repeating.bin | 5 | 1 | False | 67605 | 0.2015% | 1.254 | 1.082–1.531 |
| repeating.bin | 5 | 8 | False | 67605 | 0.2015% | 0.522 | 0.514–0.574 |
| similar | 3 | 8 | False | 33558036 | 100.0107% | 0.683 | 0.632–0.699 |
| similar | 3 | 8 | True | 1090800 | 3.2508% | 0.452 | 0.422–0.493 |

Interpretation: on synthetic records, eight-thread fastest compression took 0.293 s versus 0.806 s for normal and 1.887 s for best. Best saved only 15,888 bytes versus normal (0.46% of the normal archive, 0.047% of the original input). Eight threads improved normal-mode text time about 4.4x over one thread; scaling is workload-dependent. One best-mode single-thread run took 22.454 s, illustrating the variability of this live-host test.

On pseudorandom data, every method produced the same 33,554,598-byte archive, 166 bytes larger than the input. Stored mode took 0.123 s versus 1.254 s for normal/eight-thread compression: about 10.2x less wall time. This supports avoiding wasted search, not a claim that arbitrary real media can be recognized perfectly.

For the deliberately related files, solid mode reduced the archive from 33,558,036 to 1,090,800 bytes (96.75% smaller) and median time from 0.683 s to 0.452 s. This is an intentionally favorable cross-file-repetition case; it demonstrates the mechanism, not an expected everyday gain. On repeated blocks, fastest mode was both quicker and 93 bytes smaller than normal/best, so increasing level is not strictly monotonic in final archive size.

## Meaningful improvements to pursue

| Priority | Proposed Rust behavior | Evidence and limits |
|---|---|---|
| 1 | Sample several file regions, attempt fast compression, then store when predicted savings do not cover the cost | Random data shows the cost of unnecessary search. Use extension/magic as a hint; entropy alone misses long-distance repeats. Samples can miss useful regions, so offer override and record the decision. |
| 1 | Offer fast/balanced/compact presets with CPU and memory budgets | Search-effort and thread sweeps show substantial time tradeoffs. Tune budgets across the whole job to avoid outer file workers multiplied by inner codec workers. |
| 2 | Put related files into bounded solid groups, with user-selectable random-access tradeoff | The related-file test demonstrates cross-file redundancy. Grouping must preserve paths and file contents; do not assume same extension means similar content. |
| 2 | Select dictionary/window from file and group sizes under a hard memory cap | Huge windows need enough useful distant repetition to justify their footprint. This 32 MiB experiment cannot establish optimal settings for gigabyte-scale files. |
| 3 | Investigate standard reversible transforms for appropriate formats | Delta and executable filtering are existing RAR features. In a replacement, only use transforms whose inverse is represented by the selected interoperable format; otherwise an unmodified extractor cannot reconstruct the original bytes. |
| Experimental | A transfer-aware GPU codec backend with reliable CPU fallback | No GPU compression ran. Require measured end-to-end benefit before the UI selects it automatically. |

“Improve RAR” and “build a better archiver” are different implementation goals. One can tune the installed RAR encoder now; changing its internal algorithm requires unavailable implementation/profiling evidence. The Rust app can instead implement a better job policy around open-format codecs without recreating the proprietary encoder. This review supplies no evidence that a new codec already beats RAR's best ratio.

## Where a GPU can help, and why it might lose

**Architectural inference:** independent blocks, batches of files, byte transforms, hashes, and some match-finding steps can expose parallel work. A full high-ratio dictionary compressor also has irregular data-dependent memory accesses and dependencies in choosing/encoding matches. Splitting history into many independent chunks increases parallelism but can sacrifice cross-chunk matches. This is a candidate tradeoff to test, not an observed bottleneck in RAR's binary.

NVIDIA nvCOMP offers GPU implementations of open codecs including LZ4, Deflate/Gzip, and Zstandard, alongside specialized formats. It does not list RAR. Its algorithm guide distinguishes throughput-oriented choices from denser compression and recommends typed-data approaches for repetitive numeric columns. This is a plausible library for a separate Rust FFI backend; it does not accelerate RAR by being installed. [nvCOMP overview](https://docs.nvidia.com/cuda/nvcomp/index.html), [algorithm selection](https://docs.nvidia.com/cuda/nvcomp/algorithm_selection.html)

The current prerequisites include Windows support, compatible NVIDIA GPU/driver and CUDA versions. A backend must probe those requirements and the chosen package/API, and keep the CPU path available; there is no universal NVIDIA/AMD/Intel GPU backend in this prototype. [nvCOMP installation](https://docs.nvidia.com/cuda/nvcomp/installation.html)

For a disk-to-disk archive, compare complete costs, approximately:

```text
CPU: read + CPU compression + write compressed bytes
GPU: read + stage/transfer input + GPU compression + transfer output + write
```

Pipeline overlap can reduce the sum, but cannot remove every bottleneck. Small files may lose to initialization and launch latency; incompressible input wastes transfer and codec work; a slow drive may dominate both paths. Large batches and data already on the device are more promising. Ratios, memory use and interoperability must be compared alongside time. GPU-specific framing is not automatically a standard ZIP, Zstandard frame, or RAR stream.

Hardware **decompression** marketing numbers do not establish archive-creation speed. NVIDIA's dedicated decompression engine applies to supported formats and particular data-center Blackwell products; it is not a RAR compressor, nor a feature established for every consumer Blackwell GPU. [NVIDIA decompression-engine FAQ](https://docs.nvidia.com/cuda/nvcomp/decompression_engine_faq.html)

## Remaining work before claiming an algorithmic or GPU win

1. Profile the encoder on approved real corpora, separating read/write, checksums, transforms, match search, encoding, allocation, and scheduling. The now-functioning REA native provider enables specific function-level static analysis; CPU sampling and tracing are still needed for runtime bottleneck claims.
2. Benchmark the Rust CPU backend against RAR on the **same** representative files, extraction verification, memory cap, and speed/ratio frontier. Include huge files, many small files, already compressed media, executables, raw numeric data, and long-distance repetition. Add warm/cold cache runs and enough repetitions to report uncertainty.
3. Measure GPU initialization, transfers, packing/framing, kernel time, synchronization, final writes, and CPU decode compatibility. Test several chunk sizes and batch thresholds on each supported GPU family. Compare both bytes saved and total job latency; include energy/thermals for laptop use.
4. Add the GPU path only where its observed benefit justifies the dependency and resource cost. Keep stored mode, CPU compression and bounded working memory available regardless of GPU capability.

The review therefore answers the tuning question with measured evidence, answers GPU feasibility conditionally, and leaves internal RAR algorithm changes and actual GPU speedup unresolved.
