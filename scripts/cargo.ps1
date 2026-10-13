param([Parameter(ValueFromRemainingArguments = $true)][string[]]$CargoArgs)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$toolchainBin = Join-Path $env:USERPROFILE '.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin'
if (Test-Path -LiteralPath $toolchainBin) { $env:PATH = "$toolchainBin;$env:PATH" }
$env:CARGO_HOME = Join-Path $projectRoot '.cargo-home'
Push-Location $projectRoot
try { & cargo @CargoArgs; exit $LASTEXITCODE } finally { Pop-Location }
