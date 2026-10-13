//! Portable, independent-block LZ4. GPU timing is optional; wall timing is always measured.
use anyhow::{Context, Result, ensure};
use rayon::prelude::*;
use serde::Serialize;
use std::{io::{self, Write}, sync::atomic::{AtomicBool, Ordering}, time::Instant};
use std::sync::{Arc, Mutex, OnceLock};
use xxhash_rust::xxh32::{xxh32, Xxh32};
use std::io::Read;

// Strict framing: lz4_flex 0.11's frame reader treats a missing block header
// as EOF. Parse independent frames here so a missing end marker cannot pass.
pub(crate) struct Decoder<R:Read> { reader:R, data:Vec<u8>, offset:usize, max:usize, flags:u8, hash:Xxh32, length:u64, expected:Option<u64>, ended:bool }
impl<R:Read> Decoder<R> {
    pub fn new(mut reader:R)->Result<Self> {
        let mut header=[0u8;6]; reader.read_exact(&mut header)?;
        ensure!(header[..4]==[4,34,77,24],"Invalid LZ4 magic");
        let flags=header[4]; let bd=header[5];
        ensure!(flags>>6==1 && flags&0x22==0x20 && flags&1==0,"Only independent LZ4 frames without dictionaries are supported");
        ensure!(bd&0x8f==0,"Invalid LZ4 block descriptor");
        let max=match (bd>>4)&7 {4=>65536,5=>262144,6=>1048576,7=>4194304,_=>anyhow::bail!("Invalid LZ4 block maximum")};
        let mut descriptor=vec![flags,bd]; let expected=if flags&8!=0 {let mut n=[0u8;8];reader.read_exact(&mut n)?;descriptor.extend_from_slice(&n);Some(u64::from_le_bytes(n))}else{None};
        let mut checksum=[0];reader.read_exact(&mut checksum)?;ensure!(checksum[0]==(xxh32(&descriptor,0)>>8) as u8,"LZ4 header checksum mismatch");
        Ok(Self{reader,data:Vec::new(),offset:0,max,flags,hash:Xxh32::new(0),length:0,expected,ended:false})
    }
    fn next(&mut self)->Result<()> {
        let mut header=[0u8;4]; self.reader.read_exact(&mut header)?; let size=u32::from_le_bytes(header);
        if size==0 {
            if self.flags&4!=0 {self.reader.read_exact(&mut header)?;ensure!(u32::from_le_bytes(header)==self.hash.digest(),"LZ4 content checksum mismatch");}
            if let Some(expected)=self.expected {ensure!(expected==self.length,"LZ4 content length mismatch");}
            let mut extra=[0];ensure!(self.reader.read(&mut extra)?==0,"Trailing or concatenated LZ4 frames are unsupported");
            self.data.clear();self.offset=0;self.ended=true;return Ok(());
        }
        let count=(size&0x7fffffff) as usize;ensure!(count>0 && count<=self.max,"Invalid LZ4 block size");
        let mut compressed=vec![0;count]; self.reader.read_exact(&mut compressed)?;
        if self.flags&16!=0 {self.reader.read_exact(&mut header)?;ensure!(u32::from_le_bytes(header)==xxh32(&compressed,0),"LZ4 block checksum mismatch");}
        self.data=if size&0x80000000!=0 {compressed} else {let mut output=vec![0;self.max];let n=lz4_flex::block::decompress_into(&compressed,&mut output)?;output.truncate(n);output};
        ensure!(!self.data.is_empty(),"Empty LZ4 data block");self.length+=self.data.len() as u64; self.hash.update(&self.data); self.offset=0; Ok(())
    }
}
impl<R:Read> Read for Decoder<R> {
    fn read(&mut self,out:&mut [u8])->io::Result<usize> {
        if out.is_empty() || self.ended{return Ok(0);} if self.offset==self.data.len(){self.next().map_err(io::Error::other)?;}
        let n=out.len().min(self.data.len()-self.offset);out[..n].copy_from_slice(&self.data[self.offset..self.offset+n]);self.offset+=n;Ok(n)
    }
}

