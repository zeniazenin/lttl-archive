# lttl registration — Okta

These instructions create an application in the company’s existing IDP. The lttl key service is a separate, company-hosted key-release API, not another IDP. Registration is performed by the tenant administrator; this guide does not create it automatically.

## Registration values

- Application: lttl key service
- Type: confidential Web application
- Flow: Authorization Code with S256 PKCE
- Redirect / callback URI: `https://keys.example.com/auth/callback`
- Scopes: `openid profile email`
- No wildcard redirect URIs, implicit flow, password grant, refresh token or desktop client secret.

## Provider steps

1. In the Okta Admin Console, create an app integration with **OIDC - OpenID Connect** and **Web Application**. Enable Authorization Code.
2. Set Sign-in redirect URI to the exact callback below. Restrict assignments to your intended users/groups.
3. Copy the client ID and client secret. Configure the issuer shown by your Okta authorization server; the organization URL below is a starting point, not a substitute for checking discovery. Use client_secret_post for token endpoint authentication.
4. Apply the app sign-on/authentication policy for MFA, device requirements and network zones. Confirm that max_age=0 produces a fresh auth_time claim. The application requests a fresh login for every archive-opening authorization.

[Okta web redirect integration](https://developer.okta.com/docs/guides/sign-into-web-app-redirect/) · [Create OIDC integrations](https://help.okta.com/en-us/content/topics/apps/apps_app_integration_wizard_oidc.htm)

## Key service configuration

Copy these IDP fields into the chosen hosting example in `identity-broker/deploy/serverless/`, then add its native KMS key reference. The hosting guide below supplies database and function settings.

```json
{
  "client_id": "CLIENT_ID",
  "issuer": "https://company.okta.com",
  "public_url": "https://keys.example.com"
}
```

The key service runs on Azure Functions, AWS Lambda or Google Cloud Run with shared PostgreSQL state. Inject `LTTL_OIDC_CLIENT_SECRET` and `LTTL_DATABASE_URL` through protected platform settings. Native KMS wraps archive keys without exporting the cloud master key. Do not set `LTTL_BROKER_MASTER_KEY` for these deployments. No VM or cloud CLI is required at runtime. See the hosting instructions below and `identity-broker/deploy/test-environment.md`. Build the broker with Rust 1.92 or newer; initialize the PostgreSQL schema before deployment. Restrict logs so they do not capture authorization headers, response keys or callback query strings.

## Desktop profile

Save this JSON, or enter the service URL in lttl → Identity & sharing.

```json
{
  "broker_url": "https://keys.example.com",
  "provider": "okta"
}
```

## Enroll and share

Each recipient signs in once to enroll with this key service. Refresh the recipient picker, select the intended accounts, and create a `.lttl` protected archive. Names/email are display labels only; authorization uses the verified issuer and subject. The creator is not automatically added as a recipient. To open/list/test/extract, the recipient must sign in again and be in the archive’s allowlist. The key service must be reachable.

The sender already has the source data, and an authorized recipient can retain decrypted data or keys. This is access-controlled encryption, not DRM or revocation of copies already obtained. The key-service operator can access stored keys. Keep its database and cloud key versions recoverable; losing either can make archives permanently unreadable. The preview uses temporary plaintext archive files during compression/decryption and cleans them on normal completion/errors, but cannot guarantee erasure after a crash or from storage snapshots. Use an encrypted, access-restricted temporary volume.

## Acceptance checks

Validate recipient success, nonrecipient denial, two accounts with the same display email remaining distinct, disabled-user/policy denial, fresh MFA/location enforcement, wrong tenant, service outage, revocation, tampered archive, and a second opening requiring fresh sign-in. Changing the IDP app client ID or issuer can change subject identifiers and break existing access; plan identity migrations explicitly.

---

# AWS Lambda + native KMS

Deploy the Rust HTTP broker as a Lambda container using AWS Lambda Web Adapter. A Function URL provides HTTPS. No EC2 instance is needed, and the user IDP can still be Entra, Okta or another compatible OIDC provider.

1. Create a customer-managed **symmetric**, `ENCRYPT_DECRYPT` KMS key with origin **AWS_KMS**, in the Lambda region. Copy its full `arn:aws:kms:REGION:ACCOUNT:key/UUID` into `wrapping.key_id`; configure the same `wrapping.region`. Do not use an alias, imported key material, or Secrets Manager for the master key.
2. Create a Lambda execution role with normal log permissions and `kms:Encrypt`/`kms:Decrypt` for that exact ARN. The key policy must also allow the role (directly, or through the account's IAM delegation). Optionally constrain `kms:EncryptionContext:lttl-purpose` to `archive-key-v1`; the broker also supplies `archive-id` and `wrapping-key`. Do not grant wildcard KMS administration.
3. Provision PostgreSQL, for example Aurora PostgreSQL Serverless v2. Initialize the schema once. Configure Lambda VPC subnets/security groups and outbound connectivity to KMS and the IDP. A VPC-attached function does not automatically get internet access. Use database DNS and trusted RDS CA certificates with `sslmode=verify-full`; package the public CA under `config/` if needed.
4. Create an ECR repository. Prepare `serverless/config/broker.json` from the AWS example, containing only public configuration. Build and push the Linux x64 image using an authenticated ECR workflow:

```sh
docker build --platform linux/amd64 -f identity-broker/deploy/serverless/Dockerfile --target lambda -t YOUR_ECR_REPOSITORY:lttl-test .
docker push YOUR_ECR_REPOSITORY:lttl-test
```

5. Create an x86_64 Lambda using this image, the execution role, 1024 MB memory, a 120-second timeout, and a small reserved concurrency (for example 2 during the pilot). Create a Function URL with auth type **NONE** and the required resource policy for URL invocation. This permits browser access; the broker still authenticates/authorizes all protected endpoints. Do not place an IAM-signing requirement in front of its browser callback. Keep CORS disabled unless a separately reviewed browser client requires it.
6. Copy the Function URL origin into `broker.json`, update the IDP's exact `/auth/callback` Web redirect, rebuild/push and deploy the final image digest. The URL remains stable when function code is updated. Inject `LTTL_OIDC_CLIENT_SECRET` and `LTTL_DATABASE_URL` through encrypted Lambda environment settings using your approved secret provisioning workflow. Lambda does **not** resolve a Secrets Manager ARN written as an environment value: supply the actual secret via your deployment's secret-resolution mechanism. Never put secret values in shell arguments, image layers or public config. Restrict permission to read Lambda configuration.
7. Test from the function's execution role. The official AWS Rust SDK signs requests and uses the platform's role credentials; do not deploy static AWS access keys. Exercise permission denial and cold starts. The Web Adapter readiness path is `/health`; asynchronous initialization is enabled for cold starts, but an unhealthy KMS/IDP/database must still prevent readiness.

An example runtime KMS statement, added to the role policy (key policy authorization is also required):

```json
{"Effect":"Allow","Action":["kms:Encrypt","kms:Decrypt"],"Resource":"arn:aws:kms:REGION:ACCOUNT:key/UUID","Condition":{"StringEquals":{"kms:EncryptionContext:lttl-purpose":"archive-key-v1"}}}
```

Automatic rotation on the same KMS key ARN preserves decryption of old ciphertext. A different key ARN must retain the old ARN in `decrypt_key_ids` while old archives and the startup marker use it.

References: [Lambda Web Adapter](https://github.com/aws/aws-lambda-web-adapter), [Function URL authorization](https://docs.aws.amazon.com/lambda/latest/dg/urls-auth.html), [KMS encryption context](https://docs.aws.amazon.com/kms/latest/developerguide/encrypt_context.html), [Rust KMS SDK examples](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/rust_kms_code_examples.html).


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
