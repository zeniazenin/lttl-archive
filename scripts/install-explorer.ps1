#Requires -Version 5.1
<#
.SYNOPSIS
Adds lttl's single-selection commands to the selected scope's Explorer menu.
.DESCRIPTION
Creates ten unique HKCU (default) or HKLM (AllUsers) Software\Classes verb keys, without changing default
file associations. Existing verb keys cause the entire installation to stop.
On Windows 11 these legacy static verbs appear under Show more options.
The .zst verbs are filtered to filenames ending in .tar.zst; standalone .zst
streams are not supported by these archive extraction commands.
Use -WhatIf to inspect the plan without writing any registry keys.
.PARAMETER ExecutablePath
Path to the built lttl.exe. Keep it at this location after installation.
.EXAMPLE
.\scripts\install-explorer.ps1 -ExecutablePath .\target\release\lttl.exe -WhatIf
#>
[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'Medium')]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string] $ExecutablePath,
    [ValidateSet("CurrentUser", "AllUsers")]
    [string] $Scope = "CurrentUser"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Explorer integration requires Windows.' }
$executable = Get-Item -LiteralPath $ExecutablePath
if ($executable.PSIsContainer -or $executable.Extension -ine '.exe') {
    throw 'ExecutablePath must identify an existing .exe file.'
}
$exePath = $executable.FullName
# Prefer the Windows-subsystem desktop entry point; the CLI remains scriptable.
$desktop = Join-Path $executable.DirectoryName 'lttl-gui.exe'
if (Test-Path -LiteralPath $desktop -PathType Leaf) { $exePath = $desktop }
if ($exePath.Contains('"') -or $exePath.Contains('%')) {
    throw 'The executable path cannot contain quotation marks or percent signs in an Explorer command.'
}
$owner = 'lttl.Explorer.63baf176-34a8-49e9-bb2c-1f62bf013ec7.v1'
$verbs = @(
    @{ Path = 'Software\Classes\*\shell\lttl.Create'; Label = 'Create archive with lttl...'; Action = 'gui-create'; Filter = '' },
    @{ Path = 'Software\Classes\Directory\shell\lttl.Create'; Label = 'Create archive with lttl...'; Action = 'gui-create'; Filter = '' },
    @{ Path = 'Software\Classes\SystemFileAssociations\.zip\shell\lttl.ExtractHere'; Label = 'Extract here with lttl'; Action = 'gui-extract-here'; Filter = '' },
    @{ Path = 'Software\Classes\SystemFileAssociations\.zip\shell\lttl.ExtractFolder'; Label = 'Extract to named folder with lttl'; Action = 'gui-extract-folder'; Filter = '' },
    @{ Path = 'Software\Classes\SystemFileAssociations\.zst\shell\lttl.ExtractHere'; Label = 'Extract here with lttl'; Action = 'gui-extract-here'; Filter = 'System.FileName:~>".tar.zst"' },
    @{ Path = 'Software\Classes\SystemFileAssociations\.zst\shell\lttl.ExtractFolder'; Label = 'Extract to named folder with lttl'; Action = 'gui-extract-folder'; Filter = 'System.FileName:~>".tar.zst"' },
    @{ Path = 'Software\Classes\SystemFileAssociations\.lz4\shell\lttl.ExtractHere'; Label = 'Extract here with lttl'; Action = 'gui-extract-here'; Filter = 'System.FileName:~>".tar.lz4"' },
    @{ Path = 'Software\Classes\SystemFileAssociations\.lz4\shell\lttl.ExtractFolder'; Label = 'Extract to named folder with lttl'; Action = 'gui-extract-folder'; Filter = 'System.FileName:~>".tar.lz4"' },
    @{ Path = 'Software\Classes\SystemFileAssociations\.lttl\shell\lttl.ExtractHere'; Label = 'Extract here with lttl'; Action = 'gui-extract-here'; Filter = '' },
    @{ Path = 'Software\Classes\SystemFileAssociations\.lttl\shell\lttl.ExtractFolder'; Label = 'Extract to named folder with lttl'; Action = 'gui-extract-folder'; Filter = '' }
)

# RegistryKey treats '*' literally; PowerShell registry provider wildcard paths
# must not be used for these registrations.
$hive = if ($Scope -eq "AllUsers") { [Microsoft.Win32.Registry]::LocalMachine } else { [Microsoft.Win32.Registry]::CurrentUser }
$hiveLabel = if ($Scope -eq "AllUsers") { "HKLM" } else { "HKCU" }
try {
    foreach ($verb in $verbs) {
        $existing = $hive.OpenSubKey($verb.Path)
        if ($null -ne $existing) {
            $existing.Dispose()
            throw "Registry key ${hiveLabel}\$($verb.Path) already exists. Nothing was changed. Uninstall a prior lttl registration before reinstalling; inspect any conflicting key manually."
        }
    }
    foreach ($verb in $verbs) {
        $command = '"{0}" {1} "%1"' -f $exePath, $verb.Action
        Write-Output ("${hiveLabel}\{0}: {1}" -f $verb.Path, $command)
    }
    if (-not $PSCmdlet.ShouldProcess("${hiveLabel}\Software\Classes (ten lttl verb keys)", 'Install single-selection Explorer commands')) {
        return
    }
    $created = [System.Collections.Generic.List[string]]::new()
    try {
        foreach ($verb in $verbs) {
            $key = $hive.CreateSubKey($verb.Path)
            $created.Add($verb.Path)
            try {
                $key.SetValue('lttlOwner', $owner, [Microsoft.Win32.RegistryValueKind]::String)
                $key.SetValue('', $verb.Label, [Microsoft.Win32.RegistryValueKind]::String)
                $key.SetValue('MultiSelectModel', 'Single', [Microsoft.Win32.RegistryValueKind]::String)
                $key.SetValue('Icon', ('"{0}",0' -f $exePath), [Microsoft.Win32.RegistryValueKind]::String)
                if ($verb.Filter) {
                    $key.SetValue('AppliesTo', $verb.Filter, [Microsoft.Win32.RegistryValueKind]::String)
                }
                $commandKey = $key.CreateSubKey('command')
                try {
                    $commandKey.SetValue('', ('"{0}" {1} "%1"' -f $exePath, $verb.Action), [Microsoft.Win32.RegistryValueKind]::String)
                } finally { $commandKey.Dispose() }
            } finally { $key.Dispose() }
        }
    } catch {
        $installError = $_
        foreach ($keyPath in $created) {
            try { $hive.DeleteSubKeyTree($keyPath, $false) }
            catch { Write-Warning "Could not roll back ${hiveLabel}\${keyPath}: $_" }
        }
        throw $installError
    }
    Write-Output "Installed for $Scope. On Windows 11, open Show more options. Reopen Explorer if its menu is cached."
} finally { $hive.Dispose() }
