# Set up an IDP + native key-store pilot

Start with **Microsoft Entra ID + Azure Functions + Key Vault** if you want one provider for the first pilot. AWS Lambda/KMS and Google Cloud Run/KMS can use the exact same Entra user registration flow. You only need to set up **one** hosting option initially. No cloud resources or registrations have been created for you, and no live cloud credentials are available in this workspace.

## 1. Prepare the environment

Use a test tenant/subscription/account/project with an administrator who can register apps, assign users/policies, create serverless workloads and assign key permissions. Create three test accounts: sender, intended recipient and nonrecipient. Obtain the chosen function/service's stable HTTPS URL; a custom domain is optional. The broker URL is an origin, without a path prefix.

You will provision a serverless workload, a native KMS key, a shared managed PostgreSQL database, and protected settings for the IDP client secret and database credentials. PostgreSQL stores permissions and wrapped keys across cold starts; it never stores the raw master key or archive contents. A function's temporary filesystem cannot replace that database. Set a pilot budget and small instance/concurrency limits.

## 2. Register the Entra user-sign-in application

1. In **Microsoft Entra admin center → App registrations → New registration**, create **lttl pilot key service**. Select accounts in this organizational directory only.
2. Add the **Web** platform and the exact `https://YOUR_SERVICE_HOST/auth/callback` redirect URI. Use a confidential server application; leave implicit grants and public-client flows disabled. The desktop does not receive its client secret.
3. Record **Directory (tenant) ID** and **Application (client) ID**. Under **Certificates & secrets**, create a client secret. Copy its **value** directly to your chosen platform's protected secret setting. Do not paste it into chat or place it in `broker.json`. Record its expiration for rotation.
4. Under **Token configuration**, add the optional **auth_time** claim to ID tokens. The broker requests `openid profile email` and `max_age=0`, and rejects tokens without a fresh authentication timestamp. It requires no Graph/SCIM directory permission for the enrolled-user picker.
5. Under **Enterprise applications**, require user assignment. Assign the three pilot accounts (including the nonrecipient, so that archive authorization can be tested independently of app assignment).
6. Apply Conditional Access to this app: required MFA/authentication strength and your intended network/location conditions. Confirm that your tenant licenses support the policies. Use a controlled test group and an administrative recovery path. A location rule evaluates the IDP's signals, not a geographic guarantee from lttl. Test the policy with Entra sign-in logs.
7. Configure the broker issuer as `https://login.microsoftonline.com/TENANT_ID/v2.0`, and set the recorded client ID. Leave `required_acr` null unless you have a verified provider-specific authentication-context contract; do not invent an ACR value as a substitute for Conditional Access.

The workload's managed identity/execution role/service account is **separate** from this user-sign-in app. It receives KMS permissions. End users do not.

