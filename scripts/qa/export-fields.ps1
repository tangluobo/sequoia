param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=[IO.Path]::GetFullPath($Executable);$hash=(Get-FileHash -LiteralPath $source).Hash
$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $hash){throw 'Snapshot differs'}
$config=Join-Path ([Environment]::GetFolderPath('UserProfile')) '.tomato/connections.json'
$before=if(Test-Path -LiteralPath $config){(Get-FileHash -LiteralPath $config).Hash}else{$null}
$keys=@('TEMP','TMP','TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_EXPORT_STAGE','TOMATO_EXPORT_FORMAT','TOMATO_EXPORT_FIELD_COUNT','WGPU_BACKEND')
$saved=@{};foreach($key in $keys){$saved[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$captures=@()
try {
 foreach($case in @(@('fields',4,'txt'),@('fields',256,'txt'),@('fields-bottom',256,'txt'),@('fields',40,'dbf'),@('fields-bottom',40,'dbf'))){
  $stage=$case[0];$count=$case[1];$format=$case[2];$name=$stage+'-'+$count+'-'+$format
  $profile=Join-Path $probe ('tomato-export-fixture-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($profile)
  $env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP;$env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_PREVIEW='data_export'
  $env:TOMATO_EXPORT_STAGE=$stage;$env:TOMATO_EXPORT_FORMAT=$format;$env:TOMATO_EXPORT_FIELD_COUNT=[string]$count
  $env:TOMATO_UI_CAPTURE=Join-Path $probe ($name+'.png');$env:TOMATO_UI_CAPTURE_FRAMES='1';$env:TOMATO_UI_CAPTURE_DELAY_MS='5000'
  [Environment]::SetEnvironmentVariable('WGPU_BACKEND',[NullString]::Value,'Process')
  $process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($name+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($name+'.stderr.log'))
  $ownedPid=$process.Id;$ownedHandle=$process.Handle
  try {
   for($attempt=0;$attempt -lt 5 -and !$process.HasExited;$attempt++){[void]$process.WaitForExit(10000)}
   if(!$process.HasExited){throw ('Owned preview timed out: '+$name)}
   $process.WaitForExit()
   if($process.ExitCode -ne 0){throw ('Owned preview failed: '+$name+' exit='+[string]$process.ExitCode)}
  }finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
  $statePath=[IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'state.json');$state=Get-Content -LiteralPath $statePath -Raw|ConvertFrom-Json
  if(!$state.ready -or $state.error -or $state.step -ne 2 -or $state.totalFields -ne $count -or $state.selectedFields -ne $count -or $state.osInputUsed -or $state.savedHostsContacted -or $state.systemClipboardWritten){throw ('Fields preview failed: '+$name)}
  if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Missing native framebuffer'}
  $captures+=[pscustomobject]@{case=$name;pid=$ownedPid;normalExit=$true;png=$env:TOMATO_UI_CAPTURE;pngSha256=(Get-FileHash -LiteralPath $env:TOMATO_UI_CAPTURE).Hash;proof=$statePath;state=$state}
 }
}finally{foreach($key in $keys){$value=if($null -eq $saved[$key]){[NullString]::Value}else{$saved[$key]};[Environment]::SetEnvironmentVariable($key,$value,'Process')}}
$after=if(Test-Path -LiteralPath $config){(Get-FileHash -LiteralPath $config).Hash}else{$null}
if($before -ne $after){throw 'Shared configuration changed'}
[pscustomobject]@{status='passed';label=$Label;guiSha256=$hash;nativeChildViewport=$true;osInputUsed=$false;savedHostsContacted=$false;sharedConfigurationUnchanged=$true;captures=$captures}|ConvertTo-Json -Depth 12|Set-Content (Join-Path $probe 'validation.json')
Write-Output ('Native export-field checks passed: '+$Label)
