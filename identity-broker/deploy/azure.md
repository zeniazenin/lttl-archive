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
