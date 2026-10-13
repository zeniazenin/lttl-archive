# Signed audit evidence, retention and legal holds

Alpha.3 adds organization audit records, an independently deployed serverless audit vault, retention schedules, and legal holds. The Windows installer includes the administrator CLI, this guide and public vault deployment sources. Upgrade the broker to alpha.3 and configure the vault separately; a desktop upgrade alone does not enable signed export. These controls support a compliance program; they do not establish HIPAA or SOX compliance.

## Trust boundaries

The broker signs each event and each export batch with Ed25519. It holds a dedicated signing seed in its secret store, separate from archive wrapping keys. The audit vault pins the corresponding public key and a stable stream ID. It must run in a separately administered cloud account/project/subscription or equivalent isolated security boundary. The broker receives only permission to submit signed evidence, not cloud storage credentials or deletion privileges.

The vault checks event signatures, exact-byte hashes, contiguous sequence numbers and the preceding batch. It writes create-only objects under both the first sequence and batch digest. Conflicting histories are rejected; identical retries repair partial delivery safely. Only an exact-digest storage receipt permits the broker to mark records exported. Database write failure prevents successful mutation/key response; remote vault failure leaves evidence queued for retry. Export is scheduled, not synchronous with every key release. Monitor export lag: the interval before independent preservation is a remaining exposure window. A compromised broker process/signing seed can forge new events. A database administrator can damage local state; independent evidence detects conflicts with exported history but cannot prevent every direct database attack or detect all deletion before export.

Each batch contains at most 16 records. Records include actor ID, archive ID, UTC seconds, action, details, sequence and preceding event hash. Authentication acceptance also captures verified issuer/subject, authentication time and ACR when present. Creation, audience changes, approval attestations, authentication results, key decisions, revocation, retention/hold actions, disposition, administrator denial, and broker audit configuration at startup are recorded. Optional tracking events are included when tracking is enabled. Disabling location tracking does not disable broker security auditing. Keep case references and reasons free of PHI, secrets and unnecessary personal data.

An approval attestation records an authenticated administrator's decision and reference. It does **not** create a two-person approval workflow or automatically authorize another action. Direct cloud KMS operations, database edits and cloud IAM changes require the corresponding platform audit logs. No broker endpoint deletes shared KMS master keys.

## Configure and migrate the broker

Back up the database and stop older broker instances before upgrading. Run `lttl-key-broker --init-db broker.json` using the migration identity. The schema is additive. Existing archive records receive indefinite preservation and no access expiration. Old `audit` rows remain in the legacy table and are not retroactively signed: retain an independently protected pre-upgrade snapshot and clearly label it as legacy evidence.

Add this object to the existing broker configuration **before the first upgraded startup**:

```json
"audit": {
  "enabled": true,
  "stream_id": "company-production-01",
  "vault_url": "https://AUDIT_FUNCTION_HOST/audit",
  "administrator_subjects": ["EXACT_OIDC_SUBJECT"],
  "evidence_retention_days": 2555,
  "archive_retention_days": 2555
}
```

2555 days is only an example, not a legal recommendation or calendar-year calculation. Choose schedules with your records owner. **Zero means indefinite**, and is the default. The evidence deadline is snapshotted per record and the archive deadline at creation. Configuration changes do not shorten existing deadlines. An archive deadline can only be extended (or made indefinite). Indefinite preservation cannot be changed to a finite deadline through the API. Global audit records are retained indefinitely locally; archive-specific records can expire subject to the safeguards below.

Set separate secrets in the broker's platform secret store:

| Variable | Purpose |
|---|---|
| `LTTL_AUDIT_SIGNING_SEED` | Cryptographically random 32-byte base64url value without padding; stable across instances/restarts |
| `LTTL_AUDIT_VAULT_TOKEN` | At least 32 random bytes of text; matches the vault ingest token |
| `LTTL_AUDIT_DISPATCH_TOKEN` | Separate random scheduler credential, at least 32 bytes of text |

The existing `--generate-master-key` command generates a suitable random 32-byte value; generate a **new distinct value** for the signing seed, never reuse the actual master key. Transfer it directly into the secret store without putting it in source control or chat. With the seed configured, `lttl-key-broker --audit-public-key broker.json` outputs only the public key and stream ID for the security team to pin in the vault.

Once signing is enabled, changing the identity or disabling audit makes startup fail. This release does not automate signing-key rotation or retroactive conversion of an already-running unsigned new ledger. Enabling signing after such unsigned records exist requires an explicitly planned evidence migration/new stream. Do not reset audit tables, bypass this check, or run older broker versions against the upgraded database.

Configure a scheduled authenticated `POST /v1/audit/dispatch`, with `Authorization: Bearer <LTTL_AUDIT_DISPATCH_TOKEN>`. Each request sends one batch (10-second HTTP timeout), then performs local cleanup. Repeat until `batch` is null and schedule regularly even when traffic is idle. Rate-limit and size-limit ingress. Monitor export lag, pending rows, signature/continuity errors and storage failures independently. The scheduler credential cannot change holds or release archive keys. Existing tracking delivery uses its separate endpoint/token.

## Independent serverless vault

Deploy [audit-vault](../identity-broker/audit-vault/) under the security team's identity. Install its requirements into the deployment package, then use:

| Cloud | Function entry point | Storage adapter and preservation |
|---|---|---|
| Azure Functions Python v2 | `function_app.py`, POST `/api/audit` | Azure Blob container with a security-admin-managed active legal hold |
| AWS Lambda, API Gateway HTTP API | `vault.lambda_handler` | Versioned S3 Object Lock bucket; every object receives legal hold `ON` |
| Google Cloud Run functions | `main.ingest` | Cloud Storage; each object is created with a temporary hold, without automatic expiry |

