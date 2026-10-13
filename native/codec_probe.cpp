// Isolated in-memory codec experiment, not an archive writer or app backend.
// Sources: https://docs.nvidia.com/cuda/nvcomp/algorithm_selection.html
// https://docs.nvidia.com/cuda/nvcomp/gdeflate.html
// https://github.com/lz4/lz4/blob/dev/doc/lz4_Block_format.md
#include <cuda_runtime_api.h>
#include <nvcomp/zstd.h>
#include <nvcomp/lz4.h>
#include <nvcomp/gdeflate.h>
#include <algorithm>
#include <chrono>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <memory>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

using Clock = std::chrono::steady_clock;
static double since(Clock::time_point start) { return std::chrono::duration<double, std::milli>(Clock::now() - start).count(); }
static void need(bool value, const char* message) { if (!value) throw std::runtime_error(message); }
static void cu(cudaError_t error) { if (error != cudaSuccess) throw std::runtime_error(cudaGetErrorString(error)); }
static void nv(nvcompStatus_t error) { if (error != nvcompSuccess) throw std::runtime_error(nvcompGetStatusString(error)); }
static std::string json(const std::string& value) {
    std::ostringstream out; out << '"';
    for (unsigned char c : value) {
        if (c == '"' || c == '\\') out << '\\' << c;
        else if (c < 32) out << "\\u" << std::hex << std::setw(4) << std::setfill('0') << unsigned(c) << std::dec;
        else out << c;
    }
    out << '"'; return out.str();
}
struct Device {
    void* p = nullptr;
    ~Device() { if (p) cudaFree(p); }
    void alloc(size_t size) { if (size) cu(cudaMalloc(&p, size)); }
    Device() = default; Device(const Device&) = delete;
};
template<class T> struct Pinned {
    T* p = nullptr;
    ~Pinned() { if (p) cudaFreeHost(p); }
    void alloc(size_t count) { cu(cudaHostAlloc(reinterpret_cast<void**>(&p), count * sizeof(T), cudaHostAllocDefault)); }
    Pinned() = default; Pinned(const Pinned&) = delete;
};

