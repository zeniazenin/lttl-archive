//! Shared durable state; DELETE ... RETURNING makes grants single-use across instances.
use crate::{User, crypto, now};
use anyhow::{Context, Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use sqlx::{
    AnyPool, ConnectOptions, Row,
    any::{AnyConnectOptions, AnyPoolOptions},
};
use std::{str::FromStr, time::Duration};

pub struct Store {
    pub pool: AnyPool,
    pub signer: Option<std::sync::Arc<crate::audit::Signer>>,
    pub evidence_retention_days: u32,
    pub archive_retention_days: u32,
}
const SCHEMA:&str="
CREATE TABLE IF NOT EXISTS users(id TEXT PRIMARY KEY,issuer TEXT NOT NULL,subject TEXT NOT NULL,name TEXT NOT NULL,email TEXT NOT NULL,disabled BIGINT NOT NULL DEFAULT 0,UNIQUE(issuer,subject));
CREATE TABLE IF NOT EXISTS archives(id TEXT PRIMARY KEY,owner TEXT NOT NULL REFERENCES users(id),wrapped BYTEA NOT NULL,recipients TEXT NOT NULL,revoked BIGINT NOT NULL DEFAULT 0,created BIGINT NOT NULL);
CREATE TABLE IF NOT EXISTS audit(at BIGINT NOT NULL,user_id TEXT,archive_id TEXT,event TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS audit_head(id BIGINT PRIMARY KEY,seq BIGINT NOT NULL,hash TEXT NOT NULL,exported_seq BIGINT NOT NULL,exported_hash TEXT NOT NULL,batch_hash TEXT NOT NULL);
INSERT INTO audit_head VALUES(1,0,'',0,'','') ON CONFLICT(id) DO NOTHING;
CREATE TABLE IF NOT EXISTS audit_records(seq BIGINT PRIMARY KEY,archive_id TEXT,at BIGINT NOT NULL,retain_until BIGINT NOT NULL,payload TEXT NOT NULL,hash TEXT NOT NULL,signature TEXT NOT NULL,exported BIGINT NOT NULL);
CREATE INDEX IF NOT EXISTS audit_archive ON audit_records(archive_id,seq);
CREATE TABLE IF NOT EXISTS audit_pending(id BIGINT PRIMARY KEY,packet TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS archive_lifecycle(archive_id TEXT PRIMARY KEY REFERENCES archives(id),retain_until BIGINT NOT NULL DEFAULT 0,access_expires BIGINT NOT NULL DEFAULT 0,destroyed BIGINT NOT NULL DEFAULT 0);
INSERT INTO archive_lifecycle(archive_id) SELECT id FROM archives WHERE id NOT IN (SELECT archive_id FROM archive_lifecycle);
CREATE TABLE IF NOT EXISTS legal_holds(id TEXT PRIMARY KEY,archive_id TEXT NOT NULL REFERENCES archives(id),reason TEXT NOT NULL,actor TEXT NOT NULL,created BIGINT NOT NULL,released BIGINT NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS active_holds ON legal_holds(archive_id,released);
CREATE TABLE IF NOT EXISTS broker_metadata(name TEXT PRIMARY KEY,value BYTEA NOT NULL);
CREATE TABLE IF NOT EXISTS login_flows(id TEXT PRIMARY KEY,payload TEXT NOT NULL,expires BIGINT NOT NULL,launched BIGINT NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS auth_codes(id TEXT PRIMARY KEY,payload TEXT NOT NULL,expires BIGINT NOT NULL);
CREATE TABLE IF NOT EXISTS sessions(id TEXT PRIMARY KEY,payload TEXT NOT NULL,expires BIGINT NOT NULL);
CREATE TABLE IF NOT EXISTS archive_tracking(archive_id TEXT PRIMARY KEY REFERENCES archives(id),policy TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS tracking_events(id TEXT PRIMARY KEY,archive_id TEXT NOT NULL REFERENCES archives(id),at BIGINT NOT NULL,payload TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS tracking_by_archive ON tracking_events(archive_id,at);
CREATE INDEX IF NOT EXISTS tracking_expiry ON tracking_events(at);
CREATE TABLE IF NOT EXISTS tracking_retention(id TEXT PRIMARY KEY REFERENCES tracking_events(id) ON DELETE CASCADE,retain_until BIGINT NOT NULL);
INSERT INTO tracking_retention(id,retain_until) SELECT id,0 FROM tracking_events WHERE id NOT IN (SELECT id FROM tracking_retention);
CREATE TABLE IF NOT EXISTS tracking_outbox(id TEXT PRIMARY KEY REFERENCES tracking_events(id) ON DELETE CASCADE,next_at BIGINT NOT NULL,attempts BIGINT NOT NULL,lease TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS tracking_due ON tracking_outbox(next_at);
CREATE INDEX IF NOT EXISTS flows_expiry ON login_flows(expires);
CREATE INDEX IF NOT EXISTS codes_expiry ON auth_codes(expires);
CREATE INDEX IF NOT EXISTS sessions_expiry ON sessions(expires);
";
fn validate_postgres_url(value: &str) -> Result<()> {
    let url = url::Url::parse(value)?;
    ensure!(
        matches!(url.scheme(), "postgres" | "postgresql") && url.fragment().is_none(),
        "Expected a PostgreSQL URL"
    );
    ensure!(
        url.host_str()
            .is_some_and(|h| !h.contains(['%', '/', '\\']))
            && !url.username().is_empty()
            && url.path().len() > 1,
        "PostgreSQL requires an explicit TCP host, user and database"
    );
    let mut seen = std::collections::HashSet::new();
    let mut verified = false;
    for (key, value) in url.query_pairs() {
        ensure!(
            matches!(
                &*key,
                "sslmode"
                    | "sslrootcert"
                    | "sslcert"
                    | "sslkey"
                    | "application_name"
                    | "statement-cache-capacity"
            ) && seen.insert(key.to_string()),
            "Unsupported or duplicate PostgreSQL connection parameter"
        );
        if key == "sslmode" {
            ensure!(
                value == "verify-full",
                "PostgreSQL requires sslmode=verify-full (and a trusted CA)"
            );
            verified = true;
        }
    }
    ensure!(
        verified,
        "PostgreSQL requires sslmode=verify-full (and a trusted CA)"
    );
    Ok(())
}
impl Store {
    pub async fn connect(database: &str, serverless: bool, initialize: bool) -> Result<Self> {
        sqlx::any::install_default_drivers();
        let postgres = database == "postgres";
        ensure!(
            !serverless || postgres,
            "Serverless requires shared PostgreSQL; SQLite is not durable across functions"
        );
        let url = if postgres {
            std::env::var("LTTL_DATABASE_URL")
                .context("Set LTTL_DATABASE_URL from the platform secret store")?
        } else if database == ":memory:" {
            "sqlite::memory:".into()
        } else {
            let path = std::path::absolute(database)?;
            let encoded = path
                .to_string_lossy()
                .replace('\\', "/")
                .replace('%', "%25")
                .replace('?', "%3F")
                .replace('#', "%23");
            format!("sqlite:{encoded}?mode=rwc")
        };
        if postgres {
            validate_postgres_url(&url)?;
        }
        let options = AnyConnectOptions::from_str(&url)?.disable_statement_logging();
        let pool = AnyPoolOptions::new()
            .max_connections(if postgres { 2 } else { 1 })
            .acquire_timeout(Duration::from_secs(10))
            .connect_with(options)
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Database connection failed; check the secret, TLS trust and network"
                )
            })?;
        if !postgres {
            sqlx::query("PRAGMA foreign_keys=ON").execute(&pool).await?;
            sqlx::query("PRAGMA busy_timeout=5000")
                .execute(&pool)
                .await?;
        }
        if initialize || !postgres {
            let schema = if postgres {
                SCHEMA.to_string()
            } else {
                SCHEMA.replace("BYTEA", "BLOB")
            };
            for statement in schema.split(';').filter(|s| !s.trim().is_empty()) {
                sqlx::query(statement).execute(&pool).await?;
            }
        }
        Ok(Self { pool, signer:None, evidence_retention_days:0, archive_retention_days:0 })
    }
    pub async fn audit(
        &self,
        user: Option<&str>,
        archive: Option<&str>,
        event: &str,
    ) -> Result<()> {
        self.audit_detail(user,archive,event,serde_json::json!({})).await
    }
    pub async fn metadata(&self, name: &str) -> Result<Option<Vec<u8>>> {
        Ok(
            sqlx::query("SELECT value FROM broker_metadata WHERE name=$1")
                .bind(name)
                .fetch_optional(&self.pool)
                .await?
                .map(|r| r.get(0)),
        )
    }
    pub async fn put_metadata(&self, name: &str, bytes: &[u8]) -> Result<()> {
        sqlx::query(
            "INSERT INTO broker_metadata(name,value) VALUES($1,$2) ON CONFLICT(name) DO NOTHING",
        )
        .bind(name)
        .bind(bytes)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    pub async fn first_archive(&self) -> Result<Option<(String, Vec<u8>)>> {
        Ok(sqlx::query("SELECT a.id,a.wrapped FROM archives a JOIN archive_lifecycle l ON l.archive_id=a.id WHERE l.destroyed=0 LIMIT 1")
            .fetch_optional(&self.pool)
            .await?
            .map(|r| (r.get(0), r.get(1))))
    }
    pub async fn active(&self, id: &str) -> Result<bool> {
        Ok(sqlx::query("SELECT disabled FROM users WHERE id=$1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .is_some_and(|r| r.get::<i64, _>(0) == 0))
    }
    pub async fn enroll(
        &self,
        issuer: &str,
        subject: &str,
        name: &str,
        email: &str,
    ) -> Result<User> {
        let row=sqlx::query("INSERT INTO users(id,issuer,subject,name,email) VALUES($1,$2,$3,$4,$5) ON CONFLICT(issuer,subject) DO UPDATE SET name=excluded.name,email=excluded.email RETURNING id,disabled")
            .bind(crypto::identifier()).bind(issuer).bind(subject).bind(name).bind(email).fetch_one(&self.pool).await?;
        ensure!(row.get::<i64, _>(1) == 0, "Account disabled");
        let id: String = row.get(0);
        self.audit(Some(&id), None, "sign-in").await?;
        Ok(User {
            id,
            name: name.into(),
            email: email.into(),
        })
    }
    pub async fn users(&self) -> Result<Vec<User>> {
        let rows = sqlx::query(
            "SELECT id,name,email FROM users WHERE disabled=0 ORDER BY name,id LIMIT 1001",
        )
        .fetch_all(&self.pool)
        .await?;
        ensure!(rows.len() <= 1000, "Directory exceeds 1000-user limit");
        Ok(rows
            .into_iter()
            .map(|r| User {
                id: r.get(0),
                name: r.get(1),
                email: r.get(2),
            })
            .collect())
    }
    #[cfg(test)]
    pub async fn create_archive(
        &self,
        id: &str,
        owner: &str,
        recipients: &[String],
        wrapped: &[u8],
    ) -> Result<()> {
        self.create_archive_tracked(id,owner,recipients,wrapped,&crypto::TrackingPolicy::default()).await
    }
    pub async fn create_archive_tracked(&self,id:&str,owner:&str,recipients:&[String],wrapped:&[u8],policy:&crypto::TrackingPolicy)->Result<()> {
        policy.validate()?;
        let mut tx = self.pool.begin().await?;
        // Recheck recipient state after the remote wrapping call.
        for recipient in recipients {
            let row = sqlx::query("SELECT disabled FROM users WHERE id=$1")
                .bind(recipient)
                .fetch_optional(&mut *tx)
                .await?;
            ensure!(
                row.is_some_and(|r| r.get::<i64, _>(0) == 0),
                "Recipient is not active"
            );
        }
        sqlx::query(
            "INSERT INTO archives(id,owner,wrapped,recipients,created) VALUES($1,$2,$3,$4,$5)",
        )
        .bind(id)
        .bind(owner)
        .bind(wrapped)
        .bind(serde_json::to_string(recipients)?)
        .bind(now())
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO archive_tracking(archive_id,policy) VALUES($1,$2)").bind(id).bind(serde_json::to_string(policy)?).execute(&mut *tx).await?;
        let retained = crate::audit::until(self.archive_retention_days, now());
        sqlx::query("INSERT INTO archive_lifecycle(archive_id,retain_until) VALUES($1,$2)").bind(id).bind(retained).execute(&mut *tx).await?;
        self.audit_tx(&mut tx,Some(owner),Some(id),"archive_created",serde_json::json!({"recipients":recipients,"tracking":policy,"retain_until":retained})).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn allowed_key(&self, id: &str, user: &str) -> Result<Option<Vec<u8>>> {
        let row=sqlx::query("SELECT a.wrapped,a.recipients,a.revoked,u.disabled FROM archives a JOIN users u ON u.id=$2 JOIN archive_lifecycle l ON l.archive_id=a.id WHERE a.id=$1 AND l.destroyed=0 AND (l.access_expires=0 OR l.access_expires>$3)").bind(id).bind(user).bind(now()).fetch_optional(&self.pool).await?;
        if let Some(r) = row {
            let recipients: String = r.get(1);
            if r.get::<i64, _>(2) == 0
                && r.get::<i64, _>(3) == 0
                && serde_json::from_str::<Vec<String>>(&recipients)?
                    .iter()
                    .any(|u| u == user)
            {
                return Ok(Some(r.get(0)));
            }
        }
        Ok(None)
    }
    pub async fn revoke(&self, id: &str, owner: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let changed = sqlx::query("UPDATE archives SET revoked=1 WHERE id=$1 AND owner=$2")
            .bind(id)
            .bind(owner)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if changed == 1 {
            self.audit_tx(&mut tx,Some(owner),Some(id),"archive_revoked",serde_json::json!({})).await?;
        }
        tx.commit().await?;
        Ok(changed == 1)
    }
    pub async fn prune(&self) -> Result<()> {
        for table in ["login_flows", "auth_codes", "sessions"] {
            sqlx::query(&format!("DELETE FROM {table} WHERE expires<=$1"))
                .bind(now())
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }
    pub async fn below_limits(&self) -> Result<bool> {
        for (table, max) in [
            ("login_flows", 256),
            ("auth_codes", 256),
            ("sessions", 10000),
        ] {
            let count: i64 = sqlx::query(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&self.pool)
                .await?
                .get(0);
            if count >= max {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub async fn put<T: Serialize>(
        &self,
        table: &str,
        id: &str,
        payload: &T,
        expires: i64,
    ) -> Result<()> {
        ensure!(
            matches!(table, "login_flows" | "auth_codes" | "sessions"),
            "Invalid state table"
        );
        sqlx::query(&format!(
            "INSERT INTO {table}(id,payload,expires) VALUES($1,$2,$3)"
        ))
        .bind(crypto::digest(id.as_bytes()))
        .bind(serde_json::to_string(payload)?)
        .bind(expires)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    pub async fn state<T: DeserializeOwned>(
        &self,
        table: &str,
        id: &str,
        consume: bool,
    ) -> Result<Option<T>> {
        ensure!(
            matches!(table, "login_flows" | "auth_codes" | "sessions"),
            "Invalid state table"
        );
        let launched = if table == "login_flows" {
            " AND launched=1"
        } else {
            ""
        };
        let sql = if consume {
            format!("DELETE FROM {table} WHERE id=$1 AND expires>$2{launched} RETURNING payload")
        } else {
            format!("SELECT payload FROM {table} WHERE id=$1 AND expires>$2{launched}")
        };
        let row = sqlx::query(&sql)
            .bind(crypto::digest(id.as_bytes()))
            .bind(now())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| serde_json::from_str(&r.get::<String, _>(0)).map_err(Into::into))
            .transpose()
    }
    pub async fn launch<T: DeserializeOwned>(&self, id: &str) -> Result<Option<T>> {
        let row=sqlx::query("UPDATE login_flows SET launched=1 WHERE id=$1 AND expires>$2 AND launched=0 RETURNING payload").bind(crypto::digest(id.as_bytes())).bind(now()).fetch_optional(&self.pool).await?;
        row.map(|r| serde_json::from_str(&r.get::<String, _>(0)).map_err(Into::into))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn exercise(a: &Store, b: &Store) {
        let flow = crypto::identifier();
        a.put("login_flows", &flow, &"flow", now() + 60)
            .await
            .unwrap();
        assert!(
            b.state::<String>("login_flows", &flow, true)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            b.launch::<String>(&flow).await.unwrap().as_deref(),
            Some("flow")
        );
        assert!(a.launch::<String>(&flow).await.unwrap().is_none());
        assert!(
            a.state::<String>("login_flows", &flow, true)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            b.state::<String>("login_flows", &flow, true)
                .await
                .unwrap()
                .is_none()
        );
        for table in ["auth_codes", "sessions"] {
            let id = crypto::identifier();
            a.put(table, &id, &"one-use", now() + 60).await.unwrap();
            let (x, y) = tokio::join!(
                a.state::<String>(table, &id, true),
                b.state::<String>(table, &id, true)
            );
            assert_eq!(
                usize::from(x.unwrap().is_some()) + usize::from(y.unwrap().is_some()),
                1
            );
            let id = crypto::identifier();
            a.put(table, &id, &"expired", now() - 1).await.unwrap();
            assert!(
                b.state::<String>(table, &id, false)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        let subject = crypto::identifier();
        let owner = a
            .enroll("issuer", &subject, "Owner", "same@example.com")
            .await
            .unwrap();
        let recipient = b
            .enroll(
                "issuer",
                &crypto::identifier(),
                "Recipient",
                "same@example.com",
            )
            .await
            .unwrap();
        assert_ne!(owner.id, recipient.id);
        assert_eq!(
            a.enroll("issuer", &subject, "Renamed", "new@example.com")
                .await
                .unwrap()
                .id,
            owner.id
        );
        let id = crypto::identifier();
        a.create_archive_tracked(&id, &owner.id, &[recipient.id.clone()], b"wrapped", &crypto::TrackingPolicy{enabled:true,alert_countries:vec!["KP".into()]})
            .await
            .unwrap();
        let tracking=crate::tracking::Service::new(crate::tracking::Config::default()).unwrap();
        tracking.record(a,&id,Some(&recipient.id),"key_denied",&crate::tracking::Location::default()).await.unwrap();
        assert!(b.tracking_policy(&id).await.unwrap().unwrap().enabled);
        assert_eq!(b.tracking_events(&id,&owner.id).await.unwrap().unwrap().len(),1);
        assert!(b.tracking_events(&id,&recipient.id).await.unwrap().is_none());
        let row=sqlx::query("UPDATE tracking_outbox SET lease=$1,next_at=$2,attempts=attempts+1 WHERE id=(SELECT id FROM tracking_outbox WHERE next_at<=$3 ORDER BY next_at,id LIMIT 1) AND next_at<=$3 RETURNING id,attempts")
            .bind(crypto::identifier()).bind(now()+60).bind(now()).fetch_optional(&b.pool).await.unwrap();
        assert!(row.is_some());
        assert!(b.allowed_key(&id, &owner.id).await.unwrap().is_none());
        assert_eq!(
            b.allowed_key(&id, &recipient.id).await.unwrap().unwrap(),
            b"wrapped"
        );
        assert!(!b.revoke(&id, &recipient.id).await.unwrap());
        assert!(a.revoke(&id, &owner.id).await.unwrap());
        assert!(b.allowed_key(&id, &recipient.id).await.unwrap().is_none());
        sqlx::query("UPDATE users SET disabled=1 WHERE id=$1")
            .bind(&recipient.id)
            .execute(&a.pool)
            .await
            .unwrap();
        assert!(!b.active(&recipient.id).await.unwrap());
        assert!(
            a.create_archive(
                &crypto::identifier(),
                &owner.id,
                &[recipient.id],
                b"wrapped"
            )
            .await
            .is_err()
        );
        b.prune().await.unwrap();
        assert!(a.below_limits().await.unwrap());
        assert!(a.users().await.unwrap().iter().any(|u| u.id == owner.id));
    }
    #[tokio::test]
    async fn durable_across_restart_and_atomic_across_instances() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("state.sqlite");
        let path = file.to_str().unwrap();
        let a = Store::connect(path, false, false).await.unwrap();
        let token = crypto::identifier();
        a.put("sessions", &token, &"survives", now() + 60)
            .await
            .unwrap();
        a.pool.close().await;
        let a = Store::connect(path, false, false).await.unwrap();
        let b = Store::connect(path, false, false).await.unwrap();
        assert_eq!(
            b.state::<String>("sessions", &token, true)
                .await
                .unwrap()
                .as_deref(),
            Some("survives")
        );
        exercise(&a, &b).await;
    }
    #[tokio::test]
    async fn serverless_rejects_ephemeral_storage() {
        assert!(Store::connect(":memory:", true, false).await.is_err());
    }
    #[test]
    fn postgres_tls_cannot_be_overridden_by_duplicate_alias_or_socket() {
        assert!(
            validate_postgres_url(
                "postgresql://test:secret@db.example/lttl?sslmode=verify-full&sslrootcert=/ca.pem"
            )
            .is_ok()
        );
        for value in [
            "postgresql://test@db/lttl",
            "postgresql://test@db/lttl?sslmode=require",
            "postgresql://test@db/lttl?sslmode=verify-full&sslmode=disable",
            "postgresql://test@db/lttl?sslmode=verify-full&ssl-mode=disable",
            "postgresql://test@db/lttl?sslmode=verify-full&host=/tmp/socket",
            "postgresql://test@%2Ftmp/lttl?sslmode=verify-full",
        ] {
            assert!(validate_postgres_url(value).is_err());
        }
    }
    #[tokio::test]
    #[ignore = "Requires disposable PostgreSQL with TLS in LTTL_DATABASE_URL; initializes schema"]
    async fn postgres_shared_state_contract() {
        let mut a = Store::connect("postgres", true, true).await.unwrap();
        let mut b = Store::connect("postgres", true, false).await.unwrap();
        let signer=std::sync::Arc::new(crate::audit::Signer::from_seed(&[9;32],"retention-test").unwrap());
        a.signer=Some(signer.clone());b.signer=Some(signer);
        exercise(&a, &b).await;
        crate::retention::tests::contract(&mut a).await.unwrap();
    }
}