#[derive(Debug, Clone)]
pub struct PortableOptions { pub adapter: Option<String>, pub chunk_kib: usize, pub batch_mib: usize }
impl Default for PortableOptions {
    fn default() -> Self { Self { adapter: None, chunk_kib: 16, batch_mib: 16 } }
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct PortableMetrics {
    pub device: String, pub api: String, pub chunk_bytes: usize, pub batch_bytes: usize,
    pub initialize_seconds: f64, pub host_wall_seconds: f64,
    pub session_reused: bool, pub session_acquire_seconds: f64,
    pub kernel_selection_seconds: f64, pub serial_batches: u64, pub cooperative_batches: u64,
    pub compaction_ms: Option<f64>, pub readback_wait_seconds: f64, pub max_in_flight: usize,
    pub metadata_readback_bytes: u64, pub payload_readback_bytes: u64,
    pub transfer_mode: String,
    pub host_to_device_ms: Option<f64>, pub compression_ms: Option<f64>, pub device_to_host_ms: Option<f64>,
    pub h2d_bytes: u64, pub d2h_bytes: u64, pub batches: u64, pub chunks: u64,
    pub auto_reason: Option<String>,
}
fn instance() -> wgpu::Instance {
    // Vulkan on Windows/Linux; first-run D3D12 initialization stalled locally.
    // Metal on macOS; hardware matching below excludes software adapters.
    let backends = if cfg!(windows) { wgpu::Backends::VULKAN } else if cfg!(target_os="macos") { wgpu::Backends::METAL } else { wgpu::Backends::VULKAN };
    wgpu::Instance::new(wgpu::InstanceDescriptor { backends, ..Default::default() })
}
pub fn adapters() -> Vec<serde_json::Value> {
    instance().enumerate_adapters(wgpu::Backends::all()).into_iter().map(|a| {
        let i=a.get_info(); serde_json::json!({"name":i.name,"vendor":i.vendor,"device":i.device,"api":format!("{:?}",i.backend),"type":format!("{:?}",i.device_type)})
    }).collect()
}
#[derive(Clone, PartialEq, Eq)]
struct CacheKey { selector: String, chunk: usize, batch: usize, transfer: String }
impl From<&PortableOptions> for CacheKey {
    fn from(options:&PortableOptions)->Self { Self {selector:options.adapter.as_deref().unwrap_or("").trim().to_lowercase(),chunk:options.chunk_kib,batch:options.batch_mib,transfer:std::env::var("LTTL_PORTABLE_TRANSFER").unwrap_or_else(|_|"compact-pipeline".into())} }
}
// Idle contexts only. A checked-out session has exclusive ownership; failures and
// cancellations drop it. No GPU work or device destruction occurs under the lock.
static SESSIONS: OnceLock<Mutex<Vec<(CacheKey,Session)>>> = OnceLock::new();
const CACHE_BYTES: usize = 256 * 1048576;
pub fn clear_sessions() {
    let old = SESSIONS.get().and_then(|cache|cache.lock().ok().map(|mut entries|std::mem::take(&mut *entries)));
    drop(old);
}
fn acquire(options:&PortableOptions)->Result<Session> {
    let started=Instant::now(); let key=CacheKey::from(options);
    let cached=SESSIONS.get_or_init(||Mutex::new(Vec::new())).lock().ok().and_then(|mut entries| {
        entries.iter().position(|(k,_)|k==&key).map(|index|entries.remove(index).1)
    });
    let cached=cached.filter(|session| !session.failed.load(Ordering::Relaxed));
    let mut session=if let Some(mut session)=cached {
        session.metrics=PortableMetrics {device:session.metrics.device.clone(),api:session.metrics.api.clone(),chunk_bytes:session.metrics.chunk_bytes,batch_bytes:session.metrics.batch_bytes,session_reused:true,transfer_mode:session.metrics.transfer_mode.clone(),..Default::default()};
        session
    } else { Session::new(options)? };
    session.metrics.session_acquire_seconds=started.elapsed().as_secs_f64();
    Ok(session)
}
fn release(key:CacheKey, session:Session) {
    if session.allocation_bytes>CACHE_BYTES || session.failed.load(Ordering::Relaxed) {return;}
    let mut retired=Vec::new();
    if let Ok(mut entries)=SESSIONS.get_or_init(||Mutex::new(Vec::new())).lock() {
        // Keep only one idle session per configuration and at most two overall.
        if let Some(index)=entries.iter().position(|(k,_)| k==&key) {retired.push(entries.remove(index));}
        while !entries.is_empty() && (entries.len()>=2 || entries.iter().map(|(_,s)|s.allocation_bytes).sum::<usize>()+session.allocation_bytes>CACHE_BYTES) {
            retired.push(entries.remove(0));
        }
        entries.push((key,session));
    }
    drop(retired);
}
// Arc's driver executes the original serial kernel faster on dense matches.
// Inspect at most four blocks (at most 256 KiB) to choose execution only; both
// kernels produce the same bytes. CPU/Auto and NVIDIA pay no sampling cost.
fn dense_matches(bytes:&[u8],chunk:usize)->bool {
    let blocks=bytes.len().div_ceil(chunk);
    let samples=blocks.min(4);
    let mut dense=0;
    for sample in 0..samples {
        let index=sample*blocks/samples;
        let data=&bytes[index*chunk..bytes.len().min((index+1)*chunk)];
        if data.len()<12 {continue;}
        let word=|at:usize|u32::from_le_bytes(data[at..at+4].try_into().unwrap());
        let hash=|at:usize|(word(at).wrapping_mul(2654435761)>>20) as usize;
        let mut dictionary=[usize::MAX;4096];
        for pos in 0..=data.len()-12 {let slot=hash(pos);if dictionary[slot]==usize::MAX {dictionary[slot]=pos;}}
        let start=data.len()/2;
        let end=(start+512).min(data.len()-11);
        if end<=start {continue;}
        let hits=(start..end).filter(|&pos| {let previous=dictionary[hash(pos)];previous<pos && word(previous)==word(pos)}).count();
        if hits*16>end-start {dense+=1;}
    }
    dense*2>=samples.max(1)
}
type MapResult = std::sync::mpsc::Receiver<Result<(),wgpu::BufferAsyncError>>;
fn map(buffer:&wgpu::Buffer, mode:wgpu::MapMode, end:u64)->MapResult {
    let (send,recv)=std::sync::mpsc::channel();
    buffer.slice(..end).map_async(mode,move |result| {let _=send.send(result);}); recv
}
struct Pending {
    blocks:usize, length:usize, sizes:Vec<usize>, payload:usize,
    metadata_ready:Option<MapResult>, payload_ready:Option<MapResult>, upload_ready:MapResult,
    submission:wgpu::SubmissionIndex,
}
struct Slot {
    bind:wgpu::BindGroup, compact_bind:wgpu::BindGroup,
    upload:wgpu::Buffer, input:wgpu::Buffer, output:wgpu::Buffer, sizes:wgpu::Buffer, params:wgpu::Buffer,
    packed:wgpu::Buffer, metadata_readback:wgpu::Buffer, readback:wgpu::Buffer,
    queries:Option<(wgpu::QuerySet,wgpu::Buffer)>, pending:Option<Pending>,
}
struct Session {
    device:wgpu::Device, queue:wgpu::Queue, pipeline:wgpu::ComputePipeline, serial_pipeline:Option<wgpu::ComputePipeline>,
    offsets_pipeline:wgpu::ComputePipeline, compact_pipeline:wgpu::ComputePipeline,
    slots:Vec<Slot>, compact:bool, depth:usize, stride:usize, metadata:usize,
    metrics:PortableMetrics, allocation_bytes:usize, failed:Arc<AtomicBool>,
}
impl Session {
    fn new(options: &PortableOptions) -> Result<Self> {
        let started=Instant::now();
        let trace=|phase| { if std::env::var_os("LTTL_GPU_TRACE").is_some() {eprintln!("portable init: {phase}");} };
        trace("enumerate");
        let selector=CacheKey::from(options).selector;
        let adapter=instance().enumerate_adapters(wgpu::Backends::all()).into_iter().find(|a| {
            let i=a.get_info(); matches!(i.device_type,wgpu::DeviceType::DiscreteGpu|wgpu::DeviceType::IntegratedGpu) &&
            (selector.is_empty() || i.name.to_lowercase().contains(&selector) || (selector=="intel" && i.vendor==0x8086) || (selector=="nvidia" && i.vendor==0x10de) || (selector=="amd" && i.vendor==0x1002))
        }).with_context(|| format!("No hardware GPU matches '{selector}'"))?;
        let info=adapter.get_info();
        let features=adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
        trace("request device");
        let chunk=options.chunk_kib*1024;
        let batch=options.batch_mib*1048576;
        let stride=(chunk+chunk/255+32).next_multiple_of(4);
        let blocks=batch.div_ceil(chunk);
        let mut limits=wgpu::Limits::default();
        limits.max_compute_workgroup_storage_size=17*1024;
        let (device,queue)=pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label:Some("lttl portable LZ4"), required_features:features, required_limits:limits,
        },None))?;
        let failed=Arc::new(AtomicBool::new(false));
        let on_lost=failed.clone();
        device.set_device_lost_callback(move |_,_| {on_lost.store(true,Ordering::Relaxed);});
        let on_error=failed.clone();
        device.on_uncaptured_error(Box::new(move |_| {on_error.store(true,Ordering::Relaxed);}));
        trace("shader");
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor { label:Some("LZ4 encoder"),source:wgpu::ShaderSource::Wgsl(include_str!("lz4.wgsl").into()) });
        trace("pipeline");
        let entries=(0..4).map(|binding|wgpu::BindGroupLayoutEntry{binding,visibility:wgpu::ShaderStages::COMPUTE,
            ty:wgpu::BindingType::Buffer{ty:if binding==3 {wgpu::BufferBindingType::Uniform} else {wgpu::BufferBindingType::Storage{read_only:binding==0}},has_dynamic_offset:false,min_binding_size:None},count:None}).collect::<Vec<_>>();
        let bind_layout=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor{label:None,entries:&entries});
        let layout=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor{label:None,bind_group_layouts:&[&bind_layout],push_constant_ranges:&[]});
        let pipeline=device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label:Some("LZ4 cooperative encode"),layout:Some(&layout),module:&shader,entry_point:"compress" });
        let serial_pipeline=if info.vendor==0x8086 {
            let serial_shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{label:Some("Intel dense LZ4"),source:wgpu::ShaderSource::Wgsl(include_str!("lz4-serial.wgsl").into())});
            let bind_layout=pipeline.get_bind_group_layout(0);
            let layout=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor{label:None,bind_group_layouts:&[&bind_layout],push_constant_ranges:&[]});
            Some(device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor{label:Some("Intel dense LZ4"),layout:Some(&layout),module:&serial_shader,entry_point:"compress"}))
        } else {None};
        if let Some(e)=pollster::block_on(device.pop_error_scope()) { anyhow::bail!("LZ4 shader: {e}"); }
        let mode=CacheKey::from(options).transfer;
        ensure!(["compact-pipeline","compact-serial","fixed-pipeline","fixed-serial"].contains(&mode.as_str()),"Invalid LTTL_PORTABLE_TRANSFER mode");
        let compact=mode.starts_with("compact");
        let depth=if mode.ends_with("pipeline") {2}else{1};
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{label:Some("LZ4 transfer compaction"),source:wgpu::ShaderSource::Wgsl(include_str!("lz4-compact.wgsl").into())});
        let offsets_pipeline=device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor{label:Some("LZ4 offsets"),layout:Some(&layout),module:&shader,entry_point:"offsets"});
        let compact_pipeline=device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor{label:Some("LZ4 compact"),layout:Some(&layout),module:&shader,entry_point:"compact"});
        if let Some(e)=pollster::block_on(device.pop_error_scope()) {anyhow::bail!("LZ4 compaction shader: {e}");}
        trace("buffers");
        device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let metadata=(blocks*4).next_multiple_of(256);
        let size_bytes=((blocks*2+1)*4).next_multiple_of(256);
        let capacity=blocks*stride;
        let buffer=|name,size,usage| device.create_buffer(&wgpu::BufferDescriptor{label:Some(name),size:size as u64,usage,mapped_at_creation:false});
        let mut slots=Vec::new();
        for _ in 0..depth {
            let upload=device.create_buffer(&wgpu::BufferDescriptor{label:Some("persistent upload"),size:batch as u64,usage:wgpu::BufferUsages::MAP_WRITE|wgpu::BufferUsages::COPY_SRC,mapped_at_creation:true});
            let input=buffer("input",batch,wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST);
            let output=buffer("output",capacity,wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_SRC);
            let sizes=buffer("sizes and offsets",size_bytes,wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_SRC);
            let params=buffer("params",16,wgpu::BufferUsages::UNIFORM|wgpu::BufferUsages::COPY_DST);
            let packed=buffer("packed output",if compact {batch}else{4},wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_SRC);
            let metadata_readback=buffer("size readback",metadata+32,wgpu::BufferUsages::MAP_READ|wgpu::BufferUsages::COPY_DST);
            let readback=buffer("payload readback",capacity+16,wgpu::BufferUsages::MAP_READ|wgpu::BufferUsages::COPY_DST);
            let queries=if features.contains(wgpu::Features::TIMESTAMP_QUERY) {Some((device.create_query_set(&wgpu::QuerySetDescriptor{label:None,ty:wgpu::QueryType::Timestamp,count:6}),buffer("timestamps",32,wgpu::BufferUsages::QUERY_RESOLVE|wgpu::BufferUsages::COPY_SRC)))}else{None};
            let bind_group=|input:&wgpu::Buffer,output:&wgpu::Buffer|device.create_bind_group(&wgpu::BindGroupDescriptor{label:None,layout:&bind_layout,entries:&[
                wgpu::BindGroupEntry{binding:0,resource:input.as_entire_binding()},wgpu::BindGroupEntry{binding:1,resource:output.as_entire_binding()},
                wgpu::BindGroupEntry{binding:2,resource:sizes.as_entire_binding()},wgpu::BindGroupEntry{binding:3,resource:params.as_entire_binding()},
            ]});
            let bind=bind_group(&input,&output); let compact_bind=bind_group(&output,&packed);
            slots.push(Slot{bind,compact_bind,upload,input,output,sizes,params,packed,metadata_readback,readback,queries,pending:None});
        }
        let validation=pollster::block_on(device.pop_error_scope()); let memory=pollster::block_on(device.pop_error_scope());
        if let Some(error)=validation.or(memory) {anyhow::bail!("Portable GPU allocation: {error}");}
        ensure!(!failed.load(Ordering::Relaxed),"Portable GPU initialization failed");
        Ok(Self{failed,device,queue,pipeline,serial_pipeline,offsets_pipeline,compact_pipeline,slots,compact,depth,stride,metadata,
            allocation_bytes:depth*(batch*2+capacity*2+if compact {batch}else{4}+size_bytes+metadata+96),
            metrics:PortableMetrics{device:info.name,api:format!("{:?}",info.backend),chunk_bytes:chunk,batch_bytes:batch,transfer_mode:mode,initialize_seconds:started.elapsed().as_secs_f64(),..Default::default()}})
    }
    fn submit(&mut self,bytes:&[u8])->Result<usize> {
        ensure!(!self.failed.load(Ordering::Relaxed),"Portable GPU session is unavailable");
        let started=Instant::now();let chunk=self.metrics.chunk_bytes;let blocks=bytes.len().div_ceil(chunk);
        let selection=Instant::now();let serial=self.serial_pipeline.is_some() && dense_matches(bytes,chunk);
        self.metrics.kernel_selection_seconds+=selection.elapsed().as_secs_f64();
        if serial {self.metrics.serial_batches+=1;}else{self.metrics.cooperative_batches+=1;}
        let index=self.slots.iter().position(|slot|slot.pending.is_none()).context("No free GPU batch slot")?;
        let slot=&mut self.slots[index]; let padded=bytes.len().next_multiple_of(4);
        {let mut mapped=slot.upload.slice(..).get_mapped_range_mut();mapped[..bytes.len()].copy_from_slice(bytes);mapped[bytes.len()..padded].fill(0);}
        slot.upload.unmap();
        let params=[bytes.len() as u32,chunk as u32,self.stride as u32,blocks as u32].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
        self.queue.write_buffer(&slot.params,0,&params);
        let mut encoder=self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor{label:Some("upload encode compact")});
        if let Some((q,_))=&slot.queries {encoder.write_timestamp(q,0);}
        encoder.copy_buffer_to_buffer(&slot.upload,0,&slot.input,0,padded as u64);
        if let Some((q,_))=&slot.queries {encoder.write_timestamp(q,1);}
        {let mut pass=encoder.begin_compute_pass(&wgpu::ComputePassDescriptor{label:Some("compress"),timestamp_writes:None});
            pass.set_pipeline(if serial {self.serial_pipeline.as_ref().unwrap()}else{&self.pipeline});pass.set_bind_group(0,&slot.bind,&[]);pass.dispatch_workgroups(blocks as u32,1,1);}
        if let Some((q,_))=&slot.queries {encoder.write_timestamp(q,2);}
        if self.compact {
            {let mut pass=encoder.begin_compute_pass(&wgpu::ComputePassDescriptor{label:Some("prefix offsets"),timestamp_writes:None});
                pass.set_pipeline(&self.offsets_pipeline);pass.set_bind_group(0,&slot.compact_bind,&[]);pass.dispatch_workgroups(1,1,1);}
            {let mut pass=encoder.begin_compute_pass(&wgpu::ComputePassDescriptor{label:Some("pack useful blocks"),timestamp_writes:None});
                pass.set_pipeline(&self.compact_pipeline);pass.set_bind_group(0,&slot.compact_bind,&[]);pass.dispatch_workgroups(blocks as u32,1,1);}
        }
        if let Some((q,_))=&slot.queries {encoder.write_timestamp(q,3);encoder.resolve_query_set(q,0..4,&slot.queries.as_ref().unwrap().1,0);
            encoder.copy_buffer_to_buffer(&slot.queries.as_ref().unwrap().1,0,&slot.metadata_readback,self.metadata as u64,32);}
        encoder.copy_buffer_to_buffer(&slot.sizes,0,&slot.metadata_readback,0,(blocks*4) as u64);
        // Fixed mode is retained for controlled compaction/pipeline ablations.
        let payload=if self.compact {0}else{blocks*self.stride};
        if !self.compact {Self::copy_payload(&mut encoder,slot,payload,false);}
        let submission=self.queue.submit(Some(encoder.finish()));
        let metadata_ready=Some(map(&slot.metadata_readback,wgpu::MapMode::Read,(self.metadata+32) as u64));
        let payload_ready=if self.compact {None}else{Some(map(&slot.readback,wgpu::MapMode::Read,(payload+16) as u64))};
        let upload_ready=map(&slot.upload,wgpu::MapMode::Write,self.metrics.batch_bytes as u64);
        slot.pending=Some(Pending{blocks,length:bytes.len(),sizes:Vec::new(),payload,metadata_ready,payload_ready,upload_ready,submission});
        self.metrics.max_in_flight=self.metrics.max_in_flight.max(self.slots.iter().filter(|s|s.pending.is_some()).count());
        self.metrics.host_wall_seconds+=started.elapsed().as_secs_f64();self.metrics.batches+=1;self.metrics.chunks+=blocks as u64;
        self.metrics.h2d_bytes+=padded as u64;
        Ok(index)
    }
    fn copy_payload(encoder:&mut wgpu::CommandEncoder,slot:&Slot,bytes:usize,compact:bool) {
        if let Some((q,_))=&slot.queries {encoder.write_timestamp(q,4);}
        if bytes>0 {encoder.copy_buffer_to_buffer(if compact {&slot.packed}else{&slot.output},0,&slot.readback,0,bytes as u64);}
        if let Some((q,resolve))=&slot.queries {
            encoder.write_timestamp(q,5);encoder.resolve_query_set(q,4..6,resolve,0);
            encoder.copy_buffer_to_buffer(resolve,0,&slot.readback,bytes as u64,16);
        }
    }
    // Advance size-ready batches without blocking. The exact transfer length is
    // data-dependent; wgpu has no GPU-indirect buffer-copy length API.
    fn progress(&mut self)->Result<()> {
        let started=Instant::now();self.device.poll(wgpu::Maintain::Poll);
        for index in 0..self.slots.len() {self.prepare_readback(index,false)?;}
        self.metrics.host_wall_seconds+=started.elapsed().as_secs_f64();Ok(())
    }
    fn prepare_readback(&mut self,index:usize,wait:bool)->Result<()> {
        let slot=&mut self.slots[index];
        let Some(pending)=slot.pending.as_mut() else{return Ok(())};
        let Some(receiver)=&pending.metadata_ready else{return Ok(())};
        if wait {self.device.poll(wgpu::Maintain::WaitForSubmissionIndex(pending.submission.clone()));receiver.recv()??;}
        else {match receiver.try_recv() {Ok(result)=>result?,Err(std::sync::mpsc::TryRecvError::Empty)=>return Ok(()),Err(error)=>return Err(error.into())}}
        pending.metadata_ready=None;
        ensure!(!self.failed.load(Ordering::Relaxed),"Portable GPU failed; session discarded");
        let mapped=slot.metadata_readback.slice(..(self.metadata+32) as u64).get_mapped_range();
        let mut payload=0;
        for block in 0..pending.blocks {
            let size=u32::from_le_bytes(mapped[block*4..block*4+4].try_into()?) as usize;
            ensure!(size>0 && size<=self.stride,"Invalid GPU LZ4 block size");
            pending.sizes.push(size);
            let raw=self.metrics.chunk_bytes.min(pending.length-block*self.metrics.chunk_bytes);
            if size<raw {payload+=size.next_multiple_of(4);}
        }
        if slot.queries.is_some() {
            let ts=mapped[self.metadata..self.metadata+32].chunks_exact(8).map(|v|u64::from_le_bytes(v.try_into().unwrap())).collect::<Vec<_>>();
            let scale=self.queue.get_timestamp_period() as f64/1e6;
            for (metric,index) in [(&mut self.metrics.host_to_device_ms,0),(&mut self.metrics.compression_ms,1),(&mut self.metrics.compaction_ms,2)] {
                *metric=Some(metric.unwrap_or(0.)+ts[index+1].wrapping_sub(ts[index]) as f64*scale);
            }
        }
        drop(mapped);slot.metadata_readback.unmap();
        self.metrics.metadata_readback_bytes+=(pending.blocks*4) as u64;
        if self.compact {pending.payload=payload;}
        self.metrics.payload_readback_bytes+=pending.payload as u64;
        self.metrics.d2h_bytes+=(pending.blocks*4+pending.payload) as u64;
        if self.compact {
            let mut encoder=self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor{label:Some("compact readback")});
            Self::copy_payload(&mut encoder,slot,payload,true);
            let submission=self.queue.submit(Some(encoder.finish()));
            let pending=slot.pending.as_mut().unwrap();pending.submission=submission;
            pending.payload_ready=Some(map(&slot.readback,wgpu::MapMode::Read,(payload+16) as u64));
        }
        Ok(())
    }
    fn complete<W:Write>(&mut self,index:usize,raw:&[u8],writer:&mut W)->Result<()> {
        let started=Instant::now();let wait=Instant::now();self.prepare_readback(index,true)?;
        let slot=&mut self.slots[index];let pending=slot.pending.as_ref().context("Missing GPU batch")?;
        self.device.poll(wgpu::Maintain::WaitForSubmissionIndex(pending.submission.clone()));
        pending.payload_ready.as_ref().context("Missing readback")?.recv()??;pending.upload_ready.recv()??;
        self.metrics.readback_wait_seconds+=wait.elapsed().as_secs_f64();
        ensure!(!self.failed.load(Ordering::Relaxed),"Portable GPU failed; session discarded");
        let mapped=slot.readback.slice(..(pending.payload+16) as u64).get_mapped_range();
        let mut offset=0;
        for (block,data) in raw.chunks(self.metrics.chunk_bytes).enumerate() {
            let size=pending.sizes[block];
            if size<data.len() {
                let start=if self.compact {offset}else{block*self.stride};
                ensure!(start+size<=pending.payload,"Invalid compact GPU output extent");
                writer.write_all(&(size as u32).to_le_bytes())?;writer.write_all(&mapped[start..start+size])?;
                offset+=size.next_multiple_of(4);
            } else {writer.write_all(&(data.len() as u32|0x80000000).to_le_bytes())?;writer.write_all(data)?;}
        }
        if slot.queries.is_some() {
            let ts=&mapped[pending.payload..pending.payload+16];
            let from=u64::from_le_bytes(ts[..8].try_into()?);let to=u64::from_le_bytes(ts[8..].try_into()?);
            self.metrics.device_to_host_ms=Some(self.metrics.device_to_host_ms.unwrap_or(0.)+to.wrapping_sub(from) as f64*self.queue.get_timestamp_period() as f64/1e6);
        }
        drop(mapped);slot.readback.unmap();slot.pending=None;
        self.metrics.host_wall_seconds+=started.elapsed().as_secs_f64();Ok(())
    }
}

