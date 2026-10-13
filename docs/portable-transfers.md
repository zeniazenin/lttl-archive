# Portable GPU transfer optimization

The third portable LZ4 implementation reduces readback traffic and allows two batches in flight. The LZ4 match search, parse, block settings and archive framing are unchanged. [Benchmark results](portable-v3-results.md) compare the saved v2 executable, CPU compression, and four v3 transfer modes on Intel Arc and RTX 4070 Laptop GPU through Vulkan.

## Compacted output

The compressor still produces independent blocks in fixed GPU output slots. A prefix pass calculates aligned offsets for blocks whose compressed size is smaller than their original size. A second compute pass copies those blocks into a contiguous GPU buffer. Incompressible blocks need no payload transfer: the encoder retains their original host bytes until the batch is written.

Only block sizes are read back first. The host validates them and calculates the exact packed byte count, then submits one buffer copy for that amount. Each compressed block uses four-byte alignment in the transfer buffer; the writer excludes padding from the archive. This preserves the exact old block bytes and stored/compressed decisions. Metadata and the small timing records still cross the GPU boundary, including on batches with no compressed payload.

The extra passes and size-readback dependency have a cost. The benchmark includes this cost and compares compaction with fixed-slot readback. A smaller transfer alone does not establish a faster archive operation.

## Bounded pipeline

Each session owns two independent sets of upload, input, output, compacted output, metadata and readback buffers. Submitting a batch no longer waits for its compressed output immediately. The TAR writer can fill the next host batch while the GPU processes the previous one. Nonblocking device polls during input ingestion advance completed metadata reads and queue the corresponding payload transfers. Once both slots are occupied, the oldest batch is drained before its slot is reused.

Readback memory is written directly to the archive, removing the old allocation and copy into a separate vector for every compressed block. CPU archive writes can run while a later GPU batch is pending. Input bytes and output order remain tied to the original batch; checksums are updated in input order. Completion waits target specific submissions rather than indiscriminately waiting for all work on the device.

