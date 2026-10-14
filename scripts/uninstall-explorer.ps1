#Requires -Version 5.1
<#
.SYNOPSIS
Removes only this project's owned Explorer verb keys for the selected scope.
.DESCRIPTION
Checks the fixed ten locations in the selected hive and their ownership markers before making
changes. Refuses conflicting or unexpectedly extended keys. Missing keys are
harmless. Does not remove file associations, parent keys, or other applications'
commands. Supports -WhatIf. Windows 11 registrations use Show more options.
.EXAMPLE
.\scripts\uninstall-explorer.ps1 -WhatIf
#>
[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'Medium')]
param([ValidateSet("CurrentUser", "AllUsers")][string] $Scope = "CurrentUser")

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Explorer integration requires Windows.' }
$owner = 'lttl.Explorer.63baf176-34a8-49e9-bb2c-1f62bf013ec7.v1'
$paths = @(
    'Software\Classes\*\shell\lttl.Create',
    'Software\Classes\Directory\shell\lttl.Create',
    'Software\Classes\SystemFileAssociations\.zip\shell\lttl.ExtractHere',
    'Software\Classes\SystemFileAssociations\.zip\shell\lttl.ExtractFolder',
    'Software\Classes\SystemFileAssociations\.zst\shell\lttl.ExtractHere',
    'Software\Classes\SystemFileAssociations\.zst\shell\lttl.ExtractFolder',
    'Software\Classes\SystemFileAssociations\.lz4\shell\lttl.ExtractHere',
    'Software\Classes\SystemFileAssociations\.lz4\shell\lttl.ExtractFolder',
    'Software\Classes\SystemFileAssociations\.lttl\shell\lttl.ExtractHere',
    'Software\Classes\SystemFileAssociations\.lttl\shell\lttl.ExtractFolder'
)
$hive = if ($Scope -eq "AllUsers") { [Microsoft.Win32.Registry]::LocalMachine } else { [Microsoft.Win32.Registry]::CurrentUser }
$hiveLabel = if ($Scope -eq "AllUsers") { "HKLM" } else { "HKCU" }
$owned = [System.Collections.Generic.List[string]]::new()
try {
    # Complete all preflight checks before deleting any key. RegistryKey uses
    # literal paths, so the '*' file-type key cannot expand as a wildcard.
    foreach ($keyPath in $paths) {
        $key = $hive.OpenSubKey($keyPath)
        if ($null -eq $key) { continue }
        try {
            if ($key.GetValue('lttlOwner', '') -cne $owner) {
                throw "Refusing to remove unowned key ${hiveLabel}\$keyPath. Nothing was changed."
            }
            foreach ($name in $key.GetValueNames()) {
                if ($name -notin @('', 'lttlOwner', 'MultiSelectModel', 'Icon', 'AppliesTo')) {
                    throw "Unexpected value '$name' in ${hiveLabel}\$keyPath. Nothing was changed; inspect the key manually."
                }
            }
            foreach ($child in $key.GetSubKeyNames()) {
                if ($child -ine 'command') {
                    throw "Unexpected subkey '$child' in ${hiveLabel}\$keyPath. Nothing was changed; inspect the key manually."
                }
            }
            $commandKey = $key.OpenSubKey('command')
            if ($null -ne $commandKey) {
                try {
                    if ($commandKey.SubKeyCount -ne 0 -or @($commandKey.GetValueNames() | Where-Object { $_ -ne '' }).Count -ne 0) {
                        throw "Unexpected data in ${hiveLabel}\$keyPath\command. Nothing was changed; inspect the key manually."
                    }
                } finally { $commandKey.Dispose() }
            }
            $owned.Add($keyPath)
        } finally { $key.Dispose() }
    }
    if ($owned.Count -eq 0) {
        Write-Output 'No owned lttl Explorer commands are installed.'
        return
    }
    foreach ($keyPath in $owned) { Write-Output "Remove ${hiveLabel}\$keyPath" }
    if ($PSCmdlet.ShouldProcess("${hiveLabel}\Software\Classes (owned lttl verb keys)", 'Uninstall Explorer commands')) {
        foreach ($keyPath in $owned) { $hive.DeleteSubKeyTree($keyPath, $false) }
        Write-Output 'Removed lttl Explorer commands. Reopen Explorer if its menu is cached.'
    }
} finally { $hive.Dispose() }
