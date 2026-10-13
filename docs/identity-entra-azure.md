# lttl registration — Microsoft Entra ID

These instructions create an application in the company’s existing IDP. The lttl key service is a separate, company-hosted key-release API, not another IDP. Registration is performed by the tenant administrator; this guide does not create it automatically.

## Registration values

- Application: lttl key service
- Type: confidential Web application
- Flow: Authorization Code with S256 PKCE
- Redirect / callback URI: `https://keys.example.com/auth/callback`
- Scopes: `openid profile email`
- No wildcard redirect URIs, implicit flow, password grant, refresh token or desktop client secret.

## Provider steps

1. In Microsoft Entra admin center, open App registrations → New registration. Name it **lttl key service**. Select **Accounts in this organizational directory only**.
2. Add a **Web** platform with the exact redirect URI below. This is a confidential server application; do not register the broker as a SPA or native public client, and do not enable implicit grants.
3. Copy the Directory (tenant) ID and Application (client) ID. Create a client secret under Certificates & secrets and store its **value** only in the key-service secret store. Plan secret rotation before expiration.
4. Under Token configuration, add the optional **auth_time** ID-token claim. The service requires fresh authentication time and fails closed without it. No Microsoft Graph directory permission is needed for the enrolled-user picker.
5. In Enterprise applications, require assignment and assign a pilot group. Apply Conditional Access to this application: required MFA/authentication strength and approved network/location rules. Test allowed and denied cases; a successful password sign-in alone does not prove the policy is configured.

