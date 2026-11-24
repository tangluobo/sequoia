param(
    [Parameter(Mandatory=$true)][string]$Label,
    [Parameter(Mandatory=$true)][string]$RdpHarness,
    [string]$Checkpoint='backup-213'
)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$' -or $Checkpoint -notmatch '^[a-z0-9-]+$'){throw 'Invalid verification label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh verification label'}
foreach($kind in @('build','clippy')){
    $log=Join-Path $workspace ('target/backup-release-'+$kind+'-213.log')
    if(!(Select-String -LiteralPath $log -Pattern 'Finished.*release.*target')){throw 'The final optimized build and strict Clippy must pass first'}
}
$frozen=Get-Content -LiteralPath (Join-Path $workspace ('target/checkpoints/'+$Checkpoint+'/production-sources.json')) -Raw|ConvertFrom-Json
foreach($entry in $frozen){if((Get-FileHash -LiteralPath (Join-Path $workspace $entry.path)).Hash -ne $entry.sha256){throw ('Source changed after freeze: '+$entry.path)}}
[void][IO.Directory]::CreateDirectory($probe)
$gui=Join-Path $workspace 'target/release/tomato.exe'
$cli=Join-Path $workspace 'target/release/tomato-cli.exe'
$guiHash=(Get-FileHash -LiteralPath $gui).Hash
$cliHash=(Get-FileHash -LiteralPath $cli).Hash
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json'
$sharedHash=(Get-FileHash -LiteralPath $shared).Hash
& $cli self-check *> (Join-Path $probe 'selfcheck.log')
if($LASTEXITCODE -ne 0){throw 'Final CLI self-check failed'}
& $cli check-config *> (Join-Path $probe 'config.log')
if($LASTEXITCODE -ne 0){throw 'Final legacy configuration check failed'}
$windows=@()
foreach($tab in 0..3){
    $windowLabel=$Label+'-backup-'+$tab
    & (Join-Path $PSScriptRoot 'backup-window.ps1') -Label $windowLabel -Tab $tab -Executable $gui *> (Join-Path $probe ('backup-'+$tab+'.log'))
    $windows+=Join-Path $workspace ('target/validation-visual-'+$windowLabel+'.json')
}
foreach($tab in 0..1){
    $windowLabel=$Label+'-restore-'+$tab
    & (Join-Path $PSScriptRoot 'backup-window.ps1') -Label $windowLabel -Restore -Tab $tab -Executable $gui *> (Join-Path $probe ('restore-'+$tab+'.log'))
    $windows+=Join-Path $workspace ('target/validation-visual-'+$windowLabel+'.json')
}
$windowLabel=$Label+'-restore-confirm'
& (Join-Path $PSScriptRoot 'backup-window.ps1') -Label $windowLabel -Question -Executable $gui *> (Join-Path $probe 'restore-confirm.log')
$windows+=Join-Path $workspace ('target/validation-visual-'+$windowLabel+'.json')
& (Join-Path $PSScriptRoot 'rdp.ps1') -Label ($Label+'-rdp') -Harness $RdpHarness -Executable $gui *> (Join-Path $probe 'rdp.log')
& (Join-Path $PSScriptRoot 'paired-memory.ps1') -OldExecutable (Join-Path $workspace 'target/checkpoints/release-before-200/tomato.exe') -NewExecutable $gui -Label ($Label+'-memory') *> (Join-Path $probe 'memory.log')
foreach($entry in $frozen){if((Get-FileHash -LiteralPath (Join-Path $workspace $entry.path)).Hash -ne $entry.sha256){throw ('Source changed during verification: '+$entry.path)}}
if((Get-FileHash -LiteralPath $gui).Hash -ne $guiHash -or (Get-FileHash -LiteralPath $cli).Hash -ne $cliHash){throw 'Final executables changed during verification'}
if((Get-FileHash -LiteralPath $shared).Hash -ne $sharedHash){throw 'Shared configuration changed during verification'}
[ordered]@{
    label=$Label;guiSha256=$guiHash;cliSha256=$cliHash;frozenProductionFilesVerified=$frozen.Count
    backupWindows=$windows;databaseVerification='target/database-fixtures/probes/pg-backup-213/validation.json'
    rdp=(Join-Path $workspace ('target/window-probes/'+$Label+'-rdp/validation.json'))
    memory=(Join-Path $workspace ('target/window-probes/'+$Label+'-memory/validation.json'))
    baselineSourceRevisionKnown=$true;baselineCheckpoint=199
    sharedSha256=$sharedHash;sharedConfigurationUnchanged=$true;savedHostsContacted=$false
    memoryWorkload='Idle native windows with separate empty profiles; active-session memory not measured'
    fullMigrationComplete=$false;allVisualStatesVerified=$false
}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8
Get-Content -LiteralPath (Join-Path $probe 'validation.json')
