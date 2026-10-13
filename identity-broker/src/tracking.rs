//! Archive-specific telemetry and a durable, leased, at-least-once webhook outbox.
use crate::{crypto, now, store::Store};
use anyhow::{Result, ensure, Context};
use axum::http::HeaderMap;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sqlx::Row;
use std::{net::IpAddr, time::Duration};
use zeroize::Zeroizing;

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub webhook_url:Option<String>,
    pub require_tracking:bool,
    pub alert_countries:Vec<String>,
    /// Explicit broker account IDs, not an inference from email labels.
    pub organization_member_ids:Option<Vec<String>>,
    pub signed_geo_headers:bool,
}
pub struct Service {
    pub config:Config,
    secret:Option<Zeroizing<String>>,
    dispatch:Option<Zeroizing<String>>,
    geo:Option<Zeroizing<String>>,
    http:reqwest::Client,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Location {
    pub ip:Option<String>,
    pub country:Option<String>,
    pub source:String,
}
#[derive(Serialize, Deserialize)]
pub struct Event {
    pub schema:u32,
    pub event_id:String,
    pub archive_id:String,
    pub occurred_at:i64,
    pub event:String,
    pub evidence:String,
    pub user_id:Option<String>,
    pub identity_verified:bool,
    pub issuer:Option<String>,
    pub subject:Option<String>,
    pub organization_membership:String,
    pub location:Location,
    pub alerts:Vec<String>,
}
fn secret(name:&str)->Result<Zeroizing<String>> {
    let s=std::env::var(name).with_context(||format!("Set {name} in the service secret store"))?;
    ensure!(s.len()>=32,"Tracking secrets must contain at least 32 bytes");Ok(Zeroizing::new(s))
}
pub fn sign(secret:&str,bytes:&[u8])->String {
    let mut mac=Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC key");mac.update(bytes);
    crypto::encode(&mac.finalize().into_bytes())
}
fn verify(secret:&str,bytes:&[u8],signature:&str)->bool {
    use base64::Engine;
    let Ok(signature)=base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(signature) else{return false};
    let mut mac=Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC key");mac.update(bytes);mac.verify_slice(&signature).is_ok()
}
impl Service {
    pub fn new(config:Config)->Result<Self> {
        crypto::TrackingPolicy{enabled:true,alert_countries:config.alert_countries.clone()}.validate()?;
        if let Some(ids)=&config.organization_member_ids {ensure!(ids.len()<=10000 && ids.iter().all(|id|crypto::valid_id(id)),"Invalid organization account IDs");}
        ensure!(!config.require_tracking || config.webhook_url.is_some(),"Required tracking needs a webhook");
        if let Some(url)=&config.webhook_url {
            let u=crate::safe_url(url)?;ensure!(u.query().is_none(),"Webhook URL must not contain query credentials");
        }
        let enabled=config.webhook_url.is_some();
        Ok(Self {
            secret:if enabled {Some(secret("LTTL_TRACKING_WEBHOOK_SECRET")?)}else{None},
            dispatch:if enabled {Some(secret("LTTL_TRACKING_DISPATCH_TOKEN")?)}else{None},
            geo:if config.signed_geo_headers {Some(secret("LTTL_TRACKING_GEO_SECRET")?)}else{None},
            config,http:reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(3)).build()?,
        })
    }
    pub fn policy(&self,mut requested:crypto::TrackingPolicy)->Result<crypto::TrackingPolicy> {
        requested.validate()?;
        if self.config.require_tracking {requested.enabled=true;}
        if requested.enabled {
            ensure!(self.config.webhook_url.is_some(),"Organization tracking webhook is not configured");
            for country in &self.config.alert_countries {if !requested.alert_countries.contains(country) {requested.alert_countries.push(country.clone());}}
        }
        requested.validate()?;Ok(requested)
    }
    pub fn location(&self,headers:&HeaderMap,peer:Option<IpAddr>,method:&str,path:&str)->Location {
        let fallback=Location{ip:peer.map(|p|p.to_string()),country:None,source:"transport_peer_country_unknown".into()};
        let Some(secret)=&self.geo else{return fallback};
        let h=|name|headers.get(name).and_then(|s|s.to_str().ok()).unwrap_or("");
        let ip=h("x-lttl-client-ip");let country=h("x-lttl-country");let timestamp=h("x-lttl-geo-timestamp");
        let valid_country=country=="-" || (country.len()==2 && country.bytes().all(|b|b.is_ascii_uppercase()) && country!="XX" && country!="ZZ");
        if ip.parse::<IpAddr>().is_err() || !valid_country || !timestamp.parse::<i64>().is_ok_and(|t|t.abs_diff(now())<=60) {return fallback;}
        let message=format!("{timestamp}\n{method}\n{path}\n{ip}\n{country}");
        if !verify(secret,message.as_bytes(),h("x-lttl-geo-signature")) {return fallback;}
        Location{ip:Some(ip.into()),country:if country=="-" {None}else{Some(country.into())},source:"signed_ingress_ip_estimate".into()}
    }
    pub fn dispatch_allowed(&self,headers:&HeaderMap)->bool {
        let Some(secret)=&self.dispatch else{return false};
        let supplied=headers.get("authorization").and_then(|h|h.to_str().ok()).and_then(|s|s.strip_prefix("Bearer ")).unwrap_or("");
        // Constant-time MAC verification without exposing either token.
        verify(secret,b"lttl dispatch",&sign(supplied,b"lttl dispatch"))
    }
    pub async fn record(&self,db:&Store,id:&str,user:Option<&str>,kind:&str,location:&Location)->Result<()> {
        let Some(policy)=db.tracking_policy(id).await? else{return Ok(())};
        if !policy.enabled {return Ok(())}
        let identity=if let Some(user)=user {sqlx::query("SELECT issuer,subject FROM users WHERE id=$1").bind(user).fetch_optional(&db.pool).await?}else{None};
        let verified=identity.is_some();
        let membership=match (&self.config.organization_member_ids,user.filter(|_|verified)) {
            (Some(ids),Some(id))=>if ids.iter().any(|s|s==id) {"member"}else{"external"},
            _=>"unknown",
        };
        let mut alerts=Vec::new();
        if membership=="external" {alerts.push("outside_organization".into());}
        if kind=="key_denied" {alerts.push("access_denied".into());}
        if kind=="authentication_failed" {alerts.push("authentication_failed".into());}
        if let Some(country)=&location.country {if policy.alert_countries.contains(country) {alerts.push("country_watchlist".into());}}
        let event=Event{schema:1,event_id:crypto::identifier(),archive_id:id.into(),occurred_at:now(),event:kind.into(),evidence:"broker_request_not_proof_of_file_possession_or_successful_decryption".into(),user_id:user.filter(|_|verified).map(Into::into),identity_verified:verified,
            issuer:identity.as_ref().map(|r|r.get(0)),subject:identity.as_ref().map(|r|r.get(1)),organization_membership:membership.into(),location:location.clone(),alerts};
        let mut tx=db.pool.begin().await?;
        sqlx::query("INSERT INTO tracking_events(id,archive_id,at,payload) VALUES($1,$2,$3,$4)").bind(&event.event_id).bind(id).bind(event.occurred_at).bind(serde_json::to_string(&event)?).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO tracking_outbox(id,next_at,attempts,lease) VALUES($1,$2,0,'')").bind(&event.event_id).bind(now()).execute(&mut *tx).await?;
        tx.commit().await?;
        // Await one bounded delivery: serverless runtimes may freeze spawned tasks.
        // Failure leaves the durable outbox for the scheduled dispatcher.
        if self.deliver(db,1).await.is_err() {eprintln!("Tracking webhook delivery deferred; inspect the outbox");}
        Ok(())
    }
    pub async fn deliver(&self,db:&Store,limit:usize)->Result<usize> {
        let (Some(url),Some(secret))=(&self.config.webhook_url,&self.secret) else{return Ok(0)};
        let mut sent=0;
        for _ in 0..limit.min(10) {
            let lease=crypto::identifier();
            let row=sqlx::query("UPDATE tracking_outbox SET lease=$1,next_at=$2,attempts=attempts+1 WHERE id=(SELECT id FROM tracking_outbox WHERE next_at<=$3 ORDER BY next_at,id LIMIT 1) AND next_at<=$3 RETURNING id,attempts")
                .bind(&lease).bind(now()+60).bind(now()).fetch_optional(&db.pool).await?;
            let Some(row)=row else{break};let id:String=row.get(0);let attempts:i64=row.get(1);
            let row=sqlx::query("SELECT payload FROM tracking_events WHERE id=$1").bind(&id).fetch_one(&db.pool).await?;
            let payload:String=row.get(0);let timestamp=now().to_string();
            let signature=sign(secret,format!("{id}.{timestamp}.{payload}").as_bytes());
            let result=self.http.post(url).header("content-type","application/json").header("x-lttl-event-id",&id).header("x-lttl-timestamp",timestamp).header("x-lttl-signature",signature).body(payload).send().await;
            if result.is_ok_and(|r|r.status().is_success()) {
                sqlx::query("DELETE FROM tracking_outbox WHERE id=$1 AND lease=$2").bind(&id).bind(&lease).execute(&db.pool).await?;sent+=1;
            } else {
                let delay=30i64.saturating_mul(1i64<<attempts.min(10)).min(21600);
                sqlx::query("UPDATE tracking_outbox SET next_at=$1 WHERE id=$2 AND lease=$3").bind(now()+delay).bind(&id).bind(&lease).execute(&db.pool).await?;
            }
        }
        Ok(sent)
    }
}
impl Store {
    pub async fn tracking_policy(&self,id:&str)->Result<Option<crypto::TrackingPolicy>> {
        let row=sqlx::query("SELECT policy FROM archive_tracking WHERE archive_id=$1").bind(id).fetch_optional(&self.pool).await?;
        row.map(|r|serde_json::from_str(&r.get::<String,_>(0)).map_err(Into::into)).transpose()
    }
    pub async fn tracking_events(&self,id:&str,owner:&str)->Result<Option<Vec<Event>>> {
        if sqlx::query("SELECT id FROM archives WHERE id=$1 AND owner=$2").bind(id).bind(owner).fetch_optional(&self.pool).await?.is_none() {return Ok(None)}
        let rows=sqlx::query("SELECT payload FROM tracking_events WHERE archive_id=$1 ORDER BY at DESC,id DESC LIMIT 200").bind(id).fetch_all(&self.pool).await?;
        Ok(Some(rows.into_iter().map(|r|serde_json::from_str(&r.get::<String,_>(0))).collect::<std::result::Result<_,_>>()?))
    }
    pub async fn prune_tracking(&self)->Result<()> {
        // Bounded data retention; undelivered events expire too, without silent success.
        sqlx::query("DELETE FROM tracking_events WHERE at<$1").bind(now()-90*86400).execute(&self.pool).await?;Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn service()->Service {
        Service{config:Config::default(),secret:Some(Zeroizing::new("a".repeat(32))),dispatch:Some(Zeroizing::new("b".repeat(32))),geo:Some(Zeroizing::new("c".repeat(32))),
            http:reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(3)).build().unwrap()}
    }
    #[test]
    fn policy_and_location_trust_boundaries() {
        let mut svc=service();
        assert!(!svc.policy(crypto::TrackingPolicy::default()).unwrap().enabled);
        let p=crypto::TrackingPolicy{enabled:true,alert_countries:vec!["KP".into()]};
        assert!(svc.policy(p).is_err());
        svc.config.webhook_url=Some("https://security.example/hook".into());svc.config.require_tracking=true;svc.config.alert_countries=vec!["KP".into()];
        assert_eq!(svc.policy(crypto::TrackingPolicy::default()).unwrap().alert_countries,vec!["KP"]);
        let mut h=HeaderMap::new();h.insert("x-lttl-client-ip","203.0.113.1".parse().unwrap());h.insert("x-lttl-country","KP".parse().unwrap());
        assert!(svc.location(&h,None,"POST","/v1/auth/start").country.is_none());
        let t=now().to_string();h.insert("x-lttl-geo-timestamp",t.parse().unwrap());
        h.insert("x-lttl-geo-signature",sign(&"c".repeat(32),format!("{t}\nPOST\n/v1/auth/start\n203.0.113.1\nKP").as_bytes()).parse().unwrap());
        assert_eq!(svc.location(&h,None,"POST","/v1/auth/start").country.as_deref(),Some("KP"));
        assert!(svc.location(&h,None,"POST","/v1/archives/x/key").country.is_none());
        h.insert("x-lttl-country","US".parse().unwrap());assert!(svc.location(&h,None,"POST","/v1/auth/start").country.is_none());
        h.insert("x-lttl-country","KP".parse().unwrap());h.insert("x-lttl-geo-timestamp",(now()-120).to_string().parse().unwrap());
        assert!(svc.location(&h,None,"POST","/v1/auth/start").country.is_none());
        assert!(!svc.dispatch_allowed(&h));h.insert("authorization",format!("Bearer {}","b".repeat(32)).parse().unwrap());assert!(svc.dispatch_allowed(&h));
    }
    #[tokio::test]
    async fn archive_isolation_retry_signatures_and_retention()->Result<()> {
        use std::sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}};
        use axum::{routing::post,Router};
        let fail=Arc::new(AtomicBool::new(true));let deliveries=Arc::new(Mutex::new(Vec::new()));
        let (f,d)=(fail.clone(),deliveries.clone());
        let route=Router::new().route("/hook",post(move |h:HeaderMap,body:String|{let(f,d)=(f.clone(),d.clone());async move {
            d.lock().unwrap().push((h,body));if f.load(Ordering::SeqCst) {axum::http::StatusCode::SERVICE_UNAVAILABLE}else{axum::http::StatusCode::NO_CONTENT}
        }}));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await?;let url=format!("http://{}/hook",listener.local_addr()?);
        let server=tokio::spawn(async move{axum::serve(listener,route).await.unwrap()});
        let temp=tempfile::tempdir()?;let path=temp.path().join("tracking.sqlite");let db=Store::connect(path.to_str().unwrap(),false,false).await?;
        let owner=db.enroll("issuer","owner","Owner","").await?;let outsider=db.enroll("issuer","guest","Guest","").await?;
        let id=crypto::identifier();let off=crypto::identifier();
        db.create_archive(&off,&owner.id,&[owner.id.clone()],b"wrapped").await?;
        db.create_archive_tracked(&id,&owner.id,&[owner.id.clone()],b"wrapped",&crypto::TrackingPolicy{enabled:true,alert_countries:vec!["KP".into()]}).await?;
        let mut svc=service();svc.config.webhook_url=Some(url.clone());svc.config.organization_member_ids=Some(vec![owner.id.clone()]);
        let location=Location{ip:Some("203.0.113.1".into()),country:Some("KP".into()),source:"test".into()};
        svc.record(&db,&off,None,"open_attempt",&location).await?;assert!(db.tracking_events(&off,&owner.id).await?.unwrap().is_empty());
        svc.record(&db,&id,Some(&outsider.id),"key_denied",&location).await?;
        assert!(db.tracking_events(&id,&outsider.id).await?.is_none());
        let events=db.tracking_events(&id,&owner.id).await?.unwrap();assert_eq!(events.len(),1);
        assert_eq!(events[0].alerts,vec!["outside_organization","access_denied","country_watchlist"]);assert_eq!(events[0].subject.as_deref(),Some("guest"));
        assert_eq!(deliveries.lock().unwrap().len(),1);assert_eq!(svc.deliver(&db,10).await?,0);
        db.pool.close().await;let db=Store::connect(path.to_str().unwrap(),false,false).await?;
        let mut restarted=service();restarted.config.webhook_url=Some(url);
        sqlx::query("UPDATE tracking_outbox SET next_at=0").execute(&db.pool).await?;fail.store(false,Ordering::SeqCst);
        let(a,b)=tokio::join!(restarted.deliver(&db,1),restarted.deliver(&db,1));assert_eq!(a?+b?,1);
        let packets=deliveries.lock().unwrap().clone();assert_eq!(packets.len(),2);
        assert_eq!(packets[0].0["x-lttl-event-id"],packets[1].0["x-lttl-event-id"]);
        for(h,body)in packets {let text=format!("{}.{}.{}",h["x-lttl-event-id"].to_str()?,h["x-lttl-timestamp"].to_str()?,body);
            assert!(verify(&"a".repeat(32),text.as_bytes(),h["x-lttl-signature"].to_str()?));assert!(!verify(&"a".repeat(32),b"tampered",h["x-lttl-signature"].to_str()?));}
        sqlx::query("UPDATE tracking_events SET at=0").execute(&db.pool).await?;db.prune_tracking().await?;
        assert!(db.tracking_events(&id,&owner.id).await?.unwrap().is_empty());server.abort();Ok(())
    }
}
