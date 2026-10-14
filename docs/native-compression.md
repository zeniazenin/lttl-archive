# Native lttl compression

Alpha.4 integrates **lttl Fast** and the optimized **lttl Dense** codec into the Windows app and CLI. Native `.lttl` is the default for new archives. ZIP, TAR.ZST and TAR.LZ4 remain explicit compatibility choices. Old archives remain readable; older clients do not understand the new native container.

In **Create**, choose **lttl** and **Auto**, **lttl Fast**, **lttl Dense**, or **Zstandard**. Auto's preference controls its tradeoff: Fast favors speed, Balanced considers speed and size, and Smallest favors size. Direct lttl Fast/Dense choices retain their own packing/density policy, including Zstandard or storage fallback. The completion panel reports the actual block codecs and selection time.

```powershell
lttl create input -o archive.lttl
lttl create input -o fast.lttl --backend lttl-fast
lttl create input -o dense.lttl --backend lttl-dense
lttl create input -o speed.lttl --backend auto --preset fast
lttl create input -o compatible.tar.zst --format tar-zstd
lttl list archive.lttl
lttl test archive.lttl
lttl extract archive.lttl -d restored
```

The numeric paths run on CPU, using AVX2 where supported and a scalar fallback elsewhere. They reuse worker Zstandard contexts and parallelize independent work. Native Auto does not choose a GPU: no numeric GPU implementation has yet demonstrated an integrated advantage at the required density. Explicit TAR GPU paths and their existing Auto policies remain available.

## Auto selection

Each file is split into blocks of at most **16,776,960 bytes** (16 times the numeric codec's chunk size). This boundary preserves packed triples and word alignment; prediction resets at block/chunk boundaries. Files can exceed the research prototype's 512 MiB limit, subject to the existing 100 GiB archive expansion limit. One worker pool survives for the whole archive. Separate files and blocks are reconsidered so numeric selection is not inherited by unrelated text or images.

Native policy version 1 first skips numeric search below 1 MiB for Fast or 256 KiB for other goals. Three 4 KiB interior samples screen predominantly printable text. Other inputs use the Fast layout search; candidates whose best packing estimate retains at least 90% of input do not justify additional numeric trials. The search supports 16/32/64-bit little/big-endian words, packed 12-bit samples, multiple channel strides, modular differences and XOR.

Eligible inputs compare fixed candidate plans against ordinary Zstandard on three aligned 65,520-byte windows. Fast considers only packing. Balanced/Smallest additionally consider Dense. The ordinary comparison uses level 1, 3 or 9 respectively. A speed choice requires at least 15% estimated savings, including amortized selection cost, with at most 1% sampled size growth. A size choice requires more than 3% sampled improvement. Balanced accepts either; Smallest accepts size improvement. Dense's own entropy stage remains level 3.

These are bounded heuristics, not a guarantee of optimal whole-file speed or size. Probe timing can vary with CPU load. Sampled dictionary behavior can differ from a full block, and independent blocks do not retain TAR.ZST's cross-file history. Text and small files avoid the numeric trial cost. Stored fallback caps a block at its original length plus 29 bytes of frame overhead. JSON `numeric` reports actual codec block counts, worker count, total selection time and cumulative sample bytes examined (including repeated sample regions).

[Production-container measurements](native-codec-benchmark.md) include selection and file IO and must be used instead of substituting earlier research-only timings. The [original algorithm](../research/numeric-v2/README.md) and [optimization experiment](../research/dense-opt/README.md) preserve prior evidence.

## Sharing, encryption and compatibility

The `.lttl` extension can identify a plain native archive or an identity-encrypted envelope. **The extension alone does not indicate encryption.** Select **Encrypt for selected organization users** to require IDP access. Protected files keep the existing authenticated `LTTLID01` envelope, with `lttl` allowed as an inner format; unprotected native archives begin with `LTTLAR01`. Opening detects the bytes, not the filename. Explorer extraction therefore works for both and requests authentication only for a protected envelope.

Recipient authorization, tracking, audit, retention and legal holds continue to be enforced through the existing identity flow. Compression does not replace encryption or change IDP policy enforcement. An organization using native payloads must update recipients to alpha.4 or later. Sharing old ZIP/TAR payloads remains supported.

## Container and validation

`LTTLAR01` is followed by records. Each starts with a little-endian u32 JSON metadata length (maximum 192,128 bytes), UTF-8 `{path,size,is_dir}`, and its little-endian XXH64 checksum. Directories must have zero size. File data consists of u32 encoded length followed by an `LTNV0002` block, repeated until the declared file size is satisfied. Each block's original length is implied by the remaining file size and fixed maximum block size. A zero metadata length terminates the archive; trailing bytes are rejected. Empty files have no blocks.

The reader bounds metadata, compressed and decoded lengths before allocating, validates block checksums, rejects unsupported versions, truncation and trailing data, and applies the existing path, duplicate-name, entry-count and expansion limits. Extraction uses staging and no-clobber publication. Checksums detect accidental damage; only the encrypted envelope provides cryptographic authenticity. Normal checksum archives are not tamper-proof.

Cancellation is checked between bounded blocks and during extraction reads. A block already in compression/decompression completes before cancellation takes effect. Numeric scratch memory and Zstandard contexts are bounded by block size and worker count rather than whole-file size; peak memory has not been measured.

[Validation record](native-compression-validation.json): 49 passing release tests, seven hardware tests skipped, 336 production round trips and 171 local identity/audit checks. No live cloud IDP acceptance or visual GUI test is implied.
