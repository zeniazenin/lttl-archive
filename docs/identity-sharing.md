# Identity-protected sharing

Optional [archive access tracking](archive-tracking.md) adds an authenticated per-file policy, signed organization security webhooks and country-watchlist alerts. It requires the upgraded broker schema and explicit webhook setup.

Alpha.3 adds [signed audit evidence, retention and legal holds](audit-retention.md), with independently administered cloud storage and fresh-IDP administrator CLI/API access. Security auditing operates independently of optional location tracking. The broker must also be upgraded and configured.

lttl can wrap ZIP, TAR.ZST or TAR.LZ4 in an authenticated encrypted `.lttl` envelope. The company runs a small Rust key service. Recipients authenticate through the existing IDP before listing, testing or extracting an archive. The compression backends remain available inside the encrypted envelope.

## Two independent choices

| Choice | Options |
|---|---|
| Identity provider | Entra, Okta, Auth0, Keycloak, generic compatible OIDC; SAML through an OIDC federation bridge |
| Key-service hosting | Azure Functions, AWS Lambda, Google Cloud Run; optional self-hosting |

Google Workspace registration guidance is included, but the broker's fresh `auth_time` requirement must be proven with that provider; it fails closed if unsupported. Google Cloud **hosting** does not depend on using Google as the IDP. No provider has been validated against a live company tenant yet.

The service authenticates users and holds protected archive keys and recipient IDs, **not archive files**. The function workload identity calls native KMS; it is separate from the identity of a person opening an archive. No VM is required. Start with the [Entra and native key-store test guide](../identity-broker/deploy/test-environment.md). See the [deployment options](../identity-broker/deploy/README.md).

## Desktop workflow

1. Open **Identity & sharing**. Choose the company's IDP and enter the HTTPS key-service URL. Save the connection. The profile contains no credentials.
2. Under **Administrator setup instructions**, choose Azure, AWS, Google Cloud or Self-hosted, enter tenant/domain and client ID, then generate/save instructions. The administrator registers the Web app and configures the selected hosting environment. No app registration or cloud resources are created automatically.
3. Each recipient uses **Sign in / enroll** once. On Create, enable **Encrypt for selected organization users**, refresh recipients, and select the intended accounts. The directory is enrolled users, not a full Graph/SCIM directory. Add yourself if you need to reopen the archive.
4. Create the archive, then distribute the `.lttl` file through the company's usual channel. The recipient configures the same trusted service and signs in when opening it. An archive cannot redirect the desktop to an unconfigured key service.

The Explorer installer includes sign-in-and-extract actions for `.lttl`. The script remains opt-in and does not change default file associations. If previously installed, uninstall the owned lttl verbs before installing the updated set. Registry changes were not applied during this implementation.

## Command line

```powershell
# Hosting and IDP can be mixed; this example is Entra on AWS.
.\target\release\lttl.exe identity guide --provider entra --hosting aws --broker-url https://keys.example.com --tenant TENANT_ID --client-id CLIENT_ID -o setup.md
.\target\release\lttl.exe identity configure --provider entra --broker-url https://keys.example.com -o company.json
.\target\release\lttl.exe identity users --profile company.json

# Use the enrolled opaque account IDs returned by identity users.
.\target\release\lttl.exe create .\Documents -o shared.lttl --format tar-zstd --identity-profile company.json --recipient RECIPIENT_ID
.\target\release\lttl.exe extract shared.lttl -d .\Received --identity-profile company.json
.\target\release\lttl.exe identity revoke shared.lttl --profile company.json
```

Only the creator can revoke future key releases. Already decrypted copies or retained keys cannot be recalled.

## Security contract and limits

- Each archive has a random 256-bit key. XChaCha20-Poly1305 authenticates the header and bounded 1 MiB records, their lengths and order, plus a mandatory final record. Changed, truncated or extended streams are rejected before extracted files are published. Archive content and member names are encrypted; broker URL, inner format, nonce and random archive ID are visible.
- The broker validates OIDC signature, issuer, audience, expiry, nonce and fresh authentication time. Native loopback and IDP authorization flows use PKCE. Recipient authorization is bound to verified issuer + subject, not email or display name. MFA/location rules must be configured and tested in the actual IDP. A fresh login request does not independently prove that a particular MFA policy was applied.
- The opening authorization is single-use and bound to one archive. The broker rechecks recipient membership and revocation before releasing its key. The desktop holds tokens/archive keys during the operation and does not save them. No recipient private-key files, offline fallback or key export UI are provided. A privileged process can still inspect application memory; an authorized user can save plaintext. This is not DRM or guaranteed destruction at session expiration.
- Native Azure Key Vault, AWS KMS and Google Cloud KMS wrap each archive data key; the master/wrapping keys stay inside their cloud key service. The broker receives only individual archive keys, briefly in memory. Records are bound to the archive and configured cloud key, with no fallback on KMS denial. Broker operators remain trusted because they can invoke unwrap. Legacy local-master mode is retained only as an explicit alternative; migrating an existing local database requires an offline rewrap tool that is not yet implemented.
- Encrypted archive creation and opening use temporary plaintext inner archives. Use encrypted, access-restricted temporary storage. Normal cleanup does not guarantee secure erasure from crash dumps, snapshots or storage media. The envelope supports up to 110 GiB of compressed plaintext input. The broker supports up to 1,000 active enrolled recipients and one company/issuer per deployment.
- Serverless deployments use shared PostgreSQL for recipients, wrapped keys, revocations, short-lived sign-in state and single-use grants. Atomic database operations prevent grant replay across instances. Managed database/KMS/networking are separately provisioned and billed. The broker must be reachable for every new opening.

## Validation status

The [local integration report](identity-validation.json) records 75 successful checks using a disposable RSA-signed mock OIDC provider and the real release broker/CLI. It verifies all three formats by file hash, denied recipients, same-email distinct accounts, invalid token claims/signatures, native PKCE and replay rejection, one-use grants, revocation, changed/truncated archives, restart recovery, and no file publication on failed extraction. It is not a live Entra/cloud test.

See the [serverless/native-KMS validation record](identity-serverless-validation.md) for current contract, storage and packaging checks and their limits. The old [hosting report](identity-hosting-validation.json) is historical evidence for the legacy secret-loader implementation. The UI has not had a visual acceptance review for this feature. Live cloud IAM, IDP MFA/location enforcement and cloud deployment remain pilot acceptance requirements.
