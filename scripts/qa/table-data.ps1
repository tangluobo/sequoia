param([Parameter(Mandatory=$true)][string]$Label,[string]$Executable,[ValidateSet('plain','selected','editing','row-menu','multi-row-menu','cell-menu','filter','sort')][string[]]$States=@('plain','selected'))
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
if(!$Executable){$Executable=Join-Path $workspace 'target/qa-active-200/debug/tomato.exe'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($probe)
$sha=(Get-FileHash -LiteralPath $Executable).Hash
$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $Executable -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $sha){throw 'GUI changed while copying'}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$captures=@()
$keys=@('TEMP','TMP','TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_TABLE_STATE','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','WGPU_BACKEND')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
try {
foreach($state in $States){
    $profile=Join-Path $workspace ('target/tomato-sql-workbench-fixture-'+[guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($profile)
    $env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP
    $env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_PREVIEW='table_data';$env:TOMATO_TABLE_STATE=$state
    $env:TOMATO_UI_CAPTURE=Join-Path $probe ($state+'.png');$env:TOMATO_UI_CAPTURE_FRAMES='1';$env:WGPU_BACKEND='vulkan'
    $env:TOMATO_UI_CAPTURE_DELAY_MS='5000'
    $process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($state+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($state+'.stderr.log'))
    try {
        for($attempt=0;$attempt -lt 6 -and !$process.HasExited;$attempt++){[void]$process.WaitForExit(10000)}
        if(!$process.HasExited){throw 'Own table viewport timed out'}
        if($process.ExitCode -ne 0){throw 'Own table viewport failed'}
    } finally {if(!$process.HasExited){$process.Kill()};$process.Dispose()}
    if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Missing framebuffer'}
    $metadata=Get-Content ([IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'state.json')) -Raw|ConvertFrom-Json
    if(!$metadata.ready -or $metadata.clientWidth -ne 896 -or $metadata.clientHeight -ne 650 -or $metadata.databaseRequested -or $metadata.systemClipboardWritten){throw 'Unexpected table viewport state'}
    $captures+=@{state=$state;path=$env:TOMATO_UI_CAPTURE;sha256=(Get-FileHash -LiteralPath $env:TOMATO_UI_CAPTURE).Hash}
}
if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared config changed'}
@{guiSha256=$sha;captures=$captures;savedHostsContacted=$false;sharedConfigurationUnchanged=$true;sharedSha256=$before;allStatesPixelIdentical=$false}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $probe 'validation.json')
Get-Content (Join-Path $probe 'validation.json')
} finally {foreach($key in $keys){if($null -eq $previous[$key]){[Environment]::SetEnvironmentVariable($key,[NullString]::Value,'Process')}else{[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}}
