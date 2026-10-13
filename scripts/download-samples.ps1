#Requires -Version 7.0
# Download public compression-benchmark data as inert inputs, never execute files.
[CmdletBinding()]
param([switch]$DownloadOnly)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = Split-Path $PSScriptRoot -Parent
$destination = Join-Path $root 'bench-output/downloaded-samples'
$downloads = Join-Path $destination 'downloads'
$corpus = Join-Path $destination 'corpus'
[IO.Directory]::CreateDirectory($downloads) | Out-Null
[IO.Directory]::CreateDirectory($corpus) | Out-Null
$sources = @(
    @{ Name='enwik8'; URL='https://mattmahoney.net/dc/enwik8.zip'; File='enwik8.zip'; Description='https://mattmahoney.net/dc/textdata.html'; MaxBytes=100000000 }
)
$provenance = @()
foreach ($source in $sources) {
    $path = Join-Path $downloads $source.File
    if (!(Test-Path -LiteralPath $path)) {
        Invoke-WebRequest -Uri $source.URL -OutFile ($path+'.partial') -TimeoutSec 180
        Move-Item -LiteralPath ($path+'.partial') -Destination $path
    }
    $provenance += [ordered]@{Name=$source.Name;URL=$source.URL;Description=$source.Description;ArchiveSHA256=(Get-FileHash -LiteralPath $path).Hash;ArchiveBytes=(Get-Item -LiteralPath $path).Length}
    if ($DownloadOnly) { continue }
    $folder = Join-Path $corpus $source.Name
    [IO.Directory]::CreateDirectory($folder) | Out-Null
    $archive = [IO.Compression.ZipFile]::OpenRead($path)
    try {
        $total = 0L
        foreach ($entry in $archive.Entries) {
            # These two corpora contain only flat regular files. Reject unexpected paths.
            if ($entry.FullName -notmatch '^[A-Za-z0-9_.-]+$' -or $entry.FullName -in '.', '..') { throw "Unexpected sample path: $($entry.FullName)" }
            $total += $entry.Length
            if ($total -gt $source.MaxBytes) { throw 'Downloaded corpus exceeds its expected expansion limit' }
            $file = Join-Path $folder $entry.FullName
            if (Test-Path -LiteralPath $file) { continue }
            $input = $entry.Open(); $output = [IO.File]::Open($file, [IO.FileMode]::CreateNew)
            try { $input.CopyTo($output) } finally { $output.Dispose(); $input.Dispose() }
        }
    } finally { $archive.Dispose() }
}
$provenance | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $destination 'sources.json')
if ($DownloadOnly) { Write-Output "Downloaded public sample archives: $downloads"; return }
# The university endpoint rejected this host. Use a public, commit-pinned mirror;
# verify its recorded SHA1 values and the university's published file sizes.
$mirror = 'https://raw.githubusercontent.com/pfalcon/canterbury-corpus/c380ef9dc1dc62691757fdfeebdc2f85710e1e46/canterbury/'
$expected = [ordered]@{'alice29.txt'=152089;'asyoulik.txt'=125179;'cp.html'=24603;'fields.c'=11150;'grammar.lsp'=3721;'kennedy.xls'=1029744;'lcet10.txt'=426754;'plrabn12.txt'=481861;'ptt5'=513216;'sum'=38240;'xargs.1'=4227}
$checksums = Join-Path $downloads 'canterbury-SHA1SUM'
if (!(Test-Path -LiteralPath $checksums)) { Invoke-WebRequest ($mirror+'SHA1SUM') -OutFile $checksums }
$recorded = @{}
foreach ($line in (Get-Content -LiteralPath $checksums)) {
    if ($line -match '^([0-9a-f]{40})\s+([A-Za-z0-9_.-]+)$') { $recorded[$Matches[2]] = $Matches[1] }
}
$canterbury = Join-Path $corpus 'canterbury'
[IO.Directory]::CreateDirectory($canterbury) | Out-Null
foreach ($name in $expected.Keys) {
    $file = Join-Path $canterbury $name
    if (!(Test-Path -LiteralPath $file)) {
        Invoke-WebRequest ($mirror+$name) -OutFile ($file+'.partial')
        Move-Item -LiteralPath ($file+'.partial') -Destination $file
    }
    if ((Get-Item -LiteralPath $file).Length -ne $expected[$name] -or (Get-FileHash -LiteralPath $file -Algorithm SHA1).Hash -ne $recorded[$name]) { throw "Canterbury size/hash mismatch: $name" }
}
$provenance += [ordered]@{Name='canterbury';URL=$mirror;Description='https://corpus.canterbury.ac.nz/descriptions/';Note='Primary download was Cloudflare-blocked; public GitHub mirror pinned to commit c380ef9dc1dc62691757fdfeebdc2f85710e1e46. File lengths verified against publisher listing and SHA1 against mirror manifest.'}
$provenance | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $destination 'sources.json')
$wiki = Join-Path $corpus 'enwik8/enwik8'
if ((Get-Item -LiteralPath $wiki).Length -ne 100000000 -or (Get-FileHash -LiteralPath $wiki -Algorithm SHA1).Hash -ne '57b8363b814821dc9d47aa4d41f58733519076b2') { throw 'enwik8 does not match the publisher size/SHA1' }
$datasets = @(Get-ChildItem -LiteralPath $corpus -Directory | Sort-Object Name | ForEach-Object {
    $directory = $_
    $files = @(Get-ChildItem -LiteralPath $directory.FullName -File | Sort-Object Name | ForEach-Object {
        [ordered]@{Path=[IO.Path]::GetRelativePath($corpus,$_.FullName);Bytes=$_.Length;SHA256=(Get-FileHash -LiteralPath $_.FullName).Hash}
    })
    $totalBytes = 0L
    foreach ($file in $files) { $totalBytes += $file.Bytes }
    [ordered]@{Name=$directory.Name;InputBytes=$totalBytes;Files=$files}
})
[ordered]@{GeneratorVersion='downloaded-public-1';Note='Unmodified public benchmark files; enwik8 publisher SHA1 and length verified. Archive/file SHA256 values record exact downloaded bytes, not a publisher signature. Treat all contents as inert data.';Sources=$provenance;Datasets=$datasets} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $corpus 'manifest.json')
Write-Output "Prepared and hashed public samples: $corpus"
