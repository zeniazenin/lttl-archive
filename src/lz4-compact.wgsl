// Keep the encoder's exact block bytes. Raw blocks stay in host memory;
// only useful compressed blocks are packed into aligned transfer slots.
struct Params { length: u32, chunk: u32, stride: u32, blocks: u32 }
@group(0) @binding(0) var<storage, read> source: array<u32>;
@group(0) @binding(1) var<storage, read_write> packed: array<u32>;
@group(0) @binding(2) var<storage, read_write> sizes: array<u32>;
@group(0) @binding(3) var<uniform> p: Params;

@compute @workgroup_size(1)
fn offsets() {
    var cursor=0u;
    for(var block=0u; block<p.blocks; block+=1u) {
        sizes[p.blocks+block]=cursor;
        let raw=min(p.chunk,p.length-block*p.chunk);
        let size=sizes[block];
        if size<raw { cursor+=(size+3u)/4u; }
    }
    sizes[2u*p.blocks]=cursor;
}

@compute @workgroup_size(64)
fn compact(@builtin(workgroup_id) group:vec3<u32>, @builtin(local_invocation_index) lane:u32) {
    let block=group.x;
    let size=sizes[block];
    if size>=min(p.chunk,p.length-block*p.chunk) { return; }
    let destination=sizes[p.blocks+block];
    let start=block*p.stride/4u;
    for(var word=lane; word<(size+3u)/4u; word+=64u) {
        packed[destination+word]=source[start+word];
    }
}
