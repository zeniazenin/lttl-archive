//! Native cloud wrapping. Only archive data keys enter this process.
use crate::crypto;
use anyhow::{Context, Result, ensure};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Auth {
    #[default]
    Workload,
    Environment,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
pub enum Config {
    #[default]
    Local,
    Azure {
        key_id: String,
        #[serde(default)]
        decrypt_key_ids: Vec<String>,
        #[serde(default)]
        auth: Auth,
        #[serde(default)]
        client_id: Option<String>,
    },
    Aws {
        key_id: String,
        region: String,
        #[serde(default)]
        decrypt_key_ids: Vec<String>,
    },
    Gcp {
        key_id: String,
        #[serde(default)]
        decrypt_key_ids: Vec<String>,
        #[serde(default)]
        auth: Auth,
    },
}
impl Config {
    pub fn provider(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Azure { .. } => "azure",
            Self::Aws { .. } => "aws",
            Self::Gcp { .. } => "gcp",
        }
    }
    fn key_id(&self) -> &str {
        match self {
            Self::Local => "",
            Self::Azure { key_id, .. } | Self::Aws { key_id, .. } | Self::Gcp { key_id, .. } => {
                key_id
            }
        }
    }
    fn allowed(&self, id: &str) -> bool {
        let previous = match self {
            Self::Local => return false,
            Self::Azure {
                decrypt_key_ids, ..
            }
            | Self::Aws {
                decrypt_key_ids, ..
            }
            | Self::Gcp {
                decrypt_key_ids, ..
            } => decrypt_key_ids,
        };
        id == self.key_id() || previous.iter().any(|v| v == id)
    }
    pub fn validate(&self) -> Result<()> {
        if matches!(self, Self::Local) {
            return Ok(());
        }
        let previous = match self {
            Self::Azure {
                decrypt_key_ids, ..
            }
            | Self::Aws {
                decrypt_key_ids, ..
            }
            | Self::Gcp {
                decrypt_key_ids, ..
            } => decrypt_key_ids,
            _ => unreachable!(),
        };
        ensure!(previous.len() <= 32, "Too many old wrapping keys");
        for id in std::iter::once(self.key_id()).chain(previous.iter().map(String::as_str)) {
            ensure!(id.len() <= 1024, "Invalid KMS key reference");
            match self {
                Self::Azure { client_id, .. } => {
                    let u = url::Url::parse(id)?;
                    let p: u32 = u.port_or_known_default().unwrap_or(0).into();
                    let host = u.host_str().unwrap_or("");
                    let name = host.strip_suffix(".vault.azure.net").unwrap_or("");
                    let parts = u.path().split('/').collect::<Vec<_>>();
                    ensure!(
                        u.scheme() == "https"
                            && p == 443
                            && u.port().is_none()
                            && u.username().is_empty()
                            && u.password().is_none()
                            && u.query().is_none()
                            && u.fragment().is_none()
                            && (3..=24).contains(&name.len())
                            && segment(name)
                            && parts.len() == 4
                            && parts[1] == "keys"
                            && segment(parts[2])
                            && parts[3].len() == 32
                            && parts[3].bytes().all(|b| b.is_ascii_hexdigit()),
                        "Azure requires a versioned public-cloud Key Vault key URL"
                    );
                    if let Some(id) = client_id {
                        ensure!(
                            id.len() == 36
                                && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'),
                            "Invalid managed identity client ID"
                        );
                    }
                }
                Self::Aws { region, .. } => {
                    let parts = id.split(':').collect::<Vec<_>>();
                    ensure!(
                        segment(region)
                            && region.len() < 32
                            && parts.len() == 6
                            && parts[0] == "arn"
                            && parts[1] == "aws"
                            && parts[2] == "kms"
                            && parts[3] == region
                            && parts[4].len() == 12
                            && parts[4].bytes().all(|b| b.is_ascii_digit())
                            && parts[5].starts_with("key/")
                            && segment(&parts[5][4..]),
                        "AWS requires a full key ARN in the configured region; aliases are forbidden"
                    );
                }
                Self::Gcp { .. } => {
                    let p = id.split('/').collect::<Vec<_>>();
                    ensure!(
                        p.len() == 10
                            && p[0] == "projects"
                            && p[2] == "locations"
                            && p[4] == "keyRings"
                            && p[6] == "cryptoKeys"
                            && p[8] == "cryptoKeyVersions"
                            && [1, 3, 5, 7].iter().all(|i| segment(p[*i]))
                            && !p[9].is_empty()
                            && p[9].bytes().all(|b| b.is_ascii_digit())
                            && p[9] != "0",
                        "GCP requires a numeric CryptoKeyVersion resource name"
                    );
                }
                Self::Local => unreachable!(),
            }
        }
        Ok(())
    }
}
fn segment(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn azure_identity_endpoint(endpoint: &str) -> Result<url::Url> {
    let u = url::Url::parse(endpoint)?;
    let local = match u.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback() || ip.is_link_local(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    ensure!(
        u.scheme() == "http"
            && local
            && u.username().is_empty()
            && u.password().is_none()
            && u.query().is_none()
            && u.fragment().is_none(),
        "Unexpected Azure identity endpoint"
    );
    Ok(u)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wrapped {
    version: u8,
    provider: String,
    key_id: String,
    ciphertext: String,
}
struct CachedToken {
    value: Zeroizing<String>,
    until: Instant,
}
pub struct Kms {
    config: Config,
    local: Option<crypto::Key>,
    http: reqwest::Client,
    metadata: reqwest::Client,
    aws: Option<aws_sdk_kms::Client>,
    token: tokio::sync::Mutex<Option<CachedToken>>,
    #[cfg(test)]
    endpoint: Option<String>,
}
pub fn aws_http() -> aws_sdk_kms::config::SharedHttpClient {
    use aws_smithy_http_client::{Builder, tls};
    Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::Ring,
        ))
        .build_https()
}
impl Kms {
    pub async fn new(config: Config) -> Result<Self> {
        config.validate()?;
        let local = if matches!(config, Config::Local) {
            Some(crypto::decode_key(&Zeroizing::new(
                std::env::var("LTTL_BROKER_MASTER_KEY")
                    .context("Local wrapping requires LTTL_BROKER_MASTER_KEY")?,
            ))?)
        } else {
            ensure!(
                std::env::var_os("LTTL_BROKER_MASTER_KEY").is_none(),
                "Native KMS mode forbids LTTL_BROKER_MASTER_KEY; remove the old master-secret injection"
            );
            None
        };
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .build()?;
        let metadata = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()?;
        let aws = if let Config::Aws { region, .. } = &config {
            let shared = aws_config::defaults(aws_config::BehaviorVersion::latest())
                .region(aws_sdk_kms::config::Region::new(region.clone()))
                .http_client(aws_http())
                .load()
                .await;
            let conf = aws_sdk_kms::config::Builder::from(&shared)
                .endpoint_url(format!("https://kms.{region}.amazonaws.com"))
                .timeout_config(
                    aws_sdk_kms::config::timeout::TimeoutConfig::builder()
                        .operation_timeout(Duration::from_secs(30))
                        .build(),
                )
                .retry_config(
                    aws_sdk_kms::config::retry::RetryConfig::standard().with_max_attempts(2),
                )
                .build();
            Some(aws_sdk_kms::Client::from_conf(conf))
        } else {
            None
        };
        Ok(Self {
            config,
            local,
            http,
            metadata,
            aws,
            token: tokio::sync::Mutex::new(None),
            #[cfg(test)]
            endpoint: None,
        })
    }
    pub fn local_key(&self) -> Option<&crypto::Key> {
        self.local.as_ref()
    }
    pub fn provider(&self) -> &str {
        self.config.provider()
    }
    fn context(&self, id: &str, key_id: &str) -> Vec<u8> {
        serde_json::to_vec(&["lttl-archive-key-v1", self.provider(), id, key_id]).unwrap()
    }
    async fn bearer(&self) -> Result<Zeroizing<String>> {
        #[cfg(test)]
        if self.endpoint.is_some() {
            return Ok(Zeroizing::new("contract-test-token".into()));
        }
        let auth = match &self.config {
            Config::Azure { auth, .. } | Config::Gcp { auth, .. } => auth,
            _ => anyhow::bail!("No bearer token for this provider"),
        };
        if *auth == Auth::Environment {
            let token = Zeroizing::new(std::env::var("LTTL_KMS_ACCESS_TOKEN").context(
                "Set short-lived LTTL_KMS_ACCESS_TOKEN for the explicit environment auth mode",
            )?);
            ensure!(!token.is_empty(), "Empty KMS access token");
            return Ok(token);
        }
        let mut cache = self.token.lock().await;
        if let Some(token) = cache.as_ref().filter(|t| t.until > Instant::now()) {
            return Ok(token.value.clone());
        }
        let request=match &self.config {
            Config::Azure{client_id,..}=> {
                // Azure Functions/App Service uses an environment-owned identity endpoint.
                let mut request=if let Ok(endpoint)=std::env::var("IDENTITY_ENDPOINT") {
                    let u=azure_identity_endpoint(&endpoint)?;
                    self.metadata.get(u).header("X-IDENTITY-HEADER",std::env::var("IDENTITY_HEADER").context("Missing Azure identity header")?).query(&[("api-version","2019-08-01"),("resource","https://vault.azure.net")])
                }else{self.metadata.get("http://169.254.169.254/metadata/identity/oauth2/token").header("Metadata","true").query(&[("api-version","2018-02-01"),("resource","https://vault.azure.net")])};
                if let Some(id)=client_id {request=request.query(&[("client_id",id)]);}request
            }
            Config::Gcp{..}=>self.metadata.get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token").header("Metadata-Flavor","Google"),
            _=>unreachable!(),
        };
        let value = response_json(
            request
                .send()
                .await
                .map_err(|_| anyhow::anyhow!("Workload token request failed"))?,
        )
        .await?;
        let token = value
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .context("Workload identity returned no token")?;
        let seconds = value
            .get("expires_in")
            .and_then(number)
            .or_else(|| {
                value
                    .get("expires_on")
                    .and_then(number)
                    .map(|v| v.saturating_sub(crate::now() as u64))
            })
            .context("Missing token expiration")?;
        ensure!(seconds > 60, "Workload token is near expiration");
        let token = Zeroizing::new(token.to_string());
        *cache = Some(CachedToken {
            value: token.clone(),
            until: Instant::now() + Duration::from_secs(seconds.saturating_sub(60).min(3600)),
        });
        Ok(token)
    }
    async fn post(&self, url: String, body: Value) -> Result<Value> {
        let token = self.bearer().await?;
        #[cfg(test)]
        let url = if let Some(endpoint) = &self.endpoint {
            let u = url::Url::parse(&url)?;
            format!(
                "{endpoint}{}{}",
                u.path(),
                u.query().map(|q| format!("?{q}")).unwrap_or_default()
            )
        } else {
            url
        };
        let response = self
            .http
            .post(url)
            .bearer_auth(&*token)
            .json(&body)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("KMS network request failed"))?;
        response_json(response).await
    }
    pub async fn wrap(&self, id: &str, key: &crypto::Key) -> Result<Vec<u8>> {
        ensure!(crypto::valid_id(id), "Invalid wrapping context");
        if let Some(master) = &self.local {
            return crypto::seal(master, &**key, id.as_bytes());
        }
        let key_id = self.config.key_id();
        let context = self.context(id, key_id);
        // Azure RSA wrap has no AAD. An encrypted context commitment binds every
        // provider's ciphertext to this archive and the exact configured key.
        let mut payload = Zeroizing::new(b"LTTLKW01".to_vec());
        payload.extend(Sha256::digest(&context));
        payload.extend_from_slice(&**key);
        let ciphertext = match &self.config {
            Config::Azure { .. } => {
                let value = self
                    .post(
                        format!("{key_id}/wrapkey?api-version=2025-07-01"),
                        json!({"alg":"RSA-OAEP-256","value":URL_SAFE_NO_PAD.encode(&*payload)}),
                    )
                    .await?;
                ensure!(
                    value["kid"].as_str() == Some(key_id),
                    "Azure returned a different key version"
                );
                URL_SAFE_NO_PAD.decode(value["value"].as_str().context("Missing wrapped key")?)?
            }
            Config::Aws { .. } => {
                let value = self
                    .aws
                    .as_ref()
                    .unwrap()
                    .encrypt()
                    .key_id(key_id)
                    .encryption_algorithm(
                        aws_sdk_kms::types::EncryptionAlgorithmSpec::SymmetricDefault,
                    )
                    .plaintext(aws_sdk_kms::primitives::Blob::new(payload.to_vec()))
                    .set_encryption_context(Some(aws_context(id, key_id)))
                    .send()
                    .await
                    .map_err(|_| anyhow::anyhow!("AWS KMS Encrypt denied or unavailable"))?;
                ensure!(
                    value.key_id() == Some(key_id),
                    "AWS returned a different key ARN"
                );
                value
                    .ciphertext_blob()
                    .context("Missing wrapped key")?
                    .as_ref()
                    .to_vec()
            }
            Config::Gcp { .. } => {
                let value=self.post(format!("https://cloudkms.googleapis.com/v1/{key_id}:encrypt"),json!({"plaintext":STANDARD.encode(&*payload),"additionalAuthenticatedData":STANDARD.encode(&context),"plaintextCrc32c":crc(&payload),"additionalAuthenticatedDataCrc32c":crc(&context)})).await?;
                ensure!(
                    value["name"].as_str() == Some(key_id)
                        && value["verifiedPlaintextCrc32c"] == true
                        && value["verifiedAdditionalAuthenticatedDataCrc32c"] == true,
                    "GCP key version or integrity confirmation mismatch"
                );
                let bytes = STANDARD.decode(
                    value["ciphertext"]
                        .as_str()
                        .context("Missing wrapped key")?,
                )?;
                verify_crc(&bytes, &value["ciphertextCrc32c"])?;
                bytes
            }
            Config::Local => unreachable!(),
        };
        ensure!(
            !ciphertext.is_empty() && ciphertext.len() <= 16384,
            "Invalid KMS ciphertext length"
        );
        Ok(serde_json::to_vec(&Wrapped {
            version: 1,
            provider: self.provider().into(),
            key_id: key_id.into(),
            ciphertext: STANDARD.encode(ciphertext),
        })?)
    }
    pub async fn unwrap(&self, id: &str, stored: &[u8]) -> Result<crypto::Key> {
        ensure!(crypto::valid_id(id), "Invalid wrapping context");
        if let Some(master) = &self.local {
            return as_key(&crypto::unseal(master, stored, id.as_bytes())?);
        }
        ensure!(stored.len() <= 32768, "Oversized wrapped key record");
        let stored: Wrapped = serde_json::from_slice(stored)
            .context("Legacy local keys require offline migration; refusing automatic fallback")?;
        ensure!(
            stored.version == 1
                && stored.provider == self.provider()
                && self.config.allowed(&stored.key_id),
            "Untrusted KMS key reference"
        );
        let key_id = &stored.key_id;
        let context = self.context(id, key_id);
        let ciphertext = STANDARD.decode(&stored.ciphertext)?;
        ensure!(
            !ciphertext.is_empty() && ciphertext.len() <= 16384,
            "Invalid KMS ciphertext"
        );
        let payload = Zeroizing::new(match &self.config {
            Config::Azure { .. } => {
                let value = self
                    .post(
                        format!("{key_id}/unwrapkey?api-version=2025-07-01"),
                        json!({"alg":"RSA-OAEP-256","value":URL_SAFE_NO_PAD.encode(&ciphertext)}),
                    )
                    .await?;
                ensure!(
                    value["kid"].as_str() == Some(key_id.as_str()),
                    "Azure returned a different key version"
                );
                URL_SAFE_NO_PAD.decode(value["value"].as_str().context("Missing unwrapped key")?)?
            }
            Config::Aws { .. } => {
                let value = self
                    .aws
                    .as_ref()
                    .unwrap()
                    .decrypt()
                    .key_id(key_id)
                    .encryption_algorithm(
                        aws_sdk_kms::types::EncryptionAlgorithmSpec::SymmetricDefault,
                    )
                    .ciphertext_blob(aws_sdk_kms::primitives::Blob::new(ciphertext))
                    .set_encryption_context(Some(aws_context(id, key_id)))
                    .send()
                    .await
                    .map_err(|_| anyhow::anyhow!("AWS KMS Decrypt denied or unavailable"))?;
                ensure!(
                    value.key_id() == Some(key_id),
                    "AWS returned a different key ARN"
                );
                value
                    .plaintext()
                    .context("Missing unwrapped key")?
                    .as_ref()
                    .to_vec()
            }
            Config::Gcp { .. } => {
                let parent = key_id.rsplit_once("/cryptoKeyVersions/").unwrap().0;
                let value=self.post(format!("https://cloudkms.googleapis.com/v1/{parent}:decrypt"),json!({"ciphertext":STANDARD.encode(&ciphertext),"additionalAuthenticatedData":STANDARD.encode(&context),"ciphertextCrc32c":crc(&ciphertext),"additionalAuthenticatedDataCrc32c":crc(&context)})).await?;
                let bytes = STANDARD.decode(
                    value["plaintext"]
                        .as_str()
                        .context("Missing unwrapped key")?,
                )?;
                verify_crc(&bytes, &value["plaintextCrc32c"])?;
                bytes
            }
            Config::Local => unreachable!(),
        });
        ensure!(
            payload.len() == 72
                && &payload[..8] == b"LTTLKW01"
                && payload[8..40] == Sha256::digest(&context)[..],
            "Wrapped key context authentication failed"
        );
        as_key(&payload[40..])
    }
    pub async fn check(&self) -> Result<()> {
        let id = crypto::identifier();
        let key = crypto::key();
        let wrapped = self.wrap(&id, &key).await?;
        let recovered = self.unwrap(&id, &wrapped).await?;
        ensure!(*key == *recovered, "KMS round trip mismatch");
        ensure!(
            self.unwrap(&crypto::identifier(), &wrapped).await.is_err(),
            "KMS context isolation failed"
        );
        Ok(())
    }
}
fn as_key(bytes: &[u8]) -> Result<crypto::Key> {
    ensure!(bytes.len() == 32, "Invalid unwrapped key length");
    let mut key = Zeroizing::new([0; 32]);
    key.copy_from_slice(bytes);
    Ok(key)
}
fn aws_context(id: &str, key: &str) -> HashMap<String, String> {
    HashMap::from([
        ("lttl-purpose".into(), "archive-key-v1".into()),
        ("archive-id".into(), id.into()),
        ("wrapping-key".into(), key.into()),
    ])
}
fn crc(bytes: &[u8]) -> String {
    crc32c::crc32c(bytes).to_string()
}
fn number(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}
fn verify_crc(bytes: &[u8], expected: &Value) -> Result<()> {
    ensure!(
        number(expected) == Some(crc32c::crc32c(bytes).into()),
        "KMS response checksum mismatch"
    );
    Ok(())
}
async fn response_json(mut response: reqwest::Response) -> Result<Value> {
    ensure!(
        response.status().is_success(),
        "KMS or identity service returned HTTP {}",
        response.status().as_u16()
    );
    let mut bytes = Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("Cloud response read failed"))?
    {
        ensure!(
            bytes.len() + chunk.len() <= 65536,
            "Cloud response too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("Invalid cloud response JSON"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::State,
        http::{HeaderMap, StatusCode, Uri},
        routing::post,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicU8, AtomicUsize, Ordering},
    };
    const AZ: &str = "https://lttl-test.vault.azure.net/keys/wrap/11111111111111111111111111111111";
    const AWS: &str = "arn:aws:kms:us-east-1:123456789012:key/11111111-1111-1111-1111-111111111111";
    const GCP: &str = "projects/lttl-test/locations/us-central1/keyRings/test/cryptoKeys/wrap/cryptoKeyVersions/1";
    struct Mock {
        config: Config,
        key: crypto::Key,
        mode: AtomicU8,
        calls: AtomicUsize,
    }
    // Deliberately uses authenticated local encryption to emulate cloud opacity.
    // This tests API contracts, not the cloud's implementation or IAM enforcement.
    async fn endpoint(
        State(m): State<Arc<Mock>>,
        uri: Uri,
        headers: HeaderMap,
        body: axum::body::Bytes,
    ) -> std::result::Result<Json<Value>, StatusCode> {
        let body: Value = serde_json::from_slice(&body).unwrap();
        m.calls.fetch_add(1, Ordering::SeqCst);
        let mode = m.mode.load(Ordering::SeqCst);
        if mode == 1 {
            return Err(StatusCode::FORBIDDEN);
        }
        let key = m.config.key_id();
        let (encrypt, bytes, aad) = match &m.config {
            Config::Azure { .. } => {
                assert_eq!(headers["authorization"], "Bearer contract-test-token");
                assert_eq!(body["alg"], "RSA-OAEP-256");
                assert_eq!(uri.query(), Some("api-version=2025-07-01"));
                assert!(uri.path().ends_with("/wrapkey") || uri.path().ends_with("/unwrapkey"));
                (
                    uri.path().ends_with("/wrapkey"),
                    URL_SAFE_NO_PAD
                        .decode(body["value"].as_str().unwrap())
                        .unwrap(),
                    Vec::new(),
                )
            }
            Config::Aws { .. } => {
                assert!(
                    headers["authorization"]
                        .to_str()
                        .unwrap()
                        .starts_with("AWS4-HMAC-SHA256 ")
                );
                assert_eq!(body["KeyId"], key);
                assert_eq!(body["EncryptionAlgorithm"], "SYMMETRIC_DEFAULT");
                let encrypt = headers["x-amz-target"] == "TrentService.Encrypt";
                assert_eq!(body["EncryptionContext"]["lttl-purpose"], "archive-key-v1");
                (
                    encrypt,
                    STANDARD
                        .decode(
                            body[if encrypt {
                                "Plaintext"
                            } else {
                                "CiphertextBlob"
                            }]
                            .as_str()
                            .unwrap(),
                        )
                        .unwrap(),
                    serde_json::to_vec(&body["EncryptionContext"]).unwrap(),
                )
            }
            Config::Gcp { .. } => {
                assert_eq!(headers["authorization"], "Bearer contract-test-token");
                let encrypt = uri.path().ends_with(":encrypt");
                assert_eq!(
                    uri.path(),
                    if encrypt {
                        format!("/v1/{key}:encrypt")
                    } else {
                        format!(
                            "/v1/{}:decrypt",
                            key.rsplit_once("/cryptoKeyVersions/").unwrap().0
                        )
                    }
                );
                let field = if encrypt { "plaintext" } else { "ciphertext" };
                let bytes = STANDARD.decode(body[field].as_str().unwrap()).unwrap();
                let aad = STANDARD
                    .decode(body["additionalAuthenticatedData"].as_str().unwrap())
                    .unwrap();
                verify_crc(&bytes, &body[format!("{field}Crc32c")]).unwrap();
                verify_crc(&aad, &body["additionalAuthenticatedDataCrc32c"]).unwrap();
                (encrypt, bytes, aad)
            }
            _ => unreachable!(),
        };
        let output = if encrypt {
            crypto::seal(&m.key, &bytes, &aad)
        } else {
            crypto::unseal(&m.key, &bytes, &aad).map(|v| v.to_vec())
        }
        .map_err(|_| StatusCode::BAD_REQUEST)?;
        let returned_key = if mode == 2 { "wrong-key" } else { key };
        let mut result = match m.config {
            Config::Azure { .. } => {
                json!({"kid":returned_key,"value":URL_SAFE_NO_PAD.encode(&output)})
            }
            Config::Aws { .. } => {
                if encrypt {
                    json!({"KeyId":returned_key,"CiphertextBlob":STANDARD.encode(&output)})
                } else {
                    json!({"KeyId":returned_key,"Plaintext":STANDARD.encode(&output)})
                }
            }
            Config::Gcp { .. } => {
                if encrypt {
                    json!({"name":returned_key,"ciphertext":STANDARD.encode(&output),"ciphertextCrc32c":crc(&output),"verifiedPlaintextCrc32c":true,"verifiedAdditionalAuthenticatedDataCrc32c":true})
                } else {
                    json!({"plaintext":STANDARD.encode(&output),"plaintextCrc32c":crc(&output)})
                }
            }
            _ => unreachable!(),
        };
        if mode == 3 {
            result[if encrypt {
                "ciphertextCrc32c"
            } else {
                "plaintextCrc32c"
            }] = json!("4294967296");
        }
        if mode == 4 {
            result["verifiedPlaintextCrc32c"] = json!(false);
        }
        Ok(Json(result))
    }
    async fn fixture(config: Config) -> (Kms, Arc<Mock>, tokio::task::JoinHandle<()>) {
        config.validate().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint_url = format!("http://{}", listener.local_addr().unwrap());
        let m = Arc::new(Mock {
            config: config.clone(),
            key: crypto::key(),
            mode: AtomicU8::new(0),
            calls: AtomicUsize::new(0),
        });
        let router = Router::new().fallback(post(endpoint)).with_state(m.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let aws = if matches!(config, Config::Aws { .. }) {
            let conf = aws_sdk_kms::Config::builder()
                .behavior_version_latest()
                .region(aws_sdk_kms::config::Region::new("us-east-1"))
                .credentials_provider(aws_sdk_kms::config::Credentials::new(
                    "contract-access",
                    "contract-secret",
                    None,
                    None,
                    "test",
                ))
                .endpoint_url(&endpoint_url)
                .http_client(aws_http())
                .retry_config(aws_sdk_kms::config::retry::RetryConfig::disabled())
                .build();
            Some(aws_sdk_kms::Client::from_conf(conf))
        } else {
            None
        };
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        (
            Kms {
                config,
                local: None,
                http: http.clone(),
                metadata: http,
                aws,
                token: tokio::sync::Mutex::new(None),
                endpoint: Some(endpoint_url),
            },
            m,
            server,
        )
    }
    fn configs() -> [Config; 3] {
        [
            Config::Azure {
                key_id: AZ.into(),
                decrypt_key_ids: vec![],
                auth: Auth::Workload,
                client_id: None,
            },
            Config::Aws {
                key_id: AWS.into(),
                region: "us-east-1".into(),
                decrypt_key_ids: vec![],
            },
            Config::Gcp {
                key_id: GCP.into(),
                decrypt_key_ids: vec![],
                auth: Auth::Workload,
            },
        ]
    }
    #[tokio::test]
    async fn provider_contracts_roundtrip_context_tamper_and_denial() {
        for config in configs() {
            let (kms, m, server) = fixture(config).await;
            let id = crypto::identifier();
            let key = crypto::key();
            let wrapped = kms.wrap(&id, &key).await.unwrap();
            assert_eq!(*kms.unwrap(&id, &wrapped).await.unwrap(), *key);
            assert!(kms.unwrap(&crypto::identifier(), &wrapped).await.is_err());
            let mut tampered: Wrapped = serde_json::from_slice(&wrapped).unwrap();
            let mut bytes = STANDARD.decode(&tampered.ciphertext).unwrap();
            bytes[0] ^= 1;
            tampered.ciphertext = STANDARD.encode(bytes);
            assert!(
                kms.unwrap(&id, &serde_json::to_vec(&tampered).unwrap())
                    .await
                    .is_err()
            );
            let count = m.calls.load(Ordering::SeqCst);
            tampered.key_id = "https://untrusted.invalid/keys/redirect".into();
            assert!(
                kms.unwrap(&id, &serde_json::to_vec(&tampered).unwrap())
                    .await
                    .is_err()
            );
            assert_eq!(m.calls.load(Ordering::SeqCst), count);
            assert!(kms.unwrap(&id, b"old-local-key-record").await.is_err());
            m.mode.store(2, Ordering::SeqCst);
            assert!(kms.wrap(&id, &key).await.is_err());
            m.mode.store(1, Ordering::SeqCst);
            assert!(kms.wrap(&id, &key).await.is_err());
            assert!(kms.unwrap(&id, &wrapped).await.is_err());
            server.abort();
        }
    }
    #[tokio::test]
    async fn gcp_rejects_crc_and_integrity_confirmation_mismatches() {
        let (kms, m, server) = fixture(configs()[2].clone()).await;
        let id = crypto::identifier();
        let key = crypto::key();
        let wrapped = kms.wrap(&id, &key).await.unwrap();
        m.mode.store(3, Ordering::SeqCst);
        assert!(kms.wrap(&id, &key).await.is_err());
        assert!(kms.unwrap(&id, &wrapped).await.is_err());
        m.mode.store(4, Ordering::SeqCst);
        assert!(kms.wrap(&id, &key).await.is_err());
        server.abort();
    }
    #[test]
    fn key_references_are_pinned_and_previous_keys_are_explicit() {
        for config in configs() {
            assert!(config.validate().is_ok());
            assert!(config.allowed(config.key_id()));
            assert!(!config.allowed("other"));
        }
        let mut config = configs()[0].clone();
        if let Config::Azure {
            key_id,
            decrypt_key_ids,
            ..
        } = &mut config
        {
            decrypt_key_ids.push(key_id.clone());
            *key_id = AZ.replace(
                "11111111111111111111111111111111",
                "22222222222222222222222222222222",
            );
        }
        assert!(config.validate().is_ok());
        assert!(config.allowed(AZ));
        for bad in [
            AZ.replace("https://", "http://"),
            AZ.replace("vault.azure.net", "vault.azure.net.evil.example"),
            format!("{AZ}?redirect=evil"),
            AZ.rsplit_once('/').unwrap().0.into(),
        ] {
            assert!(
                Config::Azure {
                    key_id: bad,
                    decrypt_key_ids: vec![],
                    auth: Auth::Workload,
                    client_id: None
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            Config::Aws {
                key_id: AWS.replace("key/", "alias/"),
                region: "us-east-1".into(),
                decrypt_key_ids: vec![]
            }
            .validate()
            .is_err()
        );
        assert!(
            Config::Gcp {
                key_id: GCP.replace("Versions/1", "Versions/latest"),
                decrypt_key_ids: vec![],
                auth: Auth::Workload
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn azure_workload_endpoint_accepts_local_platform_addresses_only() {
        for endpoint in [
            "http://127.0.0.1:8081/msi/token",
            "http://169.254.129.2:8081/msi/token",
            "http://[::1]:8081/token",
        ] {
            assert!(azure_identity_endpoint(endpoint).is_ok());
        }
        for endpoint in [
            "http://example.com/token",
            "http://user:secret@127.0.0.1/token",
            "http://127.0.0.1/token?redirect=evil",
            "https://169.254.169.254/token",
        ] {
            assert!(azure_identity_endpoint(endpoint).is_err());
        }
    }
    fn local(key: crypto::Key) -> Kms {
        let http = reqwest::Client::new();
        Kms {
            config: Config::Local,
            local: Some(key),
            http: http.clone(),
            metadata: http,
            aws: None,
            token: tokio::sync::Mutex::new(None),
            endpoint: None,
        }
    }
    #[tokio::test]
    async fn startup_checks_reject_wrong_keys_and_cross_mode_fallback() {
        let db = crate::store::Store::connect(":memory:", false, false)
            .await
            .unwrap();
        let local = local(crypto::key());
        crate::verify_wrapping(&db, &local).await.unwrap();
        crate::verify_wrapping(&db, &local).await.unwrap();
        let wrong = self::local(crypto::key());
        assert!(crate::verify_wrapping(&db, &wrong).await.is_err());
        let (cloud, _, server) = fixture(configs()[0].clone()).await;
        assert!(crate::verify_wrapping(&db, &cloud).await.is_err());
        let db = crate::store::Store::connect(":memory:", false, false)
            .await
            .unwrap();
        crate::verify_wrapping(&db, &cloud).await.unwrap();
        crate::verify_wrapping(&db, &cloud).await.unwrap();
        assert!(crate::verify_wrapping(&db, &local).await.is_err());
        server.abort();
    }
}