[App registration](https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app) · [OIDC](https://learn.microsoft.com/en-us/entra/identity-platform/v2-protocols-oidc) · [Optional claims](https://learn.microsoft.com/en-us/entra/identity-platform/optional-claims-reference) · [Conditional Access](https://learn.microsoft.com/en-us/entra/identity/conditional-access/overview)

## Key service configuration

Copy these IDP fields into the chosen hosting example in `identity-broker/deploy/serverless/`, then add its native KMS key reference. The hosting guide below supplies database and function settings.

```json
{
  "client_id": "CLIENT_ID",
  "issuer": "https://login.microsoftonline.com/TENANT_ID/v2.0",
  "public_url": "https://keys.example.com"
}
```

The key service runs on Azure Functions, AWS Lambda or Google Cloud Run with shared PostgreSQL state. Inject `LTTL_OIDC_CLIENT_SECRET` and `LTTL_DATABASE_URL` through protected platform settings. Native KMS wraps archive keys without exporting the cloud master key. Do not set `LTTL_BROKER_MASTER_KEY` for these deployments. No VM or cloud CLI is required at runtime. See the hosting instructions below and `identity-broker/deploy/test-environment.md`. Build the broker with Rust 1.92 or newer; initialize the PostgreSQL schema before deployment. Restrict logs so they do not capture authorization headers, response keys or callback query strings.

## Desktop profile

Save this JSON, or enter the service URL in lttl → Identity & sharing.

```json
{
  "broker_url": "https://keys.example.com",
  "provider": "entra"
}
```

## Enroll and share

Each recipient signs in once to enroll with this key service. Refresh the recipient picker, select the intended accounts, and create a `.lttl` protected archive. Names/email are display labels only; authorization uses the verified issuer and subject. The creator is not automatically added as a recipient. To open/list/test/extract, the recipient must sign in again and be in the archive’s allowlist. The key service must be reachable.

The sender already has the source data, and an authorized recipient can retain decrypted data or keys. This is access-controlled encryption, not DRM or revocation of copies already obtained. The key-service operator can access stored keys. Keep its database and cloud key versions recoverable; losing either can make archives permanently unreadable. The preview uses temporary plaintext archive files during compression/decryption and cleans them on normal completion/errors, but cannot guarantee erasure after a crash or from storage snapshots. Use an encrypted, access-restricted temporary volume.

## Acceptance checks

Validate recipient success, nonrecipient denial, two accounts with the same display email remaining distinct, disabled-user/policy denial, fresh MFA/location enforcement, wrong tenant, service outage, revocation, tampered archive, and a second opening requiring fresh sign-in. Changing the IDP app client ID or issuer can change subject identifiers and break existing access; plan identity migrations explicitly.

---

# Azure Functions + native Key Vault

Use an **Azure Functions custom handler** with the supplied HTTP forwarding configuration. The Flex Consumption plan lists custom handlers 1.0 as supported; choose an available region, Linux x64, and that runtime. No VM is required. Entra is the initial user-IDP pilot; another compatible company IDP can be substituted.

1. Create a dedicated test resource group, Function App with its required storage account, and a managed identity. Use the app's stable HTTPS hostname as `public_url`. Enable HTTPS-only. Use the custom handler runtime; retain the root catch-all route and anonymous trigger from the package. The broker performs OIDC authorization itself; platform “Easy Auth” or function-key authentication must not intercept its browser callbacks/API.
2. Create a Key Vault with RBAC, soft delete and purge protection. In **Keys**, generate a new RSA 3072-bit key called `lttl-wrap`, allowing `wrapKey` and `unwrapKey`. Generate it within Key Vault; do not create/import a local master secret. RSA-HSM can be used with a suitable vault tier if required. Copy the full versioned key identifier into `wrapping.key_id`.
3. Grant the Function App's managed identity **Key Vault Crypto Service Encryption User**, scoped to this key. It needs wrap/unwrap, not key export, key creation or vault administration. If using a user-assigned identity, also set `wrapping.client_id` to its client ID and configure that identity on the Function App.
4. Create managed PostgreSQL and initialize it using the common instructions. Configure verified TLS, DNS and VNet egress if private. Store the IDP client secret and database URL as separate Key Vault **secrets**. These are credentials, not the wrapping key. Give the managed identity **Key Vault Secrets User** only for those secrets and use versioned Key Vault references in application settings `LTTL_OIDC_CLIENT_SECRET` and `LTTL_DATABASE_URL`.
5. Build the Linux x64 function package from the repository root on a Docker-enabled builder:

```sh
docker build --platform linux/amd64 -f identity-broker/deploy/serverless/Dockerfile --target azure-package --output type=local,dest=.qa/azure-function .
```

6. Copy your completed `serverless/azure.broker.example.json` to `.qa/azure-function/broker.json`. Include a public PostgreSQL CA certificate if your connection URL references one; use its path under `/home/site/wwwroot/`. Keep the binary executable. Publish the **contents** of this directory with Azure Functions Core Tools (`func azure functionapp publish YOUR_FUNCTION_APP --no-build`) or the supported custom-handler package deployment flow. Do not include local settings or secrets. The package contains `host.json`, `broker/function.json`, and a statically linked Rust executable.
7. Configure the IDP Web callback to `https://YOUR_FUNCTION_HOST/auth/callback`, complete Entra registration in the test guide, and run the common acceptance cases. Verify managed-identity KMS access from the deployed function; a successful local admin token test does not prove its role assignment. Azure Functions exposes the workload token endpoint; the broker retrieves and caches short-lived vault tokens directly without `az` at runtime.

Optional administrative key-creation command (no private material is printed):

```sh
az keyvault key create --vault-name YOUR_VAULT --name lttl-wrap --kty RSA --size 3072 --ops wrapKey unwrapKey --query key.kid -o tsv
```

Native operations use RSA-OAEP-256. The encrypted payload includes an archive/key context commitment because RSA wrapping has no separate AAD parameter. Keep old versions enabled and explicitly allowlisted when rotating.

References: [custom handler packaging/HTTP forwarding](https://learn.microsoft.com/en-us/azure/azure-functions/functions-custom-handlers), [Flex supported runtimes](https://learn.microsoft.com/en-us/azure/azure-functions/flex-consumption-plan), [Key Vault roles](https://learn.microsoft.com/en-us/azure/key-vault/general/rbac-guide), [managed identity endpoint](https://learn.microsoft.com/en-us/azure/app-service/overview-managed-identity), [native wrap API](https://learn.microsoft.com/en-us/rest/api/keyvault/keys/wrap-key/wrap-key?view=rest-keyvault-keys-2025-07-01).


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
