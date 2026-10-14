//! Experimental numeric codec; not a production archive format.
//! Independently implemented delta/zigzag/tile bit packing, with standard Zstd fallback.
use anyhow::{bail, ensure, Result};
use rayon::prelude::*;
use std::io::{Read, Write};
use xxhash_rust::xxh64::xxh64;

const MAGIC: &[u8;8] = b"LTEX0001";
const CHUNK: usize = 1024*1024;
pub const MAX: usize = 512*1024*1024;
const STRIDES: [usize;7] = [1,2,4,8,16,32,64];

fn word(data:&[u8], i:usize)->u16 { u16::from_le_bytes([data[i*2],data[i*2+1]]) }
fn residual(data:&[u8],i:usize,stride:usize)->u16 {
    let previous=if i>=stride {word(data,i-stride)}else{0};
    let difference=word(data,i).wrapping_sub(previous) as i16;
    ((difference as u16)<<1)^((difference>>15) as u16)
}
fn tile(data:&[u8],start:usize,stride:usize)->([u16;32],usize,u8) {
    let n=(data.len()/2-start).min(32);
    let mut values=[0u16;32];let mut maximum=0u16;
    for (j,value) in values.iter_mut().take(n).enumerate(){*value=residual(data,start+j,stride);maximum|=*value;}
    (values,n,(16-maximum.leading_zeros()) as u8)
}
fn pack(data:&[u8],stride:usize)->Vec<u8> {
    let mut out=Vec::with_capacity(data.len());
    for start in (0..data.len()/2).step_by(32) {
        let (values,n,bits)=tile(data,start,stride);out.push(bits);
        let mut accumulator=0u64;let mut used=0usize;
        for value in values.iter().take(n) {
            accumulator|=u64::from(*value)<<used;used+=bits as usize;
            while used>=8 {out.push(accumulator as u8);accumulator>>=8;used-=8;}
        }
        if used!=0 {out.push(accumulator as u8);}
    }
    if data.len()%2!=0 {out.push(*data.last().unwrap());}
    out
}
struct Reader<'a>{data:&'a [u8],pos:usize}
impl<'a> Reader<'a>{
    fn take(&mut self,n:usize)->Result<&'a [u8]>{ensure!(n<=self.data.len()-self.pos,"Truncated stream");let v=&self.data[self.pos..self.pos+n];self.pos+=n;Ok(v)}
    fn byte(&mut self)->Result<u8>{Ok(self.take(1)?[0])}
    fn u32(&mut self)->Result<u32>{Ok(u32::from_le_bytes(self.take(4)?.try_into()?))}
    fn u64(&mut self)->Result<u64>{Ok(u64::from_le_bytes(self.take(8)?.try_into()?))}
    fn end(&self)->Result<()>{ensure!(self.pos==self.data.len(),"Trailing data");Ok(())}
}
fn unpack(data:&[u8],size:usize,stride:usize)->Result<Vec<u8>> {
    ensure!(size<=CHUNK && STRIDES.contains(&stride),"Invalid numeric block");
    let mut reader=Reader{data,pos:0};let mut out=vec![0u8;size];
    for start in (0..size/2).step_by(32) {
        let n=(size/2-start).min(32);let bits=reader.byte()? as usize;
        ensure!(bits<=16,"Invalid packed width");
        let bytes=reader.take((n*bits).div_ceil(8))?;
        let mut index=0;let mut used=0;let mut accumulator=0u64;
        let mask=(1u64<<bits)-1;
        for j in 0..n {
            while used<bits {accumulator|=u64::from(bytes[index])<<used;used+=8;index+=1;}
            let folded=(accumulator&mask) as u16;accumulator>>=bits;used-=bits;
            let difference=(folded>>1)^0u16.wrapping_sub(folded&1);
            let i=start+j;let previous=if i>=stride {word(&out,i-stride)}else{0};
            out[i*2..i*2+2].copy_from_slice(&previous.wrapping_add(difference).to_le_bytes());
        }
        ensure!(accumulator==0,"Noncanonical padding bits");
    }
    if size%2!=0 {out[size-1]=reader.byte()?;}
    reader.end()?;Ok(out)
}
fn zstd(data:&[u8],level:i32,threads:usize)->Result<Vec<u8>> {
    let mut encoder=zstd::stream::write::Encoder::new(Vec::new(),level)?;
    encoder.include_checksum(true)?;
    if threads>1 {encoder.multithread(threads as u32)?;}
    encoder.write_all(data)?;Ok(encoder.finish()?)
}
fn choose_stride(data:&[u8])->Result<Option<usize>> {
    if data.len()<16384{return Ok(None);}
    let width=4096;
    let offsets=[0,(data.len()/2)&!1,(data.len()-width)&!1];
    let mut scores=[0usize;7];let mut baseline=0;
    for offset in offsets {
        let sample=&data[offset..offset+width];baseline+=zstd(sample,1,1)?.len();
        for (index,stride) in STRIDES.iter().enumerate(){
            for start in (0..sample.len()/2).step_by(32){let (_,n,bits)=tile(sample,start,*stride);scores[index]+=1+(n*bits as usize).div_ceil(8);}
        }
    }
    let (index,size)=scores.iter().enumerate().min_by_key(|(_,size)|**size).unwrap();
    // A conservative sample margin, not a proof of globally optimal selection.
    Ok((*size*100 < baseline*85 && *size*100 < 3*width*90).then_some(STRIDES[index]))
}
pub fn encode(data:&[u8],method:&str,threads:usize)->Result<(Vec<u8>,String)> {
    ensure!(data.len()<=MAX && (1..=64).contains(&threads),"Research size/worker limit");
    let stride=match method {
        "adaptive"=>choose_stride(data)?,
        "numeric"=>Some(choose_stride(data)?.unwrap_or(1)),
        "zstd-1"|"zstd-3"|"zstd-9"=>None,
        _=>bail!("Unknown method"),
    };
    let mut mode=1;let mut body=Vec::new();let mut label=method.to_string();
    if let Some(stride)=stride {
        mode=2;label=format!("delta16-pack-stride-{stride}");
        let chunks:Vec<_>=data.par_chunks(CHUNK).map(|chunk|{
            let packed=pack(chunk,stride);
            if packed.len()<chunk.len(){(packed,false)}else{(chunk.to_vec(),true)}
        }).collect();
        for (chunk,raw) in chunks {body.extend_from_slice(&((chunk.len() as u32)|if raw {1<<31}else{0}).to_le_bytes());body.extend_from_slice(&chunk);}
    } else {
        let level=match method {"zstd-1"=>1,"zstd-9"=>9,_=>3};
        body=zstd(data,level,threads)?;
        if method=="adaptive"{label="zstd-3-fallback".into();}
    }
    if body.len()>=data.len(){mode=0;body=data.to_vec();label="stored".into();}
    let mut out=MAGIC.to_vec();out.push(mode);out.push(if mode==2 {stride.unwrap() as u8}else{0});
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());out.extend_from_slice(&xxh64(data,0).to_le_bytes());out.extend(body);
    Ok((out,label))
}
pub fn decode(data:&[u8])->Result<Vec<u8>> {
    ensure!(data.len()<=MAX+26,"Research stream size limit");
    let mut reader=Reader{data,pos:0};ensure!(reader.take(8)?==MAGIC,"Not an experimental stream");
    let mode=reader.byte()?;let stride=reader.byte()? as usize;let size=reader.u64()?;
    ensure!(size<=MAX as u64,"Decoded size limit");let size=size as usize;let hash=reader.u64()?;
    ensure!((mode==2 && STRIDES.contains(&stride)) || (mode<2 && stride==0),"Invalid mode/stride");
    let out=match mode {
        0=>reader.take(size)?.to_vec(),
        1=>{
            let payload=reader.take(data.len()-reader.pos)?;
            let frame_size=zstd::zstd_safe::find_frame_compressed_size(payload).map_err(|_|anyhow::anyhow!("Invalid Zstd frame"))?;
            ensure!(frame_size==payload.len(),"Trailing Zstd frames/data");
            let mut decoder=zstd::stream::read::Decoder::new(payload)?;decoder.window_log_max(27)?;
            let mut out=Vec::new();decoder.take(size as u64+1).read_to_end(&mut out)?;out
        },
        2=>{
            let mut blocks=Vec::new();let mut remaining=size;
            while remaining>0 {
                let count=remaining.min(CHUNK);let encoded=reader.u32()?;let raw=encoded>>31!=0;let n=(encoded&0x7fffffff) as usize;
                ensure!(n>0 && n<=count && (!raw || n==count),"Invalid chunk length");
                blocks.push((reader.take(n)?,count,raw));remaining-=count;
            }
            let decoded:Result<Vec<Vec<u8>>>=blocks.into_par_iter().map(|(bytes,n,raw)|if raw{Ok(bytes.to_vec())}else{unpack(bytes,n,stride)}).collect();
            decoded?.concat()
        },
        _=>bail!("Unknown experimental mode"),
    };
    reader.end()?;ensure!(out.len()==size && xxh64(&out,0)==hash,"Content length/checksum mismatch");Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(n:usize)->Vec<u8>{let mut state=97u64;(0..n).map(|_|{state^=state<<13;state^=state>>7;state^=state<<17;state as u8}).collect()}
    #[test]
    fn inverse_all_strides_wraps_and_odd_tails()->Result<()> {
        for n in [0,1,2,3,63,64,65,127,128,129,8193,CHUNK+3] {
            let bytes=fixture(n);
            for stride in STRIDES {for block in bytes.chunks(CHUNK){assert_eq!(unpack(&pack(block,stride),block.len(),stride)?,block);}}
        }Ok(())
    }
    fn numeric()->Vec<u8>{(0..CHUNK).flat_map(|i|((i/16+(i%16)*1000) as u16).to_le_bytes()).collect()}
    #[test]
    fn frame_modes_roundtrip()->Result<()> {
        for data in [vec![],vec![0;32768],fixture(32769),numeric()] {
            for method in ["adaptive","numeric","zstd-1","zstd-3","zstd-9"] {
                let (encoded,_)=encode(&data,method,2)?;assert_eq!(decode(&encoded)?,data);
            }
        }Ok(())
    }
    #[test]
    fn truncation_corruption_and_bounds()->Result<()> {
        let (encoded,_)=encode(&numeric(),"numeric",2)?;
        for n in [0,7,8,17,25,26,encoded.len()/2,encoded.len()-1]{assert!(decode(&encoded[..n]).is_err());}
        for index in [0,8,9,18,30,encoded.len()-1]{let mut damaged=encoded.clone();damaged[index]^=127;assert!(decode(&damaged).is_err());}
        let mut huge=encoded.clone();huge[10..18].copy_from_slice(&u64::MAX.to_le_bytes());assert!(decode(&huge).is_err());
        let mut extra=encoded;extra.push(0);assert!(decode(&extra).is_err());
        Ok(())
    }
}
