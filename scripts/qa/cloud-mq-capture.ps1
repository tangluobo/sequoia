param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Executable,[ValidateSet('cloud_dns','cloud_certificate','kafka_topics','kafka_send','rocketmq_topics','rocketmq_send')][string[]]$Cases=@('cloud_dns','cloud_certificate','kafka_topics','kafka_send','rocketmq_topics','rocketmq_send'))
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'runtime-harness.ps1')
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=(Resolve-Path -LiteralPath $Executable).Path
if(!$source.StartsWith((Join-Path $workspace 'target/'),[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($source) -ne 'tomato.exe'){throw 'Expected workspace GUI'}
$hash=(Get-FileHash -LiteralPath $source).Hash
$program=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $program
$fixtureProfile=Join-Path $probe ('tomato-cloud-mq-fixture-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($fixtureProfile)
$serverProfile=Join-Path $probe ('tomato-cloud-fixture-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($serverProfile)
[IO.File]::WriteAllText((Join-Path $fixtureProfile 'connections.json'),'{"connections":[],"groups":[]}',[Text.UTF8Encoding]::new($false))
$keys=@('TOMATO_DATA_DIR','TOMATO_CLOUD_FIXTURE_DIR','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_PREVIEW','WGPU_BACKEND');$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$server=Get-CurrentHarness -Pattern 'cloud-*.exe'
$ownedServer=Join-Path $probe "cloud-harness.exe";Copy-Item -LiteralPath $server.FullName -Destination $ownedServer
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$hostProcess=$null;$captures=@()
try{
 $env:TOMATO_CLOUD_FIXTURE_DIR=$serverProfile
 $hostProcess=Start-Process -FilePath $ownedServer -ArgumentList @('--ignored','--exact','host_cloud_fixture','--nocapture') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe 'server.stdout.log') -RedirectStandardError (Join-Path $probe 'server.stderr.log')
 $deadline=[DateTime]::UtcNow.AddSeconds(20)
 while(!(Test-Path -LiteralPath (Join-Path $serverProfile 'ready.json'))){if($hostProcess.HasExited -or [DateTime]::UtcNow -gt $deadline){throw 'Fixture not ready'};Start-Sleep -Milliseconds 100}
 $ready=Get-Content -LiteralPath (Join-Path $serverProfile 'ready.json') -Raw|ConvertFrom-Json
 $ready|Add-Member -NotePropertyName kafka -NotePropertyValue $env:TOMATO_KAFKA_FIXTURE
 $ready|Add-Member -NotePropertyName rocketmq -NotePropertyValue $env:TOMATO_ROCKET_FIXTURE
 $ready|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $fixtureProfile 'ready.json') -Encoding utf8NoBOM
 $env:TOMATO_DATA_DIR=$fixtureProfile;$env:WGPU_BACKEND='vulkan';$env:TOMATO_UI_CAPTURE_FRAMES='4'
 foreach($case in $Cases){
  $env:TOMATO_UI_PREVIEW=$case;$env:TOMATO_UI_CAPTURE=Join-Path $probe ($case+'.png')
  $process=Start-Process -FilePath $program -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($case+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($case+'.stderr.log'))
  try{if(!$process.WaitForExit(45000)){throw ('Owned capture timeout: '+$case)};if($process.ExitCode -ne 0){throw ('Owned GUI failed: '+$case)}}finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
  $last=Join-Path $probe ($case+'-03.png');if(!(Test-Path -LiteralPath $last)){throw ('Missing own framebuffer: '+$case)}
  $captures+=@{case=$case;path=$last;sha256=(Get-FileHash -LiteralPath $last).Hash}
 }
 if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared config changed'}
 if($hash -ne (Get-FileHash -LiteralPath $source).Hash){throw 'Build changed during captures'}
 @{guiSha256=$hash;captures=$captures;sharedConfigurationUnchanged=$true;sharedSha256=$before;savedHostsContacted=$false;fullVisualParityVerified=$false}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $probe 'validation.json')
}finally{
 [IO.File]::WriteAllText((Join-Path $serverProfile 'stop'),'stop')
 if($hostProcess){if(!$hostProcess.HasExited -and !$hostProcess.WaitForExit(3000)){$hostProcess.Kill()};$hostProcess.Dispose()}
 foreach($key in $keys){if($null -eq $previous[$key]){[Environment]::SetEnvironmentVariable($key,[NullString]::Value,'Process')}else{[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}
}