wgpu 0.19 exposes a single submission queue. This implementation overlaps host staging/writing with queued GPU work and avoids a mandatory full-batch wait at every submission. It does **not** explicitly control separate DMA/compute queues or prove simultaneous copy-engine and shader execution. `max_in_flight` counts pending batches, not concurrently executing GPU kernels. The serial/pipeline benchmark comparison measures the practical benefit of this scheduling. See the pinned [Device polling API](https://docs.rs/wgpu/0.19.4/wgpu/struct.Device.html#method.poll) and [Queue API](https://docs.rs/wgpu/0.19.4/wgpu/struct.Queue.html).

## Sessions, memory and failures

The existing exclusive session cache is retained. Transfer mode is part of its key alongside adapter, block size and batch size. Idle buffer allocations remain limited to 256 MiB total and at most two sessions. Two compact slots at the benchmark's 16 MiB batch setting allocate approximately 160 MiB; therefore only one such idle session fits the budget. Larger configurations can run but may exceed the idle budget and be discarded after use. Driver allocations, active sessions and host input buffers are outside that budget.

All pending batches drain before a successful session is cached. Failed, cancelled or abandoned encoders drop their sessions. Slots are reused only after both readback completion and upload remapping. Device-error callbacks invalidate the session as before; actual device removal has not been induced in tests.

## Reproduction and diagnostic controls

The default is compacted output with two slots. The diagnostic environment variable is read when acquiring a session; set it before launching the process and do not mutate it during an archive job.

```powershell
$env:LTTL_PORTABLE_TRANSFER = 'compact-pipeline'
New-Item -ItemType Directory .qa\transfer-session
.\target\release\lttl.exe benchmark-session C:\Data --adapter nvidia --output-dir .qa\transfer-session --runs 4 --chunk-kib 16 --batch-mib 16
Remove-Item Env:LTTL_PORTABLE_TRANSFER
```

Replace `nvidia` with `intel`. The output directory must be empty and outside the input tree. First-job measurements include context initialization; the following jobs reuse the session. Separate `create` processes do not share sessions.

| Mode | Readback | Pending batches |
|---|---|---:|
| `compact-pipeline` | Useful compressed payload only | 2 |
| `compact-serial` | Useful compressed payload only | 1 |
| `fixed-pipeline` | Complete fixed output slots | 2 |
| `fixed-serial` | Complete fixed output slots | 1 |

All four modes write directly from mapped GPU output. They are execution alternatives, not different codecs or compression settings. CPU and Auto do not use these modes. Invalid mode names fail an explicit GPU request.

```powershell
python scripts/benchmark-portable-v3.py
python scripts/summarize-portable-v3.py
```

The harness uses the existing SHA-256 corpus manifest, the preserved `target/release/lttl-portable-v2.exe` control, and the current `lttl.exe`. Thirteen configurations across nine datasets produce 468 creation/extraction pairs: four jobs per group, including three warm samples. Groups are randomized and executed sequentially. Every output is extracted and every file hash checked. Complete archive SHA-256 values must match across both GPUs, all transfer modes, and v2. CPU LZ4 with one/eight workers and CPU Zstd Fast are rerun as controls. Raw samples, process-group times, extraction times and executable hashes are retained.

## Metrics

- `metadata_readback_bytes` counts block-size bytes; `payload_readback_bytes` counts padded output transferred; their sum is `d2h_bytes`. Timing records are excluded, as they were in v2.
- `compaction_ms` covers prefix and packing passes, or the timestamp interval between compression and metadata readback in fixed mode. The latter may be slightly nonzero without compaction work.
- `device_to_host_ms` measures the payload copy only. It excludes the metadata copy, mapping, queue waits and CPU writes. Use complete archive time for performance decisions.
- `readback_wait_seconds` is host time blocked while completing batches and can include outstanding compression or other queued work.
- `host_wall_seconds` accumulates time spent in GPU session submission, progress and completion methods, including waits and archive writes. It excludes upstream input production and checksum work. It must not be added to GPU timestamp totals.
- `max_in_flight` is peak pending batch count; `transfer_mode` identifies the diagnostic configuration.

The no-ratio-sacrifice CPU comparison requires no larger archive as well as a speed improvement. Byte-identical output to v2 proves preservation of the previous GPU ratio, not equality with CPU compression. Auto changes require the joint speed-and-size criterion; kernel or transfer improvements alone are insufficient.

## Measured outcome

All 468 creation/extraction pairs and 4,316 extracted-file hashes passed. Readback traffic fell 10.3–99.95% across the nine datasets. RTX complete warm archive operations improved 1.12–1.46× versus the rerun v2 control, with raw CPU LZ4 speed wins on seven datasets, five by at least 5%. Intel's speedup range was 0.90–1.32×: JSON regressed about 11%, and no Intel workload beat CPU LZ4. Neither device produced an archive no larger than CPU's, so neither met the joint criterion; Auto remains on CPU.

Compaction is not universally faster than fixed readback. For example, RTX enwik8 took 0.198 s with the compact pipeline versus 0.192 s with the fixed pipeline; CSV favored compaction (0.104 s versus 0.118 s). The default retains compacted output to reduce transfer traffic, while the diagnostic alternatives make these tradeoffs measurable. These are small sample medians, not a proven routing rule for arbitrary content. CPU Zstd Fast also remains a strong speed-and-size alternative on these fixtures.

The measured executable is preserved as `target/release/lttl-portable-v3-benchmark.exe`. Hardware coverage includes 4/16/64 KiB blocks, multi-batch input, partial batches, Unicode filenames, standard LZ4 decoding, corruption/truncation, and session lifecycle. Additional direct-stream tests cover zero compressed payload, awkward write boundaries, ordered output, and cancellation/output failures with queued work.

[Validation metadata](portable-v3-validation.json) records 28 ordinary tests, six explicitly run hardware tests, and executable hashes. Only test coverage was strengthened after the benchmark; the release executable was rebuilt with unchanged compression/transfer code. Its final Intel/NVIDIA smoke checks verify pipelining, reuse, extracted hashes and byte-identical archives against the preserved measured executable.
