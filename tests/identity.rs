use anyhow::Result;
use lttl::identity::{crypto,Profile,guides::{Provider,generate},broker_url};
use std::sync::atomic::AtomicBool;

fn encoded(data:&[u8])->Result<(Vec<u8>,crypto::Key)> {
    let key=crypto::key();let h=crypto::Header::new("https://keys.example".into(),crypto::identifier(),"tar.zst".into());
    let mut bytes=Vec::new();crypto::encrypt(&mut &*data,&mut bytes,&h,&key,&AtomicBool::new(false))?;Ok((bytes,key))
}
#[test]fn archive_tracking_is_authenticated_and_legacy_headers_stay_readable()->Result<()> {
    let cancel=AtomicBool::new(false);let key=crypto::key();
    let mut h=crypto::Header::new("https://keys.example".into(),crypto::identifier(),"zip".into());
    assert!(!serde_json::to_string(&h)?.contains("tracking"));
    h.tracking=Some(crypto::TrackingPolicy{enabled:true,alert_countries:vec!["KP".into()]});
    let mut bytes=Vec::new();crypto::encrypt(&mut &b"payload"[..],&mut bytes,&h,&key,&cancel)?;
    let mut restored=Vec::new();let read=crypto::decrypt(&mut bytes.as_slice(),&mut restored,&key,&cancel)?;
    assert_eq!(read.tracking,h.tracking);assert_eq!(restored,b"payload");
    let n=u32::from_le_bytes(bytes[8..12].try_into()?)as usize;
    let mut value:serde_json::Value=serde_json::from_slice(&bytes[12..12+n])?;value.as_object_mut().unwrap().remove("tracking");
    let header=serde_json::to_vec(&value)?;let mut changed=b"LTTLID01".to_vec();changed.extend((header.len()as u32).to_le_bytes());changed.extend(header);changed.extend(&bytes[12+n..]);
    assert!(crypto::decrypt(&mut changed.as_slice(),&mut Vec::new(),&key,&cancel).is_err());
    assert!(crypto::TrackingPolicy{enabled:false,alert_countries:vec!["KP".into()]}.validate().is_err());Ok(())
}
#[test]fn encrypted_stream_roundtrips_and_requires_authenticated_end()->Result<()> {
    for size in [0,1,15,1048576,2*1048576+13] {
        let data=(0..size).map(|i|(i%251) as u8).collect::<Vec<_>>();let (bytes,key)=encoded(&data)?;let mut out=Vec::new();
        crypto::decrypt(&mut bytes.as_slice(),&mut out,&key,&AtomicBool::new(false))?;assert_eq!(out,data);
        for end in [bytes.len()-1,bytes.len()-16,bytes.len()-20] {assert!(crypto::decrypt(&mut &bytes[..end],&mut Vec::new(),&key,&AtomicBool::new(false)).is_err());}
        let mut trailing=bytes.clone();trailing.push(0);assert!(crypto::decrypt(&mut trailing.as_slice(),&mut Vec::new(),&key,&AtomicBool::new(false)).is_err());
        assert!(!bytes.windows(32).any(|w|w==&*key));
    }Ok(())
}
#[test]fn changed_header_key_payload_and_order_are_rejected()->Result<()> {
    let (bytes,key)=encoded(&vec![17;3*1048576])?;
    assert!(crypto::decrypt(&mut bytes.as_slice(),&mut Vec::new(),&crypto::key(),&AtomicBool::new(false)).is_err());
    let offset=12+u32::from_le_bytes(bytes[8..12].try_into()?) as usize;
    for position in [20,offset+4,bytes.len()-1] {let mut changed=bytes.clone();changed[position]^=1;assert!(crypto::decrypt(&mut changed.as_slice(),&mut Vec::new(),&key,&AtomicBool::new(false)).is_err());}
    let record=4+1048576+16;let mut reordered=bytes.clone();
    reordered[offset..offset+record].copy_from_slice(&bytes[offset+record..offset+2*record]);
    reordered[offset+record..offset+2*record].copy_from_slice(&bytes[offset..offset+record]);
    assert!(crypto::decrypt(&mut reordered.as_slice(),&mut Vec::new(),&key,&AtomicBool::new(false)).is_err());
    let mut oversized=bytes.clone();oversized[offset..offset+4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(crypto::decrypt(&mut oversized.as_slice(),&mut Vec::new(),&key,&AtomicBool::new(false)).is_err());
    let mut oversized=bytes;oversized[8..12].copy_from_slice(&u32::MAX.to_le_bytes());assert!(crypto::read_header(&mut oversized.as_slice()).is_err());Ok(())
}
#[test]fn wrapped_key_binds_archive_id_and_cancellation_is_observed()->Result<()> {
    let master=crypto::key();let key=crypto::key();let a=crypto::identifier();let b=crypto::identifier();
    let wrapped=crypto::seal(&master,&*key,a.as_bytes())?;
    assert_eq!(crypto::unseal(&master,&wrapped,a.as_bytes())?.as_slice(),&*key);
    assert!(crypto::unseal(&master,&wrapped,b.as_bytes()).is_err());
    let (bytes,key)=encoded(b"secret")?;
    assert!(crypto::decrypt(&mut bytes.as_slice(),&mut Vec::new(),&key,&AtomicBool::new(true)).is_err());Ok(())
}
#[test]fn profiles_and_provider_guides_use_trusted_origins()->Result<()> {
    for bad in ["http://keys.example","https://user:secret@keys.example","https://keys.example/path","https://keys.example?query=1","https://keys.example#fragment","file:///tmp/x"] {assert!(broker_url(bad).is_err());}
    assert_eq!(broker_url("http://127.0.0.1:8787/")?,"http://127.0.0.1:8787");
    let root=tempfile::tempdir()?;let path=root.path().join("profile.json");let profile=Profile{broker_url:"https://keys.example".into(),provider:Provider::Entra};profile.save(&path)?;assert_eq!(Profile::load(&path)?.broker_url,profile.broker_url);
    for provider in Provider::ALL {let guide=generate(provider,"https://keys.example","tenant","client-id")?;assert!(guide.contains("https://keys.example/auth/callback"));assert!(guide.contains("LTTL_BROKER_MASTER_KEY"));}
    for hosting in lttl::identity::hosting::Hosting::ALL {
        for provider in Provider::ALL {
            let guide=lttl::identity::hosting::generate(provider,hosting,"https://keys.example","tenant","client-id")?;
            assert!(guide.contains(provider.label()));assert!(guide.contains(hosting.instructions()));
            assert!(guide.contains("https://keys.example/auth/callback"));assert!(guide.contains("Native KMS"));
        }
    }
    Ok(())
}
