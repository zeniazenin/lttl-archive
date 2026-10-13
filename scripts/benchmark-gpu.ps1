#Requires -Version 7.0
[CmdletBinding()]
param(
    [string]$ExecutablePath = 'target/release/lttl.exe',
    [string]$ReferenceExecutablePath,
    [string]$CorpusDirectory,
    [ValidateRange(8,512)][int]$SizeMiB = 64,
    [ValidateRange(1,10)][int]$Repeats = 3,
    [ValidateRange(1,10)][int]$StartRepeat = 1,
    [ValidateSet('fast','balanced','small')][string[]]$Presets = @('fast','balanced','small'),
    [ValidateSet('cpu','cpu-chunked','gpu','auto')][string[]]$Backends = @('cpu','cpu-chunked','gpu'),
    [ValidateRange(1,64)][int]$Threads = 8,
    [ValidateRange(1,64)][int]$ChunkMiB = 1,
    [ValidateRange(1,1024)][int]$BatchMiB = 32,
    [ValidateRange(10,600)][int]$TimeoutSeconds = 120,
    [ValidateRange(1,120)][int]$MaximumMinutes = 20,
    [switch]$IncludeInstalledBinaries,
    [switch]$PrepareOnly,
    [switch]$KeepArchives,
    [switch]$SkipChunkSweep,
    [switch]$TimeExtraction
)
$ErrorActionPreference='Stop'
if($StartRepeat -gt $Repeats) { throw 'StartRepeat must not exceed Repeats' }
$project=Split-Path $PSScriptRoot -Parent
function Resolve-ProjectPath([string]$Path) {
    if([IO.Path]::IsPathRooted($Path)) { return [IO.Path]::GetFullPath($Path) }
    return [IO.Path]::GetFullPath((Join-Path $project $Path))
}
$run=Join-Path $project ('bench-output/gpu-review/'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'))
New-Item -ItemType Directory -Path $run | Out-Null
$newCorpus=[string]::IsNullOrWhiteSpace($CorpusDirectory)
$corpus=if($newCorpus){Join-Path $run 'corpus'}else{Resolve-ProjectPath $CorpusDirectory}
if($newCorpus) {
    New-Item -ItemType Directory -Path $corpus | Out-Null
    Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Text;
using System.IO.Compression;
public static class LttlGpuCorpus {
 static ulong state=0x8C032FC1A125A377UL;
 static ulong Next() { state ^= state >> 12; state ^= state << 25; state ^= state >> 27; return unchecked(state * 2685821657736338717UL); }
 static void Noise(byte[] b) { for(int i=0;i<b.Length;i+=8) { ulong x=Next(); for(int k=0;k<8 && i+k<b.Length;k++) b[i+k]=(byte)(x>>(k*8)); } }
 static string Dir(string root,string name) { string p=Path.Combine(root,name); Directory.CreateDirectory(p); return p; }
 public static void Generate(string root,int mib) {
  long bytes=(long)mib*1024*1024;
  string csv=Dir(root,"structured-csv");
  using(var f=File.Create(Path.Combine(csv,"events.csv"))) {
   long n=0;
   while(f.Length<bytes) {
    string row=(n++)+",sensor-"+(n%64)+",2026-10-12T12:"+(n%60).ToString("D2")+":00Z,"+(Next()%10000)+",ok,request-"+(Next()%1048576).ToString("X5")+",completed synthetic measurement\n";
    byte[] line=Encoding.UTF8.GetBytes(row); f.Write(line,0,(int)Math.Min(line.Length,bytes-f.Length));
   }
  }
  string sensor=Dir(root,"raw-sensors");
  using(var f=File.Create(Path.Combine(sensor,"sensor-u16le.raw"))) {
   byte[] block=new byte[1024*1024];
   for(int b=0;b<mib;b++) {
    for(int i=0;i<block.Length;i+=2) {
     long sample=((long)b*block.Length+i)/2;
     ushort v=(ushort)(10000+(sample/16)%1024+(sample%16)*32+(long)(Next()%7));
     block[i]=(byte)v; block[i+1]=(byte)(v>>8);
    }
    f.Write(block,0,block.Length);
   }
  }
  string image=Dir(root,"raw-rgb");
  using(var f=File.Create(Path.Combine(image,"gradient-rgb.raw"))) {
   byte[] block=new byte[1024*1024];
   for(int b=0;b<mib;b++) {
    for(int i=0;i<block.Length;i++) {
     long pixel=((long)b*block.Length+i)/3; int channel=(int)(((long)b*block.Length+i)%3);
     block[i]=(byte)(((pixel%2048)/8+(pixel/2048)%256+channel*40+(long)(Next()%3))%256);
    }
    f.Write(block,0,block.Length);
   }
  }
  string repeating=Dir(root,"repeating-binary"); byte[] motif=new byte[65536]; Noise(motif);
  using(var f=File.Create(Path.Combine(repeating,"repeat-64k.bin"))) { for(int i=0;i<mib*16;i++) f.Write(motif,0,motif.Length); }
  string noise=Dir(root,"independent-noise"); byte[] buf=new byte[1024*1024];
  using(var f=File.Create(Path.Combine(noise,"noise.bin"))) { for(int i=0;i<mib;i++) { Noise(buf); f.Write(buf,0,buf.Length); } }
  string similar=Dir(root,"similar-files"); byte[] shared=new byte[1024*1024]; Noise(shared);
  for(int n=0;n<mib;n++) { byte[] item=(byte[])shared.Clone(); byte[] tail=new byte[1024]; Noise(tail); Array.Copy(tail,0,item,item.Length-tail.Length,tail.Length); File.WriteAllBytes(Path.Combine(similar,"item-"+n.ToString("D3")+".bin"),item); }
  string zipped=Dir(root,"precompressed-zip");
  using(var stream=File.Create(Path.Combine(zipped,"sensors-and-rgb.zip")))
  using(var archive=new ZipArchive(stream,ZipArchiveMode.Create)) {
   foreach(string source in new[]{Path.Combine(sensor,"sensor-u16le.raw"),Path.Combine(image,"gradient-rgb.raw")}) {
    var entry=archive.CreateEntry(Path.GetFileName(source),CompressionLevel.Optimal);
    entry.LastWriteTime=new DateTimeOffset(2020,1,1,0,0,0,TimeSpan.Zero);
    using(var output=entry.Open()) using(var input=File.OpenRead(source)) {
     long left=bytes/2; byte[] readBuffer=new byte[1024*1024];
     while(left>0) { int count=input.Read(readBuffer,0,(int)Math.Min(readBuffer.Length,left)); if(count==0) throw new Exception("Short raw source"); output.Write(readBuffer,0,count); left-=count; }
    }
   }
  }
  // Keep the already-compressed dataset exactly the requested size without
  // repeated filler. Deflated raw data is mixed with independently generated
  // noise in a Stored member; a short random ZIP comment completes the size.
  string zipPath=Path.Combine(zipped,"sensors-and-rgb.zip");
  long gap=bytes-new FileInfo(zipPath).Length;
  if(gap<8192) throw new Exception("Precompressed base is too large for requested exact size");
  using(var stream=File.Open(zipPath,FileMode.Open,FileAccess.ReadWrite))
  using(var archive=new ZipArchive(stream,ZipArchiveMode.Update)) {
   var entry=archive.CreateEntry("independent-noise.bin",CompressionLevel.NoCompression);
   entry.LastWriteTime=new DateTimeOffset(2020,1,1,0,0,0,TimeSpan.Zero);
   using(var output=entry.Open()) { long left=gap-4096; while(left>0) { Noise(buf); int n=(int)Math.Min(left,buf.Length); output.Write(buf,0,n); left-=n; } }
  }
  using(var stream=File.Open(zipPath,FileMode.Open,FileAccess.ReadWrite)) {
   long missing=bytes-stream.Length;
   if(missing<0 || missing>65535) throw new Exception("Unexpected ZIP sizing overhead");
   stream.Position=stream.Length-22; byte[] end=new byte[22]; stream.ReadExactly(end);
   if(end[0]!=0x50 || end[1]!=0x4b || end[2]!=5 || end[3]!=6 || end[20]!=0 || end[21]!=0) throw new Exception("Unexpected ZIP end record");
   stream.Position=stream.Length-2; stream.WriteByte((byte)missing); stream.WriteByte((byte)(missing>>8));
   byte[] comment=new byte[(int)missing]; Noise(comment); stream.Write(comment,0,comment.Length);
  }
  using(var stream=File.OpenRead(zipPath)) using(var archive=new ZipArchive(stream,ZipArchiveMode.Read)) {
   foreach(var entry in archive.Entries) using(var input=entry.Open()) input.CopyTo(Stream.Null);
  }
 }
}
'@
    [LttlGpuCorpus]::Generate($corpus,$SizeMiB)
    $sourceDir=Join-Path $corpus 'project-sources'
    New-Item -ItemType Directory -Path $sourceDir | Out-Null
    # Explicit project files only: do not sweep caches, private config, or outputs.
    Copy-Item -LiteralPath (Join-Path $project 'src') -Destination $sourceDir -Recurse
    foreach($file in 'Cargo.toml','Cargo.lock','README.md') {
        if(Test-Path -LiteralPath (Join-Path $project $file)) { Copy-Item -LiteralPath (Join-Path $project $file) -Destination $sourceDir }
    }
    if($IncludeInstalledBinaries) {
        $binaryDir=Join-Path $corpus 'installed-binaries'
        New-Item -ItemType Directory -Path $binaryDir | Out-Null
        foreach($file in 'Rar.exe','WinRAR.exe') {
            $original=Join-Path 'C:/Program Files/WinRAR' $file
            if(Test-Path -LiteralPath $original) { Copy-Item -LiteralPath $original -Destination $binaryDir }
        }
    }
    $datasets=@(Get-ChildItem -LiteralPath $corpus -Directory | Sort-Object Name | ForEach-Object {
        $dir=$_
        $files=@(Get-ChildItem -LiteralPath $dir.FullName -File -Recurse | Sort-Object FullName | ForEach-Object {
            [pscustomobject]@{Path=[IO.Path]::GetRelativePath($corpus,$_.FullName);Bytes=$_.Length;SHA256=(Get-FileHash -LiteralPath $_.FullName).Hash}
        })
        [pscustomobject]@{Name=$dir.Name;InputBytes=($files|Measure-Object -Property Bytes -Sum).Sum;Files=$files}
    })
    [ordered]@{GeneratorVersion=2;SizeMiB=$SizeMiB;Runtime=[Runtime.InteropServices.RuntimeInformation]::FrameworkDescription;Note='Synthetic byte patterns, not a representative real-world benchmark. Exact-size precompressed ZIP contains genuinely Deflated half-size prefixes of sensor/RGB files, independently generated Stored noise, and a short random ZIP comment. Source snapshot and optional real installed binaries have actual sizes.';Datasets=$datasets}|ConvertTo-Json -Depth 7|Set-Content -LiteralPath (Join-Path $corpus 'manifest.json')
}
$manifest=Get-Content -LiteralPath (Join-Path $corpus 'manifest.json') -Raw|ConvertFrom-Json
foreach($dataset in $manifest.Datasets) {
    $verifiedBytes=[long]0
    foreach($file in $dataset.Files) {
        $sourcePath=Join-Path $corpus $file.Path
        $length=(Get-Item -LiteralPath $sourcePath).Length
        if($length -ne $file.Bytes -or (Get-FileHash -LiteralPath $sourcePath).Hash -ne $file.SHA256) { throw "Source manifest mismatch: $($file.Path)" }
        $verifiedBytes+=$length
    }
    if($null -eq $dataset.InputBytes) {
        $dataset.InputBytes=$verifiedBytes
        $dataset|Add-Member -NotePropertyName InputBytesDerivedFromVerifiedFiles -NotePropertyValue $true
    } elseif([long]$dataset.InputBytes -ne $verifiedBytes) { throw "Dataset byte total mismatch: $($dataset.Name)" }
    if($verifiedBytes -eq 0) { throw "Empty benchmark dataset: $($dataset.Name)" }
}
if($PrepareOnly) { Write-Output "Prepared and hashed corpus: $corpus"; return }
$exe=Resolve-ProjectPath $ExecutablePath
if(!(Test-Path -LiteralPath $exe)) { throw "Build lttl first: $exe" }
function Invoke-Lttl([string[]]$Arguments,[string]$Binary=$exe) {
    $info=[Diagnostics.ProcessStartInfo]::new($Binary)
    $info.UseShellExecute=$false; $info.CreateNoWindow=$true; $info.RedirectStandardOutput=$true; $info.RedirectStandardError=$true
    foreach($arg in $Arguments) { $info.ArgumentList.Add($arg) }
    $p=[Diagnostics.Process]::new(); $p.StartInfo=$info
    $watch=[Diagnostics.Stopwatch]::StartNew(); $null=$p.Start()
    $stdout=$p.StandardOutput.ReadToEndAsync(); $stderr=$p.StandardError.ReadToEndAsync()
    if(!$p.WaitForExit($TimeoutSeconds*1000)) { $p.Kill($true); throw "lttl exceeded $TimeoutSeconds seconds" }
    $watch.Stop(); $text=$stdout.GetAwaiter().GetResult(); $errors=$stderr.GetAwaiter().GetResult()
    $result=[pscustomobject]@{ExitCode=$p.ExitCode;WallSeconds=$watch.Elapsed.TotalSeconds;CPUSeconds=$p.TotalProcessorTime.TotalSeconds;PeakWorkingSetBytes=$p.PeakWorkingSet64;Output=$text;Errors=$errors}
    $p.Dispose(); return $result
}
function Remove-OwnedTree([string]$Path) {
    $absolute=[IO.Path]::GetFullPath($Path)
    $prefix=[IO.Path]::GetFullPath($run).TrimEnd([IO.Path]::DirectorySeparatorChar)+[IO.Path]::DirectorySeparatorChar
    if(!$absolute.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)) { throw "Cleanup path outside this run: $absolute" }
    Remove-Item -LiteralPath $absolute -Recurse -Force
}
$rows=[Collections.Generic.List[object]]::new()
$roundTrips=[Collections.Generic.List[object]]::new()
$report=[ordered]@{
    DateUTC=[DateTime]::UtcNow.ToString('o');Executable=$exe;ExecutableSHA256=(Get-FileHash -LiteralPath $exe).Hash
    OS=[Runtime.InteropServices.RuntimeInformation]::OSDescription;LogicalProcessors=[Environment]::ProcessorCount
    Corpus=$corpus;Manifest=$manifest;Repeats=$Repeats;StartRepeat=$StartRepeat;Threads=$Threads;ChunkMiB=$ChunkMiB;BatchMiB=$BatchMiB
    Note='Separate fresh processes; cold GPU context initialization included each run, warm OS cache; no CPU/GPU/thermal or background-load isolation. Rotated cases, with order reversed on even repetitions. Codec framing and ratio may differ by backend.'
    Results=$rows;RoundTrips=$roundTrips;TimeExtractionEveryRun=[bool]$TimeExtraction
    GpuRuntimeFiles=@(Get-ChildItem -LiteralPath (Join-Path (Split-Path $exe -Parent) 'gpu') -File -Filter '*.dll'|ForEach-Object {[pscustomobject]@{Name=$_.Name;Bytes=$_.Length;SHA256=(Get-FileHash -LiteralPath $_.FullName).Hash;FileVersion=$_.VersionInfo.FileVersion}})
}
$reference=$null
if($ReferenceExecutablePath) {
    $reference=Resolve-ProjectPath $ReferenceExecutablePath
    if(!(Test-Path -LiteralPath $reference)) { throw "Reference executable missing: $reference" }
    $report.ReferenceExecutable=$reference
    $report.ReferenceExecutableSHA256=(Get-FileHash -LiteralPath $reference).Hash
}
try { $doctor=Invoke-Lttl @('doctor'); $report.Doctor=$doctor.Output|ConvertFrom-Json } catch { $report.DoctorError=$_.Exception.Message }
function Save-Report { $report|ConvertTo-Json -Depth 12|Set-Content -LiteralPath (Join-Path $run 'results.json') }
$cases=[Collections.Generic.List[object]]::new()
foreach($dataset in $manifest.Datasets) { foreach($backend in $Backends) {
    $selectedPresets=if($backend -in @('cpu','auto')){$Presets}else{@('fast')}
    foreach($preset in $selectedPresets) {
        $label=if($backend -eq 'gpu'){'nvcomp-fixed'}else{$preset}
        $cases.Add([pscustomobject]@{Dataset=$dataset;Preset=$preset;PresetLabel=$label;Backend=$backend;CreateBackend=$backend;Executable=$exe;ChunkMiB=$ChunkMiB;FocusedSweep=$false})
        if($reference -and $backend -eq 'auto') {
            $cases.Add([pscustomobject]@{Dataset=$dataset;Preset=$preset;PresetLabel=$label;Backend='auto-v1';CreateBackend='auto';Executable=$reference;ChunkMiB=$ChunkMiB;FocusedSweep=$false})
        }
    }
} }
$similar=$manifest.Datasets|Where-Object Name -eq 'similar-files'
if(!$SkipChunkSweep -and $null -ne $similar) {
    foreach($chunkSize in 4,16) { foreach($backend in $Backends|Where-Object {$_ -in @('gpu','cpu-chunked')}) {
        if($chunkSize -eq $ChunkMiB) { continue }
        $label=if($backend -eq 'gpu'){'nvcomp-fixed'}else{'fast'}
        $cases.Add([pscustomobject]@{Dataset=$similar;Preset='fast';PresetLabel=$label;Backend=$backend;CreateBackend=$backend;Executable=$exe;ChunkMiB=$chunkSize;FocusedSweep=$true})
    } }
}
$verified=[Collections.Generic.HashSet[string]]::new()
$timer=[Diagnostics.Stopwatch]::StartNew()
Save-Report
for($repeat=$StartRepeat;$repeat -le $Repeats;$repeat++) {
    for($offset=0;$offset -lt $cases.Count;$offset++) {
        if($timer.Elapsed.TotalMinutes -gt $MaximumMinutes) { Save-Report; throw "Total benchmark budget reached; partial report retained at $run" }
        $caseIndex=($offset+($repeat-1)*11)%$cases.Count
        if($repeat%2 -eq 0) { $caseIndex=$cases.Count-1-$caseIndex }
        $case=$cases[$caseIndex]
        $name=$case.Dataset.Name
        $archive=Join-Path $run "$name-$($case.Backend)-$($case.Preset)-c$($case.ChunkMiB)-r$repeat.tar.zst"
        $arguments=@('create',(Join-Path $corpus $name),'-o',$archive,'--backend',$case.CreateBackend,'--format','tar-zstd','--preset',$case.Preset,'--threads',"$Threads",'--chunk-mib',"$($case.ChunkMiB)",'--batch-mib',"$BatchMiB")
        $measurement=Invoke-Lttl $arguments $case.Executable
        if($measurement.ExitCode -ne 0) {
            $rows.Add([pscustomobject]@{Dataset=$name;Backend=$case.Backend;Preset=$case.PresetLabel;ChunkMiB=$case.ChunkMiB;Repeat=$repeat;Succeeded=$false;Error=$measurement.Errors;Arguments=$arguments})
            Save-Report; throw "Creation failed; partial report saved: $($measurement.Errors)"
        }
        $stats=$measurement.Output|ConvertFrom-Json
        $check=Invoke-Lttl @('test',$archive) $case.Executable
        if($check.ExitCode -ne 0) { Save-Report; throw "Integrity failure: $($check.Errors)" }
        $bytes=(Get-Item -LiteralPath $archive).Length
        $testStats=$check.Output|ConvertFrom-Json
        $row=[pscustomobject]@{Dataset=$name;Backend=$case.Backend;Preset=$case.PresetLabel;ChunkMiB=$case.ChunkMiB;BatchMiB=$BatchMiB;FocusedSweep=$case.FocusedSweep;Repeat=$repeat;Succeeded=$true;InputBytes=$case.Dataset.InputBytes;ArchiveBytes=$bytes;Ratio=$bytes/$case.Dataset.InputBytes;WallSeconds=$measurement.WallSeconds;CPUSeconds=$measurement.CPUSeconds;PeakWorkingSetBytes=$measurement.PeakWorkingSetBytes;Stats=$stats;IntegrityVerified=$true;VerificationWallSeconds=$check.WallSeconds;VerificationEngineSeconds=$testStats.elapsed_seconds;ExtractionWallSeconds=$null;ExtractionEngineSeconds=$null;ExtractedHashSeconds=$null;Arguments=$arguments}
        $rows.Add($row)
        Save-Report
        $verifyKey="$name/$($case.Backend)/$($case.ChunkMiB)"
        if($TimeExtraction -or !$verified.Contains($verifyKey)) {
            $destination=Join-Path $run "verify-$name-$($case.Backend)-c$($case.ChunkMiB)"
            $extraction=Invoke-Lttl @('extract',$archive,'-d',$destination) $case.Executable
            if($extraction.ExitCode -ne 0) { throw "Extraction failed: $($extraction.Errors)" }
            $extractionStats=$extraction.Output|ConvertFrom-Json
            $row.ExtractionWallSeconds=$extraction.WallSeconds
            $row.ExtractionEngineSeconds=$extractionStats.elapsed_seconds
            $hashTimer=[Diagnostics.Stopwatch]::StartNew()
            foreach($file in $case.Dataset.Files) {
                if((Get-FileHash -LiteralPath (Join-Path $destination $file.Path)).Hash -ne $file.SHA256) { throw "Extracted bytes differ: $($file.Path)" }
            }
            $hashTimer.Stop()
            $row.ExtractedHashSeconds=$hashTimer.Elapsed.TotalSeconds
            $roundTrips.Add([pscustomobject]@{Dataset=$name;Backend=$case.Backend;Preset=$case.PresetLabel;ChunkMiB=$case.ChunkMiB;Repeat=$repeat;Files=$case.Dataset.Files.Count;SHA256Matches=$true})
            $null=$verified.Add($verifyKey)
            Remove-OwnedTree $destination
        }
        if(!$KeepArchives) { Remove-Item -LiteralPath $archive }
        Save-Report
        Write-Host ('{0} {1}/{2} r{3}: create {4:F3}s, {5:F3}% original size, extract {6}' -f $name,$case.Backend,$case.PresetLabel,$repeat,$measurement.WallSeconds,(100*$bytes/$case.Dataset.InputBytes),$row.ExtractionWallSeconds)
    }
}
$report.Completed=$true
$report.TotalIntegrityChecks=$rows.Count
$report.TotalRoundTrips=$roundTrips.Count
$report.TotalComparedFileHashes=($roundTrips.Files|Measure-Object -Sum).Sum
$report.MemoryMeasurementNote='PeakWorkingSetBytes may be unavailable (null). No VRAM peak or power measurement is collected.'
Save-Report
Write-Output "Results: $run"
