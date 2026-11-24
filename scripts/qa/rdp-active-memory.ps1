param(
 [Parameter(Mandatory=$true)][string]$OldExecutable,
 [Parameter(Mandatory=$true)][string]$NewExecutable,
 [Parameter(Mandatory=$true)][string]$Harness,
 [Parameter(Mandatory=$true)][string]$Label,
 [ValidateSet('TAB','WINDOW')][string]$Mode='TAB',
 [ValidateSet('auto','vulkan')][string]$Backend='vulkan',
 [ValidateSet('sparse','full')][string]$Pattern='sparse',
 [ValidateSet('raw','remotefx','clearcodec','adaptive','avc420','progressive')][string]$OldCodec='raw',
 [ValidateSet('raw','remotefx','clearcodec','adaptive','avc420','progressive')][string]$NewCodec='raw'
)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
. (Join-Path $PSScriptRoot 'rdp-thread-times.ps1')
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
$harnessSource=[IO.Path]::GetFullPath($Harness)
$harnessRoots=@('target/debug/deps/','target/qa-active-200/debug/deps/') | ForEach-Object { [IO.Path]::GetFullPath((Join-Path $workspace $_)) }
if(!($harnessRoots | Where-Object { $harnessSource.StartsWith($_,[StringComparison]::OrdinalIgnoreCase) }) -or [IO.Path]::GetFileName($harnessSource) -notmatch '^remote_desktop-[a-f0-9]+\.exe$'){throw 'Expected own RDP harness'}
[void][IO.Directory]::CreateDirectory($probe)
$harnessCopy=Join-Path $probe 'rdp-fixture.exe';Copy-Item -LiteralPath $harnessSource -Destination $harnessCopy
$harnessHash=(Get-FileHash -LiteralPath $harnessCopy).Hash
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$keys=@('TOMATO_RDP_CAPTURE_EXE','TOMATO_RDP_CAPTURE_PATH','TOMATO_RDP_CAPTURE_MODE','TOMATO_RDP_CAPTURE_FULLSCREEN','TOMATO_RDP_CAPTURE_MOTION','TOMATO_RDP_CAPTURE_PATTERN','TOMATO_RDP_CAPTURE_CODEC','TOMATO_UI_CAPTURE_DELAY_MS','WGPU_BACKEND')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$copies=@{};$results=@()
try {
 foreach($spec in @(@('old',$OldExecutable),@('new',$NewExecutable))){
  $source=[IO.Path]::GetFullPath($spec[1]);if(!$source.StartsWith((Join-Path $workspace 'target/'),[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($source) -ne 'tomato.exe'){throw 'Expected workspace GUI'}
  $directory=Join-Path $probe $spec[0];[void][IO.Directory]::CreateDirectory($directory)
  $copy=Join-Path $directory 'tomato.exe';$hash=(Get-FileHash -LiteralPath $source).Hash;Copy-Item -LiteralPath $source -Destination $copy
  if((Get-FileHash -LiteralPath $copy).Hash -ne $hash){throw 'GUI changed during copy'}
  $copies[$spec[0]]=@{path=$copy;sha256=$hash}
 }
 foreach($run in @('old-1','new-1','new-2','old-2')){
  $version=$run.Split('-')[0];$capture=Join-Path $probe ($run+'.png')
  $env:TOMATO_RDP_CAPTURE_EXE=$copies[$version].path;$env:TOMATO_RDP_CAPTURE_PATH=$capture
  $env:TOMATO_RDP_CAPTURE_MODE=$Mode;$env:TOMATO_RDP_CAPTURE_FULLSCREEN='0';$env:TOMATO_RDP_CAPTURE_MOTION='1';$env:TOMATO_RDP_CAPTURE_PATTERN=$Pattern
  $env:TOMATO_RDP_CAPTURE_CODEC=if($version -eq 'old'){$OldCodec}else{$NewCodec}
  $env:TOMATO_UI_CAPTURE_DELAY_MS='10000';if($Backend -eq 'auto'){Remove-Item Env:WGPU_BACKEND -ErrorAction SilentlyContinue}else{$env:WGPU_BACKEND=$Backend}
  $fixture=$null;$application=$null
  try {
   $fixture=Start-Process -FilePath $harnessCopy -ArgumentList @('rdp_fixture::capture_native_rdp_window','--exact','--ignored','--nocapture') -WorkingDirectory $workspace -WindowStyle Hidden -RedirectStandardOutput (Join-Path $probe ($run+'.log')) -RedirectStandardError (Join-Path $probe ($run+'.fixture.stderr.log')) -PassThru
   $deadline=[DateTime]::UtcNow.AddSeconds(25);$pidFile=[IO.Path]::ChangeExtension($capture,'pid')
   while(!(Test-Path -LiteralPath $pidFile)){
    $fixture.Refresh();if($fixture.HasExited){throw 'Fixture exited before launching its GUI'}
    if([DateTime]::UtcNow -gt $deadline){throw 'Missing owned GUI PID'}
    Start-Sleep -Milliseconds 50
   }
   $application=[Diagnostics.Process]::GetProcessById([int](Get-Content -LiteralPath $pidFile -Raw))
   if(![string]::Equals($application.MainModule.FileName,$copies[$version].path,[StringComparison]::OrdinalIgnoreCase)){throw 'Owned GUI identity mismatch'}
   $readiness='launch only; first remote frame not proven'
   if($env:TOMATO_RDP_CAPTURE_CODEC -in @('clearcodec','adaptive','avc420','progressive')){
    $readyFile=[IO.Path]::ChangeExtension($capture,'ready');$readyDeadline=[DateTime]::UtcNow.AddSeconds(30)
    while(!(Test-Path -LiteralPath $readyFile)){
     $fixture.Refresh();$application.Refresh()
     if($fixture.HasExited -or $application.HasExited){throw 'Owned RDP process exited before its first frame acknowledgement'}
     if([DateTime]::UtcNow -gt $readyDeadline){throw 'No first-frame acknowledgement; active memory sample is invalid'}
     Start-Sleep -Milliseconds 50
    }
    if((Get-Content -LiteralPath $readyFile -Raw).Trim() -ne 'first_frame_ack'){throw 'Invalid owned readiness proof'}
    $readiness='first EGFX frame acknowledgement, then 2 seconds warmup'
   }
   Start-Sleep -Seconds 2
   $threadStart=Get-OwnedRdpThreadTimes $application $copies[$version].path
   $application.Refresh();$fixture.Refresh();$sourceCpu=$fixture.TotalProcessorTime.TotalSeconds;$cpu=$application.TotalProcessorTime.TotalSeconds;$watch=[Diagnostics.Stopwatch]::StartNew();$samples=@()
   while($watch.Elapsed.TotalSeconds -lt 4){
    $application.Refresh();if($application.HasExited){throw 'Owned GUI exited before sampling completed'}
    $samples+=@{workingSetMiB=$application.WorkingSet64/1MB;privateCommitMiB=$application.PrivateMemorySize64/1MB}
    Start-Sleep -Milliseconds 100
   }
   if($samples.Count -lt 20){throw 'Too few active memory samples'}
   $application.Refresh();$fixture.Refresh();$sourceCores=($fixture.TotalProcessorTime.TotalSeconds-$sourceCpu)/$watch.Elapsed.TotalSeconds;$cores=($application.TotalProcessorTime.TotalSeconds-$cpu)/$watch.Elapsed.TotalSeconds;$watch.Stop()
   $threadEnd=Get-OwnedRdpThreadTimes $application $copies[$version].path
   $threadCpu=@(foreach($threadId in $threadEnd.Keys){if($threadStart.ContainsKey($threadId) -and $threadStart[$threadId].CreatedTicks -eq $threadEnd[$threadId].CreatedTicks){
    @{id=$threadId;name=$threadEnd[$threadId].Name;averageCpuCores=($threadEnd[$threadId].CpuTicks-$threadStart[$threadId].CpuTicks)/10000000.0/$watch.Elapsed.TotalSeconds}
   }}) | Sort-Object averageCpuCores -Descending | Select-Object -First 12
   # The harness allows its GUI 30 seconds, then validates the framebuffer and
   # releases the peer. Let that bounded path report its own result before the
   # outer sampler cancels it; a 20-second outer wait could cut it off first.
   if(!$fixture.WaitForExit(45000)){throw 'Fixture did not finish'}
   if($fixture.ExitCode -ne 0 -or !(Test-Path -LiteralPath $capture)){throw 'RDP capture failed'}
   $results+=@{run=$run;version=$version;codec=$env:TOMATO_RDP_CAPTURE_CODEC;backend=$Backend;mode=$Mode;readiness=$readiness;guiSha256=$copies[$version].sha256;averageCpuCores=$cores;sourceCpuCores=$sourceCores;threadCpu=$threadCpu;threadCpuScope='Threads present in both samples; inaccessible threads skipped';sampleSeconds=$watch.Elapsed.TotalSeconds;workingSetMiB=($samples|Measure-Object workingSetMiB -Average).Average;privateCommitMiB=($samples|Measure-Object privateCommitMiB -Average).Average;samples=$samples;capture=$capture}
  } finally {
   # These are handles of the fixture and its identity-checked private GUI only.
   if($application){if(!$application.HasExited -and [string]::Equals($application.MainModule.FileName,$copies[$version].path,[StringComparison]::OrdinalIgnoreCase)){$application.Kill();[void]$application.WaitForExit(10000)};$application.Dispose()}
   if($fixture){if(!$fixture.HasExited){$fixture.Kill();[void]$fixture.WaitForExit(10000)};$fixture.Dispose()}
  }
 }
 $after=(Get-FileHash -LiteralPath $shared).Hash;if($before -ne $after){throw 'Shared configuration changed'}
 @{pattern=$Pattern;runs=$results;harnessSha256=$harnessHash;sharedConfigurationUnchanged=$true;sharedSha256=$after;savedHostsContacted=$false;scope=('Owned TLS/CredSSP 1080p '+$Pattern+' changes; EGFX (ClearCodec/adaptive/AVC420/Progressive) samples start 2 seconds after first frame acknowledgement; raw/RemoteFX only prove process launch; not peak memory or physical display FPS')}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $probe 'validation.json')
} finally {foreach($key in $keys){if($null -eq $previous[$key]){Remove-Item -LiteralPath ('Env:'+ $key) -ErrorAction SilentlyContinue}else{Set-Item -LiteralPath ('Env:'+ $key) -Value $previous[$key]}}}
