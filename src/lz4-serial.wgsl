struct Params { length: u32, chunk: u32, stride: u32, blocks: u32 }
@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
@group(0) @binding(2) var<storage, read_write> sizes: array<u32>;
@group(0) @binding(3) var<uniform> p: Params;
var<workgroup> dictionary: array<atomic<u32>, 4096>;
fn byte_at(i: u32) -> u32 { return (input[i / 4u] >> ((i % 4u) * 8u)) & 255u; }
fn word_at(i: u32) -> u32 {
    return byte_at(i) | (byte_at(i+1u)<<8u) | (byte_at(i+2u)<<16u) | (byte_at(i+3u)<<24u);
}
fn hash(i: u32) -> u32 { return (word_at(i) * 2654435761u) >> 20u; }
fn emit(base: u32, cursor: ptr<function, u32>, value: u32) {
    let pos = base + *cursor;
    let shift = (pos % 4u) * 8u;
    if shift == 0u { output[pos/4u] = value; }
    else { output[pos/4u] = output[pos/4u] | (value << shift); }
    *cursor += 1u;
}
fn extension(base: u32, cursor: ptr<function, u32>, length: u32) {
    var left = length;
    while left >= 255u { emit(base,cursor,255u); left -= 255u; }
    emit(base,cursor,left);
}
@compute @workgroup_size(64)
fn compress(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    let block = group.x;
    let start = block * p.chunk;
    let count = min(p.chunk, p.length-start);
    for(var i=lane; i<4096u; i+=64u) { atomicStore(&dictionary[i], 0xffffffffu); }
    workgroupBarrier();
    for(var i=lane; i+12u<=count; i+=64u) { atomicMin(&dictionary[hash(start+i)], i); }
    workgroupBarrier();
    if lane != 0u { return; }
    let base = block*p.stride;
    var cursor=0u;
    var anchor=0u;
    var pos=0u;
    while pos+12u<=count {
        let previous=atomicLoad(&dictionary[hash(start+pos)]);
        if previous>=pos || word_at(start+previous)!=word_at(start+pos) { pos+=1u; continue; }
        var length=4u;
        while pos+length<count-5u && byte_at(start+previous+length)==byte_at(start+pos+length) { length+=1u; }
        let literals=pos-anchor;
        emit(base,&cursor,(min(literals,15u)<<4u)|min(length-4u,15u));
        if literals>=15u { extension(base,&cursor,literals-15u); }
        for(var i=anchor; i<pos; i+=1u) { emit(base,&cursor,byte_at(start+i)); }
        let distance=pos-previous;
        emit(base,&cursor,distance&255u); emit(base,&cursor,distance>>8u);
        if length>=19u { extension(base,&cursor,length-19u); }
        pos+=length; anchor=pos;
    }
    let remaining=count-anchor;
    emit(base,&cursor,min(remaining,15u)<<4u);
    if remaining>=15u { extension(base,&cursor,remaining-15u); }
    for(var i=anchor; i<count; i+=1u) { emit(base,&cursor,byte_at(start+i)); }
    sizes[block]=cursor;
}
