# Independent audit vault

Deploy this function under the organization's security team, separately from the key broker. It verifies pinned Ed25519 signatures, event/batch continuity, and create-only storage before acknowledging a batch. It has no delete API or archive keys.

Azure Functions, AWS Lambda, and Google Cloud Run functions entry points and storage adapters are included. All external evidence is held indefinitely; the security team controls eventual disposition. Do not deploy with broad storage administration privileges.

See [configuration, IAM boundaries, migration, and retention semantics](../../docs/audit-retention.md).

Run protocol tests locally:

```text
python -m unittest discover -s identity-broker/audit-vault -v
```

Tests use a storage double, not a live cloud account. Validate cloud permissions and hold behavior before production.
