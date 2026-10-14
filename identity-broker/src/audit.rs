//! Exact-byte signed records and batches. The independent vault pins the public key.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Any, Row, Transaction};
use std::{sync::Arc, time::Duration};
use crate::{crypto, now, store::Store};

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    /// Stable deployment identity, pinned by the independently administered vault.
    pub stream_id: String,
    pub vault_url: Option<String>,
    pub administrator_subjects: Vec<String>,
    /// Zero means indefinite; never a hidden default expiration.
    pub evidence_retention_days: u32,
    pub archive_retention_days: u32,
}
pub struct Signer { key: Ed25519KeyPair, pub public_key: String, pub stream: String }
impl Signer {
    pub fn from_seed(seed: &[u8], stream: &str) -> Result<Self> {
        ensure!(!stream.is_empty() && stream.len() <= 128 && stream.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c)), "Invalid audit stream ID");
        let key = Ed25519KeyPair::from_seed_unchecked(seed).map_err(|_| anyhow::anyhow!("Invalid audit signing seed"))?;
        let public_key = crypto::encode(key.public_key().as_ref());
        Ok(Self { key, public_key, stream: stream.into() })
    }
    fn sign(&self, payload: &str) -> String { crypto::encode(self.key.sign(payload.as_bytes()).as_ref()) }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signed { pub payload: String, pub signature: String }
pub fn verify(signed: &Signed, public: &str) -> Result<()> {
    let key = URL_SAFE_NO_PAD.decode(public)?;
    let signature = URL_SAFE_NO_PAD.decode(&signed.signature)?;
    UnparsedPublicKey::new(&ED25519, key).verify(signed.payload.as_bytes(), &signature)
        .map_err(|_| anyhow::anyhow!("Audit signature mismatch"))
}
pub struct Service {
    pub config: Config,
    pub signer: Option<Arc<Signer>>,
    token: Option<zeroize::Zeroizing<String>>,
    http: reqwest::Client,
}
impl Service {
    pub fn new(config: Config) -> Result<Self> {
        ensure!(config.evidence_retention_days <= 36500 && config.archive_retention_days <= 36500, "Retention exceeds 100 years");
        let (signer, token) = if config.enabled {
            let url = crate::safe_url(config.vault_url.as_deref().context("Audit vault URL required")?)?;
            ensure!(url.query().is_none(), "Vault URL must not contain query credentials");
            let seed = crypto::decode_key(&std::env::var("LTTL_AUDIT_SIGNING_SEED").context("Set LTTL_AUDIT_SIGNING_SEED in the broker secret store")?)?;
            let token = zeroize::Zeroizing::new(std::env::var("LTTL_AUDIT_VAULT_TOKEN").context("Set LTTL_AUDIT_VAULT_TOKEN")?);
            ensure!(token.len() >= 32, "Vault credential must contain at least 32 bytes");
            (Some(Arc::new(Signer::from_seed(&*seed, &config.stream_id)?)), Some(token))
        } else { (None, None) };
        Ok(Self { config, signer, token, http: reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(10)).build()? })
    }
    pub async fn dispatch(&self, db: &Store) -> Result<Option<String>> {
        let signer = self.signer.as_ref().context("Signed audit export is disabled")?;
        let Some(packet) = db.audit_batch(signer).await? else { return Ok(None) };
        let id = crypto::digest(packet.payload.as_bytes());
        // Success alone is insufficient: the vault must acknowledge this exact stored batch.
        let response = self.http.post(self.config.vault_url.as_ref().unwrap())
            .bearer_auth(&**self.token.as_ref().unwrap()).json(&packet).send().await?;
        ensure!(response.status().is_success(), "Audit vault rejected batch; retained for retry");
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? { ensure!(bytes.len() + chunk.len() <= 4096, "Oversized vault receipt"); bytes.extend_from_slice(&chunk); }
        let receipt: Value = serde_json::from_slice(&bytes)?;
        ensure!(receipt["stored_sha256"].as_str() == Some(&id) && receipt["immutable"].as_bool() == Some(true), "Vault did not confirm immutable storage of exact batch");
        db.acknowledge_batch(&id).await?;
        Ok(Some(id))
    }
}

pub fn until(days: u32, at: i64) -> i64 { if days == 0 { 0 } else { at + i64::from(days) * 86400 } }

