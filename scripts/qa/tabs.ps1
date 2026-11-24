param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Executable,[ValidateSet('plain','selected','overflow','popup')][string[]]$States=@('plain','selected','overflow','popup'),[ValidateRange(700,10000)][int]$CaptureDelayMs=5000)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=(Resolve-Path -LiteralPath $Executable).Path
if(!$source.StartsWith((Join-Path $workspace 'target/'),[StringComparison]::OrdinalIgnoreCase)){throw 'Expected own build'}
$sha=(Get-FileHash -LiteralPath $source).Hash;$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $sha){throw 'GUI changed during copy'}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$keys=@('TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_TAB_STATE','WGPU_BACKEND')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
try {
 foreach($state in $States){
  $tabProfileDirectory=Join-Path $probe ('tomato-tabs-fixture-'+$state);[void][IO.Directory]::CreateDirectory($tabProfileDirectory)
  $env:TOMATO_DATA_DIR=$tabProfileDirectory;$env:TOMATO_UI_PREVIEW='tab_strip';$env:TOMATO_UI_CAPTURE=Join-Path $probe ($state+'.png');$env:TOMATO_UI_CAPTURE_FRAMES='1';$env:TOMATO_TAB_STATE=$state;$env:WGPU_BACKEND='vulkan'
  $env:TOMATO_UI_CAPTURE_DELAY_MS=[string]$CaptureDelayMs
  $process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($state+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($state+'.stderr.log'))
  try {if(!$process.WaitForExit(30000)){throw 'Owned tab capture timed out'};if($process.ExitCode -ne 0 -or !(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Owned tab capture failed'}}finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
 }
 if((Get-FileHash -LiteralPath $shared).Hash -ne $before){throw 'Shared configuration changed'}
 @{guiSha256=$sha;sharedSha256=$before;sharedConfigurationUnchanged=$true;savedHostsContacted=$false;isolatedInertTabs=$true}|ConvertTo-Json|Set-Content (Join-Path $probe 'validation.json')
} finally {foreach($key in $keys){if($null -eq $previous[$key]){[Environment]::SetEnvironmentVariable($key,[NullString]::Value,'Process')}else{[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}}
