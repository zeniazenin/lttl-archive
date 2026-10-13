param([Parameter(ValueFromRemainingArguments = $true)][string[]]$ReaArgs)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$env:GHIDRA_INSTALL_DIR = Join-Path $projectRoot 'tools/ghidra_12.1.4_PUBLIC'
$env:JAVA_HOME = Join-Path $projectRoot 'tools/jdk-25.0.4.1+1'
$nodeExe = 'C:/Program Files/nodejs/node.exe'
$reaEntry = Join-Path $env:APPDATA 'npm/node_modules/rea-agents/scripts/rea.mjs'
$env:PATH = "$env:JAVA_HOME/bin;C:/Program Files/nodejs;$env:PATH"
& $nodeExe $reaEntry @ReaArgs
exit $LASTEXITCODE
