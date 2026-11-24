param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$RdpHarness)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid verification label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
foreach($kind in @('build','clippy')){
 $log=Join-Path $workspace ('target/tools-release-'+$kind+'-225.log')
 if(!(Select-String -LiteralPath $log -Pattern 'Finished.*release.*target')){throw 'Final optimized build and strict Clippy must pass first'}
}
& (Join-Path $PSScriptRoot 'checkpoint.ps1') -Label tools-225 -Verify
[void][IO.Directory]::CreateDirectory($probe)
$gui=Join-Path $workspace 'target/release/tomato.exe';$cli=Join-Path $workspace 'target/release/tomato-cli.exe'
$guiHash=(Get-FileHash -LiteralPath $gui).Hash;$cliHash=(Get-FileHash -LiteralPath $cli).Hash
if($guiHash -eq '71B71B201ED9E245C235EC8333E1A389D87DB194F9FD054F67410D96EC116F5C'){throw 'Old checkpoint 213 GUI is still present'}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$sharedHash=(Get-FileHash -LiteralPath $shared).Hash
& $cli self-check *> (Join-Path $probe 'selfcheck.log');if($LASTEXITCODE -ne 0){throw 'Final CLI self-check failed'}
& $cli --config $shared check-config *> (Join-Path $probe 'config.log');if($LASTEXITCODE -ne 0){throw 'Final legacy read-only check failed'}
& (Join-Path $PSScriptRoot 'tools.ps1') -Label ($Label+'-tools') -Executable $gui
& (Join-Path $PSScriptRoot 'rdp.ps1') -Label ($Label+'-rdp') -Harness $RdpHarness -Executable $gui
& (Join-Path $PSScriptRoot 'ssh-native.ps1') -Label ($Label+'-ssh') -Executable $gui
$windows=@()
foreach($tab in 0..3){
 $name=$Label+'-backup-'+$tab;& (Join-Path $PSScriptRoot 'backup-window.ps1') -Label $name -Tab $tab -Executable $gui;$windows+=$name
}
foreach($tab in 0..1){
 $name=$Label+'-restore-'+$tab;& (Join-Path $PSScriptRoot 'backup-window.ps1') -Label $name -Restore -Tab $tab -Executable $gui;$windows+=$name
}
$name=$Label+'-question';& (Join-Path $PSScriptRoot 'backup-window.ps1') -Label $name -Question -Executable $gui;$windows+=$name
foreach($kind in @('complete','error')){
 $name=$Label+'-alert-'+$kind;& (Join-Path $PSScriptRoot 'backup-window.ps1') -Label $name -Alert $kind -Executable $gui;$windows+=$name
}
if($guiHash -ne (Get-FileHash -LiteralPath $gui).Hash -or $cliHash -ne (Get-FileHash -LiteralPath $cli).Hash){throw 'Optimized artifact changed during verification'}
if($sharedHash -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared configuration changed'}
& (Join-Path $PSScriptRoot 'checkpoint.ps1') -Label tools-225 -Verify
@{productionCheckpoint='tools-225';guiSha256=$guiHash;cliSha256=$cliHash;nativeTools=($Label+'-tools');nativeRdp=($Label+'-rdp');nativeSsh=($Label+'-ssh');backupWindows=$windows;sharedConfigurationUnchanged=$true;sharedSha256=$sharedHash;savedHostsContacted=$false;fullMigrationComplete=$false;fullApplicationVisualParityVerified=$false}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8
