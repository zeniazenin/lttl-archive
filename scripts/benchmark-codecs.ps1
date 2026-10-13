#Requires -Version 5.1
<#
.SYNOPSIS
Builds an isolated nvCOMP codec probe; optionally runs the comparison matrix.
.DESCRIPTION
Default is build only. Pass -Run only in an exclusive GPU benchmark window.
Tests Zstd at 64/256/1024 KiB, LZ4 at 64 KiB, GDeflate modes 0/1/2 at 64 KiB.
Reads each input once before timing, allocates reusable pinned/device buffers,
performs a complete warmup, then three warm repetitions. Every compressed batch
in warmup and every repetition is GPU-decoded and compared bytewise outside
the compression timer. LZ4 also gets independent CPU validation every time.
Compression wall times include host staging/copies, H2D, GPU compression,
status/size transfers and D2H of actual packed payloads. Disk I/O, allocation,
GPU verification, checksums, and archive containers are excluded and labeled.
CUDA compression_stage_ms is an event span including queue/launch gaps and
status initialization; it is not a pure isolated-kernel timing.
LZ4 additionally uses an independent bounded CPU block decoder for validation.
GDeflate payloads are not standard Deflate or ZIP; this does not add an app codec.
.EXAMPLE
.\scripts\benchmark-codecs.ps1
.EXAMPLE
.\scripts\benchmark-codecs.ps1 -Run -InputPaths C:\data\enwik8,C:\data\noise.bin
#>
[CmdletBinding()]
param(
    [switch] $Run,
    [string[]] $InputPaths,
    [ValidateRange(16, 128)][int] $BatchMiB = 32,
    [ValidateRange(1, 10)][int] $Repeats = 3,
    [string] $OutputDirectory
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$nvRoot = Join-Path $projectRoot 'tools/nvcomp-windows-x86_64-5.3.0.16_cuda12-archive'
$cuRoot = Join-Path $projectRoot 'tools/cuda_cudart-windows-x86_64-12.9.79-archive'
$headersRoot = Join-Path $projectRoot 'tools/cuda_nvcc-windows-x86_64-12.9.86-archive'
$buildDir = Join-Path $projectRoot 'target/codec-probe'
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $projectRoot ('bench-output/codec-probe/' + [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')) }
if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $vsPath) { throw 'MSVC x64 Build Tools are required.' }
    Import-Module (Join-Path $vsPath 'Common7/Tools/Microsoft.VisualStudio.DevShell.dll')
    Enter-VsDevShell -VsInstallPath $vsPath -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
}
[IO.Directory]::CreateDirectory($buildDir) | Out-Null
$executable = Join-Path $buildDir 'codec_probe.exe'
$arguments = @('/nologo', '/std:c++17', '/O2', '/EHsc', '/MD', '/W4', '/DNOMINMAX',
    ('/I' + (Join-Path $nvRoot 'include')), ('/I' + (Join-Path $cuRoot 'include')), ('/I' + (Join-Path $headersRoot 'include')),
    (Join-Path $projectRoot 'native/codec_probe.cpp'), ('/Fo' + (Join-Path $buildDir 'codec_probe.obj')), ('/Fe' + $executable),
    '/link', ('/LIBPATH:' + (Join-Path $nvRoot 'lib')), ('/LIBPATH:' + (Join-Path $cuRoot 'lib/x64')), 'nvcomp.lib', 'cudart.lib')
& cl.exe @arguments
if ($LASTEXITCODE -ne 0) { throw "Probe compilation failed (exit $LASTEXITCODE)." }
foreach ($dll in @('nvcomp64_5.dll', 'nvcomp_cpu64_5.dll')) { Copy-Item -LiteralPath (Join-Path $nvRoot ('bin/' + $dll)) -Destination (Join-Path $buildDir $dll) -Force }
Copy-Item -LiteralPath (Join-Path $cuRoot 'bin/cudart64_12.dll') -Destination (Join-Path $buildDir 'cudart64_12.dll') -Force
Write-Output "Built $executable"
if (-not $Run) { Write-Output 'Build only: no GPU workload was executed. Add -Run during an exclusive benchmark window.'; return }
if (-not $InputPaths -or $InputPaths.Count -eq 0) { throw '-Run requires explicit -InputPaths to the existing benchmark corpus.' }
$resolved = @($InputPaths | ForEach-Object { (Get-Item -LiteralPath $_ -ErrorAction Stop).FullName })
[IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
$manifest = @($resolved | ForEach-Object { $file = Get-Item -LiteralPath $_; [pscustomobject]@{ path = $_; bytes = $file.Length; sha256 = (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant() } })
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'inputs.json') -Encoding utf8
$raw = Join-Path $OutputDirectory 'results.jsonl'
$errorFile = Join-Path $OutputDirectory 'stderr.txt'
$probeArgs = @('--batch-mib', $BatchMiB.ToString(), '--repeats', $Repeats.ToString()) + $resolved
& $executable @probeArgs 2> $errorFile | Tee-Object -FilePath $raw
$probeExit = $LASTEXITCODE
$results = @(Get-Content -LiteralPath $raw | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json })
$results | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'results.json') -Encoding utf8
Write-Output "Raw codec comparison saved in $OutputDirectory"
if ($probeExit -ne 0) { throw "Probe reported failures (exit $probeExit); inspect saved JSON/error records." }
