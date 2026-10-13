#include "gpu_bridge.h"
#include <cuda_runtime_api.h>
#include <nvcomp/zstd.h>
#include <algorithm>
#include <cstring>
#include <limits>
#include <memory>
#include <stdexcept>
#include <string>

namespace {
void message(char* out, size_t cap, const char* text) noexcept {
    if (!out || !cap) return;
    const size_t count = (std::min)(cap - 1, std::strlen(text));
    std::memcpy(out, text, count);
    out[count] = '\0';
}
void require(bool condition, const char* text) {
    if (!condition) throw std::runtime_error(text);
}
void cuda_check(cudaError_t status, const char* operation) {
    if (status != cudaSuccess)
        throw std::runtime_error(std::string(operation) + ": " + cudaGetErrorString(status));
}
void nv_check(nvcompStatus_t status, const char* operation) {
    if (status != nvcompSuccess)
        throw std::runtime_error(std::string(operation) + ": " + nvcompGetStatusString(status));
}
size_t multiply(size_t a, size_t b) {
    require(!b || a <= (std::numeric_limits<size_t>::max)() / b, "GPU buffer size overflow");
    return a * b;
}
size_t add(size_t a, size_t b) {
    require(a <= (std::numeric_limits<size_t>::max)() - b, "GPU buffer size overflow");
    return a + b;
}
size_t align_up(size_t value, size_t alignment) {
    require(alignment != 0, "Invalid nvCOMP alignment");
    return multiply((add(value, alignment - 1) / alignment), alignment);
}
struct DeviceBuffer {
    void* data = nullptr;
    ~DeviceBuffer() { if (data) cudaFree(data); }
    void allocate(size_t bytes) { if (bytes) cuda_check(cudaMalloc(&data, bytes), "cudaMalloc"); }
    DeviceBuffer() = default;
    DeviceBuffer(const DeviceBuffer&) = delete;
    DeviceBuffer& operator=(const DeviceBuffer&) = delete;
};
template<typename T> struct PinnedBuffer {
    T* data = nullptr;
    ~PinnedBuffer() { if (data) cudaFreeHost(data); }
    void allocate(size_t count) {
        cuda_check(cudaHostAlloc(reinterpret_cast<void**>(&data), multiply(count, sizeof(T)), cudaHostAllocDefault), "cudaHostAlloc");
    }
    PinnedBuffer() = default;
    PinnedBuffer(const PinnedBuffer&) = delete;
    PinnedBuffer& operator=(const PinnedBuffer&) = delete;
};
struct Context {
    int device = 0;
    std::string name;
    size_t chunk = 0, batch = 0, max_chunks = 0, output_bound = 0, stride = 0, scratch_size = 0;
    cudaStream_t stream = nullptr;
    cudaEvent_t events[8] = {};
    bool failed = false;
    DeviceBuffer input, output, scratch, input_ptrs, output_ptrs, input_sizes, output_sizes, statuses;
    PinnedBuffer<uint8_t> host_input, host_output;
    PinnedBuffer<const void*> host_input_ptrs;
    PinnedBuffer<void*> host_output_ptrs;
    PinnedBuffer<size_t> host_input_sizes, host_output_sizes;
    PinnedBuffer<nvcompStatus_t> host_statuses;
    ~Context() {
        cudaSetDevice(device);
        if (stream) cudaStreamSynchronize(stream);
        for (auto event : events) if (event) cudaEventDestroy(event);
        if (stream) cudaStreamDestroy(stream);
        // Members free device and pinned buffers after the stream is stopped.
    }
    void record(size_t event) { cuda_check(cudaEventRecord(events[event], stream), "cudaEventRecord"); }
    double elapsed(size_t first, size_t last) {
        float ms = 0;
        cuda_check(cudaEventElapsedTime(&ms, events[first], events[last]), "cudaEventElapsedTime");
        return ms;
    }
    void copy(void* to, const void* from, size_t bytes, cudaMemcpyKind direction) {
        if (bytes) cuda_check(cudaMemcpyAsync(to, from, bytes, direction, stream), "cudaMemcpyAsync");
    }
};
static_assert(sizeof(LttlGpuMetrics) == 48, "Rust ABI expects 48-byte metrics");
}

