# Windows installer

Download the setup executable and checksum from [v0.1.0-alpha.1](https://github.com/zeniazenin/lttl-archive/releases/tag/v0.1.0-alpha.1). Windows 10/11 x64 is required. This is an unsigned alpha build; publisher/SmartScreen warnings are possible. The release checksum verifies the file against the published artifact; it is not an Authenticode signature.

```powershell
Get-FileHash .\lttl-0.1.0-alpha.1-windows-x64-setup.exe -Algorithm SHA256
Get-Content .\lttl-0.1.0-alpha.1-windows-x64-setup.exe.sha256
```

The default directory is `%LOCALAPPDATA%\Programs\lttl`. Installation is per user without administrator elevation. Start Menu shortcuts and a Windows Installed Apps uninstall entry are created. The executable is not added to PATH; file associations are not changed.

| Component | Default | Contents |
|---|---|---|
| lttl | Required | GUI, CLI, CPU codecs, portable wgpu LZ4, app-local MSVC runtime, documentation and license notices |
| NVIDIA acceleration | Off | nvCOMP/CUDA runtime DLLs for Zstandard on compatible NVIDIA GPUs; NVIDIA terms apply |
| Explorer menu | Off | Current-user create/extract actions, including authenticated `.lttl` extraction |

GPU drivers are not installed. Portable LZ4 remains experimental and Auto uses CPU to preserve compression ratio. Selecting NVIDIA acceleration does not guarantee faster archive creation. No broker, database, cloud key or IDP application is created by installation; follow the [organization setup guide](../identity-broker/deploy/test-environment.md) separately.

Use **Installed Apps → lttl → Uninstall** or the Start Menu uninstall shortcut. The uninstaller removes known installed files and its owned menu/registration entries, preserving archives, identity profiles and unrelated user-added files. Existing installations must be uninstalled before installing another version. Conflicting Explorer registrations cause that optional step to fail safely.

For automation, NSIS supports `/S` and a final `/D=ABSOLUTE_PATH`; `/NVIDIA` selects the optional NVIDIA component. Explorer registration remains unselected in a silent installation. A nonzero exit code indicates failure (including refused upgrades or optional integration conflicts). Do not run an unsigned installer obtained from an unverified mirror.

[Build the installer from source](development.md). The initial release's actual install/uninstall and payload validation results are recorded in [installer validation](windows-installer-validation.json).

The published artifact passed 14 automated checks: payload hashes, CPU-only and optional NVIDIA installations, creation/test/extraction for all three formats, refusal of an existing installation, and uninstall cleanup while preserving a user-added file. The installed NVIDIA runtime detected the RTX 4070 Laptop GPU. Interactive wizard appearance and the optional Explorer installation step were not validated by this smoke test.