Set `LTTL_VAULT_PROVIDER` to `azure`, `aws`, or `gcp`; `LTTL_VAULT_STREAM` and `LTTL_VAULT_PUBLIC_KEY` to the pinned broker identity; and secret `LTTL_VAULT_INGEST_TOKEN` to the broker's vault token. Azure also uses `LTTL_VAULT_ACCOUNT_URL` and `LTTL_VAULT_CONTAINER`. AWS/GCP use `LTTL_VAULT_BUCKET`. Use workload identity/default platform credentials for storage. Set the broker `vault_url` to the actual routed HTTPS endpoint. Limit request bodies to 6 MiB at ingress. Never expose plaintext HTTP beyond local tests.

The broker has **no** storage identity. The vault runtime needs only object create/read and the ability to create objects with holds; no delete, overwrite, hold release, bucket policy change, or KMS administration. In AWS explicitly deny setting object legal hold to OFF and deny object/version deletion; do not grant bypass-governance permissions. In GCP withhold `storage.objects.update` and delete permissions so the runtime cannot remove holds. In Azure give the runtime data-plane read/create permissions and no management-plane permission to remove the container hold. Validate these policies using the exact workload identity before production.

All external evidence is deliberately held indefinitely in this first implementation, including batches containing records for multiple archives. Broker expiry/hold release never removes external storage holds. Disposition is a separate security-team process: check **every record's** retention and all applicable holds before releasing any batch/container hold. This avoids a delayed hold event racing automatic vault deletion. The receiver checks hold state when reading/acknowledging objects, but cloud administrators remain part of the trust boundary. Do not configure lifecycle deletion or remove holds independently of records governance. Cloud retention configuration, tenant sign-in, and IAM still require live acceptance testing; the automated suite uses mock cloud storage.

Provider references: [Azure immutable storage](https://learn.microsoft.com/en-us/azure/storage/blobs/immutable-storage-overview), [S3 Object Lock](https://docs.aws.amazon.com/AmazonS3/latest/userguide/object-lock.html), [GCP object holds](https://cloud.google.com/storage/docs/object-holds).

## Administrator operations

Administrators are selected by exact OIDC subject within the broker's configured issuer, never by email or client input. Use fresh organization sign-in through the CLI:

```powershell
lttl identity retention .\shared.lttl --profile .\company.json
lttl identity retention .\shared.lttl --profile .\company.json --action .\hold.json
```

Example `hold.json`:

```json
{"action":"hold","hold_id":"REPLACE_WITH_RANDOM_32_BYTE_BASE64URL_ID","reason":"Case reference 2026-001"}
```

The placeholder must be replaced with a real 43-character base64url ID (32 random bytes, no padding). Use the same ID to release that hold. Multiple holds can coexist; all must be released before destruction. The CLI sends administrator requests to `GET/POST /v1/archives/{id}/retention` using a management session. Archive-opening grants and ordinary recipients cannot administer retention.

| Action | Additional fields | Effect |
|---|---|---|
| `extend` | `retain_until` (Unix UTC seconds; 0 indefinite), `reason` | Extend preservation only |
| `access_expiry` | `expires` (Unix UTC seconds; 0 none), `reason` | Change access expiry without shortening preservation |
| `hold` | `hold_id`, `reason` | Preserve key/evidence; reject if key already destroyed |
| `release_hold` | `hold_id`, `reason` | Release this hold only; keep its history |
| `destroy_key` | `reason` | Erase the live wrapped archive data key only after finite retention expires and no hold remains |
| `approval` | `reference`, `decision` (`approved` or `denied`), `reason` | Record a decision attestation; no permission changes |

The owner can change the audience with `POST /v1/archives/{id}/recipients`, body `{"recipients":["BROKER_USER_ID"]}`. Old/new lists are recorded transactionally. Access expiry and revocation still apply on held archives: preserving records must not force ongoing recipient access.

Key destruction and its audit record commit together. The operation is logical deletion from the live database, not guaranteed erasure from backups, storage remnants, recipients or already-released keys. Preserve required KMS key versions with separate cloud IAM/deletion protection; a broker hold cannot stop a cloud administrator deleting a master key.

## Cleanup and limits

Local audit deletion requires: the event's snapshotted finite deadline elapsed, the archive's finite deadline elapsed, no active legal hold, and an acknowledged external copy. Tracking additionally requires completed webhook delivery and no pending audit evidence for the archive. Undelivered tracking messages no longer expire at 90 days. Indefinite and unexported evidence remains. Holds and cleanup serialize on the archive lifecycle row; disposition takes the same lock. A hold cannot recover information legitimately deleted before it was placed.

The broker stores keys and metadata, **not archive bytes**. Put retained `.lttl` files in company-controlled immutable storage with matching holds; this feature cannot prevent deletion of a copy on someone's laptop. There is no archive-file deletion API or automatic external archive upload. Decrypted files remain outside these retention controls. No live infrastructure is provisioned by the client. Administrative retention/hold controls are currently CLI/API features, not new GUI panels.

Offline batch verification: `python identity-broker/audit-vault/vault.py batch.json --stream STREAM --public-key PUBLIC_KEY`. This validates signatures and in-batch continuity; verify predecessor batches too for full history. Keep public keys and evidence access under auditor control. See [test results](audit-retention-validation.json).
