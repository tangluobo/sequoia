param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Executable,[Parameter(Mandatory=$true)][string]$AdbFixture,[string]$VideoFixture,[ValidateRange(1,8)][int]$CaptureFrames=4,[ValidateRange(700,10000)][int]$CaptureDelayMs=700,[ValidateSet('auto','vulkan','primary')][string]$Backend='vulkan',[switch]$AdbOnly,[switch]$StaticVideo)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'adb-peer.ps1')
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a new label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=(Resolve-Path -LiteralPath $Executable).Path
if(!$source.StartsWith((Join-Path $workspace 'target/'),[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($source) -ne 'tomato.exe'){throw 'Expected workspace GUI'}
$fixture=(Resolve-Path -LiteralPath $AdbFixture).Path
if(!$fixture.StartsWith((Join-Path $workspace 'target/'),[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($fixture) -ne 'adb_fixture.exe'){throw 'Expected owned adb_fixture example'}
$guiHash=(Get-FileHash -LiteralPath $source).Hash;$fixtureHash=(Get-FileHash -LiteralPath $fixture).Hash
$program=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $program
$fixtureProfile=Join-Path $probe ('tomato-device-fixture-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($fixtureProfile)
[IO.File]::WriteAllText((Join-Path $fixtureProfile 'connections.json'),'{"connections":[],"groups":[]}',[Text.UTF8Encoding]::new($false))
$ownedAdb=Join-Path $fixtureProfile 'adb-fixture'
if($VideoFixture){$video=(Resolve-Path -LiteralPath $VideoFixture).Path;if(!$video.StartsWith((Join-Path $workspace 'target/'),[StringComparison]::OrdinalIgnoreCase)){throw 'Expected owned video fixture'};Copy-Item -LiteralPath $video -Destination ([IO.Path]::ChangeExtension($ownedAdb,'video.scrcpy'))}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$legacyMappings=Join-Path $env:USERPROFILE '.tomata/port-mappings.json';$mappingBefore=if(Test-Path -LiteralPath $legacyMappings){(Get-FileHash -LiteralPath $legacyMappings).Hash}else{$null}
$keys=@('TOMATO_DATA_DIR','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_UI_PREVIEW','WGPU_BACKEND');$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$captures=@()
$peer=$null
try{
 $peer=Start-AdbTestPeer $fixture $ownedAdb
 $env:TOMATO_DATA_DIR=$fixtureProfile;$env:WGPU_BACKEND=switch($Backend){'auto'{$null};'primary'{'vulkan,dx12'};default{$Backend}}
 foreach($case in $(if($AdbOnly){@('adb')}else{@('port_mapping','port_mapping_edit','adb')})){
  $env:TOMATO_UI_PREVIEW=$case;$env:TOMATO_UI_CAPTURE=Join-Path $probe ($case+'.png');$env:TOMATO_UI_CAPTURE_FRAMES=if($case -eq 'adb'){[string]$CaptureFrames}else{'1'};$env:TOMATO_UI_CAPTURE_DELAY_MS=if($case -eq 'adb'){[string]$CaptureDelayMs}else{'700'}
  $process=Start-Process -FilePath $program -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($case+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($case+'.stderr.log'))
  $activeMemory=@();$elapsed=[Diagnostics.Stopwatch]::StartNew()
  try{
   while(!$process.WaitForExit(200)){
    if($elapsed.Elapsed.TotalSeconds -gt 45){throw ('Owned device capture timed out: '+$case)}
    if($case -eq 'adb' -and $elapsed.Elapsed.TotalSeconds -ge 2 -and !$process.HasExited){$process.Refresh();$activeMemory+=@{workingSetMiB=$process.WorkingSet64/1MB;privateMiB=$process.PrivateMemorySize64/1MB;cpuSeconds=$process.TotalProcessorTime.TotalSeconds;elapsedSeconds=$elapsed.Elapsed.TotalSeconds}}
   }
   if($process.ExitCode -ne 0){throw ('Owned GUI failed: '+$case)}
   if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Missing own framebuffer'}
  }finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
  if($case -eq 'adb' -and $activeMemory.Count -gt 0){@{samples=$activeMemory.Count;meanCpuCores=if($activeMemory.Count -gt 1){($activeMemory[-1].cpuSeconds-$activeMemory[0].cpuSeconds)/($activeMemory[-1].elapsedSeconds-$activeMemory[0].elapsedSeconds)}else{0};meanWorkingSetMiB=($activeMemory.workingSetMiB|Measure-Object -Average).Average;maxWorkingSetMiB=($activeMemory.workingSetMiB|Measure-Object -Maximum).Maximum;meanPrivateMiB=($activeMemory.privateMiB|Measure-Object -Average).Average;maxPrivateMiB=($activeMemory.privateMiB|Measure-Object -Maximum).Maximum;scope='Owned ADB stream and actual GUI, sampled after 2 seconds; not a whole-process peak bound'}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $probe 'adb.memory.json') -Encoding utf8}
  $last=if($case -eq 'adb' -and $CaptureFrames -gt 1){Join-Path $probe ('adb-{0:00}.png' -f ($CaptureFrames-1))}else{$env:TOMATO_UI_CAPTURE};if(!(Test-Path -LiteralPath $last)){throw 'Missing final framebuffer'}
  $captures+=@{case=$case;path=$last;sha256=(Get-FileHash -LiteralPath $last).Hash}
 }
 $commands=Get-Content -LiteralPath ([IO.Path]::ChangeExtension($ownedAdb,'commands.jsonl')) -Raw
 if(!$commands.Contains('connect') -or !$commands.Contains('app_process') -or !$commands.Contains('max_fps=60') -or !$commands.Contains('remove')){throw 'Native ADB session and scrcpy cleanup did not reach the loopback peer'}
 $sourceStatsPath=[IO.Path]::ChangeExtension($ownedAdb,'video-stats.json');$sourceMetrics=if(Test-Path -LiteralPath $sourceStatsPath){Get-Content -LiteralPath $sourceStatsPath -Raw|ConvertFrom-Json}else{$null}
 $videoMetrics=Get-Content -LiteralPath (Join-Path $probe 'adb.video.json') -Raw|ConvertFrom-Json;if($StaticVideo){if($videoMetrics.pixelFormat -ne 'GPU-I420'){throw 'Static frame did not reach the native GPU path'}}elseif($videoMetrics.textureUpdates -lt 2){throw 'Native H264 was not rendered'}
 $heartbeat=[IO.Path]::ChangeExtension($ownedAdb,'h264-heartbeat')
 $stopped=(Get-FileHash -LiteralPath $heartbeat).Hash
 Start-Sleep -Milliseconds 250
 if($stopped -ne (Get-FileHash -LiteralPath $heartbeat).Hash){throw 'ADB video channel survived closing the GUI'}
 if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared connections changed'}
 $mappingAfter=if(Test-Path -LiteralPath $legacyMappings){(Get-FileHash -LiteralPath $legacyMappings).Hash}else{$null};if($mappingBefore -ne $mappingAfter){throw 'Shared port mappings changed'}
 if($guiHash -ne (Get-FileHash -LiteralPath $program).Hash -or $guiHash -ne (Get-FileHash -LiteralPath $source).Hash){throw 'GUI changed during capture'}
 @{guiSha256=$guiHash;backend=$Backend;adbFixtureSha256=$fixtureHash;captures=$captures;sharedSha256=$before;sharedConfigurationUnchanged=$true;sharedMappingsUnchanged=$true;nativeAdbClosed=$true;savedHostsContacted=$false;fullVisualParityVerified=$false;adbH264Implemented=$true;staticQualityOnly=[bool]$StaticVideo;sourceMetrics=$sourceMetrics;videoMetrics=$videoMetrics}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8
}finally{
 foreach($key in $keys){if($null -eq $previous[$key]){[Environment]::SetEnvironmentVariable($key,[NullString]::Value,'Process')}else{[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}
 Stop-AdbTestPeer $peer
}
