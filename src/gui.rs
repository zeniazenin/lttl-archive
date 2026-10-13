//! Native desktop front end; archive work never runs on the UI thread.
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver}},
    time::Duration,
};

use eframe::egui::{self, Color32, RichText};
use lttl::{Backend, CreateOptions, EntryInfo, Format, GpuOptions, Preset, Progress, Stats};

const ACCENT: Color32 = Color32::from_rgb(116, 171, 255);
const MUTED: Color32 = Color32::from_rgb(153, 165, 185);

pub enum GuiAction {
    Create(PathBuf),
    ExtractHere(PathBuf),
    ExtractFolder(PathBuf),
    Open(PathBuf),
}

pub fn run(initial: Option<GuiAction>) -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("lttl")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([820.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native("lttl", options, Box::new(move |cc| {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(19, 24, 34);
        visuals.window_fill = Color32::from_rgb(27, 34, 47);
        visuals.extreme_bg_color = Color32::from_rgb(13, 18, 27);
        visuals.selection.bg_fill = Color32::from_rgb(48, 86, 142);
        visuals.widgets.active.bg_fill = Color32::from_rgb(49, 83, 132);
        cc.egui_ctx.set_visuals(visuals);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(12.0, 12.0);
        style.spacing.button_padding = egui::vec2(14.0, 9.0);
        style.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        style.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        cc.egui_ctx.set_style(style);
        Ok(Box::new(LttlApp::new(initial)))
    })).map_err(|error| anyhow::anyhow!("Cannot open desktop interface: {error}"))
}

#[derive(Clone, Copy, PartialEq)]
enum View { Create, Extract, Inspect, Identity }

enum Job {
    Create(CreateOptions, Backend, GpuOptions, lttl::PortableOptions),
    SharedCreate(CreateOptions, Backend, GpuOptions, lttl::PortableOptions, lttl::identity::Profile, Vec<String>, lttl::identity::crypto::TrackingPolicy),
    SignIn(lttl::identity::Profile),
    Extract(PathBuf, PathBuf),
    Inspect(PathBuf),
    Test(PathBuf),
}

enum JobOutput { Archive(Stats), Entries(Vec<EntryInfo>), Identity(Vec<lttl::identity::User>, lttl::identity::User) }
enum Event { Progress(Progress), Browser(String), Finished(Result<JobOutput, String>) }

struct Worker {
    receiver: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    cancellable: bool,
}

struct LttlApp {
    view: View,
    inputs: Vec<PathBuf>,
    output: String,
    format: Format,
    preset: Preset,
    threads: u32,
    backend: Backend,
    gpu_chunk_kib: usize,
    gpu_batch_mib: usize,
    portable_adapter: String,
    archive: String,
    destination: String,
    worker: Option<Worker>,
    progress: Option<Progress>,
    status: String,
    error: Option<String>,
    stats: Option<Stats>,
    entries: Vec<EntryInfo>,
    inspected_archive: String,
    filter: String,
    close_after_work: bool,
    identity_profile: lttl::identity::Profile,
    identity_hosting: lttl::identity::hosting::Hosting,
    share_identity: bool,
    track_access:bool,
    alert_countries:String,
    recipients: Vec<lttl::identity::User>, selected_recipients: Vec<String>, recipient_filter: String,
    identity_user: String, sign_in_url: String, guide: String, idp_tenant: String, idp_client_id: String,

}

impl LttlApp {
    fn new(initial: Option<GuiAction>) -> Self {
        let mut app = Self {
            view: View::Create, inputs: Vec::new(), output: String::new(),
            format: Format::TarZstd, preset: Preset::Balanced, threads: 0,
            backend: Backend::Auto, gpu_chunk_kib: 1024, gpu_batch_mib: 32, portable_adapter: String::new(),
            archive: String::new(), destination: String::new(), worker: None,
            progress: None, status: "Ready when you are".into(), error: None,
            stats: None, entries: Vec::new(), inspected_archive: String::new(), filter: String::new(), close_after_work: false,
            identity_profile: lttl::identity::Profile::default(),share_identity:false,track_access:false,alert_countries:String::new(),
            identity_hosting:lttl::identity::hosting::Hosting::default(),
            recipients:Vec::new(),selected_recipients:Vec::new(),recipient_filter:String::new(),identity_user:String::new(),
            sign_in_url:String::new(),guide:String::new(),idp_tenant:String::new(),idp_client_id:String::new(),
        };
        let profile_path=lttl::identity::default_profile_path();
        if profile_path.exists() {match lttl::identity::Profile::load(&profile_path) {Ok(profile)=>app.identity_profile=profile,Err(error)=>app.error=Some(format!("Identity profile: {error:#}"))}}
        match initial {
            Some(GuiAction::Create(path)) => app.add_inputs(vec![path]),
            Some(GuiAction::ExtractHere(path)) => {
                app.view = View::Extract;
                app.destination = archive_parent(&path).display().to_string();
                app.archive = path.display().to_string();
            }
            Some(GuiAction::ExtractFolder(path)) => {
                app.view = View::Extract;
                app.destination = named_destination(&path).display().to_string();
                app.archive = path.display().to_string();
            }
            Some(GuiAction::Open(path)) => {
                app.view = View::Inspect;
                app.archive = path.display().to_string();
            }
            None => {}
        }
        app
    }

    fn add_inputs(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            if !self.inputs.contains(&path) { self.inputs.push(path); }
        }
        if self.output.is_empty() {
            if let Some(first) = self.inputs.first() {
                let name = first.file_name().unwrap_or_default().to_string_lossy();
                self.output = first.with_file_name(format!("{name}.{}", self.format.extension())).display().to_string();
            }
        }
    }

    fn choose_archive(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Supported archives", &["zip", "zst", "zstd", "lz4", "lttl"]).pick_file() {
            self.archive = path.display().to_string();
            self.destination = named_destination(&path).display().to_string();
        }
    }

    fn start(&mut self, ctx: &egui::Context, job: Job) {
        if self.worker.is_some() { return; }
        self.error = None;
        self.sign_in_url.clear();
        self.stats = None;
        self.progress = None;
        self.status = match &job {
            Job::Create(_, Backend::Auto, _, _) => "Choosing compression for this content",
            Job::Create(_, Backend::Cpu | Backend::CpuChunked, _, _) => "Creating archive on CPU",
            Job::Create(_, Backend::Wgpu, _, _) => "Creating archive on portable GPU",
            Job::Create(_, Backend::Gpu, _, _) => "Creating archive on NVIDIA GPU",
            Job::SharedCreate(..) => "Signing in to protect the archive",
            Job::SignIn(_) => "Signing in to your organization",
            Job::Extract(..) => "Extracting archive",
            Job::Inspect(_) => "Reading archive contents",
            Job::Test(_) => "Verifying archive integrity",
        }.into();
        if let Job::Inspect(path) = &job {
            self.entries.clear();
            self.inspected_archive = path.display().to_string();
        }
        let cancellable = !matches!(&job, Job::Inspect(path) if !lttl::identity::is_protected(path).unwrap_or(false));
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let (sender, receiver) = mpsc::sync_channel(64);
        let repaint = ctx.clone();
        let identity_profile=self.identity_profile.clone();
        std::thread::spawn(move || {
            let mut progress = |value| {
                let _ = sender.try_send(Event::Progress(value));
                repaint.request_repaint();
            };
            let mut notice=|url:String| {let _=sender.send(Event::Browser(url));repaint.request_repaint();};
            let result = match job {
                Job::Create(options, backend, gpu, portable) => lttl::create_with_provider(&options, backend, &gpu, &portable, &worker_cancel, &mut progress).map(JobOutput::Archive),
                Job::SharedCreate(options,backend,gpu,portable,profile,recipients,tracking) => lttl::identity::login(&profile,None,&worker_cancel,&mut notice).and_then(|session|lttl::identity::create_tracked(&options,backend,&gpu,&portable,&session,&recipients,&tracking,&worker_cancel,&mut progress)).map(JobOutput::Archive),
                Job::SignIn(profile) => lttl::identity::login(&profile,None,&worker_cancel,&mut notice).and_then(|session|Ok(JobOutput::Identity(session.users()?,session.user.clone()))),
                Job::Extract(archive,dest) => lttl::identity::is_protected(&archive).and_then(|protected|if protected {lttl::identity::extract(&archive,&dest,&identity_profile,&worker_cancel,&mut progress,&mut notice)} else {lttl::extract(&archive,&dest,&worker_cancel,&mut progress)}).map(JobOutput::Archive),
                Job::Inspect(archive) => lttl::identity::is_protected(&archive).and_then(|protected|if protected {lttl::identity::list(&archive,&identity_profile,&worker_cancel,&mut notice)} else {lttl::list(&archive)}).map(JobOutput::Entries),
                Job::Test(archive) => lttl::identity::is_protected(&archive).and_then(|protected|if protected {lttl::identity::test(&archive,&identity_profile,&worker_cancel,&mut notice)} else {lttl::test(&archive,&worker_cancel)}).map(JobOutput::Archive),
            }.map_err(|error| format!("{error:#}"));
            let _ = sender.send(Event::Finished(result));
            repaint.request_repaint();
        });
        self.worker = Some(Worker { receiver, cancel, cancellable });
    }

    fn poll(&mut self) {
        let mut finished = None;
        if let Some(worker) = &self.worker {
            loop {
                match worker.receiver.try_recv() {
                    Ok(Event::Browser(url)) => {self.sign_in_url=url;self.status="Complete sign-in in your browser".into();},
                    Ok(Event::Progress(progress)) => self.progress = Some(progress),
                    Ok(Event::Finished(result)) => { finished = Some(result); break; }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        finished = Some(Err("The archive worker stopped unexpectedly. Please retry.".into()));
                        break;
                    }
                }
            }
        }
        if let Some(result) = finished {
            let cancelled = self.worker.as_ref().is_some_and(|w| w.cancel.load(Ordering::Relaxed));
            self.worker = None;
            match result {
                Ok(JobOutput::Identity(users,user)) => {
                    self.identity_user=format!("{} ({})",user.name,user.email);self.recipients=users;
                    self.selected_recipients.retain(|id|self.recipients.iter().any(|u|&u.id==id));
                    self.status=format!("Signed in as {}. {} enrolled recipients available.",self.identity_user,self.recipients.len());
                    self.sign_in_url.clear();
                }
                Ok(JobOutput::Archive(stats)) => { self.status = "Operation completed".into(); self.stats = Some(stats); }
                Ok(JobOutput::Entries(entries)) => {
                    self.status = format!("{} entries in archive", entries.len());
                    self.entries = entries;
                }
                Err(error) => {
                    self.status = if cancelled { "Operation stopped" } else { "Operation failed" }.into();
                    self.error = Some(error);
                }
            }
        }
    }

    fn create_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section_heading(ui, "Create an archive", "Collect your files, choose a format, and make room for more.");
        ui.horizontal(|ui| {
            ui.label(RichText::new("SOURCE FILES").small().color(MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Add folder").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() { self.add_inputs(vec![path]); }
                }
                if ui.button("Add files").clicked() {
                    if let Some(paths) = rfd::FileDialog::new().pick_files() { self.add_inputs(paths); }
                }
            });
        });
        egui::Frame::group(ui.style()).inner_margin(14.0).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if self.inputs.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Drop files or folders here").size(19.0));
                    ui.label(RichText::new("Or use Add files and Add folder above").color(MUTED));
                });
                ui.add_space(20.0);
            } else {
                let mut remove = None;
                egui::ScrollArea::vertical().id_salt("sources").max_height(155.0).show(ui, |ui| {
                    for (index, path) in self.inputs.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(if path.is_dir() { "FOLDER" } else { "FILE" });
                            ui.add_sized([ui.available_width() - 95.0, 26.0], egui::Label::new(path.display().to_string()).truncate().halign(egui::Align::LEFT)).on_hover_text(path.display().to_string());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Remove").clicked() { remove = Some(index); }
                            });
                        });
                    }
                });
                if let Some(index) = remove { self.inputs.remove(index); }
            }
        });
        ui.add_space(3.0);
        ui.columns(2, |columns| {
            columns[0].label(RichText::new("FORMAT").small().color(MUTED));
            let previous = self.format;
            columns[0].horizontal(|ui| {
                ui.selectable_value(&mut self.format, Format::TarZstd, "TAR + Zstandard");
                ui.selectable_value(&mut self.format, Format::Zip, "ZIP");
                ui.selectable_value(&mut self.format, Format::TarLz4, "TAR + LZ4");
            });
            columns[0].label(RichText::new(match self.format {
                Format::TarLz4 => "Independent LZ4 blocks for CPU or portable GPU compression.",
                Format::TarZstd => "Zstandard with CPU or NVIDIA GPU compression.",
                Format::Zip => "Broad compatibility. Deflate uses a single CPU thread.",
            }).small().color(MUTED));
            if self.format != previous && !self.output.is_empty() {
                let old_ext = format!(".{}", previous.extension());
                if let Some(stem) = self.output.strip_suffix(&old_ext) {
                    self.output = format!("{stem}.{}", self.format.extension());
                }
            }
            if self.format == Format::Zip { self.backend = Backend::Cpu; }
            if self.format == Format::TarLz4 && matches!(self.backend, Backend::Gpu | Backend::CpuChunked) { self.backend = Backend::Auto; }
            if self.format == Format::TarZstd && self.backend == Backend::Wgpu { self.backend = Backend::Auto; }
            columns[1].label(RichText::new("COMPRESSION PREFERENCE").small().color(MUTED));
            columns[1].add_enabled_ui(self.format != Format::TarLz4 && matches!(self.backend, Backend::Cpu | Backend::Auto), |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.preset, Preset::Fast, "Fast");
                    ui.selectable_value(&mut self.preset, Preset::Balanced, "Balanced");
                    ui.selectable_value(&mut self.preset, Preset::Small, "Smallest");
                });
            });
            let preset_hint = if self.format == Format::TarLz4 { "LZ4 uses a fixed fast setting; presets apply to ZIP and Zstandard." } else if self.backend == Backend::Gpu {
                "GPU uses a fixed setting; CPU presets do not apply."
            } else { match self.preset {
                Preset::Fast if self.format == Format::Zip => "Favor speed; store data when sampling finds little benefit.",
                Preset::Fast => "Favor speed for everyday transfers.",
                Preset::Balanced => "A practical balance of size and speed.",
                Preset::Small => "Favor smaller output; Auto may compare CPU and GPU samples.",
            }};
            columns[1].label(RichText::new(preset_hint).small().color(MUTED));
        });
        if self.format == Format::TarZstd {
            ui.horizontal(|ui| {
                ui.label(RichText::new("COMPRESS WITH").small().color(MUTED));
                ui.selectable_value(&mut self.backend, Backend::Auto, "Auto");
                ui.selectable_value(&mut self.backend, Backend::Cpu, "CPU");
                ui.selectable_value(&mut self.backend, Backend::Gpu, "NVIDIA GPU");
            });
            if self.backend == Backend::Auto {
                ui.label(RichText::new("Samples content, keeps shared history, and reports the chosen setting. Sampling is a heuristic, not a guaranteed optimum.").small().color(MUTED));
            } else if self.backend == Backend::Gpu {
                ui.label(RichText::new("GPU speed and archive size depend on your files. Chunk and batch sizes are in Advanced controls.").small().color(MUTED));
            }
        }
        if self.format == Format::TarLz4 {
            ui.horizontal(|ui| {
                ui.label("Compress with");
                ui.selectable_value(&mut self.backend, Backend::Auto, "Auto");
                ui.selectable_value(&mut self.backend, Backend::Cpu, "CPU");
                ui.selectable_value(&mut self.backend, Backend::Wgpu, "Portable GPU");
            });
            if self.backend == Backend::Wgpu {
                ui.horizontal(|ui| {ui.label("GPU name or vendor"); ui.add(egui::TextEdit::singleline(&mut self.portable_adapter).hint_text("intel, nvidia, amd; blank = first GPU"));});
            } else if self.backend == Backend::Auto {ui.label("Auto keeps CPU LZ4 until a portable GPU shows an end-to-end advantage.");}
        }
        ui.add_space(5.0);
        ui.label(RichText::new("SAVE ARCHIVE TO").small().color(MUTED));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.output).desired_width(ui.available_width() - 110.0).hint_text("Choose an archive location"));
            if ui.button("Browse…").clicked() {
                let fallback = format!("Archive.{}", self.format.extension());
                let file_name = Path::new(&self.output).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or(fallback);
                let mut dialog = rfd::FileDialog::new().set_file_name(file_name);
                if let Some(parent) = Path::new(&self.output).parent().filter(|p| p.is_dir()) { dialog = dialog.set_directory(parent); }
                if let Some(path) = dialog.save_file() { self.output = path.display().to_string(); }
            }
        });
        egui::CollapsingHeader::new("Advanced controls").show(ui, |ui| {
            if self.backend == Backend::Gpu && self.format == Format::TarZstd {
                ui.horizontal(|ui| {
                    ui.label("GPU chunk size");
                    ui.add(egui::DragValue::new(&mut self.gpu_chunk_kib).range(64..=16384).speed(64).suffix(" KiB"));
                    ui.label("GPU batch size");
                    ui.add(egui::DragValue::new(&mut self.gpu_batch_mib).range(16..=128).suffix(" MiB"));
                });
                ui.label(RichText::new("Larger batches use more memory. Chunks compress independently with a fixed GPU setting.").small().color(MUTED));
            } else {
                ui.horizontal(|ui| {
                    ui.label("CPU threads");
                    ui.add_enabled(self.format != Format::Zip, egui::DragValue::new(&mut self.threads).range(0..=256));
                    ui.label(RichText::new("0 = automatic").color(MUTED));
                });
            }
            ui.label(RichText::new("Existing files are never replaced. Symbolic links and reparse points are rejected.").small().color(MUTED));
        });
        ui.add_space(5.0);
        ui.separator();
        let was_shared=self.share_identity;
        ui.checkbox(&mut self.share_identity,"Encrypt for selected organization users");
        if was_shared!=self.share_identity && !self.output.is_empty() {
            if self.share_identity && !self.output.ends_with(".lttl") {self.output.push_str(".lttl");}
            else if !self.share_identity {if let Some(path)=self.output.strip_suffix(".lttl") {self.output=path.into();}}
        }
        if self.share_identity {
            ui.label("Recipients must sign in to the configured organization key service to open this archive.");
            ui.checkbox(&mut self.track_access,"Track access attempts for this archive");
            if self.track_access {
                ui.label("Reports identity, time and estimated network country to the organization's security webhook.");
                ui.add(egui::TextEdit::singleline(&mut self.alert_countries).hint_text("Alert country codes, comma separated (e.g. KP)"));
            }
            ui.label(RichText::new("Your organization may require tracking. The saved archive carries the policy; recipients cannot switch it off.").small().color(MUTED));
            ui.horizontal(|ui| {
                if ui.button("Identity settings").clicked() {self.view=View::Identity;}
                if ui.add_enabled(!self.identity_profile.broker_url.is_empty(),egui::Button::new("Sign in / refresh recipients")).clicked() {self.start(ctx,Job::SignIn(self.identity_profile.clone()));}
            });
            ui.label(format!("Service: {}",self.identity_profile.broker_url));
            ui.add(egui::TextEdit::singleline(&mut self.recipient_filter).hint_text("Find enrolled recipient"));
            let filter=self.recipient_filter.to_lowercase();
            egui::ScrollArea::vertical().id_salt("recipients").max_height(140.0).show(ui,|ui| {
                for user in &self.recipients {
                    if !format!("{} {} {}",user.name,user.email,user.id).to_lowercase().contains(&filter) {continue;}
                    let mut selected=self.selected_recipients.contains(&user.id);
                    if ui.checkbox(&mut selected,format!("{} · {} · {}",user.name,user.email,&user.id[..user.id.len().min(8)])).on_hover_text(format!("Account ID: {}",user.id)).changed() {
                        if selected {self.selected_recipients.push(user.id.clone());}else{self.selected_recipients.retain(|id|id!=&user.id);}
                    }
                }
            });
            ui.label(format!("{} selected. Add yourself if you need to reopen it. Users appear after their first sign-in.",self.selected_recipients.len()));
        }
        let ready = !self.inputs.is_empty() && !self.output.trim().is_empty() && (!self.share_identity || !self.selected_recipients.is_empty());
        if ui.add_enabled(ready, primary_button(if self.share_identity {"Create encrypted archive"}else{"Create archive"})).clicked() {
            let options=CreateOptions{inputs:self.inputs.clone(),output:PathBuf::from(self.output.trim()),format:self.format,preset:self.preset,threads:self.threads};
            let backend=if self.format==Format::Zip {Backend::Cpu}else{self.backend};
            let gpu=GpuOptions{chunk_kib:self.gpu_chunk_kib,batch_mib:self.gpu_batch_mib};
            let portable=lttl::PortableOptions{adapter:if self.portable_adapter.trim().is_empty(){None}else{Some(self.portable_adapter.trim().into())},..Default::default()};
            let job=if self.share_identity {Job::SharedCreate(options,backend,gpu,portable,self.identity_profile.clone(),self.selected_recipients.clone(),lttl::identity::crypto::TrackingPolicy{enabled:self.track_access,alert_countries:if self.track_access {self.alert_countries.split(',').map(str::trim).filter(|s|!s.is_empty()).map(|s|s.to_uppercase()).collect()}else{Vec::new()}})}else{Job::Create(options,backend,gpu,portable)};
            self.start(ctx,job);
        }
    }
    fn identity_view(&mut self,ui:&mut egui::Ui,ctx:&egui::Context) {
        section_heading(ui,"Identity & sharing","Connect to your company's identity-backed key service.");
        egui::ComboBox::from_label("Identity provider").selected_text(self.identity_profile.provider.label()).show_ui(ui,|ui| {
            for provider in lttl::identity::guides::Provider::ALL {ui.selectable_value(&mut self.identity_profile.provider,provider,provider.label());}
        });
        ui.label("Company key-service URL");
        if ui.add(egui::TextEdit::singleline(&mut self.identity_profile.broker_url).hint_text("https://keys.company.example").desired_width(f32::INFINITY)).changed() {
            self.recipients.clear();self.selected_recipients.clear();self.identity_user.clear();
        }
        ui.horizontal(|ui| {
            if ui.button("Save connection").clicked() {
                let path=lttl::identity::default_profile_path();
                let result=(||->anyhow::Result<()> {std::fs::create_dir_all(path.parent().unwrap())?;self.identity_profile.save(&path)})();
                match result {Ok(())=>self.status="Identity connection saved (no tokens or secrets)".into(),Err(e)=>self.error=Some(format!("{e:#}"))}
            }
            if ui.button("Import profile").clicked() {if let Some(path)=rfd::FileDialog::new().add_filter("Identity profile",&["json"]).pick_file() {
                match lttl::identity::Profile::load(&path) {Ok(profile)=>{self.identity_profile=profile;self.recipients.clear();self.selected_recipients.clear();},Err(e)=>self.error=Some(format!("{e:#}"))}
            }}
            if ui.button("Sign in / enroll").clicked() {self.start(ctx,Job::SignIn(self.identity_profile.clone()));}
        });
        if !self.identity_user.is_empty() {ui.label(format!("Last sign-in: {}",self.identity_user));}
        ui.separator();
        ui.label(RichText::new("Administrator setup instructions").strong());
        egui::ComboBox::from_label("Host key service on").selected_text(self.identity_hosting.label()).show_ui(ui,|ui| {
            for host in lttl::identity::hosting::Hosting::ALL {ui.selectable_value(&mut self.identity_hosting,host,host.label());}
        });
        ui.label("Hosting and identity provider are independent. Each company runs its own service.");
        ui.label("Tenant ID (Entra) or provider domain (other providers)");ui.text_edit_singleline(&mut self.idp_tenant);
        ui.label("Application client ID, if already registered (not a secret)");ui.text_edit_singleline(&mut self.idp_client_id);
        ui.horizontal(|ui| {
            if ui.button("Generate registration instructions").clicked() {
                match lttl::identity::hosting::generate(self.identity_profile.provider,self.identity_hosting,&self.identity_profile.broker_url,&self.idp_tenant,&self.idp_client_id) {Ok(guide)=>self.guide=guide,Err(e)=>self.error=Some(format!("{e:#}"))}
            }
            if ui.add_enabled(!self.guide.is_empty(),egui::Button::new("Save instructions…")).clicked() {if let Some(path)=rfd::FileDialog::new().set_file_name("lttl-idp-registration.md").save_file() {
                if let Err(e)=std::fs::write(path,&self.guide) {self.error=Some(e.to_string());}
            }}
        });
        if !self.guide.is_empty() {egui::ScrollArea::vertical().id_salt("identity_guide").max_height(350.0).show(ui,|ui|{ui.add(egui::TextEdit::multiline(&mut self.guide).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));});}
        ui.label("An IDP app registration authenticates people; the company key service enforces the recipient list. SAML is supported through federation. Authorized recipients can retain decrypted copies.");
    }

    fn archive_picker(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Protected archives contact the organization service when opened. Access attempts, identity, time and network location may be reported to its security system.").small().color(MUTED));
        ui.label(RichText::new("ARCHIVE").small().color(MUTED));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.archive).desired_width(ui.available_width() - 110.0).hint_text("Select a .zip, .tar.zst, .tar.lz4 or protected .lttl archive"));
            if ui.button("Browse…").clicked() { self.choose_archive(); }
        });
    }

    fn extract_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section_heading(ui, "Unpack your files", "Extract ZIP and TAR + Zstandard archives into a folder you choose.");
        self.archive_picker(ui);
        ui.add_space(12.0);
        ui.label(RichText::new("DESTINATION").small().color(MUTED));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.destination).desired_width(ui.available_width() - 110.0).hint_text("Choose an extraction folder"));
            if ui.button("Browse…").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() { self.destination = path.display().to_string(); }
            }
        });
        ui.add_enabled_ui(!self.archive.trim().is_empty(), |ui| {
            ui.horizontal(|ui| {
                if ui.button("Extract here").clicked() {
                    self.destination = archive_parent(Path::new(self.archive.trim())).display().to_string();
                }
                if ui.button("Use named folder").clicked() {
                    self.destination = named_destination(Path::new(self.archive.trim())).display().to_string();
                }
            });
        });
        ui.add_space(12.0);
        egui::Frame::group(ui.style()).inner_margin(16.0).show(ui, |ui| {
            ui.label(RichText::new("Your existing files stay protected").strong());
            ui.label(RichText::new("Extraction stops if a file already exists. Choose an empty destination to avoid conflicts.").color(MUTED));
            ui.label(RichText::new("RAR and 7z support are not included in this version.").small().color(MUTED));
        });
        ui.add_space(8.0);
        if ui.add_enabled(!self.archive.trim().is_empty() && !self.destination.trim().is_empty(), primary_button("Extract archive")).clicked() {
            self.start(ctx, Job::Extract(PathBuf::from(self.archive.trim()), PathBuf::from(self.destination.trim())));
        }
    }

    fn inspect_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section_heading(ui, "Look inside", "Browse archive contents or read every entry to check its integrity.");
        self.archive_picker(ui);
        ui.horizontal(|ui| {
            let ready = !self.archive.trim().is_empty();
            if ui.add_enabled(ready, primary_button("Read contents")).clicked() {
                self.start(ctx, Job::Inspect(PathBuf::from(self.archive.trim())));
            }
            if ui.add_enabled(ready, egui::Button::new("Test archive")).clicked() {
                self.start(ctx, Job::Test(PathBuf::from(self.archive.trim())));
            }
        });
        ui.label(RichText::new("Reading TAR + Zstandard contents requires streaming through the archive.").small().color(MUTED));
        if !self.inspected_archive.is_empty() {
            ui.separator();
            ui.label(RichText::new(&self.inspected_archive).small().color(MUTED));
            ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Filter entries by name").desired_width(f32::INFINITY));
            let filter = self.filter.to_lowercase();
            let rows: Vec<_> = self.entries.iter().filter(|e| e.path.to_lowercase().contains(&filter)).collect();
            ui.label(RichText::new(format!("{} entries", rows.len())).small().color(MUTED));
            egui::ScrollArea::vertical().id_salt("contents").max_height(330.0).show_rows(ui, 25.0, rows.len(), |ui, range| {
                for index in range {
                    let entry = rows[index];
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(if entry.is_dir { "DIR" } else { "FILE" }).small().color(MUTED));
                        ui.add_sized([ui.available_width() - 110.0, 25.0], egui::Label::new(&entry.path).truncate().halign(egui::Align::LEFT)).on_hover_text(&entry.path);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !entry.is_dir { ui.label(RichText::new(human_bytes(entry.size)).monospace().color(MUTED)); }
                        });
                    });
                }
            });
        }
    }

    fn status_panel(&mut self, ui: &mut egui::Ui) {
        if self.worker.is_some() && !self.sign_in_url.is_empty() {ui.hyperlink_to("Open organization sign-in in browser",&self.sign_in_url);}
        ui.horizontal(|ui| {
            if self.worker.is_some() { ui.spinner(); }
            ui.label(RichText::new(&self.status).strong().color(ACCENT));
            if let Some(worker) = &self.worker {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cancelled = worker.cancel.load(Ordering::Relaxed);
                    if ui.add_enabled(worker.cancellable && !cancelled, egui::Button::new(if cancelled { "Stopping…" } else { "Cancel" })).clicked() {
                        worker.cancel.store(true, Ordering::Relaxed);
                    }
                });
            }
        });
        if self.worker.is_some() {
            if let Some(progress) = &self.progress {
                if progress.bytes_total > 0 {
                    let ratio = (progress.bytes_done as f64 / progress.bytes_total as f64).clamp(0.0, 1.0) as f32;
                    ui.add(egui::ProgressBar::new(ratio).text(format!("{} / {}", human_bytes(progress.bytes_done), human_bytes(progress.bytes_total))).fill(ACCENT));
                } else { ui.label(RichText::new(format!("{} processed", human_bytes(progress.bytes_done))).small().color(MUTED)); }
                ui.add(egui::Label::new(format!("{}  ·  {}", progress.phase, progress.path)).truncate()).on_hover_text(&progress.path);
            } else { ui.label(RichText::new("Preparing…").small().color(MUTED)); }
        }
        if let Some(stats) = &self.stats {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("{} {}", stats.files, if stats.files == 1 { "file" } else { "files" }));
                ui.label(format!("Input {}", human_bytes(stats.input_bytes)));
                ui.label(format!("Output {}", human_bytes(stats.output_bytes)));
                ui.label(format!("{:.2} s", stats.elapsed_seconds));
                if stats.elapsed_seconds > 0.0 {
                    ui.label(format!("{}/s", human_bytes((stats.input_bytes as f64 / stats.elapsed_seconds) as u64)));
                }
            });
            if let Some(portable) = &stats.portable {
                ui.label(format!("{} / {} · initialization {:.3} s", portable.device, portable.api, portable.initialize_seconds));
                if portable.session_reused { ui.label(RichText::new("Reused GPU session").small().color(MUTED)); }
                if let Some(reason) = &portable.auto_reason { ui.label(RichText::new(reason).small().color(MUTED)); }
            }
            if let Some(choice) = &stats.selection {
                ui.label(format!("Auto chose {:?} / {} · {} · sampled {} in {:.3} s", choice.backend,
                    if choice.stored { "Stored".into() } else if choice.backend == Backend::Gpu { "fixed Zstd".into() } else { format!("{:?}", choice.preset) },
                    choice.content_class, human_bytes(choice.sampled_bytes as u64), choice.selection_seconds));
                ui.label(RichText::new(&choice.reason).small().color(MUTED));
            }
        }
        if let Some(error) = &self.error {
            egui::ScrollArea::vertical().id_salt("error").max_height(80.0).show(ui, |ui| {
                ui.label(RichText::new(error).color(Color32::from_rgb(255, 166, 153)));
            });
        }
    }
}