pub(crate) struct Encoder<'a,W:Write> {
    writer:W, buffer:Vec<u8>, options:PortableOptions, gpu:Option<Session>, pool:Option<rayon::ThreadPool>,
    pending:std::collections::VecDeque<(usize,Vec<u8>)>, spare:Vec<u8>, poll_bytes:usize,
    hash:Xxh32, cancel:&'a AtomicBool, metrics:PortableMetrics,
}
impl<'a,W:Write> Encoder<'a,W> {
    pub fn new(mut writer:W, gpu:bool, options:&PortableOptions, threads:u32, auto:bool, cancel:&'a AtomicBool) -> Result<Self> {
        ensure!(matches!(options.chunk_kib,4|8|16|32|64),"LZ4 chunk must be 4, 8, 16, 32 or 64 KiB");
        ensure!((1..=64).contains(&options.batch_mib),"LZ4 batch must be 1–64 MiB");
        let started=Instant::now();
        let session=if gpu {Some(acquire(options)?)} else {None};
        let pool=if gpu || threads==1 {None} else {Some(rayon::ThreadPoolBuilder::new().num_threads(threads as usize).build()?)};
        // Version 1, independent blocks, content checksum; maximum 64 KiB blocks.
        let header=[0x64,0x40]; writer.write_all(&[4,34,77,24,header[0],header[1],(xxh32(&header,0)>>8) as u8])?;
        Ok(Self{writer,buffer:Vec::with_capacity(options.batch_mib*1048576),options:options.clone(),gpu:session,pool,pending:Default::default(),spare:Vec::new(),poll_bytes:0,hash:Xxh32::new(0),cancel,
            metrics:PortableMetrics{device:"CPU LZ4".into(),api:"CPU".into(),chunk_bytes:options.chunk_kib*1024,batch_bytes:options.batch_mib*1048576,initialize_seconds:started.elapsed().as_secs_f64(),auto_reason:if auto {Some("CPU LZ4: portable GPU has no validated end-to-end advantage for this policy".into())} else {None},..Default::default()}})
    }
    fn batch(&mut self) -> Result<()> {
        ensure!(!self.cancel.load(Ordering::Relaxed),"Operation cancelled"); if self.buffer.is_empty(){return Ok(());}
        self.hash.update(&self.buffer); let chunk=self.options.chunk_kib*1024; let started=Instant::now();
        if self.gpu.is_some() {
            self.gpu.as_mut().unwrap().progress()?;
            if self.pending.len()>=self.gpu.as_ref().unwrap().depth {self.drain_one()?;}
            let index=self.gpu.as_mut().unwrap().submit(&self.buffer)?;
            let raw=std::mem::replace(&mut self.buffer,std::mem::take(&mut self.spare));
            self.pending.push_back((index,raw));
            if self.gpu.as_ref().unwrap().depth==1 {self.drain_one()?;}
            return Ok(());
        }
        let compressed=if let Some(pool)=&self.pool {
            pool.install(||self.buffer.par_chunks(chunk).map(lz4_flex::block::compress).collect::<Vec<_>>())
        } else {self.buffer.chunks(chunk).map(lz4_flex::block::compress).collect()};
        for (raw,packed) in self.buffer.chunks(chunk).zip(compressed) {
            let (size,data)=if packed.len()<raw.len(){(packed.len() as u32,packed.as_slice())}else{(raw.len() as u32|0x80000000,raw)};
            self.writer.write_all(&size.to_le_bytes())?; self.writer.write_all(data)?;
        }
        self.metrics.host_wall_seconds+=started.elapsed().as_secs_f64(); self.metrics.batches+=1; self.metrics.chunks+=self.buffer.len().div_ceil(chunk) as u64;
        self.buffer.clear(); Ok(())
    }
    fn drain_one(&mut self)->Result<()> {
        ensure!(!self.cancel.load(Ordering::Relaxed),"Operation cancelled");
        if let Some((index,mut raw))=self.pending.pop_front() {
            self.gpu.as_mut().unwrap().complete(index,&raw,&mut self.writer)?;
            raw.clear();self.spare=raw;
        }
        Ok(())
    }
    pub fn finish(mut self)->Result<PortableMetrics> {
        self.batch()?;while !self.pending.is_empty() {self.drain_one()?;} self.writer.write_all(&0u32.to_le_bytes())?; self.writer.write_all(&self.hash.digest().to_le_bytes())?; self.writer.flush()?;
        if let Some(session)=self.gpu.take() {
            let metrics=session.metrics.clone(); release(CacheKey::from(&self.options),session); Ok(metrics)
        } else {Ok(self.metrics)}
    }
}
impl<W:Write> Write for Encoder<'_,W> {
    fn write(&mut self,mut bytes:&[u8])->io::Result<usize>{
        let count=bytes.len();
        self.poll_bytes+=count;
        if self.poll_bytes>=1048576 {
            if let Some(gpu)=&mut self.gpu {gpu.progress().map_err(io::Error::other)?;}
            self.poll_bytes=0;
        }
        while !bytes.is_empty(){let n=bytes.len().min(self.options.batch_mib*1048576-self.buffer.len()); self.buffer.extend_from_slice(&bytes[..n]);bytes=&bytes[n..];
            if self.buffer.len()==self.options.batch_mib*1048576 {self.batch().map_err(io::Error::other)?;} } Ok(count)
    }
    fn flush(&mut self)->io::Result<()> {self.writer.flush()}
}