struct Codec {
    virtual ~Codec() = default;
    virtual void requirements(size_t chunks, size_t chunk, size_t batch, size_t& bound, size_t& scratch,
        nvcompAlignmentRequirements_t& ca, nvcompAlignmentRequirements_t& da) = 0;
    virtual void compress(const void* const* input, const size_t* sizes, size_t max_chunk, size_t count,
        void* temp, size_t temp_size, void* const* output, size_t* out_sizes, nvcompStatus_t* statuses, cudaStream_t stream) = 0;
    virtual void decompress(const void* const* input, const size_t* sizes, const size_t* capacities, size_t* actual,
        size_t count, void* temp, size_t temp_size, void* const* output, nvcompStatus_t* statuses, cudaStream_t stream) = 0;
};
#define CODEC_IMPL(CLASS, PREFIX) \
struct CLASS final : Codec { \
    nvcompBatched##PREFIX##CompressOpts_t co = nvcompBatched##PREFIX##CompressDefaultOpts; \
    nvcompBatched##PREFIX##DecompressOpts_t de = nvcompBatched##PREFIX##DecompressDefaultOpts; \
    void requirements(size_t n, size_t chunk, size_t batch, size_t& bound, size_t& scratch, nvcompAlignmentRequirements_t& ca, nvcompAlignmentRequirements_t& da) override { \
        nv(nvcompBatched##PREFIX##CompressGetRequiredAlignments(co, &ca)); \
        nv(nvcompBatched##PREFIX##DecompressGetRequiredAlignments(de, &da)); \
        nv(nvcompBatched##PREFIX##CompressGetMaxOutputChunkSize(chunk, co, &bound)); \
        size_t cs = 0, ds = 0; \
        nv(nvcompBatched##PREFIX##CompressGetTempSizeAsync(n, chunk, co, &cs, batch)); \
        nv(nvcompBatched##PREFIX##DecompressGetTempSizeAsync(n, chunk, de, &ds, batch)); \
        scratch = (std::max)(cs, ds); \
    } \
    void compress(const void* const* in, const size_t* sizes, size_t chunk, size_t n, void* temp, size_t ts, void* const* out, size_t* os, nvcompStatus_t* statuses, cudaStream_t s) override { \
        nv(nvcompBatched##PREFIX##CompressAsync(in, sizes, chunk, n, temp, ts, out, os, co, statuses, s)); \
    } \
    void decompress(const void* const* in, const size_t* sizes, const size_t* capacities, size_t* actual, size_t n, void* temp, size_t ts, void* const* out, nvcompStatus_t* statuses, cudaStream_t s) override { \
        nv(nvcompBatched##PREFIX##DecompressAsync(in, sizes, capacities, actual, n, temp, ts, out, de, statuses, s)); \
    } \
};
CODEC_IMPL(ZstdCodec, Zstd)
CODEC_IMPL(Lz4Codec, LZ4)
CODEC_IMPL(GdeflateCodec, Gdeflate)
#undef CODEC_IMPL

// Independent bounded scalar LZ4 block decoder. No NVIDIA code/API is used.
// Intended only to verify benchmark outputs; not a production extraction API.
static void cpu_lz4(const uint8_t* input, size_t size, uint8_t* output, size_t capacity) {
    size_t ip = 0, op = 0;
    auto length = [&](size_t initial) {
        size_t result = initial;
        if (initial == 15) {
            unsigned extra;
            do { need(ip < size, "LZ4 length truncated"); extra = input[ip++];
                need(result <= capacity && extra <= capacity - result, "LZ4 length exceeds output"); result += extra;
            } while (extra == 255);
        }
        return result;
    };
    while (ip < size) {
        const uint8_t token = input[ip++];
        const size_t literals = length(token >> 4);
        need(literals <= size - ip && literals <= capacity - op, "LZ4 literal bounds");
        std::memcpy(output + op, input + ip, literals); ip += literals; op += literals;
        if (ip == size) break;
        need(size - ip >= 2, "LZ4 match offset truncated");
        const size_t offset = input[ip] | (size_t(input[ip + 1]) << 8); ip += 2;
        need(offset > 0 && offset <= op, "LZ4 invalid match offset");
        const size_t match = length(token & 15) + 4;
        need(match <= capacity - op, "LZ4 match exceeds output");
        for (size_t i = 0; i < match; ++i) { output[op] = output[op - offset]; ++op; }
    }
    need(ip == size && op == capacity, "LZ4 final length mismatch");
}

struct Measurement {
    double wall_ms = 0, h2d_ms = 0, kernel_ms = 0, d2h_ms = 0;
    size_t compressed = 0, chunks = 0, h2d_bytes = 0, d2h_bytes = 0;
    void append(const Measurement& other) {
        wall_ms += other.wall_ms; h2d_ms += other.h2d_ms; kernel_ms += other.kernel_ms; d2h_ms += other.d2h_ms;
        compressed += other.compressed; chunks += other.chunks; h2d_bytes += other.h2d_bytes; d2h_bytes += other.d2h_bytes;
    }
};
struct Probe {
    std::unique_ptr<Codec> codec;
    size_t chunk, batch, max_chunks, bound = 0, stride = 0, scratch_size = 0;
    cudaStream_t stream = nullptr;
    cudaEvent_t events[8] = {};
    Device input, output, decoded, temp, in_ptrs, out_ptrs, decoded_ptrs, in_sizes, out_sizes, actual_sizes, status;
    Pinned<uint8_t> host_input, host_output, host_decoded;
    Pinned<void*> hip, hop, hdp;
    Pinned<size_t> his, hos, has;
    Pinned<nvcompStatus_t> hs;
    std::vector<uint8_t> packed;
    std::vector<uint8_t> cpu_decoded;
    Probe(std::unique_ptr<Codec> selected, size_t chunk_bytes, size_t batch_bytes)
        : codec(std::move(selected)), chunk(chunk_bytes), batch(batch_bytes), max_chunks((batch + chunk - 1) / chunk) {
        nvcompAlignmentRequirements_t ca = {}, da = {};
        codec->requirements(max_chunks, chunk, batch, bound, scratch_size, ca, da);
        const size_t alignment = (std::max)({size_t(256), ca.output, da.input});
        need(ca.input && da.output && alignment <= 256 && ca.temp <= 256 && da.temp <= 256, "Unsupported SDK alignment");
        need(chunk % ca.input == 0 && chunk % da.output == 0, "Chunk alignment invalid");
        stride = ((bound + alignment - 1) / alignment) * alignment;
        const size_t output_size = stride * max_chunks;
        size_t free_bytes, total_bytes; cu(cudaMemGetInfo(&free_bytes, &total_bytes));
        need(2 * batch + output_size + scratch_size + max_chunks * 64 + 256 * 1024 * 1024 < free_bytes,
            "Codec workspace exceeds VRAM budget");
        // Allocate first, then create stream/events so partially failed buffer
        // allocations still clean up through their owning member destructors.
        input.alloc(batch); output.alloc(output_size); decoded.alloc(batch); temp.alloc(scratch_size);
        in_ptrs.alloc(max_chunks * sizeof(void*)); out_ptrs.alloc(max_chunks * sizeof(void*)); decoded_ptrs.alloc(max_chunks * sizeof(void*));
        in_sizes.alloc(max_chunks * sizeof(size_t)); out_sizes.alloc(max_chunks * sizeof(size_t)); actual_sizes.alloc(max_chunks * sizeof(size_t));
        status.alloc(max_chunks * sizeof(nvcompStatus_t));
        host_input.alloc(batch); host_output.alloc(output_size); host_decoded.alloc(batch);
        hip.alloc(max_chunks); hop.alloc(max_chunks); hdp.alloc(max_chunks); his.alloc(max_chunks); hos.alloc(max_chunks); has.alloc(max_chunks); hs.alloc(max_chunks);
        packed.resize(output_size); cpu_decoded.resize(chunk);
        cu(cudaStreamCreateWithFlags(&stream, cudaStreamNonBlocking));
        try { for (auto& event : events) cu(cudaEventCreate(&event)); }
        catch (...) { for (auto event : events) if (event) cudaEventDestroy(event); cudaStreamDestroy(stream); stream = nullptr; throw; }
    }
    ~Probe() {
        if (stream) cudaStreamSynchronize(stream);
        for (auto event : events) if (event) cudaEventDestroy(event);
        if (stream) cudaStreamDestroy(stream);
    }
    void copy(void* to, const void* from, size_t bytes, cudaMemcpyKind kind) { if (bytes) cu(cudaMemcpyAsync(to, from, bytes, kind, stream)); }
    void record(size_t i) { cu(cudaEventRecord(events[i], stream)); }
    double elapsed(size_t a, size_t b) { float ms; cu(cudaEventElapsedTime(&ms, events[a], events[b])); return ms; }
    Measurement compress(const uint8_t* bytes, size_t size) {
        need(size > 0 && size <= batch, "Invalid batch");
        auto start = Clock::now();
        const size_t count = (size + chunk - 1) / chunk;
        std::memcpy(host_input.p, bytes, size);
        for (size_t i = 0; i < count; ++i) {
            hip.p[i] = static_cast<uint8_t*>(input.p) + i * chunk;
            hop.p[i] = static_cast<uint8_t*>(output.p) + i * stride;
            hdp.p[i] = static_cast<uint8_t*>(decoded.p) + i * chunk;
            his.p[i] = (std::min)(chunk, size - i * chunk);
        }
        record(0);
        copy(input.p, host_input.p, size, cudaMemcpyHostToDevice);
        copy(in_ptrs.p, hip.p, count * sizeof(void*), cudaMemcpyHostToDevice);
        copy(out_ptrs.p, hop.p, count * sizeof(void*), cudaMemcpyHostToDevice);
        copy(in_sizes.p, his.p, count * sizeof(size_t), cudaMemcpyHostToDevice);
        record(1); record(2);
        cu(cudaMemsetAsync(status.p, 0xff, count * sizeof(nvcompStatus_t), stream));
        cu(cudaMemsetAsync(out_sizes.p, 0, count * sizeof(size_t), stream));
        codec->compress(static_cast<const void* const*>(in_ptrs.p), static_cast<const size_t*>(in_sizes.p),
            (std::min)(chunk, size), count, temp.p, scratch_size, static_cast<void* const*>(out_ptrs.p),
            static_cast<size_t*>(out_sizes.p), static_cast<nvcompStatus_t*>(status.p), stream);
        record(3); record(4);
        copy(hos.p, out_sizes.p, count * sizeof(size_t), cudaMemcpyDeviceToHost);
        copy(hs.p, status.p, count * sizeof(nvcompStatus_t), cudaMemcpyDeviceToHost);
        record(5); cu(cudaEventSynchronize(events[5]));
        size_t total = 0;
        for (size_t i = 0; i < count; ++i) { nv(hs.p[i]); need(hos.p[i] > 0 && hos.p[i] <= bound, "Invalid compressed length"); total += hos.p[i]; }
        record(6);
        size_t offset = 0;
        for (size_t i = 0; i < count; ++i) { copy(host_output.p + offset, hop.p[i], hos.p[i], cudaMemcpyDeviceToHost); offset += hos.p[i]; }
        record(7); cu(cudaEventSynchronize(events[7]));
        std::memcpy(packed.data(), host_output.p, total);
        Measurement result;
        result.wall_ms = since(start); result.h2d_ms = elapsed(0, 1); result.kernel_ms = elapsed(2, 3); result.d2h_ms = elapsed(4, 5) + elapsed(6, 7);
        result.compressed = total; result.chunks = count;
        result.h2d_bytes = size + count * (2 * sizeof(void*) + sizeof(size_t));
        result.d2h_bytes = total + count * (sizeof(size_t) + sizeof(nvcompStatus_t));
        return result;
    }
    void verify(const uint8_t* original, size_t size, bool lz4) {
        const size_t count = (size + chunk - 1) / chunk;
        // GPU self-roundtrip uses resident compressed chunks. It is NOT reported
        // as transfer-inclusive decompression throughput and is outside timers.
        copy(decoded_ptrs.p, hdp.p, count * sizeof(void*), cudaMemcpyHostToDevice);
        cu(cudaMemsetAsync(actual_sizes.p, 0, count * sizeof(size_t), stream));
        cu(cudaMemsetAsync(status.p, 0xff, count * sizeof(nvcompStatus_t), stream));
        codec->decompress(static_cast<const void* const*>(out_ptrs.p), static_cast<const size_t*>(out_sizes.p),
            static_cast<const size_t*>(in_sizes.p), static_cast<size_t*>(actual_sizes.p), count, temp.p, scratch_size,
            static_cast<void* const*>(decoded_ptrs.p), static_cast<nvcompStatus_t*>(status.p), stream);
        copy(host_decoded.p, decoded.p, size, cudaMemcpyDeviceToHost);
        copy(has.p, actual_sizes.p, count * sizeof(size_t), cudaMemcpyDeviceToHost);
        copy(hs.p, status.p, count * sizeof(nvcompStatus_t), cudaMemcpyDeviceToHost);
        cu(cudaStreamSynchronize(stream));
        for (size_t i = 0; i < count; ++i) { nv(hs.p[i]); need(has.p[i] == his.p[i], "GPU decoded length mismatch"); }
        need(std::memcmp(original, host_decoded.p, size) == 0, "GPU roundtrip bytes differ");
        if (lz4) {
            size_t compressed_offset = 0;
            for (size_t i = 0; i < count; ++i) {
                cpu_lz4(packed.data() + compressed_offset, hos.p[i], cpu_decoded.data(), his.p[i]);
                need(std::memcmp(original + i * chunk, cpu_decoded.data(), his.p[i]) == 0, "Independent CPU LZ4 bytes differ");
                compressed_offset += hos.p[i];
            }
        }
    }
};

struct Input { std::string name; std::vector<uint8_t> bytes; double read_ms; };
struct Config { std::string name; size_t chunk; int mode; };
static std::unique_ptr<Codec> codec_for(const Config& config) {
    if (config.name == "zstd") return std::make_unique<ZstdCodec>();
    if (config.name == "lz4") return std::make_unique<Lz4Codec>();
    auto codec = std::make_unique<GdeflateCodec>(); codec->co.algorithm = config.mode; return codec;
}
static void emit_measurement(const Measurement& m, int repeat) {
    std::cout << "{\"repeat\":" << repeat << ",\"compression_wall_ms\":" << m.wall_ms
        << ",\"h2d_ms\":" << m.h2d_ms << ",\"compression_stage_ms\":" << m.kernel_ms << ",\"d2h_ms\":" << m.d2h_ms
        << ",\"compressed_payload_bytes\":" << m.compressed << ",\"chunks\":" << m.chunks
        << ",\"h2d_bytes\":" << m.h2d_bytes << ",\"d2h_bytes\":" << m.d2h_bytes << '}';
}

int wmain(int argc, wchar_t** argv) {
    try {
        size_t batch = 32 * 1024 * 1024; int repeats = 3;
        std::vector<Input> inputs;
        for (int i = 1; i < argc; ++i) {
            const std::wstring arg = argv[i];
            if (arg == L"--batch-mib" || arg == L"--repeats") {
                need(i + 1 < argc, "Option requires a number");
                const int value = std::stoi(argv[++i]);
                if (arg == L"--batch-mib") { need(value >= 16 && value <= 128, "Batch must be 16-128 MiB"); batch = size_t(value) * 1024 * 1024; }
                else { need(value >= 1 && value <= 10, "Repeats must be 1-10"); repeats = value; }
                continue;
            }
            const std::filesystem::path path(arg);
            const auto size = std::filesystem::file_size(path);
            need(size > 0 && size <= 512 * 1024 * 1024, "Each benchmark input must be 1 byte-512 MiB");
            auto start = Clock::now();
            Input input{path.u8string(), std::vector<uint8_t>(size), 0};
            std::ifstream file(path, std::ios::binary);
            file.read(reinterpret_cast<char*>(input.bytes.data()), static_cast<std::streamsize>(size));
            need(file.good(), "Input read failed"); input.read_ms = since(start); inputs.push_back(std::move(input));
        }
        need(!inputs.empty(), "Usage: codec_probe.exe [--batch-mib 32] [--repeats 3] FILE...");
        const auto cuda_start = Clock::now();
        cu(cudaSetDevice(0)); cu(cudaFree(nullptr));
        cudaDeviceProp properties = {}; cu(cudaGetDeviceProperties(&properties, 0));
        int driver = 0, runtime = 0; cu(cudaDriverGetVersion(&driver)); cu(cudaRuntimeGetVersion(&runtime));
        const double cuda_init_ms = since(cuda_start);
        const std::vector<Config> configs = {
            {"zstd", 65536, -1}, {"zstd", 262144, -1}, {"zstd", 1048576, -1},
            {"lz4", 65536, -1}, {"gdeflate", 65536, 0}, {"gdeflate", 65536, 1}, {"gdeflate", 65536, 2}
        };
        int failures = 0;
        std::cout << std::fixed << std::setprecision(6);
        for (const auto& config : configs) {
            for (const auto& input : inputs) {
                try {
                    const auto init_start = Clock::now();
                    Probe probe(codec_for(config), config.chunk, batch);
                    const double context_ms = since(init_start);
                    // Full-corpus warmup verifies every chunk before timing.
                    Measurement warm;
                    Measurement first_call;
                    const auto verify_start = Clock::now();
                    for (size_t offset = 0; offset < input.bytes.size(); offset += batch) {
                        const size_t bytes = (std::min)(batch, input.bytes.size() - offset);
                        const auto measurement = probe.compress(input.bytes.data() + offset, bytes);
                        if (offset == 0) first_call = measurement;
                        warm.append(measurement);
                        probe.verify(input.bytes.data() + offset, bytes, config.name == "lz4");
                    }
                    const double warmup_verify_ms = since(verify_start);
                    std::vector<Measurement> runs;
                    for (int repeat = 0; repeat < repeats; ++repeat) {
                        Measurement run;
                        for (size_t offset = 0; offset < input.bytes.size(); offset += batch) {
                            const size_t bytes = (std::min)(batch, input.bytes.size() - offset);
                            run.append(probe.compress(input.bytes.data() + offset, bytes));
                            // Verification is outside each measured batch interval,
                            // but checks every repetition, not merely its size.
                            probe.verify(input.bytes.data() + offset, bytes, config.name == "lz4");
                        }
                        need(run.compressed == warm.compressed, "Compressed size changed between identical runs");
                        runs.push_back(run);
                    }
                    std::vector<double> times; for (const auto& run : runs) times.push_back(run.wall_ms);
                    std::sort(times.begin(), times.end()); const double median = times[times.size() / 2];
                    std::cout << "{\"status\":\"ok\",\"scope\":\"memory_to_memory_no_archive_container\",\"file\":" << json(input.name)
                        << ",\"codec\":" << json(config.name) << ",\"mode\":" << config.mode << ",\"chunk_bytes\":" << config.chunk
                        << ",\"batch_bytes\":" << batch << ",\"input_bytes\":" << input.bytes.size() << ",\"compressed_payload_bytes\":" << warm.compressed
                        << ",\"retained_fraction\":" << double(warm.compressed) / input.bytes.size()
                        << ",\"device\":" << json(properties.name) << ",\"driver_api_version\":" << driver << ",\"cuda_runtime_version\":" << runtime
                        << ",\"nvcomp_package\":\"5.3.0.16_cuda12\",\"input_read_ms_excluded\":" << input.read_ms
                        << ",\"cuda_process_initialize_ms\":" << cuda_init_ms << ",\"context_allocate_ms\":" << context_ms
                        << ",\"first_compression_wall_ms\":" << warm.wall_ms
                        << ",\"first_compression_call_wall_ms\":" << first_call.wall_ms << ",\"first_compression_event_ms\":" << first_call.kernel_ms
                        << ",\"warmup_and_verify_ms\":" << warmup_verify_ms << ",\"gpu_exact_roundtrip\":true,\"verification_scope\":\"warmup_and_every_repeat_outside_batch_timers\",\"independent_cpu_lz4_roundtrip\":" << (config.name == "lz4" ? "true" : "null")
                        << ",\"median_compression_wall_ms\":" << median << ",\"median_mib_per_second\":" << (input.bytes.size() / 1048576.0) / (median / 1000.0)
                        << ",\"runs\":[";
                    for (size_t i = 0; i < runs.size(); ++i) { if (i) std::cout << ','; emit_measurement(runs[i], int(i + 1)); }
                    std::cout << "]}" << std::endl;
                } catch (const std::exception& error) {
                    ++failures;
                    std::cout << "{\"status\":\"error\",\"file\":" << json(input.name) << ",\"codec\":" << json(config.name)
                        << ",\"mode\":" << config.mode << ",\"chunk_bytes\":" << config.chunk << ",\"error\":" << json(error.what()) << "}" << std::endl;
                }
            }
        }
        return failures ? 1 : 0;
    } catch (const std::exception& error) { std::cerr << "Codec probe: " << error.what() << std::endl; return 2; }
}
