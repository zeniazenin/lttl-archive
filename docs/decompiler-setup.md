# REA native decompiler setup

Installed for the user's explicit request on 2026-10-12. No system-wide Java/PATH installation was performed.

| Component | Local path | Verification |
|---|---|---|
| Ghidra 12.1.4 | `C:/Projects/Archiver/tools/ghidra_12.1.4_PUBLIC` | Official release ZIP SHA-256 `ddac49f903da9d5bac833e5cc79395098b9c33cfd3279be5f31bd00387d2d4db` |
| Temurin JDK 25.0.4.1+1 x64 | `C:/Projects/Archiver/tools/jdk-25.0.4.1+1` | Official release ZIP SHA-256 `00c847d804f4a78e9f04f2683faf14fed898535b177b7fc704486cb0284e9283` |
| Existing REA | `C:/Users/zenia/AppData/Roaming/npm/node_modules/rea-agents` | Version 6.4.0, retained |

Sources: [Ghidra release](https://github.com/NationalSecurityAgency/ghidra/releases/tag/Ghidra_12.1.4_build), [Temurin release](https://github.com/adoptium/temurin25-binaries/releases/tag/jdk-25.0.4.1%2B1).

REA's provider probe accepts this pair: Ghidra 12.1.4, Windows x64 native decompiler, 64-bit full JDK 25.0.4.1 (minimum accepted JDK 21). Raw probe is `rea-providers.json`.

The setup dry run selected only the existing Codex REA registration. It was applied successfully (`rea-setup-result.json`, status ready). `C:/Users/zenia/.codex/config.toml` now supplies `GHIDRA_INSTALL_DIR` and `JAVA_HOME` specifically to REA. Existing unrelated configuration was preserved. A pre-change copy is `C:/Users/zenia/.codex/config.toml.before-ghidra-20261012`; REA also maintains `config.toml.rea.backup`.

Already-running MCP processes retain their old environment until reconnection. The `scripts/rea.ps1` wrapper supplies the new environment immediately for CLI analysis. Ghidra needs permission to create its protected temporary project and target snapshot; the restricted Windows sandbox denied that file operation, and the approved execution path is used for native analysis.

Example:

```powershell
./scripts/rea.ps1 inspect 'C:/path/to/analysis-target.exe' --provider ghidra --format json
```

`tools/` is ignored by Git and contains large official downloaded packages. These paths depend on keeping the project at this location. No IDA license, Hopper installation, or unrelated runtime was installed.
