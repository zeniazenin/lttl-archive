param(
    [string]$RarPath = 'C:\Program Files\WinRAR\Rar.exe',
    [ValidateRange(1, 256)][int]$SizeMiB = 32,
    [ValidateRange(1, 10)][int]$Repeats = 3,
    [ValidateRange(5, 300)][int]$TimeoutSeconds = 60
)
$ErrorActionPreference = 'Stop'
# Requires PowerShell 7 / .NET. Writes only into a fresh run beneath bench-output/rar-review.
$root = Join-Path (Split-Path $PSScriptRoot -Parent) 'bench-output/rar-review'
$run = Join-Path $root ([DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'))
$corpus = Join-Path $run 'corpus'
$archives = Join-Path $run 'archives'
New-Item -ItemType Directory -Path $corpus, $archives | Out-Null
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Text;
public static class RarReviewCorpus {
 public static void Generate(string dir, int mib) {
  var rng = new Random(1729);
  byte[] noise = new byte[mib * 1024 * 1024]; rng.NextBytes(noise);
  File.WriteAllBytes(Path.Combine(dir,"random.bin"),noise);
  byte[] block = new byte[65536]; rng.NextBytes(block);
  using(var f = File.Create(Path.Combine(dir,"repeating.bin"))) {
   for(int i=0;i<mib*16;i++) f.Write(block,0,block.Length);
  }
  using(var f = File.Create(Path.Combine(dir,"records.txt"))) {
   int i=0; long goal=(long)mib*1024*1024;
   while(f.Length<goal) {
    byte[] line=Encoding.UTF8.GetBytes("record="+(i++)+" service=archiver status=ok request="+rng.Next().ToString("X8")+" duration="+rng.Next(10000)+" message=completed file processing successfully\n");
    f.Write(line,0,(int)Math.Min(line.Length,goal-f.Length));
   }
  }
  string group=Path.Combine(dir,"similar"); Directory.CreateDirectory(group);
  byte[] shared=new byte[1024*1024]; rng.NextBytes(shared);
  for(int i=0;i<mib;i++) {
   byte[] item=(byte[])shared.Clone(); byte[] tail=new byte[1024]; rng.NextBytes(tail);
   Array.Copy(tail,0,item,item.Length-tail.Length,tail.Length);
   File.WriteAllBytes(Path.Combine(group,"item"+i.ToString("D3")+".bin"),item);
  }
 }
}
'@
[RarReviewCorpus]::Generate($corpus, $SizeMiB)
$manifest = Get-ChildItem -LiteralPath $corpus -File -Recurse | ForEach-Object {
    [pscustomobject]@{ Path = [IO.Path]::GetRelativePath($corpus,$_.FullName); Bytes=$_.Length; SHA256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash }
}
$manifest | Export-Csv -NoTypeInformation -Path (Join-Path $run 'corpus.csv')
function Read-Hardware([string]$Class, [string[]]$Properties) {
    try { @(Get-CimInstance $Class | Select-Object -Property $Properties) }
    catch { "Unavailable: $($_.Exception.Message)" }
}
$envInfo = [ordered]@{
    RarPath=$RarPath; RarVersion=(Get-Item -LiteralPath $RarPath).VersionInfo.FileVersion
    RarSHA256=(Get-FileHash -LiteralPath $RarPath).Hash; PowerShell=$PSVersionTable.PSVersion.ToString()
    Runtime=[Runtime.InteropServices.RuntimeInformation]::FrameworkDescription
    OS=[Runtime.InteropServices.RuntimeInformation]::OSDescription
    LogicalProcessors=[Environment]::ProcessorCount; SizeMiB=$SizeMiB; Repeats=$Repeats
    CPU=Read-Hardware 'Win32_Processor' @('Name','NumberOfCores','NumberOfLogicalProcessors')
    GPU=Read-Hardware 'Win32_VideoController' @('Name','DriverVersion')
    Memory=Read-Hardware 'Win32_ComputerSystem' @('TotalPhysicalMemory')
    Note='Synthetic corpus, warm OS cache, sequential invocations; other host work and filesystem overhead are not isolated. No GPU kernels run.'
}
$envInfo | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $run 'environment.json')
function Invoke-Rar([string[]]$Arguments) {
    $info=[Diagnostics.ProcessStartInfo]::new($RarPath)
    $info.UseShellExecute=$false; $info.CreateNoWindow=$true
    $info.RedirectStandardOutput=$true; $info.RedirectStandardError=$true
    $info.WorkingDirectory=$corpus
    $info.Environment.Remove('RARINISWITCHES') | Out-Null
    foreach($arg in $Arguments) { $info.ArgumentList.Add($arg) }
    $p=[Diagnostics.Process]::new(); $p.StartInfo=$info
    $sw=[Diagnostics.Stopwatch]::StartNew(); $p.Start() | Out-Null
    $stdout=$p.StandardOutput.ReadToEndAsync(); $stderr=$p.StandardError.ReadToEndAsync()
    if(!$p.WaitForExit($TimeoutSeconds*1000)) { $p.Kill($true); throw "RAR timed out: $($Arguments -join ' ')" }
    $sw.Stop()
    $out=$stdout.GetAwaiter().GetResult(); $err=$stderr.GetAwaiter().GetResult()
    if($p.ExitCode -ne 0) { throw "RAR exit $($p.ExitCode): $out $err" }
    $result=[pscustomobject]@{Seconds=$sw.Elapsed.TotalSeconds; CPUSeconds=$p.TotalProcessorTime.TotalSeconds; PeakWorkingSet=$p.PeakWorkingSet64}
    $p.Dispose(); return $result
}
$cases = [Collections.Generic.List[object]]::new()
foreach($file in 'records.txt','repeating.bin','random.bin') {
    foreach($m in 1,3,5) { foreach($threads in 1,8) {
        $cases.Add([pscustomobject]@{Corpus=$file;Method=$m;Threads=$threads;Solid=$false;Source=$file;Extra=@()})
    } }
}
$cases.Add([pscustomobject]@{Corpus='random.bin';Method=0;Threads=1;Solid=$false;Source='random.bin';Extra=@()})
foreach($solid in $false,$true) {
    $cases.Add([pscustomobject]@{Corpus='similar';Method=3;Threads=8;Solid=$solid;Source='similar\*';Extra=@()})
}
$rows=[Collections.Generic.List[object]]::new()
for($repeat=1;$repeat -le $Repeats;$repeat++) {
    # Rotate order between repeats to reduce a fixed startup/thermal ordering bias.
    for($offset=0;$offset -lt $cases.Count;$offset++) {
        $case=$cases[($offset+($repeat-1)*7)%$cases.Count]
        $id="$($case.Corpus)-m$($case.Method)-mt$($case.Threads)-solid$($case.Solid)-r$repeat"
        $archive=Join-Path $archives "$id.rar"
        $solidSwitch=if($case.Solid){'-s'}else{'-s-'}
        $arguments=@('a','-cfg-','-idq','-y','-md32m',"-m$($case.Method)","-mt$($case.Threads)",$solidSwitch,'-ep1',$archive,$case.Source)
        $measurement=Invoke-Rar $arguments
        # Integrity testing is outside the timed creation interval.
        $null=Invoke-Rar @('t','-cfg-','-idq','-y',$archive)
        $bytes=(Get-Item -LiteralPath $archive).Length
        $rows.Add([pscustomobject]@{
            Corpus=$case.Corpus; Method=$case.Method;Threads=$case.Threads;Solid=$case.Solid;Repeat=$repeat
            InputBytes=$SizeMiB*1MB; ArchiveBytes=$bytes;Ratio=$bytes/($SizeMiB*1MB)
            Seconds=$measurement.Seconds;MiBPerSecond=$SizeMiB/$measurement.Seconds
            CPUSeconds=$measurement.CPUSeconds;PeakWorkingSetBytes=$measurement.PeakWorkingSet
            IntegrityTest='pass';Arguments=($arguments | ConvertTo-Json -Compress)
        })
        $rows | Export-Csv -NoTypeInformation -Path (Join-Path $run 'results.csv')
        Write-Host ("{0}: {1:N3}s, {2:N2} MiB/s, {3:N3}%" -f $id,$measurement.Seconds,($SizeMiB/$measurement.Seconds),(100*$bytes/($SizeMiB*1MB)))
    }
}
Write-Host "Results: $run"
