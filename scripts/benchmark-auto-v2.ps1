#Requires -Version 7.0
[CmdletBinding()]
param(
    [ValidateRange(1,10)][int]$Repeats=4,
    [ValidateRange(1,10)][int]$StartRepeat=1,
    [string]$ReferenceExecutablePath='target/release/lttl-auto-v1.exe'
)
$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
$env:TEMP=Join-Path $root '.qa/auto-benchmark-tmp'
[IO.Directory]::CreateDirectory($env:TEMP)|Out-Null
$env:TMP=$env:TEMP
# Use the exact retained corpus/manifest, without regenerating it. Every source
# hash is verified by the harness. Preserve v1 next to the same GPU runtime.
& (Join-Path $PSScriptRoot 'benchmark-gpu.ps1') -CorpusDirectory 'bench-output/auto-corpus' `
    -ReferenceExecutablePath $ReferenceExecutablePath -Backends cpu,auto -Presets fast,balanced,small `
    -Repeats $Repeats -StartRepeat $StartRepeat -SkipChunkSweep -TimeExtraction -MaximumMinutes 30
