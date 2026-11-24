param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Executable,[string[]]$Formats=@('dbf','db','txt','csv','xls','xlsx','xml','json','accdb','odbc'),[switch]$ScreensOnly)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=[IO.Path]::GetFullPath($Executable);$hash=(Get-FileHash -LiteralPath $source).Hash
$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $exe
$keys=@('TEMP','TMP','TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_IMPORT_STAGE','TOMATO_IMPORT_FORMAT','TOMATO_IMPORT_EXTERNAL','TOMATO_IMPORT_PASSWORD','WGPU_BACKEND')
$saved=@{};foreach($key in $keys){$saved[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$cases=@();if($ScreensOnly){foreach($stage in @('formats','source','options','targets','fields','mode','advanced')){$cases+=,@($stage,$(if($stage -eq 'formats'){'txt'}else{'csv'}))}}else{foreach($format in $Formats){$cases+=,@('run',$format)}}
$checks=@()
try{
 foreach($case in $cases){
  $stage=$case[0];$format=$case[1];$name=$stage+'-'+$format
  $profile=Join-Path $probe ('tomato-import-fixture-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($profile)
  $env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP;$env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_PREVIEW='data_import'
  $env:TOMATO_IMPORT_STAGE=$stage;$env:TOMATO_IMPORT_FORMAT=$format;$env:TOMATO_UI_CAPTURE=Join-Path $probe ($name+'.png');$env:TOMATO_UI_CAPTURE_FRAMES='1';$env:TOMATO_UI_CAPTURE_DELAY_MS='12000'
  foreach($key in @('TOMATO_IMPORT_EXTERNAL','TOMATO_IMPORT_PASSWORD','WGPU_BACKEND')){[Environment]::SetEnvironmentVariable($key,[NullString]::Value,'Process')}
  $process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($name+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($name+'.stderr.log'))
  $ownedPid=$process.Id;$ownedHandle=$process.Handle
  try{for($attempt=0;$attempt -lt 6 -and !$process.HasExited;$attempt++){[void]$process.WaitForExit(10000)};if(!$process.HasExited){throw ('Import preview timed out: '+$name)};$process.WaitForExit();if($process.ExitCode -ne 0){throw ('Import preview failed: '+$name)}}finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
  $statePath=[IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'state.json');$state=Get-Content -LiteralPath $statePath -Raw|ConvertFrom-Json
  if(!$state.ready -or $state.error -or $state.running -or $state.osInputUsed -or $state.savedHostsContacted -or $state.systemClipboardWritten){throw ('Import state failed: '+$name+' '+$state.error)}
  if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Missing native framebuffer'}
  if($state.width -ne 680 -or $state.height -ne 480){throw 'Import wizard geometry differs'}
  if($ScreensOnly){$expected=@{formats=0;source=1;options=2;targets=3;fields=4;mode=5;advanced=6}[$stage];if($state.step -ne $expected){throw ('Wizard did not reach requested page: '+$name)}}
  if($stage -eq 'run'){
   if($state.step -ne 7 -or $state.report.inserted -ne 2305 -or $state.report.committed -ne 2305 -or $state.report.failed -ne 0 -or $state.report.uncertain -or $state.report.cancelled){throw ('Import counts differ: '+$name)}
   if($state.spool -and (Test-Path -LiteralPath $state.spool)){throw 'Source spool retained after exit'}
  }
  if(Test-Path -LiteralPath (Join-Path $profile 'connections.json')){throw 'Fixture modified connection configuration'}
  $checks+=[pscustomobject]@{case=$name;pid=$ownedPid;normalExit=$true;png=$env:TOMATO_UI_CAPTURE;state=$state;proof=$statePath}
 }
}finally{foreach($key in $keys){$value=if($null -eq $saved[$key]){[NullString]::Value}else{$saved[$key]};[Environment]::SetEnvironmentVariable($key,$value,'Process')}}
[pscustomobject]@{status='passed';guiSha256=$hash;nativeChildViewport=$true;osInputUsed=$false;savedHostsContacted=$false;checks=$checks}|ConvertTo-Json -Depth 12|Set-Content (Join-Path $probe 'validation.json')
Write-Output ('Native import checks passed: '+$Label)
