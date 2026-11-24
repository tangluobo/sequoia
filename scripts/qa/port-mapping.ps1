param([Parameter(Mandatory=$true)][string]$Label)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/port-fixtures/'+$Label);if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($probe)
$profile=Join-Path $probe ('tomato-port-fixture-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($profile)
$server=Get-ChildItem -LiteralPath (Join-Path $workspace 'target/debug/deps') -Filter 'port_mapping-*.exe'|Sort-Object LastWriteTime -Descending|Select-Object -First 1
$ui=Get-ChildItem -LiteralPath (Join-Path $workspace 'target/debug/deps') -Filter 'tomato_ui_egui-*.exe'|Sort-Object LastWriteTime -Descending|Select-Object -First 1
if(!$server -or !$ui){throw 'Missing owned fixture harnesses'}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$previous=$env:TOMATO_PORT_FIXTURE_DIR;$env:TOMATO_PORT_FIXTURE_DIR=$profile
$hostProcess=$null;$uiProcess=$null
try{
 $hostProcess=Start-Process -FilePath $server.FullName -ArgumentList @('--ignored','--exact','host_port_mapping_fixture','--nocapture') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe 'server.stdout.log') -RedirectStandardError (Join-Path $probe 'server.stderr.log')
 $deadline=[DateTime]::UtcNow.AddSeconds(20)
 while(!(Test-Path -LiteralPath (Join-Path $profile 'ready.json'))){if($hostProcess.HasExited){throw 'Owned SSH fixture exited before readiness'};if([DateTime]::UtcNow -gt $deadline){throw 'Owned SSH fixture readiness timeout'};Start-Sleep -Milliseconds 100}
 $uiProcess=Start-Process -FilePath $ui.FullName -ArgumentList @('--ignored','--exact','app::tests::tool_migration::actual_port_mapping_native_controls_create_transfer_stop_both_ssh_directions','--nocapture') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe 'ui.stdout.log') -RedirectStandardError (Join-Path $probe 'ui.stderr.log')
 if(!$uiProcess.WaitForExit(60000)){throw 'Native port mapping test timed out'};if($uiProcess.ExitCode -ne 0){throw 'Native SSH mapping controls failed'}
 [IO.File]::WriteAllText((Join-Path $profile 'stop'),'stop');if(!$hostProcess.WaitForExit(10000)){throw 'Owned SSH fixture did not stop'};if($hostProcess.ExitCode -ne 0){throw 'Owned SSH fixture failed'}
 if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Legacy user configuration changed'}
 @{serverHarness=$server.FullName;serverSha256=(Get-FileHash -LiteralPath $server.FullName).Hash;uiHarness=$ui.FullName;uiSha256=(Get-FileHash -LiteralPath $ui.FullName).Hash;actualNativeControls=$true;localForwardingPayload=$true;remoteForwardingPayload=$true;stopReleasesListeners=$true;nativeSaveAndDelete=$true;ownedServersClosed=$true;sharedConfigurationUnchanged=$true;sharedSha256=$before;savedHostsContacted=$false}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8
 Get-Content -LiteralPath (Join-Path $probe 'ui.stdout.log') -Tail 7
}finally{
 [IO.File]::WriteAllText((Join-Path $profile 'stop'),'stop')
 if($uiProcess){if(!$uiProcess.HasExited){$uiProcess.Kill()};$uiProcess.Dispose()}
 if($hostProcess){if(!$hostProcess.HasExited -and !$hostProcess.WaitForExit(3000)){$hostProcess.Kill()};$hostProcess.Dispose()}
 $env:TOMATO_PORT_FIXTURE_DIR=$previous
}