extern "C" LTTL_GPU_API void* lttl_gpu_create(size_t chunk_bytes, size_t batch_bytes, char* err, size_t cap) {
    message(err, cap, "");
    try {
        // Stay within the conservative documented 16 MiB limit, even where the
        // shipped header's public max constant permits larger inputs.
        require(chunk_bytes >= 4096 && chunk_bytes <= 16 * 1024 * 1024, "GPU chunk size must be 4 KiB through 16 MiB");
        require(batch_bytes >= chunk_bytes && batch_bytes <= 512 * 1024 * 1024, "GPU batch size must be at least one chunk and at most 512 MiB");
        auto ctx = std::make_unique<Context>();
        int count = 0;
        cuda_check(cudaGetDeviceCount(&count), "cudaGetDeviceCount");
        require(count > 0, "No CUDA-capable GPU is available");
        bool found = false;
        for (int device = 0; device < count; ++device) {
            cudaDeviceProp properties = {};
            cuda_check(cudaGetDeviceProperties(&properties, device), "cudaGetDeviceProperties");
            if (properties.major >= 7) {
                ctx->device = device;
                ctx->name = properties.name;
                found = true;
                break;
            }
        }
        require(found, "nvCOMP requires a Volta-or-newer NVIDIA GPU");
        cuda_check(cudaSetDevice(ctx->device), "cudaSetDevice");
        ctx->chunk = chunk_bytes;
        ctx->batch = batch_bytes;
        ctx->max_chunks = (batch_bytes + chunk_bytes - 1) / chunk_bytes;
        const auto options = nvcompBatchedZstdCompressDefaultOpts;
        nvcompAlignmentRequirements_t alignment = {};
        nv_check(nvcompBatchedZstdCompressGetRequiredAlignments(options, &alignment), "nvCOMP alignment query");
        require(chunk_bytes % alignment.input == 0, "GPU chunk size does not satisfy nvCOMP input alignment");
        require(alignment.temp <= 256, "Unsupported nvCOMP scratch alignment");
        nv_check(nvcompBatchedZstdCompressGetMaxOutputChunkSize(chunk_bytes, options, &ctx->output_bound), "nvCOMP output-bound query");
        ctx->stride = align_up(ctx->output_bound, (std::max)(alignment.output, size_t(256)));
        nv_check(nvcompBatchedZstdCompressGetTempSizeAsync(ctx->max_chunks, chunk_bytes, options, &ctx->scratch_size, batch_bytes), "nvCOMP workspace query");
        const size_t output_bytes = multiply(ctx->stride, ctx->max_chunks);
        const size_t table_bytes = multiply(ctx->max_chunks, 2 * sizeof(void*) + 2 * sizeof(size_t) + sizeof(nvcompStatus_t));
        const size_t required = add(add(batch_bytes, output_bytes), add(ctx->scratch_size, table_bytes));
        size_t free_bytes = 0, total_bytes = 0;
        cuda_check(cudaMemGetInfo(&free_bytes, &total_bytes), "cudaMemGetInfo");
        require(required <= free_bytes - (std::min)(free_bytes, size_t(256 * 1024 * 1024)), "GPU batch/workspace exceeds available VRAM budget; use a smaller batch");
        cuda_check(cudaStreamCreateWithFlags(&ctx->stream, cudaStreamNonBlocking), "cudaStreamCreateWithFlags");
        for (auto& event : ctx->events) cuda_check(cudaEventCreate(&event), "cudaEventCreate");
        ctx->input.allocate(batch_bytes);
        ctx->output.allocate(output_bytes);
        ctx->scratch.allocate(ctx->scratch_size);
        ctx->input_ptrs.allocate(multiply(ctx->max_chunks, sizeof(void*)));
        ctx->output_ptrs.allocate(multiply(ctx->max_chunks, sizeof(void*)));
        ctx->input_sizes.allocate(multiply(ctx->max_chunks, sizeof(size_t)));
        ctx->output_sizes.allocate(multiply(ctx->max_chunks, sizeof(size_t)));
        ctx->statuses.allocate(multiply(ctx->max_chunks, sizeof(nvcompStatus_t)));
        ctx->host_input.allocate(batch_bytes);
        ctx->host_output.allocate(output_bytes);
        ctx->host_input_ptrs.allocate(ctx->max_chunks);
        ctx->host_output_ptrs.allocate(ctx->max_chunks);
        ctx->host_input_sizes.allocate(ctx->max_chunks);
        ctx->host_output_sizes.allocate(ctx->max_chunks);
        ctx->host_statuses.allocate(ctx->max_chunks);
        return ctx.release();
    } catch (const std::exception& error) { message(err, cap, error.what()); }
      catch (...) { message(err, cap, "Unknown native GPU initialization failure"); }
    return nullptr;
}

