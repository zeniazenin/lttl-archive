# Internal API contract

Library crate `lttl` exports (abbreviated signatures):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, clap::ValueEnum)]
pub enum Format { Zip, TarZstd, TarLz4 }
impl Format { pub fn extension(self) -> &'static str; }
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, clap::ValueEnum)]
pub enum Preset { Fast, Balanced, Small }
#[derive(Debug, Clone)]
pub struct CreateOptions { pub inputs: Vec<std::path::PathBuf>, pub output: std::path::PathBuf, pub format: Format, pub preset: Preset, pub threads: u32 }
#[derive(Debug, Clone, serde::Serialize)]
pub struct Progress { pub phase: String, pub path: String, pub bytes_done: u64, pub bytes_total: u64 }
#[derive(Debug, Clone, serde::Serialize)]
pub struct Stats { pub input_bytes: u64, pub output_bytes: u64, pub files: u64, pub elapsed_seconds: f64, pub gpu: Option<GpuMetrics>, pub selection: Option<Selection>, pub portable: Option<PortableMetrics> }
pub enum Backend { Auto, Cpu, CpuChunked, Gpu, Wgpu }
pub struct GpuOptions { pub chunk_kib: usize, pub batch_mib: usize }
#[derive(Debug, Clone, serde::Serialize)]
pub struct EntryInfo { pub path: String, pub size: u64, pub is_dir: bool }
pub fn create(options: &CreateOptions, cancel: &std::sync::atomic::AtomicBool, progress: &mut dyn FnMut(Progress)) -> anyhow::Result<Stats>;
pub fn create_with_backend(options: &CreateOptions, backend: Backend, gpu: &GpuOptions, cancel: &std::sync::atomic::AtomicBool, progress: &mut dyn FnMut(Progress)) -> anyhow::Result<Stats>;
pub fn gpu_status() -> serde_json::Value;
pub fn extract(archive: &std::path::Path, destination: &std::path::Path, cancel: &std::sync::atomic::AtomicBool, progress: &mut dyn FnMut(Progress)) -> anyhow::Result<Stats>;
pub fn list(archive: &std::path::Path) -> anyhow::Result<Vec<EntryInfo>>;
pub fn test(archive: &std::path::Path, cancel: &std::sync::atomic::AtomicBool) -> anyhow::Result<Stats>;
```

ZIP uses serial Deflate; CPU TAR.ZST uses threaded Zstandard. `threads=0` selects up to eight available logical processors, or one for Auto inputs below 8 MiB. `create` remains an explicit CPU convenience call; the CLI and GUI default to `create_with_backend` with `Backend::Auto`. Auto samples TAR.ZST inputs, treats `preset` as a goal, and chooses its own chunk/batch settings. Auto ZIP retains the existing CPU ZIP policy and has no `selection` report. Explicit GPU supports TAR.ZST only, uses fixed nvCOMP settings, and ignores CPU preset/thread settings. It emits checksummed standard concatenated Zstandard frames. `GpuOptions` defaults to 1024 KiB chunks and 32 MiB batches; supported explicit ranges are 64–16384 KiB and 16–128 MiB. `GpuMetrics` records the device, initialization, transfer/compression event times, host-call/checksum times, counts and transfer bytes; see [timing definitions](gpu-backend.md).

`Selection` records the policy version, goal, content class, backend, CPU preset, stored flag, workers, chunk/batch sizes, sampled byte/file counts, sample ratios, elapsed selection time, reason, optional GPU trial metrics, and optional GPU error. Only the applicable settings describe execution: GPU ignores the reported CPU preset/workers; streaming CPU ignores chunk/batch sizes. Auto storage writes standard raw Zstd frames in 4 MiB batches. `Stats.elapsed_seconds` includes selection and any trial. A selected GPU reuses the trial context; initialization/trial counters remain under `selection.gpu_trial`, while `Stats.gpu` describes archive encoding without counting that work twice. See [policy and limits](auto-compression.md).

Policy version 2 bypasses sampling for Fast, uses 64 KiB regions for Balanced, and keeps Small's original sample budget. Related-file detection resolves the CPU preset before codec trials. Absent CPU ratios mean skipped probes, not zero compression benefit. Fast reports zero sampled files/bytes; large Fast inputs use the `not-sampled` class. Selection timing ends at the decision; complete `Stats.elapsed_seconds` also includes local probe cleanup.

Fail instead of overwriting archive or extracted files. Input links/reparse points are rejected. `extract` supports an existing destination for Extract Here, creating only new files; cleanup of failed extraction is best effort. Extraction stays on CPU for every encoder. GUI operations run on worker threads. Auto falls back to CPU if the GPU trial is unavailable and records the error before creating output. Explicit GPU errors, or errors during selected GPU archive encoding, fail the operation without silently switching backend. No RAR creation/extraction is implemented.

GUI module `src/gui.rs`: export `pub fn run(initial: Option<GuiAction>) -> anyhow::Result<()>`, define public enum `GuiAction { Create(std::path::PathBuf), ExtractHere(std::path::PathBuf), ExtractFolder(std::path::PathBuf), Open(std::path::PathBuf) }`. GUI can choose all controls; create/extract shell actions prefill and wait for user to start. The CLI entry point is src/main.rs.

## Portable and protected archives

`create_with_provider` additionally takes `&PortableOptions { adapter, chunk_kib, batch_mib }`. `Backend::Wgpu` is TAR.LZ4-only; `portable_adapters()` enumerates available adapters and `clear_portable_sessions()` discards cached idle sessions. TAR.LZ4 Auto currently uses CPU. The CLI and GUI use the provider-aware entry point.

`identity` exposes profile validation, login/enrollment and protected create/list/test/extract. These operations use the same compression engine within the authenticated envelope. See [architecture](architecture.md) and [identity sharing](identity-sharing.md). Public APIs are alpha and may change; src/lib.rs and src/identity/mod.rs are authoritative.
