# Documentation

## Use and build

- [Windows installer](windows-installer.md): download, checksum, optional components, removal.
- [Detailed user guide](user-guide.md): archive formats, CLI, presets, extraction and limits.
- [Explorer integration](explorer-integration.md): optional per-user menu registration.
- [Development](development.md): prerequisites, builds, tests and release packaging.
- [Architecture](architecture.md) and [library API](api-contract.md).

## Organization sharing

- [Sharing and security infographic](assets/encrypted-sharing-infographic.png): IDP policy checks, archive key release and optional tracking.
- [Archive access tracking](archive-tracking.md): per-archive policies, signed security webhooks, country estimates and serverless retries.
- [Signed audit, retention and legal holds](audit-retention.md): Windows administrator controls, migration, separate audit vault, preservation schedules, CLI/API and cloud IAM boundaries.
- [Identity sharing](identity-sharing.md): recipient enrollment, encrypted archives and revocation.
- [Entra + cloud test setup](../identity-broker/deploy/test-environment.md).
- [Serverless deployment](../identity-broker/deploy/README.md): [Azure](../identity-broker/deploy/azure.md), [AWS](../identity-broker/deploy/aws.md), [Google Cloud](../identity-broker/deploy/gcp.md).
- [Current identity/KMS validation](identity-serverless-validation.md).
- [Alpha.3 release notes](releases/v0.1.0-alpha.3.md) and [audit/retention integration results](audit-retention-validation.json).
- Historical [alpha.2 release notes](releases/v0.1.0-alpha.2.md) and [tracking integration results](tracking-validation.json).
- Generated examples: [Entra/Azure](identity-entra-azure.md), [Okta/AWS](identity-okta-aws.md), [Keycloak/GCP](identity-keycloak-gcp.md). Replace all placeholders before use.

## Compression and benchmarks

- [Latest portable transfer optimization](portable-transfers.md) and [results](portable-v3-results.md).
- [Portable GPU design](portable-gpu.md), [kernel/session optimization](portable-optimization.md), [earlier results](portable-v2-results.md).
- [Auto selection](auto-compression.md), [sampling optimization](auto-optimization.md).
- [NVIDIA backend](gpu-backend.md), [baseline](gpu-performance.md), [codec comparison](gpu-algorithms.md), [tuning](gpu-tuning.md).
- [Downloaded sample results](downloaded-performance.md), [sample provenance](downloaded-sample-sources.json).

## Historical performance records

[Earlier CPU measurements](rust-performance.md), [vendor feasibility](vendor-support.md), and [earlier validation](validation.md) preserve the state of earlier experiments. Their old capability descriptions are not the current product status. Historical research artifacts are kept separately under `research/legacy/` and are excluded from the installer documentation bundle. Older benchmark harnesses can require preserved baseline binaries/corpora that are intentionally not committed.

- [Native lttl Fast/Dense and Auto](native-compression.md), [production benchmark](native-codec-benchmark.md), and [local alpha.4 build notes](releases/v0.1.0-alpha.4.md).

- [Alpha.5 Program Files / repair fix](releases/v0.1.0-alpha.5.md).

- [Alpha.6: desktop launcher and logo](releases/v0.1.0-alpha.6.md)

- [Production lttl versus ZIP and RAR](archive-comparison.md), with [all measurements](archive-comparison.json).
