# Optional self-hosted deployment

Serverless Azure, AWS and Google deployments are the primary cloud options. Organizations that already run their own services can instead place this Rust broker behind their HTTPS gateway.

Use native KMS plus shared PostgreSQL and `serverless:true` for a stateless container, injecting `PORT`, `LTTL_DATABASE_URL` and `LTTL_OIDC_CLIENT_SECRET`. Configure a supported cloud workload identity; Azure/GCP metadata acquisition expects their cloud environment. AWS uses the SDK credentials chain. Do not deploy manually copied long-lived cloud credentials. The explicit Azure/GCP `auth:environment` mode is intended for short-lived administrative tests.

Legacy local operation is also retained: `serverless:false`, `listen:127.0.0.1:8787`, a persistent SQLite file and `wrapping:{"provider":"local"}`. Build with Rust 1.92+, inject `LTTL_OIDC_CLIENT_SECRET` and a retained `LTTL_BROKER_MASTER_KEY` (32 random bytes, unpadded base64url). The master is in this process's memory, unlike native KMS mode. The old systemd/Caddy and `launch.py` examples support this legacy path. Protect its files and online SQLite backups; keep the master recoverable. Do not silently point native KMS at a legacy database: an explicit offline rewrap migration is required and is not yet implemented.

These optional deployment modes do not change the IDP registration or desktop archive format.
