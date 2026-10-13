param(
    [string]$CorpusPath = 'bench-output/rar-review/20261012T205958852Z/corpus',
    [string]$ExecutablePath = 'target/release/archiver.exe',
    [ValidateRange(1,10)][int]$Repeats = 3
)
$ErrorActionPreference='Stop'
$project=Split-Path $PSScriptRoot -Parent
$exe=(Resolve-Path -LiteralPath (Join-Path $project $ExecutablePath)).Path
$corpus=(Resolve-Path -LiteralPath (Join-Path $project $CorpusPath)).Path
$out=Join-Path $project ('bench-output/rust-review/'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'))
New-Item -ItemType Directory -Path $out | Out-Null
$manifest=Import-Csv -LiteralPath (Join-Path (Split-Path $corpus -Parent) 'corpus.csv')
foreach($item in $manifest) {
    $actual=Get-FileHash -LiteralPath (Join-Path $corpus $item.Path)
    if($actual.Hash -ne $item.SHA256) { throw "Corpus hash mismatch: $($item.Path)" }
}
function Invoke-Archiver([string[]]$Arguments) {
    $info=[Diagnostics.ProcessStartInfo]::new($exe)
    $info.UseShellExecute=$false; $info.CreateNoWindow=$true
    $info.RedirectStandardOutput=$true; $info.RedirectStandardError=$true
    foreach($arg in $Arguments) { $info.ArgumentList.Add($arg) }
    $p=[Diagnostics.Process]::new(); $p.StartInfo=$info
    $watch=[Diagnostics.Stopwatch]::StartNew(); $null=$p.Start()
    $stdout=$p.StandardOutput.ReadToEndAsync(); $stderr=$p.StandardError.ReadToEndAsync()
    if(!$p.WaitForExit(120000)) { $p.Kill($true); throw 'Archiver timed out' }
    $watch.Stop()
    $output=$stdout.GetAwaiter().GetResult(); $errors=$stderr.GetAwaiter().GetResult()
    if($p.ExitCode -ne 0) { throw "Archiver exit $($p.ExitCode): $errors" }
    $result=[pscustomobject]@{WallSeconds=$watch.Elapsed.TotalSeconds;CPUSeconds=$p.TotalProcessorTime.TotalSeconds;PeakWorkingSetBytes=$p.PeakWorkingSet64;Stats=($output|ConvertFrom-Json)}
    $p.Dispose(); return $result
}
$cases=[Collections.Generic.List[object]]::new()
foreach($name in 'records.txt','repeating.bin','random.bin','similar') {
    foreach($format in 'zip','tar-zstd') {
        foreach($preset in 'fast','balanced','small') {
            $cases.Add([pscustomobject]@{Corpus=$name;Format=$format;Preset=$preset;Threads=8})
        }
    }
    $cases.Add([pscustomobject]@{Corpus=$name;Format='tar-zstd';Preset='balanced';Threads=1})
}
$rows=[Collections.Generic.List[object]]::new()
$report=[ordered]@{
    DateUTC=[DateTime]::UtcNow.ToString('o');Executable=$exe;ExecutableSHA256=(Get-FileHash -LiteralPath $exe).Hash
    Corpus=$corpus;CorpusHashesVerified=$true;CorpusManifest=$manifest;Repeats=$Repeats
    Note='Separate live-host run from RAR. Warm cache, no isolation; native analysis and development may contend for resources. WallSeconds includes process launch. ZIP is serial regardless of requested threads.'
    Results=$rows
}
for($repeat=1;$repeat -le $Repeats;$repeat++) {
    for($offset=0;$offset -lt $cases.Count;$offset++) {
        $case=$cases[($offset+($repeat-1)*9)%$cases.Count]
        $suffix=if($case.Format -eq 'zip'){'zip'}else{'tar.zst'}
        $archive=Join-Path $out "$($case.Corpus)-$($case.Format)-$($case.Preset)-t$($case.Threads)-r$repeat.$suffix"
        $arguments=@('create',(Join-Path $corpus $case.Corpus),'-o',$archive,'--format',$case.Format,'--preset',$case.Preset,'--threads',"$($case.Threads)")
        $measurement=Invoke-Archiver $arguments
        $null=Invoke-Archiver @('test',$archive)
        $rows.Add([pscustomobject]@{Corpus=$case.Corpus;Format=$case.Format;Preset=$case.Preset;RequestedThreads=$case.Threads;Repeat=$repeat;WallSeconds=$measurement.WallSeconds;CPUSeconds=$measurement.CPUSeconds;PeakWorkingSetBytes=$measurement.PeakWorkingSetBytes;Stats=$measurement.Stats;IntegrityVerified=$true;Arguments=$arguments})
        $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $out 'results.json')
        Write-Host ('{0} {1}/{2}/t{3}: {4:F3}s, {5} bytes' -f $case.Corpus,$case.Format,$case.Preset,$case.Threads,$measurement.WallSeconds,$measurement.Stats.output_bytes)
    }
}
# Independently compare extracted bytes against original SHA-256 manifests for
# one ZIP Fast and one TAR.ZST Balanced archive from each corpus (8 round trips).
$checks=[Collections.Generic.List[object]]::new()
foreach($name in 'records.txt','repeating.bin','random.bin','similar') {
    foreach($format in 'zip','tar-zstd') {
        $preset=if($format -eq 'zip'){'fast'}else{'balanced'}
        $suffix=if($format -eq 'zip'){'zip'}else{'tar.zst'}
        $archive=Join-Path $out "$name-$format-$preset-t8-r1.$suffix"
        $destination=Join-Path $out "verify-$name-$format"
        $null=Invoke-Archiver @('extract',$archive,'-d',$destination)
        $files=@(Get-ChildItem -LiteralPath (Join-Path $corpus $name) -File -Recurse)
        foreach($source in $files) {
            $relative=[IO.Path]::GetRelativePath($corpus,$source.FullName)
            $restored=Join-Path $destination $relative
            if((Get-FileHash -LiteralPath $source.FullName).Hash -ne (Get-FileHash -LiteralPath $restored).Hash) {
                throw "Extracted file hash mismatch: $relative"
            }
        }
        $checks.Add([pscustomobject]@{Corpus=$name;Format=$format;Preset=$preset;Files=$files.Count;ExtractedSHA256Matches=$true})
    }
}
$report.ExtractedHashChecks=$checks
$report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $out 'results.json')
Write-Host "Results: $out"
