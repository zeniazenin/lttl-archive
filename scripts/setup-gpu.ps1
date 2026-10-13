#Requires -Version 5.1
<# Download pinned NVIDIA development/runtime packages to this project only.
   Published SHA256 hashes are verified before extraction. No driver or system
   CUDA installation is changed. Run build-gpu.ps1 afterwards. #>
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$project = Split-Path $PSScriptRoot -Parent
$tools = Join-Path $project 'tools'
[IO.Directory]::CreateDirectory($tools) | Out-Null
$packages = @(
    @{Name='nvcomp';Root='nvcomp-windows-x86_64-5.3.0.16_cuda12-archive';URL='https://developer.download.nvidia.com/compute/nvcomp/redist/nvcomp/windows-x86_64/nvcomp-windows-x86_64-5.3.0.16_cuda12-archive.zip';SHA256='8549b0815770dcbf4d8496dc988fc118f9b9eb58e99a1f74b5b8f31b0e452151';HeaderOnly=$false},
    @{Name='cudart';Root='cuda_cudart-windows-x86_64-12.9.79-archive';URL='https://developer.download.nvidia.com/compute/cuda/redist/cuda_cudart/windows-x86_64/cuda_cudart-windows-x86_64-12.9.79-archive.zip';SHA256='179e9c43b0735ffe67207b3da556eb5a0c50f3047961882b7657d3b822d34ef8';HeaderOnly=$false},
    @{Name='cuda-nvcc-headers';Root='cuda_nvcc-windows-x86_64-12.9.86-archive';URL='https://developer.download.nvidia.com/compute/cuda/redist/cuda_nvcc/windows-x86_64/cuda_nvcc-windows-x86_64-12.9.86-archive.zip';SHA256='227b109663b5e57d2718bcabb24a4ba0d9d4e52d958e327dc476f7c28691be85';HeaderOnly=$true}
)
Add-Type -AssemblyName System.IO.Compression.FileSystem
foreach ($package in $packages) {
    $zip = Join-Path $tools ($package.Name+'.zip')
    if (!(Test-Path -LiteralPath $zip)) {
        Invoke-WebRequest $package.URL -OutFile ($zip+'.partial') -TimeoutSec 300
        if ((Get-FileHash -LiteralPath ($zip+'.partial')).Hash -ne $package.SHA256) { throw "Download checksum mismatch: $($package.Name)" }
        Move-Item -LiteralPath ($zip+'.partial') -Destination $zip
    }
    if ((Get-FileHash -LiteralPath $zip).Hash -ne $package.SHA256) { throw "Cached package checksum mismatch: $zip" }
    $archive = [IO.Compression.ZipFile]::OpenRead($zip)
    try {
        foreach ($entry in $archive.Entries) {
            $relative = $entry.FullName.Replace('\','/')
            if (!$relative.StartsWith($package.Root+'/',[StringComparison]::Ordinal)) { throw 'Unexpected package root' }
            if ($package.HeaderOnly -and !$relative.StartsWith($package.Root+'/include/',[StringComparison]::Ordinal)) { continue }
            $target = [IO.Path]::GetFullPath((Join-Path $tools $relative))
            $prefix = [IO.Path]::GetFullPath((Join-Path $tools $package.Root)).TrimEnd('\')+'\'
            if (!$target.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'Package entry escapes its directory' }
            if ($relative.EndsWith('/')) { [IO.Directory]::CreateDirectory($target) | Out-Null; continue }
            [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)) | Out-Null
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry,$target,$true)
        }
    } finally { $archive.Dispose() }
    Write-Output "Verified and installed $($package.Root)"
}
Write-Output 'Next: scripts/build-gpu.ps1, then scripts/cargo.ps1 build --release --locked'
