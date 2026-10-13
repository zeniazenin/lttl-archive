# lttl archive

A Rust archiver for Windows with a desktop interface, CLI, CPU/GPU compression, and identity-protected sharing.

**Alpha software.** ZIP, TAR.ZST and TAR.LZ4 work locally. GPU paths have been tested on Intel Arc and an RTX 4070 Laptop GPU. Identity sharing has local integration coverage; live cloud/IDP deployment is still awaiting pilot validation.

[Download Windows installer](https://github.com/zeniazenin/lttl-archive/releases/tag/v0.1.0-alpha.1) · [Documentation](docs/README.md) · [Build from source](docs/development.md) · [Identity setup](identity-broker/deploy/test-environment.md)

## Features

- Create, list, test and extract ZIP, TAR.ZST and TAR.LZ4 archives.
- Modern desktop interface with a simple default view and advanced controls.
- Content-aware Auto for Zstandard; Fast avoids sampling overhead.
- Multithreaded CPU compression, experimental NVIDIA nvCOMP Zstandard, and portable wgpu LZ4.
- Optional Explorer actions: **Create archive**, **Extract here**, **Extract to named folder**.
- Encrypted `.lttl` envelopes for selected enrolled organization users, with fresh IDP authentication on opening.
- Native Azure Key Vault, AWS KMS and Google Cloud KMS wrapping, plus serverless broker templates.

lttl uses open archive formats and independent compression libraries. It does **not** implement RAR/7z or full WinRAR compatibility. Historical RAR research is included as evidence, not as a reconstructed proprietary codec.

## Install on Windows

Download `lttl-0.1.0-alpha.1-windows-x64-setup.exe` and its `.sha256` file from the [release page](https://github.com/zeniazenin/lttl-archive/releases/tag/v0.1.0-alpha.1). The installer targets Windows 10/11 x64 and installs for the current user without elevation. It provides Start Menu shortcuts and an uninstaller. Explorer integration and NVIDIA Zstandard runtime components are optional. CPU and portable GPU LZ4 are included by default.

This initial installer is **unsigned**; Windows may display a publisher/SmartScreen warning. Verify the downloaded artifact's SHA-256 against the release checksum. No signing certificate was available for this release. See [installation, verification and removal](docs/windows-installer.md).

## Build and run

Install the Windows MSVC Rust toolchain, Visual Studio C++ Build Tools and Windows SDK. Rust **1.92+** builds both the desktop and identity broker. From PowerShell:

```powershell
git clone https://github.com/zeniazenin/lttl-archive.git
cd lttl-archive
.\scripts\cargo.ps1 build --release --locked
.\target\release\lttl.exe gui
```

The build does not require downloaded research tools or a GPU. Native NVIDIA Zstandard additionally requires `scripts/setup-gpu.ps1` and `scripts/build-gpu.ps1`; portable wgpu LZ4 does not require CUDA. See [development and packaging](docs/development.md).

## CLI examples

```powershell
# Content-aware compression; output names must be new.
.\target\release\lttl.exe create .\input -o .\backup.tar.zst --preset balanced
.\target\release\lttl.exe create .\input -o .\backup.zip --format zip --preset fast

# Explicit portable GPU LZ4 (Intel or NVIDIA; requires a compatible driver).
.\target\release\lttl.exe gpu-list
.\target\release\lttl.exe create .\input -o .\backup.tar.lz4 --format tar-lz4 --backend wgpu --adapter nvidia

# Inspect, verify and restore.
.\target\release\lttl.exe list .\backup.tar.zst
.\target\release\lttl.exe test .\backup.tar.zst
.\target\release\lttl.exe extract .\backup.tar.zst -d .\restored
```

Use `lttl --help` and subcommand `--help` for all options. Most results are JSON; progress goes to stderr. The installed executable is normally `%LOCALAPPDATA%\Programs\lttl\lttl.exe` and is not added to PATH.

## Compression choices and measured limits

| Path | Current behavior |
|---|---|
| CPU ZIP | Serial Deflate; Fast can store sampled incompressible files |
| CPU TAR.ZST | Streaming multithreaded Zstandard, preserving cross-file history |
| Auto TAR.ZST | Fast uses CPU without sampling; Balanced/Small use bounded content probes |
| NVIDIA TAR.ZST | Independent nvCOMP chunks; fixed codec setting and transfer/startup costs |
| CPU TAR.LZ4 | Standard LZ4 frames with CPU extraction |
| wgpu TAR.LZ4 | Reused sessions, compacted readback and two pending batches; explicit selection |

In the latest recorded warm-job benchmark, RTX portable LZ4 beat CPU LZ4 on **7/9 datasets**, but GPU archives were larger. Intel had no CPU speed wins. Therefore **Auto TAR.LZ4 still selects CPU**. These measurements include transfer/readback and archive work; they are not universal GPU speedup claims. AMD and Apple silicon hardware are untested. [Full measurements and compression ratios](docs/portable-v3-results.md).

## Identity-protected sharing

The sender selects enrolled recipients and creates an encrypted `.lttl` archive. A recipient authenticates through the configured IDP, and the broker releases the archive key only if authorized and not revoked. The master wrapping keys stay in the native cloud KMS; archive data keys exist briefly in the broker and desktop. Archive files never pass through the broker.

| Cloud | Broker compute | Wrapping service |
|---|---|---|
| Azure | Functions custom HTTP handler | Key Vault |
| AWS | Lambda with Web Adapter | KMS |
| Google Cloud | Cloud Run serverless container | Cloud KMS |

Shared managed PostgreSQL preserves permissions and one-use authorization state across cold starts. No VM is required, but the database, KMS and networking are separately provisioned services. Entra is the first pilot IDP; guides also cover Okta, Auth0, Keycloak and compatible OIDC/SAML federation.

Start with [the IDP + key-store test guide](identity-broker/deploy/test-environment.md), [sharing workflow](docs/identity-sharing.md), and [deployment overview](identity-broker/deploy/README.md). The installer does not deploy a broker or create cloud resources. Live MFA/location policy, cloud IAM and serverless packages remain pilot acceptance requirements. Access revocation cannot recall plaintext or keys already obtained by an authorized recipient.

## Validation and limitations

The recorded portable GPU experiment completed **468 archive/extraction pairs** and **4,316 file-hash checks**. The identity release suite passed **75 checks**, including restart/replay behavior and hash-verified restoration of all three formats. Native KMS contracts and a real local PostgreSQL/TLS test passed. [Validation details](docs/identity-serverless-validation.md).

Extraction rejects unsafe paths, links/reparse points, collisions and existing-file overwrites. Checksums detect corruption; identity envelopes add authenticated encryption. Windows ACLs/alternate streams, complete metadata preservation, split volumes, recovery records, archive updates and password-encrypted ZIP/7z are not implemented. Do not use actively changing input as a filesystem-consistent backup. See [the detailed user guide](docs/user-guide.md).

## Repository

| Directory | Contents |
|---|---|
| `src/` | Rust engine, GUI, identity client, WGSL kernels |
| `native/` | Optional NVIDIA bridge and codec experiments |
| `identity-broker/` | Rust identity/key service and cloud deployment templates |
| `installer/windows/` | NSIS Windows installer source |
| `tests/`, `scripts/` | Tests, packaging, benchmarks and research helpers |
| `docs/` | User/developer guides, research and recorded benchmark evidence |

Build outputs, downloaded SDKs/tools, local credentials and benchmark input corpora are excluded. Source and lockfiles are committed; installers are published as GitHub Release assets.

Original lttl code is [MIT licensed](LICENSE). Dependencies, NVIDIA/Microsoft components and historical third-party evidence retain their own terms; see [third-party notices](THIRD_PARTY_NOTICES.md).