References: [Entra registration](https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app), [OIDC flow](https://learn.microsoft.com/en-us/entra/identity-platform/v2-protocols-oidc), [optional claims](https://learn.microsoft.com/en-us/entra/identity-platform/optional-claims-reference), [Conditional Access](https://learn.microsoft.com/en-us/entra/identity/conditional-access/overview).

## 3. Choose native key hosting and deploy

Follow one complete provider guide:

- [Azure Functions + Key Vault](azure.md): generate a versioned RSA key, grant wrap/unwrap to the managed identity, publish the custom-handler package.
- [AWS Lambda + KMS](aws.md): generate a symmetric KMS key, grant Encrypt/Decrypt to the execution role, deploy the Web Adapter container.
- [Google Cloud Run + Cloud KMS](gcp.md): generate a symmetric key version, grant Encrypter/Decrypter to the service account, deploy the Rust container.

Complete [shared database initialization and secret settings](common.md). The example JSON files live in `serverless/`. Configure all placeholders, then initialize the database before deploying the serving process. `LTTL_BROKER_MASTER_KEY` must be absent. Native master keys are created and retained inside the cloud KMS; individual archive keys are briefly plaintext in the broker and authorized desktop.

## 4. Test the live KMS integration

The broker includes an administrative check which needs the provider key configuration but no IDP secret/database connection:

```powershell
.\scripts\cargo.ps1 build --locked --manifest-path identity-broker/Cargo.toml
.\identity-broker\target\debug\lttl-key-broker.exe --check-kms .\pilot-broker.json
```

It generates a disposable archive key, wraps and unwraps it, checks equality, and verifies rejection with a different archive ID. It prints only the provider and pass/fail, never key material. It performs billable KMS requests but does not create a key/resource or archive record. Do not expose this administrative command as a public HTTP endpoint.

**Azure local administrative test:** sign in to the test subscription using Azure CLI. Give the tester wrap/unwrap on the pilot key. In a *copy* of the broker JSON, set `wrapping.auth` to `environment`, then run in PowerShell:

```powershell
$env:LTTL_KMS_ACCESS_TOKEN = az account get-access-token --resource https://vault.azure.net --query accessToken --output tsv
try {
    .\identity-broker\target\debug\lttl-key-broker.exe --check-kms .\pilot-broker-local.json
    if ($LASTEXITCODE -ne 0) { throw 'Native KMS check failed' }
} finally { Remove-Item Env:LTTL_KMS_ACCESS_TOKEN -ErrorAction SilentlyContinue }
```

**AWS local administrative test:** configure your organization's short-lived SSO/assumed-role profile outside this repository. Select it with `AWS_PROFILE`; the SDK reads the normal credential chain. Run `--check-kms` against the AWS JSON. Do not use a long-lived access key for the function. SDK permissions still must match the exact key policy/region.

**GCP local administrative test:** authenticate to the test project, preferably using permitted service-account impersonation, then use a *local copy* with `wrapping.auth` set to `environment`:

```powershell
$env:LTTL_KMS_ACCESS_TOKEN = gcloud auth print-access-token --impersonate-service-account=BROKER_SERVICE_ACCOUNT
try {
    .\identity-broker\target\debug\lttl-key-broker.exe --check-kms .\pilot-broker-local.json
    if ($LASTEXITCODE -ne 0) { throw 'Native KMS check failed' }
} finally { Remove-Item Env:LTTL_KMS_ACCESS_TOKEN -ErrorAction SilentlyContinue }
```

These Azure/GCP environment tokens expire and are not automatically refreshed. Keep **`auth:workload`** in deployed configuration. Local administrative success is not proof that the function's workload identity works: validate creation/opening through the deployed broker, or run the same check in an administrative job with that identity and network. Never send token values here.

## 5. Test actual sharing and serverless behavior

1. Configure lttl with the trusted service URL. Sign in/enroll sender, recipient and nonrecipient. Confirm MFA and the application/user policy in IDP logs.
2. Create a small protected `.lttl` archive for the recipient only. Extract as that recipient and compare file hashes. Confirm the sender and nonrecipient cannot open it unless selected. Test ZIP, TAR.ZST and TAR.LZ4 as inner formats.
3. Open it a second time: a fresh identity flow must run. Cancel or deny MFA and verify no key is released. Repeat from a location/network denied by the configured policy, then test an unassigned account and a wrong-tenant account.
4. Revoke the archive as its creator and verify a new open fails. Revocation cannot recall plaintext/keys already obtained. Disable a broker user or deny the user in the IDP and verify fresh opens fail.
5. Begin a sign-in, restart/scale the function, and finish it within five minutes. Repeat with multiple instances. Replaying an exchange code or consumed opening grant must fail across instances. Permissions and revocations must survive cold starts.
6. In the isolated test environment, temporarily remove the workload's unwrap permission (allow for IAM propagation) or disable the test key version, then attempt a fresh open. It must fail closed. Restore access and verify recovery; do not delete the key. An invocation may fail startup while the key is disabled because startup also checks unwrap.
7. Test database outage and backup/restore with retained key versions. Confirm platform logs do not retain tokens, callback codes or response bodies containing archive keys. Measure cold and warm request latency; the local contract tests are not a cloud performance benchmark.

## What to provide next

Share only the public HTTPS service origin, Entra tenant/client IDs, selected provider, exact nonsecret KMS key ID/ARN/version, and workload identity identifier. Configure the OIDC client secret, database URL and cloud credentials locally/in the platform secret store. Once those resources exist, live integration results can replace the current unvalidated cloud status.

For other IDPs, generate the matching guide in the app. Okta/Auth0/Keycloak/generic OIDC must meet the same fresh `auth_time`, PKCE and token-validation contract. SAML needs a policy-aware federation bridge. Google Workspace direct OIDC freshness compatibility remains a live-test requirement; Google Cloud hosting does not require Google as the user IDP.
