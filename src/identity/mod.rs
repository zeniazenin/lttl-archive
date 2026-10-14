//! Identity-bound sharing: authenticated broker authorization plus an AEAD envelope.
pub mod crypto;
pub mod guides;
pub mod hosting;
pub mod admin;
use anyhow::{Context,Result,ensure};
use serde::{Deserialize,Serialize};
use std::{path::{Path,PathBuf},io::{Read,Seek,SeekFrom},sync::atomic::AtomicBool,time::{Duration,Instant}};
use zeroize::Zeroizing;
use crate::{CreateOptions,Backend,GpuOptions,PortableOptions,Stats,Progress,EntryInfo};

#[derive(Clone,Serialize,Deserialize,Default)]
#[serde(deny_unknown_fields)]
pub struct Profile {pub broker_url:String,#[serde(default)] pub provider:guides::Provider}
pub fn broker_url(value:&str)->Result<String> {
    let url=url::Url::parse(value)?;
    ensure!(url.scheme()=="https" || (url.scheme()=="http" && matches!(url.host_str(),Some("127.0.0.1"|"[::1]"))),"Use HTTPS for the broker (HTTP is only allowed on loopback for development)");
    ensure!(url.username().is_empty() && url.password().is_none() && url.query().is_none() && url.fragment().is_none() && url.path()=="/","Broker URL must be an origin without credentials, path, query or fragment");
    Ok(url.as_str().trim_end_matches('/').into())
}
impl Profile {
    pub fn load(path:&Path)->Result<Self> {let bytes=std::fs::read(path)?;ensure!(bytes.len()<=16384,"Profile too large");let mut profile:Self=serde_json::from_slice(&bytes)?;profile.broker_url=broker_url(&profile.broker_url)?;Ok(profile)}
    pub fn save(&self,path:&Path)->Result<()> {
        let profile=Self{broker_url:broker_url(&self.broker_url)?,provider:self.provider};
        crate::check_path(path)?;
        let mut temp=tempfile::NamedTempFile::new_in(crate::parent_dir(path))?;
        use std::io::Write;temp.write_all(&serde_json::to_vec_pretty(&profile)?)?;temp.as_file().sync_all()?;temp.persist(path).map_err(|e|e.error)?;Ok(())
    }
}
pub fn default_profile_path()->PathBuf {std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("lttl").join("identity.json")}
#[derive(Clone,Serialize,Deserialize)]
pub struct User {pub id:String,pub name:String,pub email:String}
#[derive(Deserialize)] struct Start {browser_url:String}
#[derive(Deserialize)] struct SignedIn {token:String,user:User}
#[derive(Deserialize)] struct NewArchive {archive_id:String,key:String,#[serde(default)] tracking:crypto::TrackingPolicy}
pub struct Session {profile:Profile,token:Zeroizing<String>,pub user:User}
fn http()->Result<reqwest::blocking::Client> {Ok(reqwest::blocking::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(30)).build()?)}
fn json<T:serde::de::DeserializeOwned>(response:reqwest::blocking::Response)->Result<T> {
    let status=response.status();ensure!(status.is_success(),"Identity service returned HTTP {status}; sign in again or check access and service configuration");
    let mut bytes=Vec::new();response.take(1048577).read_to_end(&mut bytes)?;ensure!(bytes.len()<=1048576,"Identity service response too large");Ok(serde_json::from_slice(&bytes)?)
}
pub fn login(profile:&Profile,archive_id:Option<&str>,cancel:&AtomicBool,notice:&mut dyn FnMut(String))->Result<Session> {
    if archive_id.is_some() {eprintln!("Opening contacts the configured organization service. Archive policy may report this attempt, authenticated identity, time and estimated network location to its security system.");}
    let profile=Profile{broker_url:broker_url(&profile.broker_url)?,provider:profile.provider};let client=http()?;
    if let Some(id)=archive_id {ensure!(crypto::valid_id(id),"Invalid archive ID");}
    // A loopback code callback with PKCE keeps a copied browser sign-in URL
    // from silently authorizing another computer's polling session.
    let listener=std::net::TcpListener::bind("127.0.0.1:0")?;listener.set_nonblocking(true)?;
    let redirect=format!("http://127.0.0.1:{}/callback",listener.local_addr()?.port());
    let state=crypto::identifier();let verifier=Zeroizing::new(crypto::identifier());
    let start:Start=json(client.post(format!("{}/v1/auth/start",profile.broker_url)).json(&serde_json::json!({"archive_id":archive_id,"redirect_uri":redirect,"state":state,"challenge":crypto::digest(verifier.as_bytes())})).send()?)?;
    let browser=url::Url::parse(&start.browser_url)?;let origin=url::Url::parse(&profile.broker_url)?;
    ensure!(browser.origin()==origin.origin() && browser.path().starts_with("/auth/login/"),"Identity service returned an unexpected sign-in destination");
    notice(start.browser_url.clone());
    if std::env::var_os("LTTL_MANUAL_SIGN_IN").is_none() {let _=webbrowser::open(&start.browser_url);}
    let started=Instant::now();
    while started.elapsed()<Duration::from_secs(300) {
        crate::cancelled(cancel)?;
        match listener.accept() {
            Ok((mut stream,_))=> {
                use std::io::{BufRead,Write};
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_secs(1)))?;
                stream.set_write_timeout(Some(Duration::from_secs(1)))?;
                let mut line=String::new();
                let mut reader=std::io::BufReader::new((&mut stream).take(8192));
                if reader.read_line(&mut line).is_err() {continue;}
                let mut complete=false;
                loop {let mut field=String::new();match reader.read_line(&mut field) {Ok(0)|Err(_)=>break,Ok(_)=>{if field=="\r\n" {complete=true;break;}}}}
                drop(reader);if !complete {continue;}
                let parts=line.split_whitespace().collect::<Vec<_>>();
                if parts.len()!=3 || parts[0]!="GET" {continue;}
                let Ok(callback)=url::Url::parse(&format!("http://127.0.0.1{}",parts[1])) else{continue;};
                let params=callback.query_pairs().collect::<std::collections::HashMap<_,_>>();
                if callback.path()!="/callback" || !params.get("state").is_some_and(|v|crypto::digest(v.as_bytes())==crypto::digest(state.as_bytes())) {continue;}
                let message=b"Return to lttl to finish signing in.";
                let _=write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",message.len());
                let _=stream.write_all(message);
                let _=stream.shutdown(std::net::Shutdown::Write);
                // Drain the peer's close before dropping the Windows socket;
                // otherwise a fast CLI exit can reset an unread HTTP response.
                let _=std::io::copy(&mut (&mut stream).take(8192),&mut std::io::sink());
                let code=params.get("code").context("Identity sign-in was denied")?;ensure!(crypto::valid_id(code),"Invalid sign-in code");
                let signed:SignedIn=json(client.post(format!("{}/v1/auth/exchange",profile.broker_url)).json(&serde_json::json!({"code":code,"verifier":&*verifier})).send()?)?;
                return Ok(Session{profile,token:Zeroizing::new(signed.token),user:signed.user});
            }
            Err(error) if error.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(50)),
            Err(error)=>return Err(error.into()),
        }
    }
    anyhow::bail!("Sign-in timed out")
}

