use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    extract::Request,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use openidconnect::{
    AuthType, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet,
    EndpointNotSet, EndpointSet, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, Scope, TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
};
mod kms;
mod store;
mod tracking;
mod audit;
mod retention;
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;
#[path = "../../src/identity/crypto.rs"]
#[allow(dead_code)]
mod crypto;
type Oidc = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    public_url: String,
    issuer: String,
    client_id: String,
    database: String,
    #[serde(default = "listen")]
    listen: String,
    #[serde(default)]
    required_acr: Option<String>,
    #[serde(default)]
    wrapping: kms::Config,
    #[serde(default)]
    serverless: bool,
    #[serde(default)]
    tracking: tracking::Config,
    #[serde(default)]
    audit: audit::Config,
}
fn listen() -> String {
    "127.0.0.1:8787".into()
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
fn safe_url(value: &str) -> Result<url::Url> {
    let u = url::Url::parse(value)?;
    ensure!(
        u.username().is_empty() && u.password().is_none() && u.fragment().is_none(),
        "Invalid URL"
    );
    ensure!(
        u.scheme() == "https"
            || (u.scheme() == "http" && matches!(u.host_str(), Some("127.0.0.1" | "[::1]"))),
        "HTTPS required except loopback development"
    );
    Ok(u)
}
#[derive(Clone, Serialize, Deserialize)]
struct User {
    id: String,
    name: String,
    email: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Grant {
    user: User,
    archive: Option<String>,
    expires: i64,
}
#[derive(Serialize, Deserialize)]
struct Flow {
    nonce: Nonce,
    verifier: PkceCodeVerifier,
    url: String,
    redirect: String,
    desktop_state: String,
    challenge: String,
    archive: Option<String>,
    created: i64,
    launched: bool,
}
#[derive(Serialize, Deserialize)]
struct Exchange {
    challenge: String,
    grant: Grant,
    expires: i64,
}
struct App {
    config: Config,
    secret: Zeroizing<String>,
    kms: kms::Kms,
    http: reqwest::Client,
    oidc: Oidc,
    db: store::Store,
    tracking: tracking::Service,
    audit: audit::Service,
}

type Shared = Arc<App>;
struct Error(StatusCode, &'static str);
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error":self.1}))).into_response()
    }
}
impl From<anyhow::Error> for Error {
    fn from(_: anyhow::Error) -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Identity service operation failed; administrator should check configuration",
        )
    }
}
impl From<sqlx::Error> for Error {
    fn from(_: sqlx::Error) -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Identity service storage failure",
        )
    }
}
type Api<T> = std::result::Result<T, Error>;
fn forbidden() -> Error {
    Error(StatusCode::FORBIDDEN, "Access denied")
}
fn bad() -> Error {
    Error(StatusCode::BAD_REQUEST, "Invalid request")
}
async fn verify_wrapping(db: &store::Store, kms: &kms::Kms) -> Result<()> {
    const MARKER: &[u8] = b"lttl master key check v1";
    if let Some(master) = kms.local_key() {
        ensure!(
            db.metadata("cloud-check").await?.is_none(),
            "Cloud-wrapped database cannot fall back to local wrapping"
        );
        if let Some(saved) = db.metadata("master-check").await? {
            ensure!(
                crypto::unseal(master, &saved, MARKER)?.as_slice() == MARKER,
                "Wrong broker master key"
            );
        } else {
            if let Some((id, wrapped)) = db.first_archive().await? {
                ensure!(
                    crypto::unseal(master, &wrapped, id.as_bytes())?.len() == 32,
                    "Wrong broker master key"
                );
            }
            db.put_metadata("master-check", &crypto::seal(master, MARKER, MARKER)?)
                .await?;
            let saved = db.metadata("master-check").await?.unwrap();
            ensure!(
                crypto::unseal(master, &saved, MARKER)?.as_slice() == MARKER,
                "Wrong broker master key"
            );
        }
    } else {
        ensure!(
            db.metadata("master-check").await?.is_none(),
            "Existing local database requires explicit offline rewrapping; use a new test database"
        );
        let id = crypto::digest(b"lttl native KMS startup check v1");
        if db.metadata("cloud-check").await?.is_none() {
            if let Some((archive, wrapped)) = db.first_archive().await? {
                kms.unwrap(&archive, &wrapped).await?;
            }
            db.put_metadata("cloud-check", &kms.wrap(&id, &crypto::key()).await?)
                .await?;
        }
        kms.unwrap(&id, &db.metadata("cloud-check").await?.unwrap())
            .await?;
    }
    Ok(())
}
async fn oidc(config: &Config, secret: &str, http: &reqwest::Client) -> Result<Oidc> {
    let metadata =
        CoreProviderMetadata::discover_async(IssuerUrl::new(config.issuer.clone())?, http).await?;
    safe_url(metadata.authorization_endpoint().as_str())?;
    safe_url(metadata.jwks_uri().as_str())?;
    safe_url(
        metadata
            .token_endpoint()
            .context("IDP has no token endpoint")?
            .as_str(),
    )?;
    Ok(CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(config.client_id.clone()),
        Some(ClientSecret::new(secret.into())),
    )
    .set_auth_type(AuthType::RequestBody)
    .set_redirect_uri(RedirectUrl::new(format!(
        "{}/auth/callback",
        config.public_url
    ))?))
}
async fn app(config: Config, secret: String, kms: kms::Kms) -> Result<Shared> {
    let tracking = tracking::Service::new(config.tracking.clone())?;
    let audit = audit::Service::new(config.audit.clone())?;
    let u = safe_url(&config.public_url)?;
    ensure!(
        u.path() == "/" && u.query().is_none(),
        "Public URL must be an origin"
    );
    safe_url(&config.issuer)?;
    let addr: SocketAddr = config.listen.parse()?;
    ensure!(
        config.serverless || addr.ip().is_loopback(),
        "Bind to loopback or select serverless mode"
    );
    ensure!(
        !config.serverless
            || (config.database == "postgres"
                && kms.provider() != "local"
                && u.scheme() == "https"),
        "Serverless requires PostgreSQL, native KMS and HTTPS"
    );
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()?;
    let oidc = oidc(&config, &secret, &http).await?;
    let mut db = store::Store::connect(&config.database, config.serverless, false).await?;
    db.tracking_policy("schema-check").await.context("Tracking schema missing; run --init-db before starting the upgraded broker")?;
    verify_wrapping(&db, &kms).await?;
    db.configure_audit(&audit).await.context("Audit configuration/schema invalid; run --init-db on upgrades")?;
    Ok(Arc::new(App {
        config,
        tracking,
        audit,
        secret: Zeroizing::new(secret),
        kms,
        http,
        oidc,
        db,
    }))
}
async fn authorize(app: &App, headers: &HeaderMap, consume: bool) -> Api<Grant> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(Error(StatusCode::UNAUTHORIZED, "Sign in required"))?;
    if !crypto::valid_id(token) {
        return Err(forbidden());
    }
    let grant: Grant = app
        .db
        .state("sessions", token, consume)
        .await?
        .ok_or(forbidden())?;
    if !app.db.active(&grant.user.id).await? {
        return Err(forbidden());
    }
    Ok(grant)
}
async fn manage(app: &App, headers: &HeaderMap) -> Api<Grant> {
    let grant = authorize(app, headers, false).await?;
    if grant.archive.is_some() {
        return Err(forbidden());
    }
    Ok(grant)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartRequest {
    archive_id: Option<String>,
    redirect_uri: String,
    state: String,
    challenge: String,
}
fn validate_start(r: &StartRequest) -> Result<()> {
    let u = url::Url::parse(&r.redirect_uri)?;
    ensure!(
        u.scheme() == "http"
            && u.host_str() == Some("127.0.0.1")
            && u.port().is_some()
            && u.path() == "/callback"
            && u.query().is_none()
            && u.fragment().is_none()
            && u.username().is_empty()
            && u.password().is_none(),
        "Invalid native callback"
    );
    ensure!(
        crypto::valid_id(&r.state)
            && crypto::valid_id(&r.challenge)
            && r.archive_id.as_ref().is_none_or(|id| crypto::valid_id(id)),
        "Invalid sign-in request"
    );
    Ok(())
}
async fn start(
    State(app): State<Shared>,
    axum::Extension(location): axum::Extension<tracking::Location>,
    Json(r): Json<StartRequest>,
) -> Api<Json<serde_json::Value>> {
    validate_start(&r).map_err(|_| bad())?;
    let id = crypto::identifier();
    let state = id.clone();
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, _, nonce) = app
        .oidc
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            move || CsrfToken::new(state),
            Nonce::new_random,
        )
        .add_scope(Scope::new("profile".into()))
        .add_scope(Scope::new("email".into()))
        .set_pkce_challenge(challenge)
        .set_max_age(Duration::ZERO)
        .url();
    app.db.prune().await?;
    if !app.db.below_limits().await? {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many sign-in requests",
        ));
    }
    if let Some(archive)=&r.archive_id {app.tracking.record(&app.db,archive,None,"open_attempt",&location).await?;}
    app.db
        .put(
            "login_flows",
            &id,
            &Flow {
                nonce,
                verifier,
                url: url.into(),
                redirect: r.redirect_uri,
                desktop_state: r.state,
                challenge: r.challenge,
                archive: r.archive_id,
                created: now(),
                launched: false,
            },
            now() + 300,
        )
        .await?;
    Ok(Json(
        serde_json::json!({"browser_url":format!("{}/auth/login/{id}",app.config.public_url)}),
    ))
}
async fn login(State(app): State<Shared>, Path(id): Path<String>) -> Api<Redirect> {
    let flow: Flow = app.db.launch(&id).await?.ok_or(forbidden())?;
    Ok(Redirect::to(&flow.url))
}
#[derive(Deserialize)]
struct Callback {
    state: String,
    code: Option<String>,
}
fn fresh(auth: Option<i64>, issued: i64, started: i64, time: i64) -> Result<()> {
    let auth = auth.context("IDP must supply auth_time when max_age=0 is requested")?;
    ensure!(
        auth >= started - 30 && auth <= time + 30 && time - auth <= 300 && issued <= time + 30,
        "Fresh authentication required"
    );
    Ok(())
}
async fn callback(State(app): State<Shared>, axum::Extension(location):axum::Extension<tracking::Location>, Query(r): Query<Callback>) -> Api<Redirect> {
    let archive=app.db.state::<Flow>("login_flows",&r.state,false).await?.and_then(|f|f.archive);
    let result=callback_inner(app.clone(),r,&location).await;
    if result.is_err() {
        if let Some(id)=archive {app.tracking.record(&app.db,&id,None,"authentication_failed",&location).await?;}
        else {app.db.audit(None,None,"authentication_failed").await?;}
    }
    result
}
async fn callback_inner(app:Shared,r:Callback,location:&tracking::Location)->Api<Redirect> {
    let f: Flow = app
        .db
        .state("login_flows", &r.state, true)
        .await?
        .ok_or(forbidden())?;
    if f.created + 300 <= now() {
        return Err(forbidden());
    }
    let code = r.code.ok_or(forbidden())?;
    // Fresh discovery/JWKS supports key rotation without token-supplied URLs.
    let client = oidc(&app.config, &app.secret, &app.http).await?;
    let token = client
        .exchange_code(AuthorizationCode::new(code))
        .map_err(|_| forbidden())?
        .set_pkce_verifier(f.verifier)
        .request_async(&app.http)
        .await
        .map_err(|_| forbidden())?;
    let verifier = client.id_token_verifier();
    let claims = token
        .id_token()
        .ok_or(forbidden())?
        .claims(&verifier, &f.nonce)
        .map_err(|_| forbidden())?;
    fresh(
        claims.auth_time().map(|v| v.timestamp()),
        claims.issue_time().timestamp(),
        f.created,
        now(),
    )
    .map_err(|_| forbidden())?;
    if let Some(acr) = &app.config.required_acr {
        if claims.auth_context_ref().map(|a| a.as_str()) != Some(acr.as_str()) {
            return Err(forbidden());
        }
    }
    let subject = claims.subject().as_str();
    if subject.is_empty() || subject.len() > 1024 {
        return Err(forbidden());
    }
    // Display labels never merge accounts or grant access.
    let email = claims
        .email()
        .map(|s| s.as_str())
        .unwrap_or("")
        .chars()
        .take(320)
        .collect::<String>();
    let name = claims
        .name()
        .and_then(|n| n.get(None))
        .map(|n| n.as_str())
        .unwrap_or(subject)
        .chars()
        .take(256)
        .collect::<String>();
    let user = app
        .db
        .enroll(&app.config.issuer, subject, &name, &email)
        .await
        .map_err(|_| forbidden())?;
    app.db.audit_detail(Some(&user.id),f.archive.as_deref(),"authentication_accepted",serde_json::json!({"issuer":app.config.issuer,"subject":subject,"auth_time":claims.auth_time().map(|v|v.timestamp()),"acr":claims.auth_context_ref().map(|v|v.as_str())})).await?;
    if let Some(id)=&f.archive {app.tracking.record(&app.db,id,Some(&user.id),"authenticated",location).await?;}
    let code = crypto::identifier();
    let mut redirect = url::Url::parse(&f.redirect).map_err(|_| bad())?;
    redirect
        .query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", &f.desktop_state);
    app.db
        .put(
            "auth_codes",
            &code,
            &Exchange {
                challenge: f.challenge,
                grant: Grant {
                    user,
                    archive: f.archive,
                    expires: now() + 300,
                },
                expires: now() + 60,
            },
            now() + 60,
        )
        .await?;
    Ok(Redirect::to(redirect.as_str()))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExchangeRequest {
    code: String,
    verifier: String,
}
async fn exchange(
    State(app): State<Shared>,
    Json(r): Json<ExchangeRequest>,
) -> Api<Json<serde_json::Value>> {
    if !crypto::valid_id(&r.code) || !crypto::valid_id(&r.verifier) {
        return Err(bad());
    }
    let e: Exchange = app
        .db
        .state("auth_codes", &r.code, true)
        .await?
        .ok_or(forbidden())?;
    if crypto::digest(r.verifier.as_bytes()) != e.challenge {
        return Err(forbidden());
    }
    let token = crypto::identifier();
    let user = e.grant.user.clone();
    app.db
        .put("sessions", &token, &e.grant, e.grant.expires)
        .await?;
    Ok(Json(serde_json::json!({"token":token,"user":user})))
}
async fn users(State(app): State<Shared>, headers: HeaderMap) -> Api<Json<Vec<User>>> {
    manage(&app, &headers).await?;
    Ok(Json(app.db.users().await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    recipients: Vec<String>,
    #[serde(default)] tracking: crypto::TrackingPolicy,
}
async fn create_archive(
    State(app): State<Shared>,
    headers: HeaderMap,
    Json(r): Json<Create>,
) -> Api<Json<serde_json::Value>> {
    let grant = manage(&app, &headers).await?;
    if r.recipients.is_empty()
        || r.recipients.len() > 1000
        || !r.recipients.iter().all(|id| crypto::valid_id(id))
    {
        return Err(bad());
    }
    for id in &r.recipients {
        if !app.db.active(id).await? {
            return Err(forbidden());
        }
    }
    let policy=app.tracking.policy(r.tracking).map_err(|_|Error(StatusCode::BAD_REQUEST,"Tracking policy invalid or organization webhook not configured"))?;
    let id = crypto::identifier();
    let key = crypto::key();
    let wrapped = app.kms.wrap(&id, &key).await?;
    app.db
        .create_archive_tracked(&id, &grant.user.id, &r.recipients, &wrapped, &policy)
        .await?;
    Ok(Json(
        serde_json::json!({"archive_id":id,"key":crypto::encode(&*key),"tracking":policy}),
    ))
}
async fn release_key(
    State(app): State<Shared>, Path(id): Path<String>,
    axum::Extension(location):axum::Extension<tracking::Location>, headers: HeaderMap,
) -> Api<Json<serde_json::Value>> {
    if !crypto::valid_id(&id) {return Err(bad());}
    let grant=match authorize(&app,&headers,true).await {
        Ok(grant)=>grant,
        Err(error)=>{app.tracking.record(&app.db,&id,None,"key_denied",&location).await?;return Err(error)}
    };
    let result=release_authorized(&app,&id,&grant).await;
    let kind=if result.is_ok(){"key_released"}else{"key_denied"};
    app.tracking.record(&app.db,&id,Some(&grant.user.id),kind,&location).await?;
    result
}
async fn release_authorized(app:&App,id:&str,grant:&Grant)->Api<Json<serde_json::Value>> {
    if grant.archive.as_deref()!=Some(id) {return Err(forbidden());}
    let Some(wrapped)=app.db.allowed_key(id,&grant.user.id).await? else {
        app.db.audit(Some(&grant.user.id),Some(id),"key-denied").await?;return Err(forbidden());
    };
    let key=app.kms.unwrap(id,&wrapped).await?;
    if app.db.allowed_key(id,&grant.user.id).await?.as_deref()!=Some(wrapped.as_slice()) {return Err(forbidden());}
    app.db.audit(Some(&grant.user.id),Some(id),"key-release").await?;
    Ok(Json(serde_json::json!({"key":crypto::encode(&*key)})))
}
async fn tracking_events(State(app):State<Shared>,Path(id):Path<String>,headers:HeaderMap)->Api<Json<serde_json::Value>> {
    let grant=manage(&app,&headers).await?;
    let events=app.db.tracking_events(&id,&grant.user.id).await?.ok_or(forbidden())?;
    Ok(Json(serde_json::json!({"events":events,"limit":200,"retention_days":app.config.audit.evidence_retention_days,"zero_means":"indefinite","legal_holds_override_expiry":true})))
}
async fn dispatch_tracking(State(app):State<Shared>,headers:HeaderMap)->Api<Json<serde_json::Value>> {
    if !app.tracking.dispatch_allowed(&headers) {return Err(forbidden());}
    app.db.prune_evidence().await?;
    let delivered=app.tracking.deliver(&app.db,10).await?;
    use sqlx::Row;
    let row=sqlx::query("SELECT COUNT(*) FROM tracking_outbox").fetch_one(&app.db.pool).await?;
    Ok(Json(serde_json::json!({"delivered":delivered,"pending":row.get::<i64,_>(0)})))
}
async fn audit_admin(app:&App,headers:&HeaderMap)->Api<Grant> {
    use sqlx::Row;
    let grant=match manage(app,headers).await {
        Ok(grant)=>grant,
        Err(error)=>{app.db.audit(None,None,"administrator_access_denied").await?;return Err(error)}
    };
    let row=sqlx::query("SELECT issuer,subject FROM users WHERE id=$1").bind(&grant.user.id).fetch_one(&app.db.pool).await?;
    if !app.config.audit.enabled || row.get::<String,_>(0)!=app.config.issuer || !app.config.audit.administrator_subjects.contains(&row.get::<String,_>(1)) {
        app.db.audit(Some(&grant.user.id),None,"administrator_access_denied").await?;return Err(forbidden());
    }
    Ok(grant)
}
async fn retention_status(State(app):State<Shared>,Path(id):Path<String>,headers:HeaderMap)->Api<Json<serde_json::Value>> {
    if !crypto::valid_id(&id) {return Err(bad());}
    let grant=audit_admin(&app,&headers).await?;
    app.db.audit(Some(&grant.user.id),Some(&id),"retention_inspected").await?;
    Ok(Json(app.db.lifecycle(&id).await?))
}
async fn retention_update(State(app):State<Shared>,Path(id):Path<String>,headers:HeaderMap,Json(action):Json<retention::Action>)->Api<Json<serde_json::Value>> {
    if !crypto::valid_id(&id) {return Err(bad());}
    let grant=audit_admin(&app,&headers).await?;
    action.validate().map_err(|_|bad())?;
    if let Err(_) = app.db.retention_action(&id,&grant.user.id,&action).await {
        app.db.audit_detail(Some(&grant.user.id),Some(&id),"retention_action_denied",serde_json::to_value(&action).map_err(|_|bad())?).await?;
        return Err(Error(StatusCode::CONFLICT,"Retention action rejected: check hold, retention, archive state and identifiers"));
    }
    Ok(Json(app.db.lifecycle(&id).await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipients {recipients:Vec<String>}
async fn recipients_update(State(app):State<Shared>,Path(id):Path<String>,headers:HeaderMap,Json(r):Json<Recipients>)->Api<Json<serde_json::Value>> {
    if !crypto::valid_id(&id) {return Err(bad());}
    let grant=manage(&app,&headers).await?;
    if app.db.change_recipients(&id,&grant.user.id,&r.recipients).await.is_err() {
        app.db.audit(Some(&grant.user.id),Some(&id),"recipient_change_denied").await?;return Err(forbidden());
    }
    Ok(Json(serde_json::json!({"updated":true})))
}
async fn audit_dispatch(State(app):State<Shared>,headers:HeaderMap)->Api<Json<serde_json::Value>> {
    // Separate machine credential: cannot read archives, place/release holds or destroy keys.
    let configured=std::env::var("LTTL_AUDIT_DISPATCH_TOKEN").map_err(|_|forbidden())?;
    let supplied=headers.get(header::AUTHORIZATION).and_then(|v|v.to_str().ok()).and_then(|v|v.strip_prefix("Bearer ")).unwrap_or("");
    if configured.len()<32 || !tracking::verify(&configured,b"audit dispatch",&tracking::sign(supplied,b"audit dispatch")) {return Err(forbidden());}
    let batch=app.audit.dispatch(&app.db).await?;
    let pruned=app.db.prune_evidence().await?;
    Ok(Json(serde_json::json!({"batch":batch,"local_records_pruned":pruned})))
}
async fn tracking_context(State(app):State<Shared>,mut request:Request,next:Next)->Response {
    let peer=request.extensions().get::<axum::extract::ConnectInfo<SocketAddr>>().map(|c|c.0.ip());
    let location=app.tracking.location(request.headers(),peer,request.method().as_str(),request.uri().path());
    request.extensions_mut().insert(location);
    next.run(request).await
}
async fn revoke(
    State(app): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Api<Json<serde_json::Value>> {
    let grant = manage(&app, &headers).await?;
    if !app.db.revoke(&id, &grant.user.id).await? {
        return Err(forbidden());
    }
    Ok(Json(serde_json::json!({"revoked":true})))
}
async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    h.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    h.insert("referrer-policy", "no-referrer".parse().unwrap());
    h.insert("x-content-type-options", "nosniff".parse().unwrap());
    h.insert(
        "content-security-policy",
        "default-src 'none'; frame-ancestors 'none'"
            .parse()
            .unwrap(),
    );
    response
}
fn router(app: Shared) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/auth/start", post(start))
        .route("/auth/login/{id}", get(login))
        .route("/auth/callback", get(callback))
        .route("/v1/auth/exchange", post(exchange))
        .route("/v1/users", get(users))
        .route("/v1/archives", post(create_archive))
        .route("/v1/archives/{id}/key", post(release_key))
        .route("/v1/archives/{id}/revoke", post(revoke))
        .route("/v1/archives/{id}/events", get(tracking_events))
        .route("/v1/tracking/dispatch", post(dispatch_tracking))
        .route("/v1/audit/dispatch", post(audit_dispatch))
        .route("/v1/archives/{id}/retention", get(retention_status).post(retention_update))
        .route("/v1/archives/{id}/recipients", post(recipients_update))
        .layer(middleware::from_fn_with_state(app.clone(),tracking_context))
        .layer(DefaultBodyLimit::max(131072))
        .layer(middleware::from_fn(security_headers))
        .with_state(app)
}
#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).map(String::as_str) == Some("--generate-master-key") {
        println!("{}", crypto::encode(&*crypto::key()));
        return Ok(());
    }
    let command = args
        .get(1)
        .context("Usage: lttl-key-broker [--check-kms|--init-db] CONFIG.json")?;
    let (operation, path) = if command.starts_with("--") {
        (
            command.as_str(),
            args.get(2).context("Config path required")?,
        )
    } else {
        ("serve", command)
    };
    ensure!(
        matches!(operation, "serve" | "--check-kms" | "--init-db" | "--audit-public-key"),
        "Unknown operation"
    );
    let mut config: Config = serde_json::from_slice(&std::fs::read(path)?)?;
    config.public_url = config.public_url.trim_end_matches('/').into();
    if operation == "--audit-public-key" {
        let seed=crypto::decode_key(&std::env::var("LTTL_AUDIT_SIGNING_SEED").context("Set LTTL_AUDIT_SIGNING_SEED")?)?;
        let signer=audit::Signer::from_seed(&*seed,&config.audit.stream_id)?;
        println!("{}",serde_json::json!({"stream":signer.stream,"public_key":signer.public_key}));return Ok(());
    }
    if operation == "--init-db" {
        store::Store::connect(&config.database, config.serverless, true).await?;
        println!("Database schema ready");
        return Ok(());
    }
    let kms = kms::Kms::new(config.wrapping.clone()).await?;
    if operation == "--check-kms" {
        kms.check().await?;
        println!(
            "Native wrapping round trip and context isolation passed for {}",
            kms.provider()
        );
        return Ok(());
    }
    if config.serverless {
        let port = std::env::var("FUNCTIONS_CUSTOMHANDLER_PORT")
            .or_else(|_| std::env::var("PORT"))
            .unwrap_or_else(|_| "8080".into());
        ensure!(
            port.parse::<u16>().is_ok_and(|p| p > 0),
            "Invalid function port"
        );
        config.listen = format!("0.0.0.0:{port}");
    }
    let secret = std::env::var("LTTL_OIDC_CLIENT_SECRET").context("Set LTTL_OIDC_CLIENT_SECRET")?;
    let app = app(config, secret, kms).await?;
    let listener = tokio::net::TcpListener::bind(&app.config.listen).await?;
    eprintln!(
        "lttl key broker listening on {} behind {}",
        app.config.listen, app.config.public_url
    );
    axum::serve(listener, router(app).into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}
