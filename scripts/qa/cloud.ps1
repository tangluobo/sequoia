param([Parameter(Mandatory=$true)][string]$Label)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'runtime-harness.ps1')
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$probe=Join-Path $workspace ('target/cloud-ui/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($probe)
$fixtureProfile=Join-Path $probe ('tomato-cloud-fixture-'+[guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($fixtureProfile)
$server=Get-CurrentHarness -Pattern 'cloud-*.exe'
$ui=Get-CurrentHarness -Pattern 'tomato_ui_egui-*.exe'
if(!$server -or !$ui){throw 'Build current workspace test harnesses first'}
$ownedUi=Join-Path $probe "ui-harness.exe";Copy-Item -LiteralPath $ui.FullName -Destination $ownedUi
$ownedServer=Join-Path $probe "cloud-harness.exe";Copy-Item -LiteralPath $server.FullName -Destination $ownedServer
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$previous=$env:TOMATO_CLOUD_FIXTURE_DIR;$env:TOMATO_CLOUD_FIXTURE_DIR=$fixtureProfile
$hostProcess=$null;$uiProcess=$null
try{
 $hostProcess=Start-Process -FilePath $ownedServer -ArgumentList @('--ignored','--exact','host_cloud_fixture','--nocapture') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe 'server.stdout.log') -RedirectStandardError (Join-Path $probe 'server.stderr.log')
 $deadline=[DateTime]::UtcNow.AddSeconds(20)
 while(!(Test-Path -LiteralPath (Join-Path $fixtureProfile 'ready.json'))){if($hostProcess.HasExited){throw 'Owned cloud fixture exited before readiness'};if([DateTime]::UtcNow -gt $deadline){throw 'Owned fixture readiness timeout'};Start-Sleep -Milliseconds 100}
 $uiProcess=Start-Process -FilePath $ownedUi -ArgumentList @('--ignored','actual_cloud_tree_dns_dialog_ddns_certificate_and_delete','--nocapture') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe 'ui.stdout.log') -RedirectStandardError (Join-Path $probe 'ui.stderr.log')
 if(!$uiProcess.WaitForExit(90000)){throw 'Native cloud workflow exceeded 90 seconds'}
 if($uiProcess.ExitCode -ne 0){throw ('Native cloud workflow failed: '+(Get-Content -LiteralPath (Join-Path $probe 'ui.stderr.log') -Raw))}
 if(!(Test-Path -LiteralPath (Join-Path $fixtureProfile 'ui-cloud-passed'))){throw 'Native completion proof missing'}
 if((Get-FileHash -LiteralPath $shared).Hash -ne $before){throw 'Shared configuration changed'}
 @{server=$server.FullName;serverSha256=(Get-FileHash -LiteralPath $server.FullName).Hash;ui=$ui.FullName;uiSha256=(Get-FileHash -LiteralPath $ui.FullName).Hash;actualNativeControls=$true;sharedConfigurationUnchanged=$true;sharedSha256=$before;savedHostsContacted=$false}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $probe 'validation.json')
 Get-Content -LiteralPath (Join-Path $probe 'ui.stdout.log') -Tail 7
}finally{
 [IO.File]::WriteAllText((Join-Path $fixtureProfile 'stop'),'stop')
 if($uiProcess){if(!$uiProcess.HasExited){$uiProcess.Kill()};$uiProcess.Dispose()}
 if($hostProcess){if(!$hostProcess.HasExited -and !$hostProcess.WaitForExit(3000)){$hostProcess.Kill()};$hostProcess.Dispose()}
 $env:TOMATO_CLOUD_FIXTURE_DIR=$previous
}