impl Session {
    pub fn audit_activity(&self,id:&str,before:Option<i64>)->Result<admin::Activity> {
        ensure!(crypto::valid_id(id) && before.is_none_or(|n|n>0),"Invalid archive ID or activity cursor");
        let mut request=http()?.get(format!("{}/v1/archives/{id}/audit",self.profile.broker_url)).bearer_auth(&*self.token);
        if let Some(before)=before {request=request.query(&[("before",before)]);}
        json(request.send()?)
    }
    pub fn retention(&self,id:&str,action:Option<&serde_json::Value>)->Result<serde_json::Value> {
        ensure!(crypto::valid_id(id),"Invalid archive ID");
        let url=format!("{}/v1/archives/{id}/retention",self.profile.broker_url);
        let client=http()?;
        let request=if let Some(action)=action {client.post(url).json(action)}else{client.get(url)};
        json(request.bearer_auth(&*self.token).send()?)
    }
    pub fn events(&self,id:&str)->Result<serde_json::Value> {ensure!(crypto::valid_id(id),"Invalid archive ID");json(http()?.get(format!("{}/v1/archives/{id}/events",self.profile.broker_url)).bearer_auth(&*self.token).send()?)}
    pub fn users(&self)->Result<Vec<User>> {let users:Vec<User>=json(http()?.get(format!("{}/v1/users",self.profile.broker_url)).bearer_auth(&*self.token).send()?)?;ensure!(users.len()<=1000 && users.iter().all(|u|crypto::valid_id(&u.id)),"Invalid recipient directory");Ok(users)}
    pub fn revoke(&self,id:&str)->Result<()> {ensure!(crypto::valid_id(id),"Invalid archive ID");let _:serde_json::Value=json(http()?.post(format!("{}/v1/archives/{id}/revoke",self.profile.broker_url)).bearer_auth(&*self.token).send()?)?;Ok(())}
}
pub fn is_protected(path:&Path)->Result<bool> {let mut file=crate::input_file(path)?;let mut bytes=[0;8];let n=file.read(&mut bytes)?;Ok(n==8 && &bytes==crypto::MAGIC)}
pub fn create(options:&CreateOptions,backend:Backend,gpu:&GpuOptions,portable:&PortableOptions,session:&Session,recipients:&[String],cancel:&AtomicBool,progress:&mut dyn FnMut(Progress))->Result<Stats> {
    create_tracked(options,backend,gpu,portable,session,recipients,&crypto::TrackingPolicy::default(),cancel,progress)
}
pub fn create_tracked(options:&CreateOptions,backend:Backend,gpu:&GpuOptions,portable:&PortableOptions,session:&Session,recipients:&[String],tracking:&crypto::TrackingPolicy,cancel:&AtomicBool,progress:&mut dyn FnMut(Progress))->Result<Stats> {
    tracking.validate()?;
    let started=Instant::now();crate::check_path(&options.output)?;crate::absent(&options.output)?;crate::cancelled(cancel)?;
    ensure!(!recipients.is_empty() && recipients.len()<=1000 && recipients.iter().all(|id|crypto::valid_id(id)),"Select between 1 and 1000 enrolled recipients");
    let temp=tempfile::tempdir()?;let mut inner=options.clone();inner.output=temp.path().join(format!("archive.{}",options.format.extension()));
    let issued:NewArchive=json(http()?.post(format!("{}/v1/archives",session.profile.broker_url)).bearer_auth(&*session.token).json(&serde_json::json!({"recipients":recipients,"tracking":tracking})).send()?)?;
    let text=Zeroizing::new(issued.key);let key=crypto::decode_key(&text)?;
    let mut header=crypto::Header::new(session.profile.broker_url.clone(),issued.archive_id.clone(),options.format.extension().into());
    ensure!(!tracking.enabled || issued.tracking.enabled,"Broker did not enable requested archive tracking");
    if issued.tracking.enabled {issued.tracking.validate()?;header.tracking=Some(issued.tracking);}
    let result=(||->Result<Stats> {
        let mut stats=crate::create_with_provider(&inner,backend,gpu,portable,cancel,progress)?;
        let mut output=tempfile::NamedTempFile::new_in(crate::parent_dir(&options.output))?;
        progress(Progress{phase:"Encrypting for selected recipients".into(),path:options.output.display().to_string(),bytes_done:0,bytes_total:0});
        crypto::encrypt(&mut crate::input_file(&inner.output)?,output.as_file_mut(),&header,&key,cancel)?;
        output.as_file().sync_all()?;stats.output_bytes=output.as_file().metadata()?.len();crate::cancelled(cancel)?;crate::check_path(&options.output)?;
        output.persist_noclobber(&options.output).map_err(|e|e.error)?;stats.elapsed_seconds=started.elapsed().as_secs_f64();Ok(stats)
    })();
    if result.is_err() {let _=session.revoke(&issued.archive_id);}result
}
pub fn with_open<T>(archive:&Path,profile:&Profile,cancel:&AtomicBool,notice:&mut dyn FnMut(String),operation:impl FnOnce(&Path)->Result<T>)->Result<T> {
    let mut file=crate::input_file(archive)?;let (header,_)=crypto::read_header(&mut file)?;
    ensure!(broker_url(&header.broker)?==broker_url(&profile.broker_url)?,"Archive belongs to a different key service. Configure its trusted service explicitly before signing in");
    let session=login(profile,Some(&header.archive_id),cancel,notice)?;
    #[derive(Deserialize)]struct Released {key:String}
    let released:Released=json(http()?.post(format!("{}/v1/archives/{}/key",session.profile.broker_url,header.archive_id)).bearer_auth(&*session.token).send()?)?;
    let text=Zeroizing::new(released.key);let key=crypto::decode_key(&text)?;
    let temp=tempfile::tempdir()?;let path=temp.path().join(format!("archive.{}",header.format));
    let mut plaintext=std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
    file.seek(SeekFrom::Start(0))?;crypto::decrypt(&mut file,&mut plaintext,&key,cancel)?;drop(plaintext);crate::cancelled(cancel)?;
    operation(&path)
}
pub fn extract(archive:&Path,dest:&Path,profile:&Profile,cancel:&AtomicBool,progress:&mut dyn FnMut(Progress),notice:&mut dyn FnMut(String))->Result<Stats> {with_open(archive,profile,cancel,notice,|inner|crate::extract(inner,dest,cancel,progress))}
pub fn list(archive:&Path,profile:&Profile,cancel:&AtomicBool,notice:&mut dyn FnMut(String))->Result<Vec<EntryInfo>> {with_open(archive,profile,cancel,notice,crate::list)}
pub fn test(archive:&Path,profile:&Profile,cancel:&AtomicBool,notice:&mut dyn FnMut(String))->Result<Stats> {with_open(archive,profile,cancel,notice,|inner|crate::test(inner,cancel))}