extern "C" LTTL_GPU_API int lttl_gpu_compress(void* context, const uint8_t* input, size_t bytes,
    uint8_t* output, size_t output_capacity, size_t* outbytes, LttlGpuMetrics* metrics, char* err, size_t cap) {
    message(err, cap, "");
    if (outbytes) *outbytes = 0;
    if (metrics) std::memset(metrics, 0, sizeof(*metrics));
    auto* ctx = static_cast<Context*>(context);
    try {
        require(ctx && outbytes && metrics, "Invalid GPU compression context or output parameters");
        require(!ctx->failed, "GPU context failed previously; recreate it before retrying");
        require(bytes <= ctx->batch, "Input exceeds configured GPU batch size");
        if (bytes == 0) return 0;
        require(input && output, "Input/output pointers must be non-null");
        cuda_check(cudaSetDevice(ctx->device), "cudaSetDevice");
        const size_t chunks = (bytes + ctx->chunk - 1) / ctx->chunk;
        std::memcpy(ctx->host_input.data, input, bytes);
        for (size_t i = 0; i < chunks; ++i) {
            ctx->host_input_ptrs.data[i] = static_cast<uint8_t*>(ctx->input.data) + i * ctx->chunk;
            ctx->host_output_ptrs.data[i] = static_cast<uint8_t*>(ctx->output.data) + i * ctx->stride;
            ctx->host_input_sizes.data[i] = (std::min)(ctx->chunk, bytes - i * ctx->chunk);
        }
        ctx->record(0);
        ctx->copy(ctx->input.data, ctx->host_input.data, bytes, cudaMemcpyHostToDevice);
        ctx->copy(ctx->input_ptrs.data, ctx->host_input_ptrs.data, chunks * sizeof(void*), cudaMemcpyHostToDevice);
        ctx->copy(ctx->output_ptrs.data, ctx->host_output_ptrs.data, chunks * sizeof(void*), cudaMemcpyHostToDevice);
        ctx->copy(ctx->input_sizes.data, ctx->host_input_sizes.data, chunks * sizeof(size_t), cudaMemcpyHostToDevice);
        ctx->record(1);
        ctx->record(2);
        cuda_check(cudaMemsetAsync(ctx->statuses.data, 0xff, chunks * sizeof(nvcompStatus_t), ctx->stream), "Clear nvCOMP statuses");
        cuda_check(cudaMemsetAsync(ctx->output_sizes.data, 0, chunks * sizeof(size_t), ctx->stream), "Clear nvCOMP sizes");
        nv_check(nvcompBatchedZstdCompressAsync(
            static_cast<const void* const*>(ctx->input_ptrs.data),
            static_cast<const size_t*>(ctx->input_sizes.data),
            (std::min)(bytes, ctx->chunk), chunks, ctx->scratch.data, ctx->scratch_size,
            static_cast<void* const*>(ctx->output_ptrs.data),
            static_cast<size_t*>(ctx->output_sizes.data), nvcompBatchedZstdCompressDefaultOpts,
            static_cast<nvcompStatus_t*>(ctx->statuses.data), ctx->stream), "nvCOMP Zstandard compression");
        ctx->record(3);
        ctx->record(4);
        ctx->copy(ctx->host_output_sizes.data, ctx->output_sizes.data, chunks * sizeof(size_t), cudaMemcpyDeviceToHost);
        ctx->copy(ctx->host_statuses.data, ctx->statuses.data, chunks * sizeof(nvcompStatus_t), cudaMemcpyDeviceToHost);
        ctx->record(5);
        cuda_check(cudaEventSynchronize(ctx->events[5]), "Synchronize nvCOMP result metadata");
        size_t packed_size = 0;
        for (size_t i = 0; i < chunks; ++i) {
            nv_check(ctx->host_statuses.data[i], "nvCOMP chunk compression status");
            require(ctx->host_output_sizes.data[i] >= 4 && ctx->host_output_sizes.data[i] <= ctx->output_bound, "nvCOMP returned invalid compressed chunk length");
            packed_size = add(packed_size, ctx->host_output_sizes.data[i]);
        }
        require(packed_size <= output_capacity, "Output buffer too small for compressed GPU frames");
        ctx->record(6);
        size_t offset = 0;
        // Copy only actual frame lengths, packing on the host rather than
        // transferring stride padding. Every destination remains pinned.
        for (size_t i = 0; i < chunks; ++i) {
            ctx->copy(ctx->host_output.data + offset, ctx->host_output_ptrs.data[i], ctx->host_output_sizes.data[i], cudaMemcpyDeviceToHost);
            offset += ctx->host_output_sizes.data[i];
        }
        ctx->record(7);
        cuda_check(cudaEventSynchronize(ctx->events[7]), "Synchronize compressed frame transfers");
        offset = 0;
        for (size_t i = 0; i < chunks; ++i) {
            const uint8_t* frame = ctx->host_output.data + offset;
            require(frame[0] == 0x28 && frame[1] == 0xb5 && frame[2] == 0x2f && frame[3] == 0xfd,
                "nvCOMP output is not an ordinary Zstandard frame; refusing private framing");
            offset += ctx->host_output_sizes.data[i];
        }
        std::memcpy(output, ctx->host_output.data, packed_size);
        metrics->host_to_device_ms = ctx->elapsed(0, 1);
        metrics->compression_ms = ctx->elapsed(2, 3);
        metrics->device_to_host_ms = ctx->elapsed(4, 5) + ctx->elapsed(6, 7);
        metrics->chunks = chunks;
        metrics->h2d_bytes = bytes + chunks * (2 * sizeof(void*) + sizeof(size_t));
        metrics->d2h_bytes = packed_size + chunks * (sizeof(size_t) + sizeof(nvcompStatus_t));
        *outbytes = packed_size;
        return 0;
    } catch (const std::exception& error) { message(err, cap, error.what()); }
      catch (...) { message(err, cap, "Unknown native GPU compression failure"); }
    if (ctx) { ctx->failed = true; if (ctx->stream) cudaStreamSynchronize(ctx->stream); }
    return 1;
}

extern "C" LTTL_GPU_API void lttl_gpu_destroy(void* context) {
    try { delete static_cast<Context*>(context); } catch (...) { /* Never unwind across FFI. */ }
}
extern "C" LTTL_GPU_API int lttl_gpu_device(void* context, char* name, size_t cap) {
    auto* ctx = static_cast<Context*>(context);
    if (!ctx || !name || !cap) return 1;
    message(name, cap, ctx->name.c_str());
    return ctx->name.size() < cap ? 0 : 1;
}
