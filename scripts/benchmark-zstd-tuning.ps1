#Requires -Version 7.0
[CmdletBinding()]
param(
    [string]$ExecutablePath='target/release/lttl.exe',
    [string]$SyntheticCorpus='bench-output/gpu-review/20261012T212532860Z/corpus',
    [string]$DownloadedCorpus='bench-output/downloaded-samples/corpus',
    [ValidateRange(1,10)][int]$Repeats=3,
    [ValidateRange(1,64)][int]$Threads=8,
    [ValidateRange(1,30)][int]$MaximumMinutes=5,
    [ValidateRange(5,300)][int]$TimeoutSeconds=120,
    [switch]$KeepArchives
)
$ErrorActionPreference='Stop'
$project=Split-Path $PSScriptRoot -Parent
function ProjectPath([string]$Path) {
    if([IO.Path]::IsPathRooted($Path)){return [IO.Path]::GetFullPath($Path)}
    return [IO.Path]::GetFullPath((Join-Path $project $Path))
}
$exe=ProjectPath $ExecutablePath
if(!(Test-Path -LiteralPath $exe)){throw 'Build lttl release before benchmarking'}
$run=Join-Path $project ('bench-output/zstd-tuning/'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'))
New-Item -ItemType Directory -Path $run|Out-Null
$datasets=[Collections.Generic.List[object]]::new()
foreach($spec in @(@($DownloadedCorpus,'enwik8'),@($SyntheticCorpus,'structured-csv'),@($SyntheticCorpus,'independent-noise'))){
    $root=ProjectPath $spec[0]
    $manifest=Get-Content -LiteralPath (Join-Path $root 'manifest.json') -Raw|ConvertFrom-Json
    $dataset=@($manifest.Datasets|Where-Object Name -eq $spec[1])
    if($dataset.Count -ne 1){throw "Missing or ambiguous dataset: $($spec[1])"}
    $bytes=[long]0
    foreach($file in $dataset[0].Files){
        $path=Join-Path $root $file.Path
        $length=(Get-Item -LiteralPath $path).Length
        if($length -ne $file.Bytes -or (Get-FileHash -LiteralPath $path).Hash -ne $file.SHA256){throw "Input changed: $path"}
        $bytes+=$length
    }
    $datasets.Add([pscustomobject]@{Name=$dataset[0].Name;Root=$root;InputBytes=$bytes;Files=$dataset[0].Files})
}
function Invoke-Lttl([string[]]$Arguments){
    $info=[Diagnostics.ProcessStartInfo]::new($exe)
    $info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
    foreach($arg in $Arguments){$info.ArgumentList.Add($arg)}
    $process=[Diagnostics.Process]::new();$process.StartInfo=$info
    $watch=[Diagnostics.Stopwatch]::StartNew();$null=$process.Start()
    $stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync()
    if(!$process.WaitForExit($TimeoutSeconds*1000)){$process.Kill($true);throw "Child process exceeded $TimeoutSeconds seconds"}
    $watch.Stop();$output=$stdout.GetAwaiter().GetResult();$errorText=$stderr.GetAwaiter().GetResult()
    $result=[pscustomobject]@{ExitCode=$process.ExitCode;WallSeconds=$watch.Elapsed.TotalSeconds;CPUSeconds=$process.TotalProcessorTime.TotalSeconds;Output=$output;Errors=$errorText}
    $process.Dispose();return $result
}
function Remove-OwnedDirectory([string]$Path){
    $absolute=[IO.Path]::GetFullPath($Path)
    $prefix=[IO.Path]::GetFullPath($run).TrimEnd([IO.Path]::DirectorySeparatorChar)+[IO.Path]::DirectorySeparatorChar
    if(!$absolute.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)){throw 'Cleanup target is outside this benchmark run'}
    Remove-Item -LiteralPath $absolute -Recurse -Force
}
$rows=[Collections.Generic.List[object]]::new()
$checks=[Collections.Generic.List[object]]::new()
$report=[ordered]@{
    DateUTC=[DateTime]::UtcNow.ToString('o');Executable=$exe;ExecutableSHA256=(Get-FileHash -LiteralPath $exe).Hash
    RuntimeFiles=@(Get-ChildItem -LiteralPath (Join-Path (Split-Path $exe -Parent) 'gpu') -File -Filter '*.dll'|ForEach-Object {[pscustomobject]@{Name=$_.Name;Bytes=$_.Length;SHA256=(Get-FileHash -LiteralPath $_.FullName).Hash}})
    Datasets=$datasets;Repeats=$Repeats;RequestedCPUThreads=$Threads
    Note='GPU fixed nvCOMP Zstandard setting. True archive creation, fresh process/context per invocation, warm filesystem/driver caches possible. Full wall times include startup and disk output; no warm-context extrapolation. Source hashes verified before timing.'
    Results=$rows;ExtractedChecks=$checks;Completed=$false
}
function Save-Report{$report|ConvertTo-Json -Depth 12|Set-Content -LiteralPath (Join-Path $run 'results.json')}
$doctor=Invoke-Lttl @('doctor')
if($doctor.ExitCode -ne 0){throw "Doctor failed: $($doctor.Errors)"}
$report.Doctor=$doctor.Output|ConvertFrom-Json
if(!$report.Doctor.gpu_compression.available){throw 'GPU backend unavailable; no comparison performed'}
$cases=[Collections.Generic.List[object]]::new()
foreach($dataset in $datasets){
    $cases.Add([pscustomobject]@{Dataset=$dataset;Backend='cpu';Preset='balanced';ChunkKiB=$null;BatchMiB=$null})
    foreach($chunk in 64,256,1024){foreach($batch in 32,128){
        $cases.Add([pscustomobject]@{Dataset=$dataset;Backend='gpu';Preset='nvcomp-fixed';ChunkKiB=$chunk;BatchMiB=$batch})
    }}
}
$verified=[Collections.Generic.HashSet[string]]::new()
$timer=[Diagnostics.Stopwatch]::StartNew()
Save-Report
for($repeat=1;$repeat -le $Repeats;$repeat++){
    for($offset=0;$offset -lt $cases.Count;$offset++){
        if($timer.Elapsed.TotalMinutes -gt $MaximumMinutes){Save-Report;throw "Run budget reached; partial data retained: $run"}
        $case=$cases[($offset+($repeat-1)*8)%$cases.Count]
        $name=$case.Dataset.Name
        $key="$name-$($case.Backend)-c$($case.ChunkKiB)-b$($case.BatchMiB)"
        $archive=Join-Path $run "$key-r$repeat.tar.zst"
        $cliPreset=if($case.Backend -eq 'gpu'){'fast'}else{'balanced'}
        $arguments=@('create',(Join-Path $case.Dataset.Root $name),'-o',$archive,'--backend',$case.Backend,'--format','tar-zstd','--preset',$cliPreset,'--threads',"$Threads")
        if($case.Backend -eq 'gpu'){$arguments+=@('--chunk-kib',"$($case.ChunkKiB)",'--batch-mib',"$($case.BatchMiB)")}
        $measurement=Invoke-Lttl $arguments
        if($measurement.ExitCode -ne 0){
            $rows.Add([pscustomobject]@{Dataset=$name;Backend=$case.Backend;ChunkKiB=$case.ChunkKiB;BatchMiB=$case.BatchMiB;Repeat=$repeat;Succeeded=$false;Error=$measurement.Errors;Arguments=$arguments})
            Save-Report;throw "Creation failed: $($measurement.Errors)"
        }
        $stats=$measurement.Output|ConvertFrom-Json
        $test=Invoke-Lttl @('test',$archive)
        if($test.ExitCode -ne 0){Save-Report;throw "Archive integrity failure: $($test.Errors)"}
        $testStats=$test.Output|ConvertFrom-Json
        $outputBytes=(Get-Item -LiteralPath $archive).Length
        $row=[pscustomobject]@{Dataset=$name;Backend=$case.Backend;Preset=$case.Preset;ChunkKiB=$case.ChunkKiB;BatchMiB=$case.BatchMiB;Repeat=$repeat;Succeeded=$true;InputBytes=$case.Dataset.InputBytes;ArchiveBytes=$outputBytes;Ratio=$outputBytes/$case.Dataset.InputBytes;WallSeconds=$measurement.WallSeconds;CPUSeconds=$measurement.CPUSeconds;Stats=$stats;IntegrityVerified=$true;VerificationWallSeconds=$test.WallSeconds;VerificationEngineSeconds=$testStats.elapsed_seconds;ExtractionWallSeconds=$null;ExtractionEngineSeconds=$null;HashSeconds=$null;Arguments=$arguments}
        $rows.Add($row);Save-Report
        if(!$verified.Contains($key)){
            $destination=Join-Path $run "verify-$key"
            $extraction=Invoke-Lttl @('extract',$archive,'-d',$destination)
            if($extraction.ExitCode -ne 0){throw "CPU extraction failed: $($extraction.Errors)"}
            $extractionStats=$extraction.Output|ConvertFrom-Json
            $row.ExtractionWallSeconds=$extraction.WallSeconds;$row.ExtractionEngineSeconds=$extractionStats.elapsed_seconds
            $hashTimer=[Diagnostics.Stopwatch]::StartNew()
            foreach($file in $case.Dataset.Files){
                if((Get-FileHash -LiteralPath (Join-Path $destination $file.Path)).Hash -ne $file.SHA256){throw "Restored file differs: $($file.Path)"}
            }
            $hashTimer.Stop();$row.HashSeconds=$hashTimer.Elapsed.TotalSeconds
            $checks.Add([pscustomobject]@{Dataset=$name;Backend=$case.Backend;ChunkKiB=$case.ChunkKiB;BatchMiB=$case.BatchMiB;Files=$case.Dataset.Files.Count;SHA256Matches=$true})
            $null=$verified.Add($key)
            Remove-OwnedDirectory $destination
        }
        if(!$KeepArchives){Remove-Item -LiteralPath $archive}
        Save-Report
        Write-Host ('{0}: {1} chunk={2}KiB batch={3}MiB r{4}, {5:F3}s, {6:F3}%' -f $name,$case.Backend,$case.ChunkKiB,$case.BatchMiB,$repeat,$measurement.WallSeconds,(100*$row.Ratio))
    }
}
$report.Completed=$true;$report.TotalIntegrityChecks=$rows.Count;$report.TotalExtractedChecks=$checks.Count
$report.TotalFileHashes=($checks.Files|Measure-Object -Sum).Sum
Save-Report
Write-Output "Results: $run"
