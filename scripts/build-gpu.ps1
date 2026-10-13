#Requires -Version 5.1
<#
.SYNOPSIS
Builds the optional NVIDIA nvCOMP bridge using MSVC, without invoking nvcc.
.DESCRIPTION
Uses project-local, verified NVIDIA redistributable packages. The cuda_nvcc
package's include headers are required by CUDA runtime headers, but the CUDA
compiler itself is not used. No system CUDA installation or PATH change is
performed. Deploys DLLs and vendor notices into target/release/gpu by default.
#>
[CmdletBinding()]
param(
    [string] $NvcompRoot,
    [string] $CudartRoot,
    [string] $CudaHeadersRoot,
    [string] $OutputDirectory
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
if (-not $NvcompRoot) { $NvcompRoot = Join-Path $projectRoot 'tools/nvcomp-windows-x86_64-5.3.0.16_cuda12-archive' }
if (-not $CudartRoot) { $CudartRoot = Join-Path $projectRoot 'tools/cuda_cudart-windows-x86_64-12.9.79-archive' }
if (-not $CudaHeadersRoot) { $CudaHeadersRoot = Join-Path $projectRoot 'tools/cuda_nvcc-windows-x86_64-12.9.86-archive' }
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $projectRoot 'target/release/gpu' }
foreach ($required in @(
    (Join-Path $NvcompRoot 'include/nvcomp/zstd.h'),
    (Join-Path $NvcompRoot 'lib/nvcomp.lib'),
    (Join-Path $CudartRoot 'include/cuda_runtime_api.h'),
    (Join-Path $CudartRoot 'lib/x64/cudart.lib'),
    (Join-Path $CudaHeadersRoot 'include/crt/host_defines.h')
)) { if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "Required NVIDIA SDK file missing: $required" } }

if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere)) { throw 'Install Visual Studio C++ Build Tools (MSVC x64) before building the GPU bridge.' }
    $vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $vsPath) { throw 'No Visual Studio installation with MSVC x64 tools was found.' }
    Import-Module (Join-Path $vsPath 'Common7/Tools/Microsoft.VisualStudio.DevShell.dll')
    Enter-VsDevShell -VsInstallPath $vsPath -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
}
$buildDir = Join-Path $projectRoot 'target/gpu-native-build'
[IO.Directory]::CreateDirectory($buildDir) | Out-Null
[IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
$buildDll = Join-Path $buildDir 'lttl_gpu.dll'
$arguments = @(
    '/nologo', '/std:c++17', '/O2', '/EHsc', '/MD', '/LD', '/W4', '/DNOMINMAX',
    ('/I' + (Join-Path $NvcompRoot 'include')),
    ('/I' + (Join-Path $CudartRoot 'include')),
    ('/I' + (Join-Path $CudaHeadersRoot 'include')),
    (Join-Path $projectRoot 'native/gpu_bridge.cpp'),
    ('/Fo' + (Join-Path $buildDir 'gpu_bridge.obj')),
    ('/Fe' + $buildDll), '/link',
    ('/LIBPATH:' + (Join-Path $NvcompRoot 'lib')),
    ('/LIBPATH:' + (Join-Path $CudartRoot 'lib/x64')),
    'nvcomp.lib', 'cudart.lib',
    ('/IMPLIB:' + (Join-Path $buildDir 'lttl_gpu.lib'))
)
& cl.exe @arguments
if ($LASTEXITCODE -ne 0) { throw "GPU bridge compilation failed (exit $LASTEXITCODE)." }
Copy-Item -LiteralPath $buildDll -Destination (Join-Path $OutputDirectory 'lttl_gpu.dll') -Force
foreach ($dll in @('nvcomp64_5.dll', 'nvcomp_cpu64_5.dll')) {
    Copy-Item -LiteralPath (Join-Path $NvcompRoot ('bin/' + $dll)) -Destination (Join-Path $OutputDirectory $dll) -Force
}
Copy-Item -LiteralPath (Join-Path $CudartRoot 'bin/cudart64_12.dll') -Destination (Join-Path $OutputDirectory 'cudart64_12.dll') -Force
Copy-Item -LiteralPath (Join-Path $NvcompRoot 'LICENSE') -Destination (Join-Path $OutputDirectory 'LICENSE-nvcomp.txt') -Force
Copy-Item -LiteralPath (Join-Path $NvcompRoot 'NOTICE') -Destination (Join-Path $OutputDirectory 'NOTICE-nvcomp.txt') -Force
Copy-Item -LiteralPath (Join-Path $CudartRoot 'LICENSE') -Destination (Join-Path $OutputDirectory 'LICENSE-cuda-runtime.txt') -Force
Write-Output "GPU bridge and runtime libraries deployed to $OutputDirectory"
Write-Output 'Compilation is not a GPU round-trip test. Verify standard Zstandard decoding before using generated archives.'
