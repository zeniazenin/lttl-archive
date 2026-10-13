# lttl registration — Keycloak

These instructions create an application in the company’s existing IDP. The lttl key service is a separate, company-hosted key-release API, not another IDP. Registration is performed by the tenant administrator; this guide does not create it automatically.

## Registration values

- Application: lttl key service
- Type: confidential Web application
- Flow: Authorization Code with S256 PKCE
- Redirect / callback URI: `https://keys.example.com/auth/callback`
- Scopes: `openid profile email`
- No wildcard redirect URIs, implicit flow, password grant, refresh token or desktop client secret.

## Provider steps

1. In the company’s Keycloak realm, create an **OpenID Connect** client. Enable client authentication and Standard Flow (Authorization Code); disable implicit and direct-access grants.
2. Set Valid redirect URIs to the exact callback below, without wildcards. Use the realm’s exact issuer and client secret, and require S256 PKCE.
3. For an existing SAML IDP, create a **SAML v2.0 identity provider** in Keycloak. Import the company IDP metadata and require signed assertions/responses with trusted signing certificates, audience and destination validation. Obtain Keycloak’s generated SP entity ID/ACS metadata and register that enterprise application in Entra, Okta, AD FS, Ping or the other SAML IDP.
4. Enable upstream Force Authentication and configure MFA/location policy for the SAML application. Use a persistent immutable account identifier; do not automatically link accounts by unverified email. Verify that a fresh downstream auth_time represents a fresh upstream policy check.
5. The lttl service consumes OIDC from Keycloak. It does not parse SAML assertions itself.

