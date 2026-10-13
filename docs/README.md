# Documentation

## Use and build

- [Windows installer](windows-installer.md): download, checksum, optional components, removal.
- [Detailed user guide](user-guide.md): archive formats, CLI, presets, extraction and limits.
- [Explorer integration](explorer-integration.md): optional per-user menu registration.
- [Development](development.md): prerequisites, builds, tests and release packaging.
- [Architecture](architecture.md) and [library API](api-contract.md).

## Organization sharing

- [Archive access tracking](archive-tracking.md): per-archive policies, signed security webhooks, country estimates and serverless retries.
- [Identity sharing](identity-sharing.md): recipient enrollment, encrypted archives and revocation.
- [Entra + cloud test setup](../identity-broker/deploy/test-environment.md).
- [Serverless deployment](../identity-broker/deploy/README.md): [Azure](../identity-broker/deploy/azure.md), [AWS](../identity-broker/deploy/aws.md), [Google Cloud](../identity-broker/deploy/gcp.md).
- [Current identity/KMS validation](identity-serverless-validation.md).
- Generated examples: [Entra/Azure](identity-entra-azure.md), [Okta/AWS](identity-okta-aws.md), [Keycloak/GCP](identity-keycloak-gcp.md). Replace all placeholders before use.

## Compression and benchmarks

- [Latest portable transfer optimization](portable-transfers.md) and [results](portable-v3-results.md).
- [Portable GPU design](portable-gpu.md), [kernel/session optimization](portable-optimization.md), [earlier results](portable-v2-results.md).
- [Auto selection](auto-compression.md), [sampling optimization](auto-optimization.md).
- [NVIDIA backend](gpu-backend.md), [baseline](gpu-performance.md), [codec comparison](gpu-algorithms.md), [tuning](gpu-tuning.md).
- [Downloaded sample results](downloaded-performance.md), [sample provenance](downloaded-sample-sources.json).

## Historical investigation

[RAR review](rar-compressor-review.md), [REA findings](native-analysis.md), [decompiler setup](decompiler-setup.md), [vendor feasibility](vendor-support.md), and [earlier validation](validation.md) preserve the state of earlier experiments. Their old capability descriptions are not the current product status. Raw evidence under `native/` may contain machine-specific paths; proprietary target binaries are not included. REA is not needed to build or use lttl. Older benchmark harnesses can require preserved baseline binaries/corpora that are intentionally not committed.
