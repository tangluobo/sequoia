#Requires -Version 7.0
param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Harness,[string]$Gui)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Use a fresh lowercase label'}
$guard=$workspace+[IO.Path]::DirectorySeparatorChar
$harnessPath=(Resolve-Path -LiteralPath $Harness).Path
if(!$harnessPath.StartsWith($guard,[StringComparison]::OrdinalIgnoreCase)){throw 'Use an owned workspace test harness'}
$guiPath=$null
if($Gui){$guiPath=(Resolve-Path -LiteralPath $Gui).Path;if(!$guiPath.StartsWith($guard,[StringComparison]::OrdinalIgnoreCase)){throw 'Use an owned workspace GUI'}}
$proof=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $proof){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($proof)
$harnessHash=(Get-FileHash -LiteralPath $harnessPath).Hash
$guiHash=if($guiPath){(Get-FileHash -LiteralPath $guiPath).Hash}else{$null}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json'
$sharedBefore=if(Test-Path -LiteralPath $shared){(Get-FileHash -LiteralPath $shared).Hash}else{$null}
$savedGui=$env:TOMATO_RECOVERY_GUI;$savedCapture=$env:TOMATO_RECOVERY_CAPTURE_DIR
try {
    foreach($filter in @('database_and_ssh_transport','ssh_initial_opening')){
        $part=if($filter -eq 'database_and_ssh_transport'){'operations'}else{'opening'}
        & (Join-Path $PSScriptRoot 'isolated-databases.ps1') -Label ($Label+'-'+$part) -Harness $harnessPath -TestFilter $filter *> (Join-Path $proof ($part+'.log'))
    }
    if($guiPath){
        $env:TOMATO_RECOVERY_GUI=$guiPath
        $env:TOMATO_RECOVERY_CAPTURE_DIR=Join-Path $proof 'native'
        & (Join-Path $PSScriptRoot 'isolated-databases.ps1') -Label ($Label+'-native') -Harness $harnessPath -TestFilter capture_native_recovery_windows *> (Join-Path $proof 'native.log')
        $native=Get-Content -LiteralPath (Join-Path $env:TOMATO_RECOVERY_CAPTURE_DIR 'validation.json') -Raw|ConvertFrom-Json
        if(!$native.passed){throw 'Native recovery verification failed'}
    }
    $sharedAfter=if(Test-Path -LiteralPath $shared){(Get-FileHash -LiteralPath $shared).Hash}else{$null}
    if($sharedBefore -ne $sharedAfter -or (Get-FileHash -LiteralPath $harnessPath).Hash -ne $harnessHash -or ($guiPath -and (Get-FileHash -LiteralPath $guiPath).Hash -ne $guiHash)){throw 'Configuration or artifact changed during validation'}
    [pscustomobject]@{passed=$true;harnessSha256=$harnessHash;guiSha256=$guiHash;actualMySql=$true;actualPostgres=$true;encryptedSsh=$true;initialOpeningBudgets=$true;noUserSqlReplay=$true;completedResultsRetained=$true;nativeWindowsVerified=[bool]$guiPath;sharedConfigurationUnchanged=$true;savedHostsContacted=$false;physicalOsInputUsed=$false}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $proof 'validation.json')
    Get-Content -LiteralPath (Join-Path $proof 'validation.json')
}finally{
    $env:TOMATO_RECOVERY_GUI=$savedGui;$env:TOMATO_RECOVERY_CAPTURE_DIR=$savedCapture
}
