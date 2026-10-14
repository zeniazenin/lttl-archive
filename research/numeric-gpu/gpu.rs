//! Persistent wgpu encoder. GPU payload equals the CPU LTEX0001 numeric payload byte for byte.
use super::cpu;
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use std::{sync::mpsc, time::Instant};

pub const BATCH: usize = 8 * cpu::CHUNK;
const TILES: usize = BATCH / 64;
const META: usize = (BATCH / cpu::CHUNK * 2 + 1) * 4;
#[derive(Default, Serialize)]
pub struct Metrics {
    pub upload_ms: Option<f64>,
    pub encode_tiles_ms: Option<f64>,
    pub scan_compact_ms: Option<f64>,
    pub readback_ms: Option<f64>,
    pub staging_ms: f64,
    pub metadata_wait_ms: f64,
    pub payload_wait_ms: f64,
    pub host_assembly_ms: f64,
    pub uploaded_bytes: usize,
    pub payload_readback_bytes: usize,
    pub metadata_readback_bytes: usize,
    pub timestamp_readback_bytes: usize,
    pub batches: usize,
}
pub struct Session {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: Vec<wgpu::ComputePipeline>,
    bind: wgpu::BindGroup,
    upload: wgpu::Buffer,
    input: wgpu::Buffer,
    params: wgpu::Buffer,
    chunks: wgpu::Buffer,
    packed: wgpu::Buffer,
    meta_read: wgpu::Buffer,
    payload_read: wgpu::Buffer,
    queries: Option<(wgpu::QuerySet, wgpu::Buffer)>,
    pub info: serde_json::Value,
}
fn map(buffer: &wgpu::Buffer, mode: wgpu::MapMode) -> mpsc::Receiver<Result<(), wgpu::BufferAsyncError>> {
    let (tx, rx) = mpsc::channel();
    buffer.slice(..).map_async(mode, move |result| { let _ = tx.send(result); }); rx
}
impl Session {
    pub fn new(selector: &str) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: if cfg!(target_os = "macos") { wgpu::Backends::METAL } else { wgpu::Backends::VULKAN },
            ..Default::default()
        });
        let adapter = instance.enumerate_adapters(wgpu::Backends::all()).into_iter().find(|adapter| {
            let info = adapter.get_info();
            matches!(info.device_type, wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::DiscreteGpu) &&
                (info.name.to_lowercase().contains(&selector.to_lowercase()) ||
                 (selector == "intel" && info.vendor == 0x8086) ||
                 (selector == "nvidia" && info.vendor == 0x10de))
        }).with_context(|| format!("No hardware adapter for {selector}"))?;
        let info = adapter.get_info();
        let features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("lttl numeric research"), required_features: features,
            required_limits: wgpu::Limits::default(),
        }, None))?;
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let entries: Vec<_> = (0..7).map(|binding| wgpu::BindGroupLayoutEntry {
            binding, visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: if binding == 6 { wgpu::BufferBindingType::Uniform }
                    else { wgpu::BufferBindingType::Storage { read_only: binding == 0 } },
                has_dynamic_offset: false, min_binding_size: None,
            }, count: None,
        }).collect();
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &entries });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&bind_layout], push_constant_ranges: &[] });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("numeric"), source: wgpu::ShaderSource::Wgsl(include_str!("numeric.wgsl").into()) });
        let pipelines = ["encode_tiles", "scan_tiles", "scan_groups", "scan_chunks", "compact"].iter().map(|entry_point| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(entry_point), layout: Some(&layout), module: &module, entry_point })
        }).collect();
        if let Some(error) = pollster::block_on(device.pop_error_scope()) { anyhow::bail!("Numeric shader: {error}"); }
        let buffer = |label, size, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size as u64, usage, mapped_at_creation: false });
        let upload = device.create_buffer(&wgpu::BufferDescriptor { label: Some("numeric upload"), size: BATCH as u64,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::MAP_WRITE, mapped_at_creation: true });
        let input = buffer("numeric input", BATCH, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        let fixed = buffer("fixed tiles", TILES * 68, wgpu::BufferUsages::STORAGE);
        let tilemeta = buffer("tile offsets", TILES * 8, wgpu::BufferUsages::STORAGE);
        let groups = buffer("group offsets", TILES / 256 * 4, wgpu::BufferUsages::STORAGE);
        let chunks = buffer("chunk offsets", META, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
        let packed = buffer("compact numeric", BATCH, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
        let params = buffer("numeric params", 16, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let meta_read = buffer("numeric metadata readback", META + 32, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
        let payload_read = buffer("numeric payload readback", BATCH + 16, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
        let queries = if features.contains(wgpu::Features::TIMESTAMP_QUERY) {
            Some((device.create_query_set(&wgpu::QuerySetDescriptor { label: None, ty: wgpu::QueryType::Timestamp, count: 6 }),
                  buffer("numeric timestamps", 32, wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC)))
        } else { None };
        let buffers = [&input, &fixed, &tilemeta, &groups, &chunks, &packed, &params];
        let entries: Vec<_> = buffers.iter().enumerate().map(|(i, b)| wgpu::BindGroupEntry { binding: i as u32, resource: b.as_entire_binding() }).collect();
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &bind_layout, entries: &entries });
        Ok(Self { device, queue, pipelines, bind, upload, input, params, chunks, packed, meta_read, payload_read, queries,
            info: serde_json::json!({"name": info.name, "vendor": info.vendor, "device": info.device,
                "driver": info.driver, "driver_info": info.driver_info, "backend": format!("{:?}", info.backend),
                "timestamp_queries": features.contains(wgpu::Features::TIMESTAMP_QUERY), "batch_bytes": BATCH,
                "allocated_buffer_bytes": BATCH * 4 + TILES * 76 + TILES / 256 * 4 + META * 2 + 96}) })
    }
    pub fn encode(&mut self, data: &[u8], stride: usize) -> Result<(Vec<u8>, Metrics)> {
        ensure!(data.len() <= 512 * cpu::CHUNK && cpu::STRIDES.contains(&stride), "Research limits");
        let mut metrics = Metrics::default();
        let mut body = Vec::new();
        for part in data.chunks(BATCH) {
            if part.len() < 2 {
                body.extend_from_slice(&((part.len() as u32) | 0x80000000).to_le_bytes()); body.extend_from_slice(part); continue;
            }
            self.batch(part, stride, &mut body, &mut metrics)?;
        }
        let started = Instant::now();
        let frame = cpu::frame(data, stride, body);
        metrics.host_assembly_ms += started.elapsed().as_secs_f64() * 1000.;
        Ok((frame, metrics))
    }
    fn batch(&mut self, data: &[u8], stride: usize, body: &mut Vec<u8>, metrics: &mut Metrics) -> Result<()> {
        let staging = Instant::now();
        let padded = data.len().next_multiple_of(4);
        { let mut mapped = self.upload.slice(..).get_mapped_range_mut();
          mapped[..data.len()].copy_from_slice(data); mapped[data.len()..padded].fill(0); }
        self.upload.unmap();
        let tiles = (data.len() / 2).div_ceil(32);
        let count = data.len().div_ceil(cpu::CHUNK);
        let params: Vec<_> = [data.len() as u32, stride as u32, tiles as u32, count as u32].into_iter().flat_map(u32::to_le_bytes).collect();
        self.queue.write_buffer(&self.params, 0, &params);
        metrics.staging_ms += staging.elapsed().as_secs_f64() * 1000.;
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("numeric batch") });
        if let Some((q, _)) = &self.queries { encoder.write_timestamp(q, 0); }
        encoder.copy_buffer_to_buffer(&self.upload, 0, &self.input, 0, padded as u64);
        if let Some((q, _)) = &self.queries { encoder.write_timestamp(q, 1); }
        let workgroups = tiles.div_ceil(4);
        let dispatches = [(workgroups.min(256), workgroups.div_ceil(256)), (tiles.div_ceil(256), 1), (count, 1), (1, 1), (workgroups.min(256), workgroups.div_ceil(256))];
        for (i, (x, y)) in dispatches.into_iter().enumerate() {
            { let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("numeric pass"), timestamp_writes: None });
              pass.set_pipeline(&self.pipelines[i]); pass.set_bind_group(0, &self.bind, &[]); pass.dispatch_workgroups(x as u32, y as u32, 1); }
            if i == 0 { if let Some((q, _)) = &self.queries { encoder.write_timestamp(q, 2); } }
        }
        if let Some((q, resolve)) = &self.queries {
            encoder.write_timestamp(q, 3); encoder.resolve_query_set(q, 0..4, resolve, 0);
            encoder.copy_buffer_to_buffer(resolve, 0, &self.meta_read, META as u64, 32);
        }
        encoder.copy_buffer_to_buffer(&self.chunks, 0, &self.meta_read, 0, ((count * 2 + 1) * 4) as u64);
        self.queue.submit(Some(encoder.finish()));
        let metadata = map(&self.meta_read, wgpu::MapMode::Read);
        let upload = map(&self.upload, wgpu::MapMode::Write);
        let wait = Instant::now(); self.device.poll(wgpu::Maintain::Wait); metadata.recv()??; upload.recv()??;
        metrics.metadata_wait_ms += wait.elapsed().as_secs_f64() * 1000.;
        let mapped = self.meta_read.slice(..).get_mapped_range();
        let read = |at| u32::from_le_bytes(mapped[at..at + 4].try_into().unwrap()) as usize;
        let meta: Vec<_> = (0..count).map(|i| (read(i * 8), read(i * 8 + 4))).collect();
        let payload = read(count * 8);
        ensure!(payload <= BATCH && payload % 4 == 0, "Invalid numeric GPU extent");
        if self.queries.is_some() {
            let ts: Vec<_> = mapped[META..META + 32].chunks_exact(8).map(|v| u64::from_le_bytes(v.try_into().unwrap())).collect();
            let scale = self.queue.get_timestamp_period() as f64 / 1e6;
            for (metric, i) in [(&mut metrics.upload_ms, 0), (&mut metrics.encode_tiles_ms, 1), (&mut metrics.scan_compact_ms, 2)] {
                *metric = Some(metric.unwrap_or(0.) + ts[i + 1].wrapping_sub(ts[i]) as f64 * scale);
            }
        }
        drop(mapped); self.meta_read.unmap();
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("numeric compact readback") });
        if let Some((q, _)) = &self.queries { encoder.write_timestamp(q, 4); }
        if payload > 0 { encoder.copy_buffer_to_buffer(&self.packed, 0, &self.payload_read, 0, payload as u64); }
        if let Some((q, resolve)) = &self.queries {
            encoder.write_timestamp(q, 5); encoder.resolve_query_set(q, 4..6, resolve, 0);
            encoder.copy_buffer_to_buffer(resolve, 0, &self.payload_read, payload as u64, 16);
        }
        self.queue.submit(Some(encoder.finish()));
        let ready = map(&self.payload_read, wgpu::MapMode::Read);
        let wait = Instant::now(); self.device.poll(wgpu::Maintain::Wait); ready.recv()??;
        metrics.payload_wait_ms += wait.elapsed().as_secs_f64() * 1000.;
        let assembly = Instant::now();
        let mapped = self.payload_read.slice(..).get_mapped_range();
        let mut expected_offset = 0;
        for ((encoded, offset), raw) in meta.into_iter().zip(data.chunks(cpu::CHUNK)) {
            let stored = encoded & 0x80000000 != 0;
            let n = encoded & 0x7fffffff;
            ensure!(n > 0 && n <= raw.len() && (!stored || n == raw.len()) && offset == expected_offset, "Invalid GPU chunk descriptor");
            body.extend_from_slice(&(encoded as u32).to_le_bytes());
            if stored { body.extend_from_slice(raw); }
            else {
                ensure!(offset + n <= payload, "GPU chunk outside payload"); body.extend_from_slice(&mapped[offset..offset + n]);
                expected_offset += n.next_multiple_of(4);
            }
        }
        ensure!(expected_offset == payload, "GPU payload size mismatch");
        if self.queries.is_some() {
            let a = u64::from_le_bytes(mapped[payload..payload + 8].try_into()?);
            let b = u64::from_le_bytes(mapped[payload + 8..payload + 16].try_into()?);
            metrics.readback_ms = Some(metrics.readback_ms.unwrap_or(0.) + b.wrapping_sub(a) as f64 * self.queue.get_timestamp_period() as f64 / 1e6);
        }
        drop(mapped); self.payload_read.unmap();
        metrics.host_assembly_ms += assembly.elapsed().as_secs_f64() * 1000.;
        metrics.batches += 1; metrics.uploaded_bytes += padded;
        metrics.payload_readback_bytes += payload; metrics.metadata_readback_bytes += (count * 2 + 1) * 4;
        if self.queries.is_some() { metrics.timestamp_readback_bytes += 48; }
        Ok(())
    }
}
