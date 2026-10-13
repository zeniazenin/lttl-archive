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
