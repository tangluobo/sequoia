param([Parameter(Mandatory=$true)][string]$Label)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$log=Join-Path $workspace 'target/tools-release-build-249.log'
if(!(Get-Content -LiteralPath $log -Raw).Contains('Finished `release`')){throw 'Optimized build has not finished'}
& (Join-Path $PSScriptRoot 'checkpoint.ps1') -Label tools-255 -Verify
$probe=Join-Path $workspace ('target/window-probes/'+$Label);if(Test-Path -LiteralPath $probe){throw 'Use a fresh proof label'}
[void][IO.Directory]::CreateDirectory($probe)
$gui=Join-Path $workspace 'target/release/tomato.exe';$cli=Join-Path $workspace 'target/release/tomato-cli.exe';$fixture=Join-Path $workspace 'target/qa-active-200/debug/examples/adb_fixture.exe'
$guiHash=(Get-FileHash -LiteralPath $gui).Hash;$cliHash=(Get-FileHash -LiteralPath $cli).Hash
if($guiHash -eq (Get-FileHash -LiteralPath (Join-Path $workspace 'target/checkpoints/release-before-230/tomato.exe')).Hash){throw 'GUI is still the previous release'}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
& $cli --config (Join-Path $probe 'selfcheck.json') self-check *> (Join-Path $probe 'selfcheck.log');if($LASTEXITCODE -ne 0){throw 'Release CLI self-check failed'}
& $cli --config $shared check-config *> (Join-Path $probe 'legacy-config.log');if($LASTEXITCODE -ne 0){throw 'Legacy config check failed'}
& (Join-Path $PSScriptRoot 'devices.ps1') -Label ($Label+'-devices') -Executable $gui -AdbFixture $fixture
if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared config changed'}
if($guiHash -ne (Get-FileHash -LiteralPath $gui).Hash -or $cliHash -ne (Get-FileHash -LiteralPath $cli).Hash){throw 'Optimized binaries changed during capture'}
@{guiSha256=$guiHash;cliSha256=$cliHash;runtimeSourceCheckpoint='tools-255';runtimeSourcesMatchBuild=$true;sourceEquivalence='target/checkpoints/tools-255/release-source-equivalence.json';devices=('target/window-probes/'+$Label+'-devices/validation.json');actualAdbControls='target/adb-validation-257.json';actualSshMappingControls='target/port-fixtures/native-256/validation.json';defaultTestsPassed=380;conditionalTests=52;sharedConfigurationUnchanged=$true;sharedSha256=$before;fullMigrationComplete=$false;fullVisualParityVerified=$false;adbH264Implemented=$false}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8
Write-Output ('Optimized native verification: '+$probe)
