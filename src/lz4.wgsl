struct Params { length: u32, chunk: u32, stride: u32, blocks: u32 }
@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
@group(0) @binding(2) var<storage, read_write> sizes: array<u32>;
@group(0) @binding(3) var<uniform> p: Params;
var<workgroup> dictionary: array<atomic<u32>, 4096>;
var<workgroup> candidates: array<u32,64>;
var<workgroup> scan_position: u32;
var<workgroup> has_candidate: atomic<u32>;
var<workgroup> literal_source: u32;
var<workgroup> literal_output: u32;
var<workgroup> literal_remaining: u32;
fn byte_at(i: u32) -> u32 { return (input[i / 4u] >> ((i % 4u) * 8u)) & 255u; }
fn word_at(i: u32) -> u32 {
    let shift = (i % 4u) * 8u;
    let low = input[i/4u];
    if shift == 0u { return low; }
    return (low >> shift) | (input[i/4u+1u] << (32u-shift));
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
// Align the output, then copy four literal bytes per store.
fn literals(base: u32, cursor: ptr<function, u32>, source: u32, count: u32) {
    var i=0u;
    while i<count && (*cursor % 4u)!=0u { emit(base,cursor,byte_at(source+i)); i+=1u; }
    while i+4u<=count { output[(base+*cursor)/4u]=word_at(source+i); *cursor+=4u; i+=4u; }
    while i<count { emit(base,cursor,byte_at(source+i)); i+=1u; }
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
    if lane==0u { scan_position=0u; }
    let base=block*p.stride;
    var cursor=0u;
    var anchor=0u;
    // All lanes inspect adjacent positions. Lane zero follows the same greedy
    // parse as v1, using shared candidates instead of serial hash/input loads.
    loop {
        if lane==0u { atomicStore(&has_candidate,0u); }
        let tile=workgroupUniformLoad(&scan_position);
        if tile+12u>count { break; }
        let at=tile+lane;
        var previous=0xffffffffu;
        if at+12u<=count {
            let candidate=atomicLoad(&dictionary[hash(start+at)]);
            if candidate<at && word_at(start+candidate)==word_at(start+at) { previous=candidate; }
        }
        candidates[lane]=previous;
        if previous!=0xffffffffu { atomicOr(&has_candidate,1u); }
        workgroupBarrier();
        if lane==0u {
            var pos=tile;
            if atomicLoad(&has_candidate)==0u { pos+=64u; }
            while pos<tile+64u && pos+12u<=count {
                let match_at=candidates[pos-tile];
                if match_at==0xffffffffu { pos+=1u; continue; }
                var length=4u;
                while pos+length+4u<=count-5u && word_at(start+match_at+length)==word_at(start+pos+length) { length+=4u; }
                while pos+length<count-5u && byte_at(start+match_at+length)==byte_at(start+pos+length) { length+=1u; }
                let literal_count=pos-anchor;
                emit(base,&cursor,(min(literal_count,15u)<<4u)|min(length-4u,15u));
                if literal_count>=15u { extension(base,&cursor,literal_count-15u); }
                literals(base,&cursor,start+anchor,literal_count);
                let distance=pos-match_at;
                emit(base,&cursor,distance&255u); emit(base,&cursor,distance>>8u);
                if length>=19u { extension(base,&cursor,length-19u); }
                pos+=length; anchor=pos;
            }
            scan_position=pos;
        }
    }
    if lane==0u {
        let remaining=count-anchor;
        emit(base,&cursor,min(remaining,15u)<<4u);
        if remaining>=15u { extension(base,&cursor,remaining-15u); }
        // Align once, then let all lanes copy the final literal run. This is
        // especially important for incompressible blocks (almost all literals).
        var consumed=0u;
        while consumed<remaining && cursor%4u!=0u { emit(base,&cursor,byte_at(start+anchor+consumed)); consumed+=1u; }
        literal_source=start+anchor+consumed;
        literal_output=base+cursor;
        literal_remaining=remaining-consumed;
        sizes[block]=cursor+literal_remaining;
    }
    workgroupBarrier();
    let whole=literal_remaining/4u*4u;
    for(var i=lane*4u; i<whole; i+=256u) { output[(literal_output+i)/4u]=word_at(literal_source+i); }
    if lane==0u {
        var tail_cursor=literal_output+whole-base;
        for(var i=whole; i<literal_remaining; i+=1u) { emit(base,&tail_cursor,byte_at(literal_source+i)); }
    }
}