impl eframe::App for LttlApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        if self.close_after_work && self.worker.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.worker.is_some() { ctx.request_repaint_after(Duration::from_millis(100)); }
        if self.worker.is_none() {
            let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
            if !dropped.is_empty() {
                if self.view == View::Create { self.add_inputs(dropped); }
                else {
                    self.archive = dropped[0].display().to_string();
                    self.destination = named_destination(&dropped[0]).display().to_string();
                }
            }
        }
        egui::TopBottomPanel::top("header").frame(egui::Frame::new().fill(Color32::from_rgb(15, 20, 29)).inner_margin(22.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("lttl").size(28.0).strong().color(ACCENT));
                ui.label(RichText::new("Files, packed thoughtfully.").color(MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("RUST ENGINE  /  PREVIEW").small().color(MUTED));
                });
            });
        });
        egui::TopBottomPanel::bottom("status").frame(egui::Frame::new().fill(Color32::from_rgb(15, 20, 29)).inner_margin(20.0)).show(ctx, |ui| self.status_panel(ui));
        egui::SidePanel::left("navigation").resizable(false).exact_width(174.0).frame(egui::Frame::new().fill(Color32::from_rgb(17, 22, 31)).inner_margin(18.0)).show(ctx, |ui| {
            ui.add_space(10.0);
            ui.add_enabled_ui(self.worker.is_none(), |ui| {
                for (view, label) in [(View::Create, "Create archive"), (View::Extract, "Extract files"), (View::Inspect, "Inspect & test"), (View::Identity,"Identity & sharing")] {
                    ui.selectable_value(&mut self.view, view, label);
                }
            });
            ui.add_space(25.0);
            ui.separator();
            ui.label(RichText::new("OPEN FORMATS").small().color(ACCENT));
            ui.label(RichText::new("ZIP\nTAR + Zstandard").color(MUTED));
            ui.add_space(16.0);
            ui.label(RichText::new("Explorer actions").strong());
            ui.label(RichText::new("Create archive\nExtract here\nExtract to folder").small().color(MUTED));
            ui.label(RichText::new("Available after installing Explorer integration with the CLI.").small().color(MUTED));
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(23, 29, 40)).inner_margin(26.0)).show(ctx, |ui| {
            egui::ScrollArea::vertical().id_salt("main").show(ui, |ui| {
                ui.add_enabled_ui(self.worker.is_none(), |ui| match self.view {
                    View::Create => self.create_view(ui, ctx),
                    View::Extract => self.extract_view(ui, ctx),
                    View::Inspect => self.inspect_view(ui, ctx),
                    View::Identity => self.identity_view(ui, ctx),
                });
            });
        });
        // Closing requests cancellation; staged outputs are cleaned by the engine while it unwinds.
        if ctx.input(|i| i.viewport().close_requested()) {
            if let Some(worker) = &self.worker {
                if !worker.cancellable { return; }
                worker.cancel.store(true, Ordering::Relaxed);
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_after_work = true;
                self.status = "Stopping archive work before closing…".into();
            }
        }
    }
}

fn section_heading(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).size(30.0).strong());
    ui.label(RichText::new(subtitle).color(MUTED));
    ui.add_space(16.0);
}

fn primary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).strong().color(Color32::from_rgb(13, 28, 49)))
        .fill(ACCENT).min_size(egui::vec2(155.0, 42.0))
}

fn named_destination(archive: &Path) -> PathBuf {
    let name = archive.file_name().unwrap_or_default().to_string_lossy();
    let lowercase = name.to_lowercase();
    let stem = [".tar.lz4.lttl", ".tar.zst.lttl", ".zip.lttl", ".lttl", ".tar.lz4", ".tar.zstd", ".tar.zst", ".tzst", ".zip"].iter()
        .find_map(|suffix| lowercase.ends_with(suffix).then(|| &name[..name.len() - suffix.len()]))
        .filter(|name| !name.is_empty()).unwrap_or("Extracted");
    archive.with_file_name(stem)
}

fn archive_parent(archive: &Path) -> &Path {
    archive.parent().filter(|path| !path.as_os_str().is_empty()).unwrap_or(Path::new("."))
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() { value /= 1024.0; unit += 1; }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}
