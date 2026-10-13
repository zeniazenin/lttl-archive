# Native KMS and serverless validation

Recorded 2026-10-12 (local workspace date). This supersedes the older VM/secret-loader hosting report. No tenant, cloud key, or paid cloud service was created during this implementation.

| Check | Result and boundary |
|---|---|
| Broker unit/HTTP contract suite | 8 passed; the PostgreSQL test is opt-in and was also run separately |
| Azure contract | RSA-OAEP-256 request encoding, exact key version, wrap/unwrap, encrypted context binding, tampering/denial rejection, allowed workload endpoints |
| AWS contract | Official Rust SDK over local HTTP: SigV4 header, Encrypt/Decrypt JSON, exact ARN and encryption context, tampering/denial rejection. The mock does not validate AWS IAM or the signature cryptographically |
| GCP contract | Versioned encrypt, parent-key decrypt, AAD, CRC32C in both directions, integrity flags, context/tampering/denial rejection |
| Cloud/local startup boundary | Wrong local master and cross-mode fallback rejected; native marker verified |
| Durable state | SQLite restart and concurrent independent-pool checks passed |
| Real PostgreSQL | PostgreSQL 17.11, disposable loopback server, SCRAM password auth and `verify-full` TLS with an ephemeral CA/IP SAN; shared-state contract passed. [Recorded result](identity-postgres-validation.json) |
| TLS configuration | Missing/weak TLS, duplicate sslmode, alias override and Unix-socket override rejected |
| Real release broker + desktop CLI | **75 passed**, **43** fresh mock-OIDC flows; ZIP/TAR.ZST/TAR.LZ4 extracted file hashes match. Pending login, exchange and management sessions survive restart; consumed grants remain consumed. [Recorded result](identity-validation.json) |
| Desktop identity tests | 4 passed, including all 28 IDP/hosting guide combinations and encrypted-stream integrity |
| Legacy launcher regression | 8 Python test methods passed; this launcher is not used by native serverless deployment |
| Windows builds | Broker and GUI-enabled desktop release builds succeeded |
| Deployment JSON | All five current example/host/function JSON documents parsed |
| Linux containers / Azure Functions package | Templates supplied, **not built or run here**: installed Docker engine did not become available |
| Live Azure/AWS/GCP KMS, workload identity, managed database | **Not tested**: user has not yet provisioned test resources |
| Live Entra/other-IDP MFA or location policy | **Not tested**: requires tenant configuration and real policy acceptance |

Cloud mocks use authenticated local encryption to represent opaque ciphertext. They test serialization, checks and failure handling, not the provider's RSA/symmetric implementation, token issuance, workload metadata service or production IAM. There is no production endpoint override or token-validation bypass for these mocks. The `auth:environment` option accepts short-lived real cloud tokens only for explicit administrative testing; deployed Azure/GCP configurations use `auth:workload`.

Serverless changes replace per-process session maps with shared database records, including atomic single-use consumption. They do not make the service fully production-qualified: deployed routing/headers, cold-start limits, IAM, private networking, managed database failover and scale behavior still need the pilot procedure. PostgreSQL/KMS are additional managed resources even when broker compute scales to zero. A legacy local-master database cannot be silently switched to cloud KMS; an offline migration tool remains future work.

## Reproduce locally

From the repository root, with a writable temporary directory and loopback network access:

```powershell
.\scripts\cargo.ps1 test --locked --manifest-path identity-broker/Cargo.toml
.\scripts\cargo.ps1 test --locked --test identity
python -m unittest discover -s identity-broker/deploy -p test_launch.py
python scripts/test-broker-postgres.py --pg-bin PATH_TO_POSTGRESQL_BIN
python scripts/test-identity-flow.py --cli target/release/lttl.exe --broker identity-broker/target/release/lttl-key-broker.exe
```

The PostgreSQL harness creates a local temporary cluster and CA, starts it on an ephemeral loopback port, runs the actual Rust shared-state contract, stops the server and removes its data. It requires Python `cryptography` and PostgreSQL binaries; it does not install a system service or change system certificate trust. The local runtime was downloaded from EDB's official Windows binary distribution linked by [PostgreSQL's Windows download page](https://www.postgresql.org/download/windows/).

For live tests, use the [Entra + cloud test-environment guide](../identity-broker/deploy/test-environment.md) and `lttl-key-broker --check-kms YOUR_CONFIG.json` under the intended workload identity.
