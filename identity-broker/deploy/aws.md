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