#[cfg(test)]
mod transfer_tests {
    use super::*;
    fn check(adapter:&str)->Result<()> {
        clear_sessions();
        let cancel=AtomicBool::new(false);
        let mut state=937123u64;
        for chunk in [4,16,64] {
            // Every block is incompressible: zero payload is a real transfer case.
            let noise=(0..3*1048576+3).map(|_|{state^=state<<13;state^=state>>7;state^=state<<17;state as u8}).collect::<Vec<_>>();
            for (kind,bytes) in [noise,vec![19u8;3*1048576+1],(0..3*1048576+7).map(|i|(i%251) as u8).collect()].into_iter().enumerate() {
                let mut encoded=Vec::new();
                let mut encoder=Encoder::new(&mut encoded,true,&PortableOptions{adapter:Some(adapter.into()),chunk_kib:chunk,batch_mib:1},1,false,&cancel)?;
                // Awkward write boundaries and flush calls must not reorder blocks.
                for part in bytes.chunks(77777) {encoder.write_all(part)?;encoder.flush()?;}
                let metrics=encoder.finish()?;
                assert_eq!(metrics.batches,4);
                assert_eq!(metrics.max_in_flight,2);
                assert!(metrics.payload_readback_bytes<metrics.h2d_bytes);
                assert_eq!(metrics.d2h_bytes,metrics.payload_readback_bytes+metrics.metadata_readback_bytes);
                let mut decoded=Vec::new();
                lz4_flex::frame::FrameDecoder::new(encoded.as_slice()).read_to_end(&mut decoded)?;
                assert_eq!(bytes,decoded);
                if kind==0 {assert_eq!(metrics.payload_readback_bytes,0);assert_eq!(metrics.d2h_bytes,metrics.chunks*4);}
            }
        }
        // Abandoning or cancelling with queued work must not recache resources.
        clear_sessions();
        let options=PortableOptions{adapter:Some(adapter.into()),chunk_kib:16,batch_mib:1};
        {
            let mut encoder=Encoder::new(io::sink(),true,&options,1,false,&cancel)?;
            encoder.write_all(&vec![91;2*1048576])?;
            cancel.store(true,Ordering::Relaxed);
            assert!(encoder.finish().is_err());
        }
        cancel.store(false,Ordering::Relaxed);
        let encoder=Encoder::new(io::sink(),true,&options,1,false,&cancel)?;
        assert!(!encoder.finish()?.session_reused);
        clear_sessions();
        // A sink failure after header creation occurs while draining queued GPU
        // output, and must not return the partially consumed session to the cache.
        struct FailingWriter(usize);
        impl Write for FailingWriter {
            fn write(&mut self,bytes:&[u8])->io::Result<usize> {
                if self.0+bytes.len()>7 {return Err(io::Error::other("injected output failure"));}
                self.0+=bytes.len();Ok(bytes.len())
            }
            fn flush(&mut self)->io::Result<()> {Ok(())}
        }
        let mut encoder=Encoder::new(FailingWriter(0),true,&options,1,false,&cancel)?;
        encoder.write_all(&vec![19;2*1048576])?;
        assert!(encoder.finish().is_err());
        let encoder=Encoder::new(io::sink(),true,&options,1,false,&cancel)?;
        assert!(!encoder.finish()?.session_reused);
        clear_sessions();Ok(())
    }
    #[test] #[ignore="requires Intel GPU, run serially"]
    fn intel_compact_pipeline()->Result<()> {check("intel")}
    #[test] #[ignore="requires NVIDIA GPU, run serially"]
    fn nvidia_compact_pipeline()->Result<()> {check("nvidia")}
}