impl Store {
    /// Bounded, archive-scoped activity. Verify the stored exact bytes before displaying them.
    /// Independent vault verification is still required for completeness and external custody.
    pub async fn archive_audit(&self, id: &str, before: Option<i64>) -> Result<Value> {
        let signer = self.signer.as_ref().context("Signed audit is disabled")?;
        // Actor lookup uses portable SQL on both PostgreSQL and SQLite.
        let rows = sqlx::query("SELECT seq,payload,hash,signature,exported FROM audit_records WHERE archive_id=$1 AND seq<$2 ORDER BY seq DESC LIMIT 11")
            .bind(id).bind(before.unwrap_or(i64::MAX)).fetch_all(&self.pool).await?;
        let more = rows.len() > 10;
        let mut events = Vec::new();
        for row in rows.iter().take(10) {
            let payload: String = row.get(1);
            ensure!(payload.len() <= 65536, "Oversized audit record");
            let signed = Signed { payload, signature: row.get(3) };
            verify(&signed, &signer.public_key)?;
            let record: Value = serde_json::from_str(&signed.payload)?;
            ensure!(record["archive"].as_str()==Some(id) && record["seq"]==row.get::<i64,_>(0)
                && row.get::<String,_>(2)==crypto::digest(signed.payload.as_bytes()), "Altered audit record");
            let actor_name = if let Some(actor) = record["actor"].as_str() {
                sqlx::query("SELECT name FROM users WHERE id=$1").bind(actor).fetch_optional(&self.pool).await?.map(|r|r.get::<String,_>(0))
            } else { None };
            events.push(json!({"record":record,"exported":row.get::<i64,_>(4)!=0,"actor_name":actor_name}));
        }
        let next = if more { events.last().and_then(|e|e["record"]["seq"].as_i64()) } else { None };
        Ok(json!({"events":events,"next_before":next}))
    }
    pub async fn configure_audit(&mut self, service: &Service) -> Result<()> {
        let saved = self.metadata("audit-identity-v1").await?;
        if let Some(signer) = &service.signer {
            let unsigned:i64=sqlx::query("SELECT COUNT(*) FROM audit_records WHERE signature=''").fetch_one(&self.pool).await?.get(0);
            ensure!(unsigned==0,"Unsigned ledger records exist: enable signing on first upgraded start, or migrate to a new externally approved stream; do not retroactively sign history");
            let identity = format!("{}:{}", signer.stream, signer.public_key);
            if let Some(saved) = saved { ensure!(saved == identity.as_bytes(), "Audit identity changed: restore configured signing identity; rotation requires a new externally approved stream"); }
            self.put_metadata("audit-identity-v1", identity.as_bytes()).await?;
            ensure!(self.metadata("audit-identity-v1").await?.as_deref() == Some(identity.as_bytes()), "Concurrent audit configuration mismatch");
            self.signer = Some(signer.clone());
        } else { ensure!(saved.is_none(), "Cannot disable previously enabled signed audit"); }
        self.evidence_retention_days = service.config.evidence_retention_days;
        self.archive_retention_days = service.config.archive_retention_days;
        // Snapshot deployment policy without credentials; configuration changes are evidence.
        self.audit_detail(None, None, "broker_started", json!({"audit":service.config})).await?;
        Ok(())
    }
    pub async fn audit_detail(&self, user: Option<&str>, archive: Option<&str>, event: &str, details: Value) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.audit_tx(&mut tx, user, archive, event, details).await?;
        tx.commit().await?; Ok(())
    }
    pub async fn audit_tx(&self, tx: &mut Transaction<'_, Any>, user: Option<&str>, archive: Option<&str>, event: &str, details: Value) -> Result<()> {
        // A singleton UPDATE serializes appenders on both PostgreSQL and SQLite.
        let head = sqlx::query("UPDATE audit_head SET seq=seq+1 WHERE id=1 RETURNING seq,hash")
            .fetch_one(&mut **tx).await?;
        let seq: i64 = head.get(0); let previous: String = head.get(1);
        let at = now();
        let payload = serde_json::to_string(&json!({"schema":1,"stream":self.signer.as_ref().map(|s|&s.stream),"seq":seq,"previous":previous,"at":at,"retain_until":until(self.evidence_retention_days,at),"actor":user,"archive":archive,"event":event,"details":details}))?;
        ensure!(payload.len() <= 65536, "Audit event too large");
        let hash = crypto::digest(payload.as_bytes());
        let signature = self.signer.as_ref().map(|s|s.sign(&payload)).unwrap_or_default();
        sqlx::query("INSERT INTO audit_records(seq,archive_id,at,retain_until,payload,hash,signature,exported) VALUES($1,$2,$3,$4,$5,$6,$7,0)")
            .bind(seq).bind(archive).bind(at).bind(until(self.evidence_retention_days, at)).bind(payload).bind(&hash).bind(signature).execute(&mut **tx).await?;
        sqlx::query("UPDATE audit_head SET hash=$1 WHERE id=1").bind(hash).execute(&mut **tx).await?;
        Ok(())
    }
    pub async fn audit_batch(&self, signer: &Signer) -> Result<Option<Signed>> {
        let mut tx = self.pool.begin().await?;
        let head = sqlx::query("UPDATE audit_head SET seq=seq WHERE id=1 RETURNING exported_seq,exported_hash,batch_hash,seq")
            .fetch_one(&mut *tx).await?;
        if let Some(row) = sqlx::query("SELECT packet FROM audit_pending WHERE id=1").fetch_optional(&mut *tx).await? {
            let packet: Signed = serde_json::from_str(&row.get::<String,_>(0))?;
            verify(&packet, &signer.public_key)?;
            tx.commit().await?; return Ok(Some(packet));
        }
        let cursor: i64 = head.get(0); let mut previous: String = head.get(1);
        let rows = sqlx::query("SELECT seq,payload,hash,signature FROM audit_records WHERE seq>$1 ORDER BY seq LIMIT 16").bind(cursor).fetch_all(&mut *tx).await?;
        if rows.is_empty() { ensure!(head.get::<i64,_>(3)==cursor,"Audit tail missing");tx.commit().await?; return Ok(None) }
        let mut events = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let seq: i64 = row.get(0); let payload: String = row.get(1); let hash: String = row.get(2); let signature: String = row.get(3);
            let value: Value = serde_json::from_str(&payload)?;
            ensure!(seq == cursor + i as i64 + 1 && value["seq"] == seq && value["previous"].as_str() == Some(&previous) && hash == crypto::digest(payload.as_bytes()), "Audit chain altered or incomplete");
            let event = Signed { payload, signature };
            verify(&event, &signer.public_key)?;
            previous = hash; events.push(event);
        }
        let payload = serde_json::to_string(&json!({"schema":1,"stream":signer.stream,"public_key":signer.public_key,"first":cursor+1,"last":cursor+rows.len() as i64,"previous_batch":head.get::<String,_>(2),"previous_event":head.get::<String,_>(1),"last_event":previous,"events":events}))?;
        let packet = Signed { signature: signer.sign(&payload), payload };
        sqlx::query("INSERT INTO audit_pending(id,packet) VALUES(1,$1)").bind(serde_json::to_string(&packet)?).execute(&mut *tx).await?;
        tx.commit().await?; Ok(Some(packet))
    }
    pub async fn acknowledge_batch(&self, digest: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE audit_head SET seq=seq WHERE id=1").execute(&mut *tx).await?;
        if let Some(row) = sqlx::query("SELECT packet FROM audit_pending WHERE id=1").fetch_optional(&mut *tx).await? {
            let packet: Signed = serde_json::from_str(&row.get::<String,_>(0))?;
            ensure!(crypto::digest(packet.payload.as_bytes()) == digest, "Receipt is for another batch");
            let value: Value = serde_json::from_str(&packet.payload)?;
            let last = value["last"].as_i64().context("Missing batch sequence")?;
            sqlx::query("UPDATE audit_records SET exported=1 WHERE seq<=$1 AND exported=0").bind(last).execute(&mut *tx).await?;
            sqlx::query("UPDATE audit_head SET exported_seq=$1,exported_hash=$2,batch_hash=$3 WHERE id=1").bind(last).bind(value["last_event"].as_str().context("Missing event hash")?).bind(digest).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM audit_pending WHERE id=1").execute(&mut *tx).await?;
        }
        tx.commit().await?; Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub async fn signed_store()->Store {
        let mut db=Store::connect(":memory:",false,false).await.unwrap();
        db.signer=Some(Arc::new(Signer::from_seed(&[7;32],"test-stream").unwrap())); db
    }
    #[tokio::test]
    async fn archive_activity_is_bounded_scoped_and_verified()->Result<()> {
        let db=signed_store().await;
        let a=crypto::identifier();let b=crypto::identifier();
        for i in 0..23 { db.audit_detail(None,Some(&a),"activity",json!({"index":i})).await?; }
        db.audit(None,Some(&b),"other archive").await?;
        let first=db.archive_audit(&a,None).await?;
        assert_eq!(first["events"].as_array().unwrap().len(),10);
        assert_eq!(first["events"][0]["record"]["seq"],23);
        let second=db.archive_audit(&a,first["next_before"].as_i64()).await?;
        assert_eq!(second["events"][0]["record"]["seq"],13);
        let last=db.archive_audit(&a,second["next_before"].as_i64()).await?;
        assert_eq!(last["events"].as_array().unwrap().len(),3);assert!(last["next_before"].is_null());
        assert!(first["events"].as_array().unwrap().iter().all(|e|e["record"]["archive"]==a));
        assert!(db.archive_audit(&crypto::identifier(),None).await?["events"].as_array().unwrap().is_empty());
        sqlx::query("UPDATE audit_records SET payload=payload || ' ' WHERE seq=23").execute(&db.pool).await?;
        assert!(db.archive_audit(&a,None).await.is_err());
        Ok(())
    }
    #[tokio::test]
    async fn signed_batches_replay_chain_and_tampering()->Result<()> {
        let db=signed_store().await;let signer=db.signer.as_ref().unwrap();
        db.audit(None,None,"one").await?;
        let packet=db.audit_batch(signer).await?.unwrap();verify(&packet,&signer.public_key)?;
        assert_eq!(db.audit_batch(signer).await?.unwrap().payload,packet.payload);
        assert!(db.acknowledge_batch("wrong receipt").await.is_err());
        db.acknowledge_batch(&crypto::digest(packet.payload.as_bytes())).await?;
        db.audit(None,None,"two").await?;
        let next=db.audit_batch(signer).await?.unwrap();
        let value:Value=serde_json::from_str(&next.payload)?;
        assert_eq!(value["previous_batch"],crypto::digest(packet.payload.as_bytes()));
        let altered=Signed{payload:format!("{} ",next.payload),signature:next.signature.clone()};
        assert!(verify(&altered,&signer.public_key).is_err());
        sqlx::query("DELETE FROM audit_pending").execute(&db.pool).await?;
        sqlx::query("UPDATE audit_records SET signature='' WHERE seq=2").execute(&db.pool).await?;
        assert!(db.audit_batch(signer).await.is_err());
        Ok(())
    }
    #[tokio::test]
    async fn transaction_failure_rolls_back_archive_and_event()->Result<()> {
        let db=signed_store().await;let user=db.enroll("issuer","owner","Owner","").await?;
        sqlx::query("CREATE TRIGGER no_audit BEFORE INSERT ON audit_records BEGIN SELECT RAISE(ABORT,'audit unavailable'); END").execute(&db.pool).await?;
        let id=crypto::identifier();
        assert!(db.create_archive(&id,&user.id,&[user.id.clone()],b"wrapped").await.is_err());
        assert!(sqlx::query("SELECT id FROM archives WHERE id=$1").bind(id).fetch_optional(&db.pool).await?.is_none());
        Ok(())
    }
    #[tokio::test]
    async fn export_failure_and_wrong_receipt_preserve_pending()->Result<()> {
        use axum::{routing::post,Router,Json};
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url=format!("http://{}",listener.local_addr()?);
        let server=tokio::spawn(async move { axum::serve(listener,Router::new().route("/",post(||async {Json(json!({"stored_sha256":"wrong","immutable":true}))}))).await.unwrap() });
        let db=signed_store().await; db.audit(None,None,"preserve").await?;
        let mut service=Service::new(Config::default())?;
        service.signer=db.signer.clone();service.config.vault_url=Some(url);service.token=Some(zeroize::Zeroizing::new("a".repeat(32)));
        assert!(service.dispatch(&db).await.is_err());
        let count:i64=sqlx::query("SELECT COUNT(*) FROM audit_records WHERE exported=0").fetch_one(&db.pool).await?.get(0);assert_eq!(count,1);
        assert!(db.audit_batch(service.signer.as_ref().unwrap()).await?.is_some());
        server.abort(); assert!(service.dispatch(&db).await.is_err()); Ok(())
    }
    #[tokio::test]
    async fn cannot_downgrade_audit_identity()->Result<()> {
        let mut db=Store::connect(":memory:",false,false).await?;
        let mut service=Service::new(Config::default())?;
        service.signer=Some(Arc::new(Signer::from_seed(&[7;32],"identity-test")?));
        db.configure_audit(&service).await?;
        assert!(db.configure_audit(&Service::new(Config::default())?).await.is_err());
        service.signer=Some(Arc::new(Signer::from_seed(&[8;32],"identity-test")?));
        assert!(db.configure_audit(&service).await.is_err());Ok(())
    }
}
