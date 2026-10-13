# Development and release packaging

Use Rust 1.92+ with `x86_64-pc-windows-msvc`, Visual Studio C++ Build Tools and Windows SDK for the supported Windows build. The desktop manifest retains its earlier 1.88 minimum; the broker's dependencies require 1.92. The repository has two Cargo packages, not a Cargo workspace. Both lockfiles are committed.

Use the `main` branch for current source and documentation. Published tags such as `v0.1.0-alpha.3` identify the source snapshot for each installer; later documentation commits do not change an existing release artifact.

```powershell
.\scripts\cargo.ps1 build --release --locked
.\scripts\cargo.ps1 test --locked
.\scripts\cargo.ps1 build --release --locked --manifest-path identity-broker/Cargo.toml
.\scripts\cargo.ps1 test --locked --manifest-path identity-broker/Cargo.toml
```

`scripts/cargo.ps1` uses project-local `.cargo-home` and the installed stable MSVC toolchain. Dependencies need network access on the first build. On other builder environments, invoke Cargo directly with the same arguments. CLI-only builds use `--no-default-features`; macOS/Linux desktop behavior has not been accepted on hardware.

For NVIDIA Zstandard, run `scripts/setup-gpu.ps1` and `scripts/build-gpu.ps1`. Downloads are pinned with SHA-256 and remain under ignored `tools/`. The resulting DLLs and notices live in `target/release/gpu`. Portable wgpu LZ4 is built into the Rust app and uses a supported graphics driver, not CUDA.

## Tests

Default desktop tests skip seven hardware-dependent checks. Run GPU tests serially only on the required adapter/runtime; consult the relevant benchmark guide before using `--ignored`. Broker tests use local HTTP cloud-contract mocks. The ignored PostgreSQL test requires a disposable TLS database; the Python harness creates and cleans up a temporary local cluster:

```powershell
python -m pip install cryptography
python scripts/test-broker-postgres.py --pg-bin PATH_TO_POSTGRESQL_BIN
python scripts/test-identity-flow.py --cli target/release/lttl.exe --broker identity-broker/target/release/lttl-key-broker.exe
python scripts/test-identity-flow.py --tracking --cli target/release/lttl.exe --broker identity-broker/target/release/lttl-key-broker.exe
python scripts/test-identity-flow.py --tracking --audit --cli target/release/lttl.exe --broker identity-broker/target/release/lttl-key-broker.exe
python -m unittest discover -s identity-broker/audit-vault -v
python -m unittest discover -s identity-broker/deploy -p test_launch.py
```

The protocol harness runs a mock RSA-signed OIDC provider and actual executables over loopback. No live tenant is contacted. Use writable TEMP/TMP paths in restricted environments. [Current identity test boundaries](identity-serverless-validation.md) distinguish contract tests from cloud acceptance.

The `--tracking` variant adds a local signed-webhook receiver, organization policy, country-header validation, outage/restart retries and a tracking-storage failure test. It writes `docs/tracking-validation.json`; the baseline run writes `docs/identity-validation.json`. Add `--audit` to test signatures, the independent mock vault, retention/holds, administrator CLI/API, destruction, tampering and export retry. This writes `docs/audit-retention-validation.json`, including hashes of the tested executables. [Current counts](identity-serverless-validation.md) distinguish local validation from live cloud acceptance.

## Build the Windows installer

Build the GUI release and NVIDIA bridge first. Download the portable **NSIS 3.12** ZIP from its [official download page](https://nsis.sourceforge.io/Download) and extract it to `tools/nsis-3.12`. The packaging script discovers the installed Visual Studio x64 CRT redistributables; override with `--crt` if needed. It collects Rust dependency licenses and includes NVIDIA notices.

```powershell
python scripts/build-installer.py --version 0.1.0-alpha.3
```

Outputs under ignored `dist/` are the setup executable, its SHA-256 text file and a payload hash manifest. Packaging refuses to overwrite an existing setup executable. It does not sign or publish automatically. For another build, choose a new version or explicitly move the previous local artifact. The current installer requires removal of an existing installation before upgrading.

Verify installation into a disposable directory, archive round trips, payload hashes, optional NVIDIA detection, and uninstall preserving user-added files before publishing. Explorer integration should be selected and visually checked separately on Windows. Do not claim signed distribution or unattended upgrade support without implementing them.

The automated smoke test temporarily creates current-user install registration and Start Menu shortcuts, tests both component configurations, and uninstalls them. It refuses to run when lttl is already installed:

```powershell
python scripts/test-windows-installer.py dist/lttl-0.1.0-alpha.3-windows-x64-setup.exe --manifest dist/lttl-0.1.0-alpha.3-payload-sha256.json --identity-broker identity-broker/target/release/lttl-key-broker.exe
```

The installer smoke test covers installed ZIP/TAR.ZST/TAR.LZ4 round trips in both configurations and confirms the retention command/setup files are present. With `--identity-broker`, it also exercises the installed CPU client's complete IDP/audit/retention flow. This is not a visual installer or Explorer integration acceptance test. The final installer report is kept in the repository rather than embedded in its own payload; otherwise its final artifact hash would be circular.

After committing the complete release and pushing `main`, a maintainer with existing GitHub Git credentials can publish the verified files with `python scripts/publish-release.py --tag v0.1.0-alpha.3 --publish`. Without `--publish` it only checks repository access. Publication requires a clean checkout, remote `main` at the tested commit, matching installer validation/checksum, and installed-client audit integration. It creates a draft, checks uploaded SHA-256 digests, then publishes the prerelease. It refuses to replace an already published release or move a conflicting tag. Credentials stay in memory and are sent only to GitHub API/upload endpoints.

## Contributing

Keep functional changes small and include relevant regression coverage. Compare compression changes using complete archive creation/extraction times, transfer costs, file hashes and compression ratios. Preserve measured baseline reports. Never add cloud credentials, master keys, local databases, downloaded SDKs or private benchmark input. Deployment templates must keep cloud/IDP validation limits explicit.
