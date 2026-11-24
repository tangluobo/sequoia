param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$FixtureLabel)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'runtime-harness.ps1')
if($Label -notmatch '^[a-z0-9-]+$' -or $FixtureLabel -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$probe=Join-Path $workspace ('target/mq-ui/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($probe)
$fixture=Get-Content -LiteralPath (Join-Path $workspace ('target/mq-fixtures/'+$FixtureLabel+'/identity.json')) -Raw|ConvertFrom-Json
$ui=Get-CurrentHarness -Pattern 'tomato_ui_egui-*.exe'
$ownedUi=Join-Path $probe 'ui-harness.exe';Copy-Item -LiteralPath $ui.FullName -Destination $ownedUi
$keys=@('TOMATO_KAFKA_FIXTURE','TOMATO_ROCKET_FIXTURE','TOMATO_ROCKET_BROKER_FIXTURE');$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash;$process=$null
try{
 $env:TOMATO_KAFKA_FIXTURE=$fixture.kafka;$env:TOMATO_ROCKET_FIXTURE=$fixture.rocketmq;$env:TOMATO_ROCKET_BROKER_FIXTURE=$fixture.broker
 $process=Start-Process -FilePath $ownedUi -ArgumentList @('--ignored','actual_mq_tree_create_send_query_and_delete_controls','--nocapture') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe 'ui.stdout.log') -RedirectStandardError (Join-Path $probe 'ui.stderr.log')
 if(!$process.WaitForExit(120000)){throw 'Owned native MQ workflow timeout'}
 if($process.ExitCode -ne 0){throw ('Native MQ workflow failed: '+(Get-Content -LiteralPath (Join-Path $probe 'ui.stderr.log') -Raw))}
 if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared config changed'}
 @{uiSha256=(Get-FileHash -LiteralPath $ownedUi).Hash;fixtureIdentity=('target/mq-fixtures/'+$FixtureLabel+'/identity.json');actualNativeControls=$true;sharedConfigurationUnchanged=$true;sharedSha256=$before;savedHostsContacted=$false}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $probe 'validation.json')
 Get-Content -LiteralPath (Join-Path $probe 'ui.stdout.log') -Tail 8
}finally{
 if($process){if(!$process.HasExited){$process.Kill()};$process.Dispose()}
 foreach($key in $keys){if($null -eq $previous[$key]){[Environment]::SetEnvironmentVariable($key,[NullString]::Value,'Process')}else{[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}
}
