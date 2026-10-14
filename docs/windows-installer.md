# Windows installer

Download `lttl-0.1.0-alpha.6-windows-x64-setup.exe` and its adjacent `.sha256` file from the [alpha.6 release](https://github.com/zeniazenin/lttl-archive/releases/tag/v0.1.0-alpha.6). This version adds a console-free desktop launcher and the lttl.info logo, alongside native compression and administrator controls.

Setup defaults to **`C:\Program Files\lttl`** on Windows 10/11 x64 and requests Windows administrator elevation. The application, Start Menu shortcuts and Installed Apps registration are available to all users. PATH and default file associations are not changed.

## Install, repair and upgrade

1. Close lttl before installing an update.
2. For alpha.4 or earlier installed under AppData, uninstall that per-user copy through Installed Apps first. Archives, identity profiles and unrelated user-added files are preserved. Setup does not run a user-writable legacy uninstaller with administrator privileges.
3. Run the new setup and approve Windows UAC.
4. Keep the Program Files default, or choose another folder. A registered machine installation is selected automatically for repair/upgrade.

Running setup again repairs/replaces packaged files while preserving unrelated files. The directory page no longer disables Install merely because `lttl.exe` exists. An unregistered executable in the destination produces an explicit message when installation is attempted, instead of being overwritten. A locked executable produces a close-and-retry message before replacement. To change the location of a registered machine installation, uninstall it first.

| Component | Default | Contents |
|---|---|---|
| lttl | Required | GUI, CLI, native Fast/Dense and Auto, audit/retention controls, CPU codecs, portable wgpu LZ4, MSVC runtime, docs and notices |
| NVIDIA acceleration | Off for new installs | Optional nvCOMP/CUDA runtime for supported NVIDIA GPUs |
| Explorer menu | Off for new installs | All-users create/extract actions; Windows 11 uses Show more options |

Existing optional components are preselected during repair/upgrade. Deselecting them removes only known files or owned registry commands. Standalone Explorer scripts still default to the current user; `-Scope AllUsers` selects the elevated machine scope.

The installer is unsigned. The SHA-256 file verifies artifact integrity but is not an Authenticode signature. GPU drivers, identity providers, brokers and cloud key stores are not installed.

```powershell
Get-FileHash .\lttl-0.1.0-alpha.6-windows-x64-setup.exe -Algorithm SHA256
Get-Content .\lttl-0.1.0-alpha.6-windows-x64-setup.exe.sha256
```

For automation, launch from an elevated process with `/S`. `/NVIDIA` and `/EXPLORER` select optional components. A custom `/D=ABSOLUTE_PATH` must be last. A nonzero exit code indicates failure. Uninstall through Installed Apps or the Start Menu shortcut; removal checks ownership and preserves unrelated files.

[Build instructions](development.md), [installer validation](windows-installer-validation.json), [payload smoke checks](native-installer-validation.json), and [alpha.6 notes](releases/v0.1.0-alpha.6.md). Validation reports remain outside the payload because they contain the final setup executable's own hash.

The Start Menu and Explorer commands launch `lttl-gui.exe`, which uses the Windows GUI subsystem and never allocates a console. `lttl.exe` remains the console executable for scripts and terminal use. Both executables embed the website logo; the desktop window sets the same icon explicitly.
