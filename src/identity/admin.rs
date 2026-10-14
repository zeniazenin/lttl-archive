//! Typed administration models shared by the desktop and authenticated broker client.
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;
use super::{Profile, broker_url, crypto};

#[derive(Clone, Debug, PartialEq)]
pub struct Target { pub archive_id: String, pub broker: String, pub path: String }
impl Target {
    pub fn read(path: &str, profile: &Profile) -> Result<Self> {
        let (header, _) = crypto::read_header(&mut crate::input_file(Path::new(path))?)?;
        let broker = broker_url(&profile.broker_url)?;
        ensure!(broker_url(&header.broker)? == broker, "Archive belongs to a different key service. Configure its trusted service in Identity & sharing before signing in.");
        Ok(Self { archive_id: header.archive_id, broker, path: path.into() })
    }
    pub fn matches(&self, path: &str, profile: &Profile) -> bool {
        self.path == path && broker_url(&profile.broker_url).is_ok_and(|url|url == self.broker)
    }
}

#[derive(Clone, Deserialize)]
pub struct Lifecycle {
    pub archive_id: String, pub retain_until: i64, pub access_expires: i64,
    pub destroyed: bool, pub holds: Vec<Hold>,
}
#[derive(Clone, Deserialize)]
pub struct Hold { pub id: String, pub reason: String, pub actor: String, pub created: i64 }
#[derive(Clone, Deserialize)]
pub struct Activity { pub events: Vec<ActivityEvent>, pub next_before: Option<i64> }
#[derive(Clone, Deserialize)]
pub struct ActivityEvent { pub record: Record, pub exported: bool, pub actor_name: Option<String> }
#[derive(Clone, Deserialize)]
pub struct Record {
    pub seq: i64, pub at: i64, pub archive: String, pub actor: Option<String>,
    pub event: String, pub details: Value,
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}
pub fn date_text(seconds: i64) -> String {
    DateTime::<Utc>::from_timestamp(seconds, 0).map(|d|d.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_else(||"Invalid date".into())
}
pub fn deadline(seconds: i64, unlimited: &str) -> String {
    if seconds == 0 { unlimited.into() } else { format!("{} UTC", date_text(seconds)) }
}
pub fn parse_date(text: &str) -> Result<i64> {
    let date = NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%d %H:%M:%S")
        .or_else(|_|NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%d %H:%M"))
        .context("Enter a UTC date as YYYY-MM-DD HH:MM (optional :SS)")?;
    let timestamp = date.and_utc().timestamp();
    ensure!(timestamp > 0 && timestamp <= now() + 36500 * 86400, "Use a date after 1970 and within 100 years. Use the checkbox for no deadline.");
    Ok(timestamp)
}

#[derive(Clone, Copy, PartialEq)]
pub enum Change { Retention, AccessExpiry, PlaceHold, ReleaseHold }
impl Change {
    pub fn label(self) -> &'static str {
        match self { Self::Retention=>"Extend retention", Self::AccessExpiry=>"Set access expiry", Self::PlaceHold=>"Place legal hold", Self::ReleaseHold=>"Release legal hold" }
    }
}
pub fn action(change: Change, state: &Lifecycle, date: &str, unlimited: bool, hold_id: &str, reason: &str) -> Result<Value> {
    ensure!(!state.destroyed, "The archive key has been destroyed; settings cannot be changed.");
    ensure!(!reason.trim().is_empty() && reason.len() <= 1024, "Enter a reason (at most 1024 UTF-8 bytes).");
    let reason = reason.trim();
    Ok(match change {
        Change::Retention => {
            let until = if unlimited { 0 } else { parse_date(date)? };
            ensure!(until == 0 || (state.retain_until != 0 && until >= state.retain_until), "Retention can only be extended. Indefinite retention cannot be shortened.");
            json!({"action":"extend","retain_until":until,"reason":reason})
        }
        Change::AccessExpiry => json!({"action":"access_expiry","expires":if unlimited {0} else {parse_date(date)?},"reason":reason}),
        Change::PlaceHold => json!({"action":"hold","hold_id":crypto::identifier(),"reason":reason}),
        Change::ReleaseHold => {
            ensure!(state.holds.iter().any(|h|h.id==hold_id), "Select an active legal hold.");
            json!({"action":"release_hold","hold_id":hold_id,"reason":reason})
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> Lifecycle { Lifecycle {archive_id:crypto::identifier(),retain_until:now()+86400,access_expires:0,destroyed:false,holds:vec![]} }
    #[test]
    fn dates_and_preservation_rules() {
        assert!(parse_date("2027-02-29 10:00").is_err());
        assert!(parse_date("2028-02-29 25:00").is_err());
        assert!(parse_date("1970-01-01 00:00").is_err());
        let date="2028-02-29 10:24:35";
        assert_eq!(date_text(parse_date(date).unwrap()),date);
        let mut s=state();
        assert!(action(Change::Retention,&s,&date_text(now()),false,"","shorten").is_err());
        assert_eq!(action(Change::Retention,&s,"",true,"","preserve").unwrap()["retain_until"],0);
        s.retain_until=0;
        assert!(action(Change::Retention,&s,date,false,"","shorten indefinite").is_err());
        assert!(action(Change::AccessExpiry,&s,&date_text(now()-60),false,"","end access").is_ok());
        assert!(action(Change::PlaceHold,&s,"",false,"","  ").is_err());
        assert!(action(Change::PlaceHold,&s,"",false,"",&"é".repeat(513)).is_err());
        assert!(action(Change::ReleaseHold,&s,"",false,"missing","release").is_err());
        let hold=action(Change::PlaceHold,&s,"",false,"","case").unwrap();
        let id=hold["hold_id"].as_str().unwrap();assert!(crypto::valid_id(id));
        s.holds.push(Hold{id:id.into(),reason:"case".into(),actor:"admin".into(),created:now()});
        assert_eq!(action(Change::ReleaseHold,&s,"",false,id,"closed").unwrap()["hold_id"],id);
        s.destroyed=true;
        assert!(action(Change::PlaceHold,&s,"",false,"","case").is_err());
    }
    #[test]
    fn untrusted_archive_cannot_redirect_administration() -> Result<()> {
        use std::io::Write;
        let mut file=tempfile::NamedTempFile::new()?;
        let header=crypto::Header::new("https://untrusted.example".into(),crypto::identifier(),"zip".into());
        let bytes=serde_json::to_vec(&header)?;
        file.write_all(crypto::MAGIC)?;file.write_all(&(bytes.len() as u32).to_le_bytes())?;file.write_all(&bytes)?;
        let profile=Profile{broker_url:"https://trusted.example".into(),..Default::default()};
        assert!(Target::read(file.path().to_str().unwrap(),&profile).is_err());
        let target=Target{archive_id:header.archive_id,broker:profile.broker_url.clone(),path:"one.lttl".into()};
        assert!(target.matches("one.lttl",&profile));assert!(!target.matches("two.lttl",&profile));
        assert!(!target.matches("one.lttl",&Profile{broker_url:header.broker,..Default::default()}));Ok(())
    }
}
