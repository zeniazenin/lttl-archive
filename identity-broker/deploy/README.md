# Serverless lttl key service

The Rust broker runs on demand. It does not need a VM, a GPU, or uploaded archive files. User authentication and key hosting are independent: Entra can protect archives whose keys are in AWS or Google Cloud.

| Deployment | Native key wrapping | Durable state |
|---|---|---|
| [Azure Functions](azure.md), custom HTTP handler | Key Vault RSA-OAEP-256 | Shared PostgreSQL |
| [AWS Lambda](aws.md), HTTP Web Adapter | KMS symmetric Encrypt/Decrypt | Shared PostgreSQL |
| [Google Cloud Run](gcp.md), serverless Rust container | Cloud KMS symmetric Encrypt/Decrypt | Shared PostgreSQL |
| [Optional self-hosted](self-hosted.md) | Native KMS, or explicit legacy local mode | PostgreSQL, or SQLite for a single local instance |

Cloud Run is the Google deployment target for this Rust application; it is not a Cloud Run functions language-runtime package. No VM administration is required. Managed database, key operations, networking, and secret storage can still incur charges while function compute scales to zero.

Start with [the Entra + cloud test-environment guide](test-environment.md), then the provider guide and [common configuration](common.md). Deployment examples do not create any paid resources automatically.

## Alpha.2 tracking upgrade

Upgrade the broker and run `lttl-key-broker --init-db CONFIG.json` against its existing managed PostgreSQL database before starting alpha.2. This adds tracking policy, event and webhook-outbox tables; retain the existing wrapping-key configuration and archive data. SQLite development instances initialize the additional tables at startup. Upgrade desktop recipients before distributing tracked archives because older clients reject the new header field.

Tracking is disabled by default. To enable it, configure the organization webhook and separate signing/dispatch secrets, then schedule authenticated POSTs to `/v1/tracking/dispatch` for retries and retention cleanup. An organization can require tracking on newly created archives. External-user alerts require an explicit broker membership list; country alerts require trusted, signed ingress estimates. See [archive tracking setup](../../docs/archive-tracking.md) for the complete configuration, receiver example and deployment limits. No webhook, scheduler or location service is provisioned automatically.

Native mode generates the master/wrapping key **inside the cloud key service**. Only individual archive data keys enter broker memory. The broker stores a wrapped data key, recipient IDs, revocations, audit events and short-lived authorization state in PostgreSQL. It never receives archive contents. All instances share that database; atomic consumption prevents grant replay across instances. Local wrapping remains available for development, but serverless mode refuses it and refuses SQLite.

`serverless/Dockerfile` provides Lambda, Cloud Run, and an Azure Functions package target. `serverless/*.broker.example.json` are the current cloud configurations. The old `launch.py`, `*.example.json` directly in this directory, systemd unit and Caddy file are **legacy local-master deployments only**, retained for existing installations. Do not use their master-secret injection with native KMS: startup rejects it. A local-master database requires an explicit offline rewrap migration before native use; that migration is not implemented. Use a new database for the pilot, and retain the old service/keys for old archives.

Validation is split deliberately: local HTTP contract tests exercise Azure/GCP REST and the AWS SDK signing/serialization path; the real CLI/OIDC harness exercises archive sharing. Neither proves cloud IAM, MFA, network access or a deployed function. See [the validation record](../../docs/identity-serverless-validation.md) for what actually ran. The live `--check-kms` and acceptance procedure are in the test guide.

The desktop's **Identity & sharing → Administrator setup instructions** combines IDP registration with these hosting instructions. CLI equivalent:

```powershell
.\target\release\lttl.exe identity guide --provider entra --hosting azure --broker-url https://YOUR_FUNCTION_HOST --tenant TENANT_ID --client-id CLIENT_ID -o setup.md
```
