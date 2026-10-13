# lttl Windows Explorer integration

The scripts register three actions for the current user: **Create archive**, **Extract here**, and **Extract to named folder**. Creation accepts one selected file or folder. Extraction appears for one selected ZIP, TAR.ZST, TAR.LZ4 or encrypted .lttl archive. The application receives the selected path through its `gui-create`, `gui-extract-here`, or `gui-extract-folder` command.

Each action opens lttl with the selected path filled in and waits for you to start. Archive creation defaults to Auto. For TAR.ZST, the desktop interface also offers NVIDIA GPU compression; ZIP uses the CPU. GPU compression has a fixed setting independent of the CPU presets, with chunk and batch sizes under Advanced controls. Selecting a GPU does not promise a speed improvement.

This is an optional installation step. The scripts are provided for review and were not used to install registry changes during development.

## Preview and install

Build the release executable, keep it in a stable directory, then run these commands from the repository root in PowerShell:

```powershell
.\scripts\install-explorer.ps1 -ExecutablePath .\target\release\lttl.exe -WhatIf
.\scripts\install-explorer.ps1 -ExecutablePath .\target\release\lttl.exe
```

Administrator privileges are not required. `-WhatIf` validates the executable and checks for conflicts, then prints each planned command without writing the registry. It does not test the application's GUI behavior. Paths with spaces and shell metacharacters are passed directly to the quoted executable; no `cmd.exe` or PowerShell intermediary interprets the selected filename. The installer rejects executable paths containing percent signs to avoid ambiguous Shell expansion.

Installation fails before changes if any of its ten verb keys already exists, including a prior installation owned by this project. To relocate or update the executable path, uninstall first and install again. An installation error triggers cleanup of verb keys created in that attempt.

These scripts manage the renamed `lttl.*` keys only. They do not migrate or remove registrations made by an earlier differently named build.

## Remove

```powershell
.\scripts\uninstall-explorer.ps1 -WhatIf
.\scripts\uninstall-explorer.ps1
```

Removal checks the ownership marker and expected registry structure. Conflicting or unexpectedly extended keys are left untouched and reported for manual inspection. Only the ten fixed verb keys are candidates for removal; parent keys and default file associations remain intact. Already missing keys are harmless.

## Registration scope and limitations

All entries live under `HKEY_CURRENT_USER\Software\Classes`:

| Relative location | Verb names |
| --- | --- |
| `*\shell` | `lttl.Create` |
| `Directory\shell` | `lttl.Create` |
| `SystemFileAssociations\.zip\shell` | `lttl.ExtractHere`, `lttl.ExtractFolder` |
| `SystemFileAssociations\.zst\shell` | `lttl.ExtractHere`, `lttl.ExtractFolder` |
| `SystemFileAssociations\.lz4\shell` | `lttl.ExtractHere`, `lttl.ExtractFolder` |
| `SystemFileAssociations\.lttl\shell` | Sign-in-and-extract variants of the same verbs |

Each verb has `MultiSelectModel=Single`. No file association or default application is replaced. The `.zst` verbs use `AppliesTo=System.FileName:~>".tar.zst"` to restrict them to TAR.ZST filenames; a standalone Zstandard stream is not a TAR archive. Filename filtering is a convenience, not format validation: the application must validate the input before extraction. The registry API treats the literal `*` key as a filename class, without wildcard expansion.

These static verbs use the classic menu, normally reached through **Show more options** on Windows 11. A first-level Windows 11 extension is separate work requiring an `IExplorerCommand` implementation and packaging/registration. Reopen Explorer if its existing window caches the menu; these scripts do not terminate Explorer. [Microsoft menu integration guidance](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/integrate-packaged-app-with-file-explorer)

Microsoft documents registry-based verbs and predefined filesystem object classes in [Extending Shortcut Menus](https://learn.microsoft.com/en-us/windows/win32/shell/context). The canonical suffix comparison used by the TAR.ZST filter is documented in [Advanced Query Syntax](https://learn.microsoft.com/en-us/windows/win32/search/-search-3x-advancedquerysyntax).

Explorer rendering and command dispatch still require manual acceptance testing after installation: one file, one directory, ZIP, TAR.ZST, standalone ZST, multiple selection, Unicode filenames, and filenames containing spaces or ampersands. Verify extraction collision behavior in the app before relying on the quick actions.

Portable LZ4 update: `.lz4` extraction verbs are restricted to filenames ending in `.tar.lz4`. The installer/uninstaller now manage ten owned keys. Existing installations need uninstall/reinstall to add these verbs; this development pass did not modify the registry.
