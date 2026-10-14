# Research artifacts

The [lttl Dense CPU optimization](dense-opt/README.md) reduces transform, sampling and allocation overhead while preserving exact compressed bytes. It compares the original numeric codec, frozen/optimized Dense, Zstandard and archive tools using one Rust executable and separate fresh-process and warm-worker measurements.

The [lttl compression experiment](numeric-v2/README.md) adds endian/word-width/packed-sample detection and an optional dense stage, with lttl Fast and lttl Dense presets and fresh comparisons against the frozen numeric baseline and standard archive tools.

The [standard-codec and archive-tool comparison](codec-comparison/README.md) benchmarks the numeric prototype against Zstandard, LZ4, ZIP/Deflate, RAR, and actual 7-Zip, with common process-level timing and separate GPU session measurements.

The [portable GPU numeric experiment](numeric-gpu/README.md) adds exact-ratio Intel/NVIDIA execution, transfer accounting, and real numeric datasets to the independent codec research.

The [independent codec experiment](codec-lab/README.md) studies compression mathematics, a Rust numeric prototype, and comparisons on identical input payloads. It is isolated from the production archive format and Auto policy.

The `legacy/` directory preserves earlier research reports and raw benchmark evidence for reproducibility. These artifacts are outside the product documentation and installer bundle. They are not dependencies of lttl’s compression engine. Original raw data and hashes are preserved; relative links in relocated notes are adjusted to their new locations.