[Keycloak identity brokering](https://www.keycloak.org/docs/latest/server_admin/index.html#_identity_broker)

## Key service configuration

Copy these IDP fields into the chosen hosting example in `identity-broker/deploy/serverless/`, then add its native KMS key reference. The hosting guide below supplies database and function settings.

```json
{
  "client_id": "CLIENT_ID",
  "issuer": "https://sso.example.com/realms/REPLACE_REALM",
  "public_url": "https://keys.example.com"
}
```

The key service runs on Azure Functions, AWS Lambda or Google Cloud Run with shared PostgreSQL state. Inject `LTTL_OIDC_CLIENT_SECRET` and `LTTL_DATABASE_URL` through protected platform settings. Native KMS wraps archive keys without exporting the cloud master key. Do not set `LTTL_BROKER_MASTER_KEY` for these deployments. No VM or cloud CLI is required at runtime. See the hosting instructions below and `identity-broker/deploy/test-environment.md`. Build the broker with Rust 1.92 or newer; initialize the PostgreSQL schema before deployment. Restrict logs so they do not capture authorization headers, response keys or callback query strings.

## Desktop profile

Save this JSON, or enter the service URL in lttl → Identity & sharing.

```json
{
  "broker_url": "https://keys.example.com",
  "provider": "keycloak"
}
```

## Enroll and share

Each recipient signs in once to enroll with this key service. Refresh the recipient picker, select the intended accounts, and create a `.lttl` protected archive. Names/email are display labels only; authorization uses the verified issuer and subject. The creator is not automatically added as a recipient. To open/list/test/extract, the recipient must sign in again and be in the archive’s allowlist. The key service must be reachable.

The sender already has the source data, and an authorized recipient can retain decrypted data or keys. This is access-controlled encryption, not DRM or revocation of copies already obtained. The key-service operator can access stored keys. Keep its database and cloud key versions recoverable; losing either can make archives permanently unreadable. The preview uses temporary plaintext archive files during compression/decryption and cleans them on normal completion/errors, but cannot guarantee erasure after a crash or from storage snapshots. Use an encrypted, access-restricted temporary volume.

## Acceptance checks

Validate recipient success, nonrecipient denial, two accounts with the same display email remaining distinct, disabled-user/policy denial, fresh MFA/location enforcement, wrong tenant, service outage, revocation, tampered archive, and a second opening requiring fresh sign-in. Changing the IDP app client ID or issuer can change subject identifiers and break existing access; plan identity migrations explicitly.

---

# Google Cloud Run + native Cloud KMS

Use **Cloud Run**, Google's serverless container platform, for this Rust service. This is a container deployment, not a Cloud Run functions runtime. It scales to zero without a Compute Engine VM. User authentication can use Entra or another compatible company IDP.

1. In a test project, enable Cloud Run, Artifact Registry, Cloud KMS and Secret Manager APIs (and Cloud SQL if chosen). Create a dedicated service account for the broker.
2. Create a regional KMS key ring and an `ENCRYPT_DECRYPT` key with algorithm `GOOGLE_SYMMETRIC_ENCRYPTION`, generated by Cloud KMS. Record the exact enabled CryptoKeyVersion resource (`projects/.../cryptoKeyVersions/1`). Grant the broker service account `roles/cloudkms.cryptoKeyEncrypterDecrypter` on this key only. Choose software or HSM protection according to requirements; the broker uses the same native API.
3. Provision shared PostgreSQL. For Cloud SQL, configure Direct VPC egress and private DNS/network access. This broker uses direct PostgreSQL TLS: configure a server certificate/CA and DNS name that support **verify-full** and mount the trusted CA. A default Cloud SQL certificate/address combination may not support hostname verification; use Cloud SQL's supported DNS/SAN configuration and CA mode, or another PostgreSQL TLS endpoint. This template does not bundle the Cloud SQL Auth Proxy or a connector. Do not switch to `sslmode=disable` or `require` to bypass verification. Initialize the schema once from an allowed administrative path.
4. Create Secret Manager secrets for the OIDC client secret, database URL, and (optionally) the public broker JSON. Grant Secret Accessor only for those secrets to the broker service account. Use explicit numeric secret versions in the deployment, not `latest`.
5. Build a Linux x64 container and push to Artifact Registry:

```sh
docker build --platform linux/amd64 -f identity-broker/deploy/serverless/Dockerfile --target cloud-run -t REGION-docker.pkg.dev/PROJECT/REPOSITORY/lttl:test .
docker push REGION-docker.pkg.dev/PROJECT/REPOSITORY/lttl:test
```

6. Deploy the image to Cloud Run with the broker service account, port 8080, 1 GiB memory, concurrency 8, minimum instances 0 and maximum instances 2 for the pilot. Allow unauthenticated **invocation** so browsers can reach sign-in; the broker enforces protected API authorization. Mount the completed JSON as `/app/config/broker.json`, along with a CA if needed. Bind secret versions to `LTTL_OIDC_CLIENT_SECRET` and `LTTL_DATABASE_URL`. Cloud Run's runtime contract supplies `PORT`; the broker binds it automatically.
7. Set the stable service HTTPS origin in that JSON and register its `/auth/callback` with the IDP. Update the secret version/revision when finalizing the service URL. Validate discovery, authentication, native wrap/unwrap and fail-closed behavior from the deployed service account. Tokens come directly from the workload metadata endpoint; no gcloud installation or service-account JSON key is needed in the image.

Administrative creation examples (replace the public identifiers):

```sh
gcloud kms keyrings create lttl-test --location=us-central1 --project=PROJECT
gcloud kms keys create lttl-wrap --keyring=lttl-test --location=us-central1 --purpose=encryption --default-algorithm=google-symmetric-encryption --project=PROJECT
gcloud kms keys add-iam-policy-binding lttl-wrap --keyring=lttl-test --location=us-central1 --project=PROJECT --member=serviceAccount:BROKER_SERVICE_ACCOUNT --role=roles/cloudkms.cryptoKeyEncrypterDecrypter
```

The broker encrypts using the pinned key version and decrypts through its parent CryptoKey, as the API requires. Both operations include archive/key AAD and CRC32C integrity checks. Add old versions to `decrypt_key_ids` and leave them enabled when rotating.

References: [Rust on Cloud Run](https://docs.cloud.google.com/rust/docs/overview), [container deployment](https://docs.cloud.google.com/run/docs/deploying), [Cloud Run to PostgreSQL](https://docs.cloud.google.com/sql/docs/postgres/connect-run), [native encryption API](https://docs.cloud.google.com/kms/docs/reference/rest/v1/projects.locations.keyRings.cryptoKeys/encrypt), [decryption API](https://docs.cloud.google.com/kms/docs/reference/rest/v1/projects.locations.keyRings.cryptoKeys/decrypt).


# Shared serverless configuration and operations

Use one broker deployment and database per company/issuer. Start with the provider's `serverless/*.broker.example.json`. Set the stable public HTTPS origin, exact issuer and application client ID; add the cloud key's immutable reference. `database` must be `postgres` and `serverless` must be `true`. Do not configure an API stage prefix: callbacks and APIs live at the origin's root.

The function platform terminates HTTPS. The broker listens on `FUNCTIONS_CUSTOMHANDLER_PORT` (Azure), or `PORT` (default 8080). No Caddy, systemd, persistent function disk or cloud CLI is needed at runtime. Do not put archive files or database files in the function image.

## Secrets and persistent state

Inject these through protected platform settings/secret references, never public configuration or image layers:

- `LTTL_OIDC_CLIENT_SECRET`: the confidential Web app's secret value.
- `LTTL_DATABASE_URL`: PostgreSQL connection URL, with a limited database account and **`sslmode=verify-full`**. URL-encode credentials. Example shape only: `postgresql://USER:PASSWORD@DATABASE_DNS:5432/lttl?sslmode=verify-full&sslrootcert=/app/config/database-ca.pem`. Trust the database provider's CA and use a hostname matching its certificate. Do not bypass verification to solve connectivity errors.

**Do not set `LTTL_BROKER_MASTER_KEY` in native mode.** KMS authorization comes from the function's managed identity, Lambda execution role, or Cloud Run service account. An IDP user does not need permission to call KMS directly.

Provision managed PostgreSQL with backups, encryption at rest and network access restricted to the workload and administrative migration path. PostgreSQL providers with compatible TLS endpoints can be used across clouds. Azure Database for PostgreSQL, Aurora PostgreSQL (including Serverless v2), and Cloud SQL are examples; they are separately billed managed services, not VMs you administer. The broker uses SQL over TLS, not a cloud database Data API. For private addresses, configure the platform's VPC/VNet egress and DNS; preserve outbound access to IDP discovery/JWKS/token endpoints and KMS. A proxy/pooler must support transactions and prepared statements used by SQLx.

Before first deployment, run once from an administrative environment that can reach the database, with `LTTL_DATABASE_URL` injected securely:

```text
lttl-key-broker --init-db broker.json
```

This command creates the schema; it needs no OIDC secret or KMS access. Use a migration account for schema creation, then a runtime role with CONNECT, schema USAGE and SELECT/INSERT/UPDATE/DELETE on these tables only: `users`, `archives`, `audit`, `broker_metadata`, `login_flows`, `auth_codes`, `sessions`. There are no sequences. Do not share this database/schema with unrelated applications. Startup does not run PostgreSQL DDL.

Each instance uses at most two database connections. Cap instances/concurrency for the pilot so their combined pools fit the database connection limit. Cold starts perform discovery, database connection and a native unwrap check; measure cold-start latency and raise platform startup/request timeouts where supported. Keep workloads, KMS and database near each other. Do not claim zero cold-start overhead.

Login state, authorization codes and sessions are durable; lookups hash bearer identifiers and one-use operations use atomic SQL. Pending flows expire after five minutes, exchange codes after one minute, and sessions after five minutes. Expired rows are pruned on new sign-ins. The database contains PKCE verifiers and account metadata, so restrict read access and backups. The global soft flow/session cap is not a replacement for edge throttling. Apply per-client rate limits at the platform/edge, and redact Authorization headers and callback query strings from platform logs and tracing.

## Identity and archive access

Use OIDC Authorization Code + S256 PKCE, discovery/JWKS and the exact `/auth/callback` Web redirect. The broker validates signature, issuer, audience, nonce, expiry and fresh `auth_time` for `max_age=0`. MFA/location rules must be assigned and tested in the IDP. SAML requires a policy-aware OIDC federation bridge. Each recipient signs in once to enroll; the picker is not a Graph/SCIM directory sync. Authorization uses issuer + subject, never email. The sender is not automatically a recipient. The enrolled picker has a 1,000-active-user limit.

Opening grants are archive-specific and single-use. The broker checks recipients, account state and revocation again after the remote unwrap call. KMS failure never falls back to a local key. The desktop holds an archive key during its operation without persisting it. Authorized recipients can retain plaintext or keys, and a privileged local process can inspect memory; this is not DRM. Archive content is processed locally, with temporary plaintext inner archives on an encrypted, access-restricted scratch volume.

## Backup, rotation and rollout

- Back up PostgreSQL and retain every cloud key version referenced by its wrapped keys and `cloud-check` startup marker. Restore with the same hostname, issuer/client configuration and KMS access. Losing either the database or needed KMS key material makes archives unreadable. Do not delete keys as routine test cleanup while retained archives matter.
- Azure and GCP active wrapping IDs are pinned to a key **version**. When changing versions, put each old version in `decrypt_key_ids` and keep it enabled. That list permits decryption only; new wraps use `key_id`. The startup marker also depends on its old key version. Bulk rewrap/retirement automation is not implemented, so keep those old versions until a reviewed migration exists.
- AWS uses a full key ARN, never an alias. KMS automatic rotation behind that ARN retains decryption capability; changing to a different ARN requires the old ARN in `decrypt_key_ids` and continued access. Current AWS validation targets the commercial `aws` partition; Azure targets public `vault.azure.net`.
- Rotate the OIDC client secret independently through the platform secret store, roll the function instances, and verify sign-in before retiring the old secret. Updating an environment-backed secret does not update an already running instance.
- Rollout acceptance includes a login begun on one instance and completed on another, a cold start between login and exchange, replay rejection, revocation, and backup restore. `/health` is liveness only; success does not prove IDP or KMS permissions.

Local development can still use `serverless:false`, a SQLite path and `wrapping:{"provider":"local"}` behind loopback HTTPS termination. This is a separate legacy mode with a persistent secret master key. It is not the serverless configuration above.
