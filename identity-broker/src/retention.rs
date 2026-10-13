//! Broker-managed preservation. No operation can erase user-owned archive files or cloud master keys.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use crate::{crypto, now, store::Store};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, tag="action", rename_all="snake_case")]
pub enum Action {
    Extend { retain_until: i64, reason: String },
    AccessExpiry { expires: i64, reason: String },
    Hold { hold_id: String, reason: String },
    ReleaseHold { hold_id: String, reason: String },
    DestroyKey { reason: String },
    /// Attestation, not an automatic authorization override or a sharing approval workflow.
    Approval { reference: String, decision: String, reason: String },
}
impl Action {
    pub fn validate(&self) -> Result<()> {
        let reason = match self {
            Self::Extend { retain_until, reason } => { ensure!(*retain_until >= 0 && *retain_until <= now()+36500*86400, "Invalid retention date"); reason },
            Self::AccessExpiry { expires, reason } => { ensure!(*expires >= 0 && *expires <= now()+36500*86400, "Invalid access expiry"); reason },
            Self::Hold { hold_id, reason } | Self::ReleaseHold { hold_id, reason } => { ensure!(crypto::valid_id(hold_id), "Invalid hold ID"); reason },
            Self::DestroyKey { reason } => reason,
            Self::Approval { reference, decision, reason } => { ensure!(!reference.is_empty() && reference.len() <= 256 && matches!(decision.as_str(),"approved"|"denied"), "Invalid approval"); reason },
        };
        ensure!(!reason.trim().is_empty() && reason.len() <= 1024, "A bounded nonempty reason is required"); Ok(())
    }
}
impl Store {
    pub async fn lifecycle(&self, id:&str) -> Result<Value> {
        let row = sqlx::query("SELECT retain_until,access_expires,destroyed FROM archive_lifecycle WHERE archive_id=$1").bind(id).fetch_optional(&self.pool).await?.context("Unknown archive")?;
        let holds = sqlx::query("SELECT id,reason,actor,created FROM legal_holds WHERE archive_id=$1 AND released=0 ORDER BY id").bind(id).fetch_all(&self.pool).await?;
        Ok(json!({"archive_id":id,"retain_until":row.get::<i64,_>(0),"access_expires":row.get::<i64,_>(1),"destroyed":row.get::<i64,_>(2)!=0,"holds":holds.into_iter().map(|r|json!({"id":r.get::<String,_>(0),"reason":r.get::<String,_>(1),"actor":r.get::<String,_>(2),"created":r.get::<i64,_>(3)})).collect::<Vec<_>>()}))
    }
    pub async fn retention_action(&self, id:&str, actor:&str, action:&Action) -> Result<()> {
        action.validate()?;
        let mut tx = self.pool.begin().await?;
        // All retention, hold and destruction operations take the same row lock.
        let row = sqlx::query("UPDATE archive_lifecycle SET destroyed=destroyed WHERE archive_id=$1 RETURNING retain_until,destroyed")
            .bind(id).fetch_optional(&mut *tx).await?.context("Unknown archive")?;
        let retained:i64=row.get(0); ensure!(row.get::<i64,_>(1)==0,"Archive key already destroyed");
        let kind = match action {
            Action::Extend{retain_until,..} => {
                ensure!(*retain_until==0 || (retained!=0 && *retain_until>=retained), "Retention cannot be shortened, including indefinite retention");
                sqlx::query("UPDATE archive_lifecycle SET retain_until=$1 WHERE archive_id=$2").bind(retain_until).bind(id).execute(&mut *tx).await?;
                "retention_extended"
            },
            Action::AccessExpiry{expires,..} => {
                sqlx::query("UPDATE archive_lifecycle SET access_expires=$1 WHERE archive_id=$2").bind(expires).bind(id).execute(&mut *tx).await?;
                "access_expiry_changed"
            },
            Action::Hold{hold_id,reason} => {
                sqlx::query("INSERT INTO legal_holds(id,archive_id,reason,actor,created) VALUES($1,$2,$3,$4,$5)").bind(hold_id).bind(id).bind(reason).bind(actor).bind(now()).execute(&mut *tx).await?;
                "legal_hold_placed"
            },
            Action::ReleaseHold{hold_id,..} => {
                ensure!(sqlx::query("UPDATE legal_holds SET released=$1 WHERE id=$2 AND archive_id=$3 AND released=0").bind(now()).bind(hold_id).bind(id).execute(&mut *tx).await?.rows_affected()==1,"Active hold not found");
                "legal_hold_released"
            },
            Action::DestroyKey{..} => {
                ensure!(retained!=0 && retained<=now(),"Archive is still retained");
                let holds:i64=sqlx::query("SELECT COUNT(*) FROM legal_holds WHERE archive_id=$1 AND released=0").bind(id).fetch_one(&mut *tx).await?.get(0);
                ensure!(holds==0,"Legal hold prevents key destruction");
                // Logical destruction of this archive's wrapped data key, never a shared KMS key.
                sqlx::query("UPDATE archives SET wrapped=$1,revoked=1 WHERE id=$2").bind(Vec::<u8>::new()).bind(id).execute(&mut *tx).await?;
                sqlx::query("UPDATE archive_lifecycle SET destroyed=1 WHERE archive_id=$1").bind(id).execute(&mut *tx).await?;
                "archive_key_destroyed"
            },
            Action::Approval{..} => "approval_recorded",
        };
        self.audit_tx(&mut tx,Some(actor),Some(id),kind,serde_json::to_value(action)?).await?;
        tx.commit().await?; Ok(())
    }
    pub async fn change_recipients(&self,id:&str,owner:&str,recipients:&[String])->Result<()> {
        ensure!(!recipients.is_empty() && recipients.len()<=1000 && recipients.iter().all(|id|crypto::valid_id(id)),"Invalid recipients");
        let mut tx=self.pool.begin().await?;
        let row=sqlx::query("UPDATE archives SET recipients=recipients WHERE id=$1 AND owner=$2 AND revoked=0 RETURNING recipients").bind(id).bind(owner).fetch_optional(&mut *tx).await?.context("Archive unavailable")?;
        let before:String=row.get(0);
        for recipient in recipients {
            ensure!(sqlx::query("SELECT id FROM users WHERE id=$1 AND disabled=0").bind(recipient).fetch_optional(&mut *tx).await?.is_some(),"Inactive recipient");
        }
        sqlx::query("UPDATE archives SET recipients=$1 WHERE id=$2").bind(serde_json::to_string(recipients)?).bind(id).execute(&mut *tx).await?;
        self.audit_tx(&mut tx,Some(owner),Some(id),"recipients_changed",json!({"before":serde_json::from_str::<Value>(&before)?,"after":recipients})).await?;
        tx.commit().await?;Ok(())
    }
    pub async fn prune_evidence(&self)->Result<u64> {
        if self.signer.is_none() {return Ok(0)}
        // Per-archive locking makes cleanup serialize with hold placement/release.
        // Never prune broker-global audit records automatically; these can describe all archives.
        let mut tx=self.pool.begin().await?;
        let candidates=sqlx::query("SELECT l.archive_id FROM archive_lifecycle l WHERE l.retain_until>0 AND l.retain_until<=$1 AND NOT EXISTS (SELECT 1 FROM legal_holds h WHERE h.archive_id=l.archive_id AND h.released=0) AND (EXISTS (SELECT 1 FROM audit_records a WHERE a.archive_id=l.archive_id AND a.exported=1 AND a.retain_until>0 AND a.retain_until<=$1) OR EXISTS (SELECT 1 FROM tracking_events t JOIN tracking_retention r ON r.id=t.id WHERE t.archive_id=l.archive_id AND r.retain_until>0 AND r.retain_until<=$1 AND NOT EXISTS (SELECT 1 FROM tracking_outbox o WHERE o.id=t.id) AND NOT EXISTS (SELECT 1 FROM audit_records a WHERE a.archive_id=l.archive_id AND a.exported=0))) ORDER BY l.archive_id LIMIT 256").bind(now()).fetch_all(&mut *tx).await?;
        let mut deleted=0;
        for candidate in candidates {
            let archive=sqlx::query("UPDATE archive_lifecycle SET destroyed=destroyed WHERE archive_id=$1 RETURNING archive_id,retain_until").bind(candidate.get::<String,_>(0)).fetch_one(&mut *tx).await?;
            let id:String=archive.get(0); let retained:i64=archive.get(1);
            if retained==0 || retained>now() {continue}
            let holds:i64=sqlx::query("SELECT COUNT(*) FROM legal_holds WHERE archive_id=$1 AND released=0").bind(&id).fetch_one(&mut *tx).await?.get(0);
            if holds!=0 {continue}
            deleted+=sqlx::query("DELETE FROM audit_records WHERE archive_id=$1 AND exported=1 AND retain_until>0 AND retain_until<=$2").bind(&id).bind(now()).execute(&mut *tx).await?.rows_affected();
            // Tracking is only pruned when delivered AND covered by acknowledged audit evidence.
            // Keep it indefinitely when signed audit export is not enabled.
            if self.signer.is_some() {
                sqlx::query("DELETE FROM tracking_events WHERE archive_id=$1 AND id IN (SELECT id FROM tracking_retention WHERE retain_until>0 AND retain_until<=$2) AND id NOT IN (SELECT id FROM tracking_outbox) AND NOT EXISTS (SELECT 1 FROM audit_records WHERE archive_id=$1 AND exported=0)")
                    .bind(&id).bind(now()).execute(&mut *tx).await?;
            }
        }
        if deleted>0 { self.audit_tx(&mut tx,None,None,"local_evidence_pruned",json!({"records":deleted,"external_copy":"retained"})).await?; }
        tx.commit().await?;Ok(deleted)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Arc;
    pub async fn contract(db:&mut Store)->Result<()> {
        db.signer=Some(Arc::new(crate::audit::Signer::from_seed(&[9;32],"retention-test")?));
        db.archive_retention_days=1;db.evidence_retention_days=1;
        let owner=db.enroll("issuer",&crypto::identifier(),"Owner","").await?;
        let id=crypto::identifier();let who=&owner.id;
        db.create_archive(&id,who,&[who.clone()],b"wrapped").await?;
        let destroy=Action::DestroyKey{reason:"approved disposition".into()};
        assert!(db.retention_action(&id,who,&destroy).await.is_err());
        assert!(db.retention_action(&id,who,&Action::Extend{retain_until:now(),reason:"shorten".into()}).await.is_err());
        db.retention_action(&id,who,&Action::AccessExpiry{expires:now()-1,reason:"contract ended".into()}).await?;
        assert!(db.allowed_key(&id,who).await?.is_none());
        let hold=crypto::identifier();
        db.retention_action(&id,who,&Action::Hold{hold_id:hold.clone(),reason:"case-001".into()}).await?;
        sqlx::query("UPDATE archive_lifecycle SET retain_until=1 WHERE archive_id=$1").bind(&id).execute(&db.pool).await?;
        sqlx::query("UPDATE audit_records SET retain_until=1 WHERE archive_id=$1").bind(&id).execute(&db.pool).await?;
        assert!(db.retention_action(&id,who,&destroy).await.is_err());
        assert_eq!(db.prune_evidence().await?,0);
        let packet=db.audit_batch(db.signer.as_ref().unwrap()).await?.unwrap();
        db.acknowledge_batch(&crypto::digest(packet.payload.as_bytes())).await?;
        assert_eq!(db.prune_evidence().await?,0); // exported, expired, but held
        assert!(db.revoke(&id,who).await?); // holding evidence must not force access to remain enabled
        db.retention_action(&id,who,&Action::ReleaseHold{hold_id:hold,reason:"case closed".into()}).await?;
        assert!(db.prune_evidence().await?>0);
        db.retention_action(&id,who,&destroy).await?;
        assert!(db.allowed_key(&id,who).await?.is_none());
        assert!(db.retention_action(&id,who,&Action::Hold{hold_id:crypto::identifier(),reason:"too late".into()}).await.is_err());
        let wrapped:Vec<u8>=sqlx::query("SELECT wrapped FROM archives WHERE id=$1").bind(&id).fetch_one(&db.pool).await?.get(0);assert!(wrapped.is_empty());
        Ok(())
    }
    #[tokio::test]
    async fn retention_hold_expiry_and_disposition()->Result<()> {
        let mut db=Store::connect(":memory:",false,false).await?; contract(&mut db).await
    }
    #[tokio::test]
    async fn unsigned_unexported_and_indefinite_evidence_never_pruned()->Result<()> {
        let db=Store::connect(":memory:",false,false).await?;
        let user=db.enroll("issuer","owner","Owner","").await?;let id=crypto::identifier();
        db.create_archive(&id,&user.id,&[user.id.clone()],b"wrapped").await?;
        assert!(db.retention_action(&id,&user.id,&Action::Extend{retain_until:now()+86400,reason:"shorten infinity".into()}).await.is_err());
        sqlx::query("UPDATE archive_lifecycle SET retain_until=1").execute(&db.pool).await?;
        sqlx::query("UPDATE audit_records SET retain_until=1").execute(&db.pool).await?;
        assert_eq!(db.prune_evidence().await?,0);Ok(())
    }
}
