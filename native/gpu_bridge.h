#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef _WIN32
#define LTTL_GPU_API __declspec(dllexport)
#else
#define LTTL_GPU_API
#endif
#ifdef __cplusplus
extern "C" {
#endif

/* CUDA event timings cover device work/transfers, not host copies, allocation,
   frame concatenation, or disk I/O. Benchmark wall time separately. */
typedef struct LttlGpuMetrics {
    double host_to_device_ms;
    double compression_ms;
    double device_to_host_ms;
    uint64_t chunks;
    uint64_t h2d_bytes;
    uint64_t d2h_bytes;
} LttlGpuMetrics;

/* One context is used by one thread at a time. No concurrent calls/destroy.
   Context construction allocates reusable bounded device and pinned buffers.
   NULL indicates failure; err is always NUL-terminated when cap > 0. */
LTTL_GPU_API void* lttl_gpu_create(size_t chunk_bytes, size_t batch_bytes,
    char* err, size_t cap);
/* 0 succeeds; nonzero fails. On failure outbytes is zero and output is not
   usable. On success output contains concatenated ordinary Zstandard frames.
   Input/output buffers must not overlap; input bytes <= configured batch.
   A zero-byte input succeeds with zero output, not an empty Zstandard frame. */
LTTL_GPU_API int lttl_gpu_compress(void* context, const uint8_t* input,
    size_t bytes, uint8_t* output, size_t output_capacity, size_t* outbytes,
    LttlGpuMetrics* metrics, char* err, size_t cap);
LTTL_GPU_API void lttl_gpu_destroy(void* context);
/* Copies a UTF-8 device name; 0 succeeds, 1 means invalid/truncated output. */
LTTL_GPU_API int lttl_gpu_device(void* context, char* name, size_t cap);

#ifdef __cplusplus
}
#endif
