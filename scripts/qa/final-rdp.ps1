param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Harness)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
$build=Join-Path $workspace 'target/rdp-release-build-198.log'
$clippy=Join-Path $workspace 'target/rdp-release-clippy-199.log'
if(!(Select-String -LiteralPath $build -Pattern 'Finished.*release.*target') -or !(Select-String -LiteralPath $clippy -Pattern 'Finished.*release.*target')){throw 'Current release build and strict Clippy must finish first'}
$frozen=Get-Content -LiteralPath (Join-Path $workspace 'target/checkpoints/rdp-199/production-sources.json') -Raw|ConvertFrom-Json
foreach($entry in $frozen){if((Get-FileHash -LiteralPath (Join-Path $workspace $entry.path)).Hash -ne $entry.sha256){throw ('Production source changed after freeze: '+$entry.path)}}
[void][IO.Directory]::CreateDirectory($probe)
$source=Join-Path $workspace 'target/release/tomato.exe'
$cli=Join-Path $workspace 'target/release/tomato-cli.exe'
$hash=(Get-FileHash -LiteralPath $source).Hash
$cliHash=(Get-FileHash -LiteralPath $cli).Hash
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json'
$before=(Get-FileHash -LiteralPath $shared).Hash
& $cli self-check *> (Join-Path $probe 'selfcheck.log')
if($LASTEXITCODE -ne 0){throw 'Release CLI self-check failed'}
& $cli check-config *> (Join-Path $probe 'config.log')
if($LASTEXITCODE -ne 0){throw 'Release legacy compatibility check failed'}
& (Join-Path $PSScriptRoot 'rdp.ps1') -Label ($Label+'-rdp') -Harness $Harness -Executable $source
& (Join-Path $PSScriptRoot 'paired-memory.ps1') -OldExecutable (Join-Path $workspace 'target/checkpoints/release-before-170/tomato.exe') -NewExecutable $source -Label ($Label+'-memory')
if((Get-FileHash -LiteralPath $source).Hash -ne $hash -or (Get-FileHash -LiteralPath $cli).Hash -ne $cliHash){throw 'Release changed during verification'}
if((Get-FileHash -LiteralPath $shared).Hash -ne $before){throw 'Shared configuration changed'}
@{frozenProductionFilesVerified=$frozen.Count;guiSha256=$hash;cliSha256=$cliHash;sharedSha256=$before;sharedConfigurationUnchanged=$true;savedHostsContacted=$false;native=(Join-Path $workspace ('target/window-probes/'+$Label+'-rdp/validation.json'));memory=(Join-Path $workspace ('target/window-probes/'+$Label+'-memory/validation.json'));baselineSourceRevisionKnown=$false;memoryWorkload='Idle native window, own empty profiles; active RDP memory not measured';fullMigrationComplete=$false}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8
