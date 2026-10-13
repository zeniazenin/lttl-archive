use anyhow::{Context, Result};
use lttl::{Backend, CreateOptions, Format, GpuOptions, Preset, Progress};
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::{path::PathBuf, sync::atomic::AtomicBool};

#[cfg(feature = "gui")]
mod gui;

#[derive(Parser)]
#[command(version, about = "lttl — CPU and portable/NVIDIA GPU archiving with standard ZIP / Zstandard / LZ4")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Create an archive. Existing outputs are never replaced.
    Create {
        #[arg(required = true)] inputs: Vec<PathBuf>,
        #[arg(short, long)] output: PathBuf,
        /// Protect the archive through this company identity/key-service profile.
        #[arg(long)] identity_profile: Option<PathBuf>,
        /// Enrolled recipient ID; repeat for each recipient. Requires identity profile.
        #[arg(long, requires="identity_profile")] recipient: Vec<String>,
        #[arg(short, long, value_enum, default_value = "tar-zstd")] format: Format,
        /// ZIP/Zstd compression goal or preset. LZ4 uses a fixed fast setting.
        #[arg(short, long, value_enum, default_value = "balanced")] preset: Preset,
        /// CPU Zstandard/LZ4 workers; 0 chooses automatically. ZIP is serial.
        #[arg(short, long, default_value_t = 0)] threads: u32,
        /// Auto selects CPU/NVIDIA Zstd; wgpu selects portable LZ4.
        #[arg(long, value_enum, default_value = "auto")] backend: Backend,
        /// Portable GPU name/vendor filter (intel, nvidia, amd). Only for TAR.LZ4.
        #[arg(long)] adapter: Option<String>,
        /// Zstd chunk MiB (default 1). Use --chunk-kib for LZ4.
        #[arg(long)] chunk_mib: Option<usize>,
        /// Chunk KiB: Zstd 64–16384; LZ4 4/8/16/32/64 (default 16).
        #[arg(long, conflicts_with = "chunk_mib")] chunk_kib: Option<usize>,
        /// Explicit GPU/CPU-chunked batch size; Auto chooses its own settings.
        #[arg(long, default_value_t = 32)] batch_mib: usize,
    },
    /// Extract safely without overwriting existing files.
    Extract { archive: PathBuf, #[arg(short, long)] destination: PathBuf, #[arg(long)] identity_profile:Option<PathBuf> },
    /// List archive members as JSON.
    List { archive: PathBuf, #[arg(long)] identity_profile:Option<PathBuf> },
    /// Read and verify archive data without publishing files.
    Test { archive: PathBuf, #[arg(long)] identity_profile:Option<PathBuf> },
    /// Compare codecs and presets on your input. Includes integrity verification.
    Benchmark {
        #[arg(required = true)] inputs: Vec<PathBuf>,
        #[arg(short, long, default_value_t = 0)] threads: u32,
        #[arg(short, long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=20))] runs: u32,
    },
    /// Repeat archive jobs in one process to measure cold versus reused GPU sessions.
    BenchmarkSession {
        #[arg(required=true)] inputs: Vec<PathBuf>,
        /// Must be an existing empty directory. Keeps archives for external verification.
        #[arg(long)] output_dir: PathBuf,
        #[arg(long, value_enum, default_value="tar-lz4")] format: Format,
        #[arg(long, value_enum, default_value="wgpu")] backend: Backend,
        #[arg(long)] adapter: Option<String>,
        #[arg(long, value_enum, default_value="fast")] preset: Preset,
        #[arg(long, default_value_t=8)] threads: u32,
        #[arg(long, default_value_t=16)] chunk_kib: usize,
        #[arg(long, default_value_t=16)] batch_mib: usize,
        #[arg(long, default_value_t=4, value_parser=clap::value_parser!(u32).range(1..=20))] runs: u32,
        /// Discard idle GPU sessions before each job for cold-context comparison.
        #[arg(long)] cold: bool,
    },
    /// Configure identity sharing, generate app-registration instructions, enroll or revoke.
    Identity { #[command(subcommand)] action:IdentityCommand },
    /// Report CPU capacity and GPU backend availability.
    Doctor,
    /// List portable hardware GPU adapters.
    GpuList,
    /// Open the desktop interface.
    Gui { archive: Option<PathBuf> },
    #[command(hide = true)] GuiCreate { path: PathBuf },
    #[command(hide = true)] GuiExtractHere { path: PathBuf },
    #[command(hide = true)] GuiExtractFolder { path: PathBuf },
}

#[derive(Subcommand)]
enum IdentityCommand {
    Guide {#[arg(long,value_enum,default_value="entra")] provider:lttl::identity::guides::Provider,#[arg(long,value_enum,default_value="azure")] hosting:lttl::identity::hosting::Hosting,#[arg(long)] broker_url:String,#[arg(long,default_value="")] tenant:String,#[arg(long,default_value="")] client_id:String,#[arg(short,long)] output:Option<PathBuf>},
    Configure {#[arg(long)] broker_url:String,#[arg(long,value_enum,default_value="entra")] provider:lttl::identity::guides::Provider,#[arg(short,long)] output:Option<PathBuf>},
    Users {#[arg(long)] profile:Option<PathBuf>},
    Revoke {archive:PathBuf,#[arg(long)] profile:Option<PathBuf>},
}
fn profile(path:Option<PathBuf>)->Result<lttl::identity::Profile> {lttl::identity::Profile::load(&path.unwrap_or_else(lttl::identity::default_profile_path))}
fn sign_in_notice(url:String) {eprintln!("Complete organization sign-in in your browser: {url}");}

fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn progress_printer() -> impl FnMut(Progress) {
    let mut last = std::time::Instant::now();
    move |p| {
        if last.elapsed().as_millis() > 250 {
            eprint!("\r{}: {} / {} MiB     ", p.phase, p.bytes_done / 1_048_576, p.bytes_total / 1_048_576);
            last = std::time::Instant::now();
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("\nError: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let cancel = AtomicBool::new(false);
    let mut progress = progress_printer();
    match cli.command {
        Some(Command::Create { inputs, output, format, preset, threads, backend, chunk_mib, chunk_kib, batch_mib, adapter, identity_profile, recipient }) => {
            anyhow::ensure!(format != Format::TarLz4 || chunk_mib.is_none(), "Use --chunk-kib for TAR.LZ4");
            anyhow::ensure!(adapter.is_none() || (format == Format::TarLz4 && backend == Backend::Wgpu), "--adapter requires --format tar-lz4 --backend wgpu");
            let options=CreateOptions{inputs,output,format,preset,threads};
            let gpu=GpuOptions{chunk_kib:chunk_kib.unwrap_or(chunk_mib.unwrap_or(1).checked_mul(1024).context("Chunk size overflow")?),batch_mib};
            let portable=lttl::PortableOptions{adapter,chunk_kib:chunk_kib.unwrap_or(16),batch_mib};
            let stats=if let Some(path)=identity_profile {
                anyhow::ensure!(!recipient.is_empty(),"Protected archives require at least one --recipient ID");
                let session=lttl::identity::login(&profile(Some(path))?,None,&cancel,&mut sign_in_notice)?;
                lttl::identity::create(&options,backend,&gpu,&portable,&session,&recipient,&cancel,&mut progress)?
            } else {lttl::create_with_provider(&options,backend,&gpu,&portable,&cancel,&mut progress)?};
            eprintln!();print_json(&stats)
        }
        Some(Command::Extract{archive,destination,identity_profile})=> {
            let stats=if lttl::identity::is_protected(&archive)? {lttl::identity::extract(&archive,&destination,&profile(identity_profile)?,&cancel,&mut progress,&mut sign_in_notice)?}else{lttl::extract(&archive,&destination,&cancel,&mut progress)?};
            eprintln!();print_json(&stats)
        }
        Some(Command::List{archive,identity_profile})=> {
            let entries=if lttl::identity::is_protected(&archive)? {lttl::identity::list(&archive,&profile(identity_profile)?,&cancel,&mut sign_in_notice)?}else{lttl::list(&archive)?};print_json(&entries)
        }
        Some(Command::Test{archive,identity_profile})=> {
            let stats=if lttl::identity::is_protected(&archive)? {lttl::identity::test(&archive,&profile(identity_profile)?,&cancel,&mut sign_in_notice)?}else{lttl::test(&archive,&cancel)?};print_json(&stats)
        }
        Some(Command::Identity{action})=>match action {
            IdentityCommand::Guide{provider,hosting,broker_url,tenant,client_id,output}=> {
                let guide=lttl::identity::hosting::generate(provider,hosting,&broker_url,&tenant,&client_id)?;
                if let Some(path)=output {use std::io::Write;let mut file=std::fs::OpenOptions::new().create_new(true).write(true).open(path)?;file.write_all(guide.as_bytes())?;}else{println!("{guide}");}Ok(())
            }
            IdentityCommand::Configure{broker_url,provider,output}=> {
                let path=output.unwrap_or_else(lttl::identity::default_profile_path);
                if let Some(parent)=path.parent().filter(|p|!p.as_os_str().is_empty()){std::fs::create_dir_all(parent)?;}
                lttl::identity::Profile{broker_url,provider}.save(&path)?;print_json(&serde_json::json!({"profile":path}))
            }
            IdentityCommand::Users{profile:path}=> {
                let session=lttl::identity::login(&profile(path)?,None,&cancel,&mut sign_in_notice)?;print_json(&session.users()?)
            }
            IdentityCommand::Revoke{archive,profile:path}=> {
                let mut file=std::fs::File::open(archive)?;let (header,_)=lttl::identity::crypto::read_header(&mut file)?;let profile=profile(path)?;
                anyhow::ensure!(lttl::identity::broker_url(&header.broker)?==profile.broker_url,"Archive and configured service differ");
                let session=lttl::identity::login(&profile,None,&cancel,&mut sign_in_notice)?;session.revoke(&header.archive_id)?;print_json(&serde_json::json!({"revoked":true}))
            }
        },
        Some(Command::Benchmark { inputs, threads, runs }) => {
            let temp = tempfile::tempdir().context("Create benchmark scratch directory")?;
            let mut rows = Vec::new();
            for format in [Format::Zip, Format::TarZstd] {
                for preset in [Preset::Fast, Preset::Balanced, Preset::Small] {
                    for run in 1..=runs {
                        let output = temp.path().join(format!("{format:?}-{preset:?}-{run}.{}", format.extension()));
                        let options = CreateOptions { inputs: inputs.clone(), output: output.clone(), format, preset, threads };
                        let stats = lttl::create(&options, &cancel, &mut progress)?;
                        lttl::test(&output, &cancel).context("Benchmark archive failed integrity verification")?;
                        rows.push(serde_json::json!({"format": format, "preset": preset, "run":run, "requested_threads":threads,
                            "ratio": if stats.input_bytes > 0 { Some(stats.output_bytes as f64 / stats.input_bytes as f64) } else { None },
                            "mib_per_second": stats.input_bytes as f64 / 1_048_576.0 / stats.elapsed_seconds.max(0.000001),
                            "stats":stats, "integrity_verified":true}));
                        std::fs::remove_file(output)?;
                    }
                }
            }
            eprintln!(); print_json(&rows)
        }
        Some(Command::BenchmarkSession {inputs,output_dir,format,backend,adapter,preset,threads,chunk_kib,batch_mib,runs,cold}) => {
            anyhow::ensure!(matches!(backend,Backend::Cpu|Backend::Wgpu|Backend::Auto), "Session benchmark supports CPU, wgpu and Auto");
            anyhow::ensure!(adapter.is_none() || backend==Backend::Wgpu,"Adapter selection requires wgpu");
            anyhow::ensure!(output_dir.is_dir() && std::fs::read_dir(&output_dir)?.next().is_none(),"Benchmark output directory must exist and be empty");
            // Later runs must not archive the outputs produced by earlier runs.
            let output_root=output_dir.canonicalize()?;
            for input in &inputs {
                let input_root=input.canonicalize()?;
                anyhow::ensure!(!input_root.is_dir() || !output_root.starts_with(&input_root), "Benchmark output directory must be outside every input directory");
            }
            let mut rows=Vec::new();
            lttl::clear_portable_sessions();
            for run in 0..runs {
                if cold {lttl::clear_portable_sessions();}
                let output=output_dir.join(format!("run-{run}.{}",format.extension()));
                let stats=lttl::create_with_provider(&CreateOptions{inputs:inputs.clone(),output:output.clone(),format,preset,threads},backend,
                    &GpuOptions::default(),&lttl::PortableOptions{adapter:adapter.clone(),chunk_kib,batch_mib},&cancel,&mut progress)?;
                rows.push(serde_json::json!({"run":run,"archive":output,"stats":stats}));
            }
            eprintln!();print_json(&rows)
        }
        Some(Command::GpuList) => print_json(&lttl::portable_adapters()),
        Some(Command::Doctor) => print_json(&serde_json::json!({
            "logical_processors": std::thread::available_parallelism().map(|n|n.get()).unwrap_or(1),
            "codecs": ["ZIP / Deflate (serial)", "tar.zst / Zstandard (multithreaded)"],
            "gpu_compression": lttl::gpu_status(),
            "rar_creation":false,
            "explorer": "Optional per-user installer script; Windows 11 Show more options"
        })),
        cmd => launch_gui(cmd),
    }
}

#[cfg(feature = "gui")]
fn launch_gui(command: Option<Command>) -> Result<()> {
    let initial = match command {
        Some(Command::Gui { archive: Some(path) }) => Some(gui::GuiAction::Open(path)),
        Some(Command::GuiCreate { path }) => Some(gui::GuiAction::Create(path)),
        Some(Command::GuiExtractHere { path }) => Some(gui::GuiAction::ExtractHere(path)),
        Some(Command::GuiExtractFolder { path }) => Some(gui::GuiAction::ExtractFolder(path)),
        _ => None,
    };
    gui::run(initial)
}

#[cfg(not(feature = "gui"))]
fn launch_gui(_: Option<Command>) -> Result<()> {
    anyhow::bail!("This build has no GUI. Build with default features or use --help for CLI commands.")
}
