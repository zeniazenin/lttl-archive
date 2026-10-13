#Requires -Version 7.0
[CmdletBinding()]
param([ValidateRange(1,5)][int]$Repeats=3,[switch]$PrepareOnly)
$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
$corpus=Join-Path $root 'bench-output/auto-corpus'
[IO.Directory]::CreateDirectory($corpus)|Out-Null
$sourceCorpus=Join-Path $root 'bench-output/gpu-review/20261012T212532860Z/corpus'
$publicCorpus=Join-Path $root 'bench-output/downloaded-samples/corpus'
foreach($name in @('independent-noise','precompressed-zip','similar-files','raw-sensors','structured-csv')) {
    if(!(Test-Path -LiteralPath (Join-Path $corpus $name))) { Copy-Item -LiteralPath (Join-Path $sourceCorpus $name) -Destination $corpus -Recurse }
}
foreach($name in @('canterbury','enwik8')) {
    if(!(Test-Path -LiteralPath (Join-Path $corpus $name))) { Copy-Item -LiteralPath (Join-Path $publicCorpus $name) -Destination $corpus -Recurse }
}
# Two independent fixtures, not used to set the initial policy thresholds.
# Names are deliberately unhelpful; selection must use bytes, not extensions.
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Text;
public static class AutoFixtures {
 public static void Generate(string root) {
  string json=Path.Combine(root,"heldout-json"); Directory.CreateDirectory(json);
  string p=Path.Combine(json,"events.jpg");
  if(!File.Exists(p)) using(var f=File.Create(p)) {
   long i=0; var random=new Random(817326);
   while(f.Length<32L*1024*1024) {
    byte[] row=Encoding.UTF8.GetBytes("{\"id\":"+(i++)+",\"host\":\"worker-"+(i%37)+"\",\"latency\":"+random.Next(1,10000)+",\"state\":\"accepted\",\"bucket\":"+random.Next(0,1000)+"}\n");
    f.Write(row,0,(int)Math.Min(row.Length,32L*1024*1024-f.Length));
   }
  }
  string mixed=Path.Combine(root,"heldout-mixed"); Directory.CreateDirectory(mixed);
  var rng=new Random(91734); byte[] b=new byte[1024*1024];
  string noise=Path.Combine(mixed,"random.csv");
  if(!File.Exists(noise)) using(var f=File.Create(noise)) { for(int i=0;i<16;i++){rng.NextBytes(b);f.Write(b,0,b.Length);} }
  string text=Path.Combine(mixed,"manual.bin");
  if(!File.Exists(text)) using(var f=File.Create(text)) {
   long i=0;
   while(f.Length<16L*1024*1024) {
    byte[] row=Encoding.UTF8.GetBytes("Section "+(i++)+": This is a reproducible manual explaining ordinary storage and extraction. Reference="+rng.Next(100000)+"\n");
    f.Write(row,0,(int)Math.Min(row.Length,16L*1024*1024-f.Length));
   }
  }
 }
}
'@
[AutoFixtures]::Generate($corpus)
$datasets=@(Get-ChildItem -LiteralPath $corpus -Directory|Sort-Object Name|ForEach-Object {
    $directory=$_; $total=0L
    $files=@(Get-ChildItem -LiteralPath $directory.FullName -File -Recurse|Sort-Object FullName|ForEach-Object {
        $total+=$_.Length
        [pscustomobject]@{Path=[IO.Path]::GetRelativePath($corpus,$_.FullName);Bytes=$_.Length;SHA256=(Get-FileHash -LiteralPath $_.FullName).Hash}
    })
    [pscustomobject]@{Name=$directory.Name;InputBytes=$total;Files=$files}
})
[ordered]@{GeneratorVersion='auto-policy-v1';Note='Five retained synthetic datasets, two public corpora, and two new deterministic fixtures. Mixed content and misleading extensions test the content selector. Each Auto call includes sampling and trials in wall time. Outcomes are estimates, not oracle choices.';Datasets=$datasets}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $corpus 'manifest.json')
if($PrepareOnly){Write-Output "Prepared $corpus"; return}
$env:TEMP=Join-Path $root '.qa/auto-benchmark-tmp'
[IO.Directory]::CreateDirectory($env:TEMP)|Out-Null
$env:TMP=$env:TEMP
& (Join-Path $PSScriptRoot 'benchmark-gpu.ps1') -CorpusDirectory $corpus -Backends cpu,gpu,auto -Presets fast,balanced,small -Repeats $Repeats -SkipChunkSweep -TimeExtraction -MaximumMinutes 20
