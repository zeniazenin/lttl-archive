//! Versioned, bounded AEAD envelope. Keys are never embedded in the archive.
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use chacha20poly1305::{XChaCha20Poly1305, XNonce, KeyInit, aead::{Aead, Payload, OsRng, rand_core::RngCore}};
use serde::{Serialize, Deserialize};
use sha2::{Digest, Sha256};
use std::{io::{Read, Write}, sync::atomic::{AtomicBool, Ordering}};
use zeroize::Zeroizing;

pub const MAGIC:&[u8;8]=b"LTTLID01";
const CHUNK:usize=1024*1024;
const MAX_PLAINTEXT:u64=110*1024*1024*1024;
pub type Key=Zeroizing<[u8;32]>;
pub fn random<const N:usize>()->[u8;N] {let mut bytes=[0;N];OsRng.fill_bytes(&mut bytes);bytes}
pub fn identifier()->String {B64.encode(random::<32>())}
pub fn valid_id(id:&str)->bool {id.len()==43 && B64.decode(id).is_ok_and(|v|v.len()==32)}
pub fn key()->Key {Zeroizing::new(random())}
pub fn encode(bytes:&[u8])->String {B64.encode(bytes)}
pub fn decode_key(text:&str)->Result<Key> {
    let bytes=Zeroizing::new(B64.decode(text).context("Invalid key encoding")?);
    ensure!(bytes.len()==32,"Invalid key length");let mut key=Zeroizing::new([0;32]);key.copy_from_slice(&bytes);Ok(key)
}
pub fn digest(bytes:&[u8])->String {B64.encode(Sha256::digest(bytes))}
pub fn seal(key:&[u8;32],bytes:&[u8],aad:&[u8])->Result<Vec<u8>> {
    let nonce=random::<24>();let cipher=XChaCha20Poly1305::new(key.into());
    let mut out=nonce.to_vec();out.extend(cipher.encrypt(XNonce::from_slice(&nonce),Payload{msg:bytes,aad}).map_err(|_|anyhow::anyhow!("Encryption failed"))?);Ok(out)
}
pub fn unseal(key:&[u8;32],bytes:&[u8],aad:&[u8])->Result<Zeroizing<Vec<u8>>> {
    ensure!(bytes.len()>=40,"Truncated encrypted key");
    let cipher=XChaCha20Poly1305::new(key.into());
    Ok(Zeroizing::new(cipher.decrypt(XNonce::from_slice(&bytes[..24]),Payload{msg:&bytes[24..],aad}).map_err(|_|anyhow::anyhow!("Key authentication failed"))?))
}
#[derive(Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {pub broker:String,pub archive_id:String,pub format:String,pub nonce:String,#[serde(default,skip_serializing_if="Option::is_none")] pub tracking:Option<TrackingPolicy>}
/// Immutable archive policy, authenticated with the rest of the envelope header.
#[derive(Clone,Debug,Default,PartialEq,Eq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackingPolicy {
    #[serde(default)] pub enabled:bool,
    #[serde(default)] pub alert_countries:Vec<String>,
}
impl TrackingPolicy {
    pub fn validate(&self)->Result<()> {
        ensure!(self.alert_countries.len()<=250 && self.alert_countries.iter().all(|c|c.len()==2 && c.bytes().all(|b|b.is_ascii_uppercase())),"Use two-letter uppercase country codes (for example KP)");
        ensure!(self.enabled || self.alert_countries.is_empty(),"Country alerts require archive tracking");Ok(())
    }
}
impl Header {
    pub fn new(broker:String,archive_id:String,format:String)->Self {Self{broker,archive_id,format,nonce:B64.encode(random::<16>()),tracking:None}}
    fn validate(&self)->Result<[u8;16]> {
        if let Some(policy)=&self.tracking {policy.validate()?;}
        ensure!(valid_id(&self.archive_id),"Invalid archive ID");
        ensure!(matches!(self.format.as_str(),"lttl"|"zip"|"tar.zst"|"tar.lz4"),"Unsupported inner archive");
        B64.decode(&self.nonce)?.try_into().map_err(|_|anyhow::anyhow!("Invalid archive nonce"))
    }
}
pub fn read_header(reader:&mut impl Read)->Result<(Header,Vec<u8>)> {
    let mut magic=[0;8];reader.read_exact(&mut magic)?;ensure!(&magic==MAGIC,"Not an identity-protected lttl archive");
    let mut len=[0;4];reader.read_exact(&mut len)?;let len=u32::from_le_bytes(len) as usize;
    ensure!((1..=16384).contains(&len),"Invalid identity archive header size");
    let mut bytes=vec![0;len];reader.read_exact(&mut bytes)?;
    let header:Header=serde_json::from_slice(&bytes)?;header.validate()?;Ok((header,bytes))
}
fn nonce(prefix:[u8;16],counter:u64)->[u8;24] {let mut n=[0;24];n[..16].copy_from_slice(&prefix);n[16..].copy_from_slice(&counter.to_le_bytes());n}
fn aad(hash:&[u8],counter:u64,len:u32)->Vec<u8> {let mut a=hash.to_vec();a.extend(counter.to_le_bytes());a.extend(len.to_le_bytes());a}
pub fn encrypt(reader:&mut impl Read,writer:&mut impl Write,header:&Header,key:&[u8;32],cancel:&AtomicBool)->Result<()> {
    let prefix=header.validate()?;let bytes=serde_json::to_vec(header)?;ensure!(bytes.len()<=16384,"Header too large");
    writer.write_all(MAGIC)?;writer.write_all(&(bytes.len() as u32).to_le_bytes())?;writer.write_all(&bytes)?;
    let hash=Sha256::digest(&bytes);let cipher=XChaCha20Poly1305::new(key.into());
    let mut buffer=Zeroizing::new(vec![0;CHUNK]);let mut total=0;
    for counter in 0u64.. {
        ensure!(!cancel.load(Ordering::Relaxed),"Operation cancelled");
        let mut len=0;while len<CHUNK {let n=reader.read(&mut buffer[len..])?;if n==0 {break;}len+=n;}
        total+=len as u64;ensure!(total<=MAX_PLAINTEXT,"Protected archive exceeds size limit");
        let encrypted=cipher.encrypt(XNonce::from_slice(&nonce(prefix,counter)),Payload{msg:&buffer[..len],aad:&aad(&hash,counter,len as u32)}).map_err(|_|anyhow::anyhow!("Archive encryption failed"))?;
        writer.write_all(&(len as u32).to_le_bytes())?;writer.write_all(&encrypted)?;
        if len==0 {return Ok(());}
    }
    anyhow::bail!("Archive counter overflow")
}
pub fn decrypt(reader:&mut impl Read,writer:&mut impl Write,key:&[u8;32],cancel:&AtomicBool)->Result<Header> {
    let (header,bytes)=read_header(reader)?;let prefix=header.validate()?;let hash=Sha256::digest(&bytes);
    let cipher=XChaCha20Poly1305::new(key.into());let mut total=0;
    for counter in 0u64.. {
        ensure!(!cancel.load(Ordering::Relaxed),"Operation cancelled");
        let mut len=[0;4];reader.read_exact(&mut len).context("Missing authenticated archive terminator")?;
        let len=u32::from_le_bytes(len);ensure!(len as usize<=CHUNK,"Invalid encrypted chunk length");
        total+=len as u64;ensure!(total<=MAX_PLAINTEXT,"Protected archive exceeds size limit");
        let mut encrypted=vec![0;len as usize+16];reader.read_exact(&mut encrypted)?;
        let plain=Zeroizing::new(cipher.decrypt(XNonce::from_slice(&nonce(prefix,counter)),Payload{msg:&encrypted,aad:&aad(&hash,counter,len)}).map_err(|_|anyhow::anyhow!("Archive authentication failed: wrong key or modified archive"))?);
        if len==0 {let mut extra=[0];ensure!(reader.read(&mut extra)?==0,"Trailing data after protected archive");return Ok(header);}
        writer.write_all(&plain)?;
    }
    anyhow::bail!("Archive counter overflow")
}
