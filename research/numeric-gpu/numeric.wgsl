// Exact LTEX0001 delta16 tile encoding. No subgroup-width assumptions.
struct Params { bytes: u32, stride: u32, tiles: u32, chunks: u32 }
@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> fixed: array<u32>;
@group(0) @binding(2) var<storage, read_write> tilemeta: array<u32>;
@group(0) @binding(3) var<storage, read_write> groups: array<u32>;
@group(0) @binding(4) var<storage, read_write> chunks: array<u32>;
@group(0) @binding(5) var<storage, read_write> packed: array<u32>;
@group(0) @binding(6) var<uniform> p: Params;
var<workgroup> values: array<u32, 128>;
var<workgroup> masks: array<u32, 128>;
var<workgroup> scan: array<u32, 256>;
fn word(i: u32) -> u32 { return (input[i / 2u] >> ((i % 2u) * 16u)) & 65535u; }
fn byte_at(i: u32) -> u32 { return (input[i / 4u] >> ((i % 4u) * 8u)) & 255u; }
fn tile_count(t: u32) -> u32 { return min(32u, p.bytes / 2u - t * 32u); }
fn tile_byte(t: u32, base: u32, b: u32, k: u32) -> u32 {
    let n = tile_count(t);
    let encoded = 1u + (n * b + 7u) / 8u;
    if k == 0u { return b; }
    if k >= encoded {
        if k == encoded && t + 1u == p.tiles && p.bytes % 2u == 1u && p.bytes % 1048576u != 1u { return byte_at(p.bytes - 1u); }
        return 0u;
    }
    // b cannot be zero here: then the encoded tile consists only of its width byte.
    let bit = (k - 1u) * 8u;
    var at = bit / b;
    var shift = bit % b;
    var result = 0u;
    var written = 0u;
    loop {
        if written >= 8u || at >= n { break; }
        let take = min(b - shift, 8u - written);
        result |= ((values[base + at] >> shift) & ((1u << take) - 1u)) << written;
        written += take; at += 1u; shift = 0u;
    }
    return result;
}
@compute @workgroup_size(128)
fn encode_tiles(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    let t = (wg.x + wg.y * 256u) * 4u + lane / 32u;
    let sub = lane % 32u;
    let base = lane - sub;
    let i = t * 32u + sub;
    var v = 0u;
    if t < p.tiles && i < p.bytes / 2u {
        var previous = 0u;
        if i % 524288u >= p.stride { previous = word(i - p.stride); }
        let d = (word(i) - previous) & 65535u;
        v = ((d << 1u) ^ select(0u, 65535u, (d & 32768u) != 0u)) & 65535u;
    }
    values[lane] = v; masks[lane] = v;
    workgroupBarrier();
    for (var step = 16u; step > 0u; step /= 2u) {
        if sub < step { masks[lane] |= masks[lane + step]; }
        workgroupBarrier();
    }
    if t >= p.tiles { return; }
    let bits = 32u - countLeadingZeros(masks[base]);
    let bytes = 1u + (tile_count(t) * bits + 7u) / 8u + select(0u, p.bytes % 2u, t + 1u == p.tiles && p.bytes % 1048576u != 1u);
    if sub == 0u { tilemeta[t * 2u] = bytes; }
    if sub * 4u < bytes {
        var value = 0u;
        for (var k = 0u; k < 4u; k += 1u) { value |= tile_byte(t, base, bits, sub * 4u + k) << (k * 8u); }
        fixed[t * 17u + sub] = value;
    }
}
// Exclusive tile offsets within groups of 256; groups never cross 1 MiB chunk boundaries.
@compute @workgroup_size(256)
fn scan_tiles(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    let t = wg.x * 256u + lane;
    var v = 0u; if t < p.tiles { v = tilemeta[t * 2u]; }
    scan[lane] = v; workgroupBarrier();
    for (var step = 1u; step < 256u; step *= 2u) {
        var add = 0u; if lane >= step { add = scan[lane - step]; }
        workgroupBarrier(); scan[lane] += add; workgroupBarrier();
    }
    if t < p.tiles { tilemeta[t * 2u + 1u] = scan[lane] - v; }
    if lane == 255u { groups[wg.x] = scan[lane]; }
}
@compute @workgroup_size(64)
fn scan_groups(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    let g = wg.x * 64u + lane;
    var v = 0u; if g < (p.tiles + 255u) / 256u { v = groups[g]; }
    scan[lane] = v; workgroupBarrier();
    for (var step = 1u; step < 64u; step *= 2u) {
        var add = 0u; if lane >= step { add = scan[lane - step]; }
        workgroupBarrier(); scan[lane] += add; workgroupBarrier();
    }
    if g < (p.tiles + 255u) / 256u { groups[g] = scan[lane] - v; }
    if lane == 63u {
        let original = min(1048576u, p.bytes - wg.x * 1048576u);
        var size = scan[63u];
        // A one-byte terminal chunk has no tiles and is always stored.
        if original == 1u || size >= original { size = original | 2147483648u; }
        chunks[wg.x * 2u] = size;
    }
}
@compute @workgroup_size(1)
fn scan_chunks() {
    var offset = 0u;
    for (var c = 0u; c < p.chunks; c += 1u) {
        chunks[c * 2u + 1u] = offset;
        let n = chunks[c * 2u];
        if (n & 2147483648u) == 0u { offset += (n + 3u) & ~3u; }
    }
    chunks[p.chunks * 2u] = offset;
}
fn fixed_byte(t: u32, k: u32) -> u32 { return (fixed[t * 17u + k / 4u] >> ((k % 4u) * 8u)) & 255u; }
// Each aligned output word has one writer. Its final bytes may come from following tiles.
// This avoids racing subword stores or atomics at unaligned tile boundaries.
@compute @workgroup_size(128)
fn compact(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    let t = (wg.x + wg.y * 256u) * 4u + lane / 32u;
    if t >= p.tiles { return; }
    let chunk = t / 16384u;
    let total = chunks[chunk * 2u];
    if (total & 2147483648u) != 0u { return; }
    let start = groups[t / 256u] + tilemeta[t * 2u + 1u];
    let end = start + tilemeta[t * 2u];
    let at = ((start + 3u) & ~3u) + (lane % 32u) * 4u;
    if at >= end { return; }
    var current = t;
    var local = at - start;
    var value = 0u;
    for (var k = 0u; k < 4u; k += 1u) {
        if at + k >= total { break; }
        loop {
            let size = tilemeta[current * 2u];
            if local < size { break; }
            local -= size; current += 1u;
        }
        value |= fixed_byte(current, local) << (k * 8u); local += 1u;
    }
    packed[(chunks[chunk * 2u + 1u] + at) / 4u] = value;
}
