param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Executable,[switch]$AllFormats)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=[IO.Path]::GetFullPath($Executable);$hash=(Get-FileHash -LiteralPath $source).Hash
$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $hash){throw 'Snapshot mismatch'}
$keys=@('TEMP','TMP','TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_EXPORT_STAGE','TOMATO_EXPORT_FORMAT','WGPU_BACKEND')
$saved=@{};foreach($key in $keys){$saved[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$captures=@();$cases=@()
foreach($stage in @('scope','formats','fields','options')){$cases+=,@($stage,'txt')}
$formats=if($AllFormats){@('dbf','txt','csv','html','xls','xlsx','sql','xml','json','accdb')}else{@('json','xls','xlsx','accdb')}
foreach($stage in @('current','all')){foreach($format in $formats){$cases+=,@($stage,$format)}}
try {
 foreach($case in $cases){
  $stage=$case[0];$format=$case[1];$name=$stage+'-'+$format
  $profile=Join-Path $probe ('tomato-export-fixture-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($profile)
  $env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP;$env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_PREVIEW='data_export'
  $env:TOMATO_EXPORT_STAGE=$stage;$env:TOMATO_EXPORT_FORMAT=$format;$env:TOMATO_UI_CAPTURE=Join-Path $probe ($name+'.png');$env:TOMATO_UI_CAPTURE_FRAMES='1';$env:TOMATO_UI_CAPTURE_DELAY_MS='8000'
  [Environment]::SetEnvironmentVariable('WGPU_BACKEND',[NullString]::Value,'Process')
  $process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($name+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($name+'.stderr.log'))
  $ownedPid=$process.Id
  $ownedHandle=$process.Handle
  try {
   for($attempt=0;$attempt -lt 5 -and !$process.HasExited;$attempt++){[void]$process.WaitForExit(10000)}
   if(!$process.HasExited){throw ('Owned export preview timed out: '+$name)}
   $process.WaitForExit()
   if($process.ExitCode -ne 0){throw ('Owned export preview failed: '+$name+' exit='+[string]$process.ExitCode)}
  }finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
  $statePath=[IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'state.json');$state=Get-Content -LiteralPath $statePath -Raw|ConvertFrom-Json
  if(!$state.ready -or $state.error -or $state.osInputUsed -or $state.savedHostsContacted -or $state.systemClipboardWritten){throw ('Export preview failed: '+$name+' '+$state.error)}
  if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Missing native framebuffer'}
  if($stage -in @('current','all')){
   $expected=if($stage -eq 'all'){5003}else{1000}
   if($state.running -or $state.step -ne 4 -or $state.report.records -ne $expected){throw ('Export count/complete differs: '+$name)}
   if(!(Test-Path -LiteralPath $state.report.path) -or (Get-Item -LiteralPath $state.report.path).Length -le 0){throw 'Export file missing'}
   if($format -eq 'json'){$rows=Get-Content -LiteralPath $state.report.path -Raw|ConvertFrom-Json;if($rows.Count -ne $expected){throw 'JSON row count differs'}}
   if(Get-ChildItem -LiteralPath $profile -Filter '.tomato-export-*'){throw 'Export left temporary files'}
  }
  $captures+=[pscustomobject]@{case=$name;pid=$ownedPid;normalExit=$true;png=$env:TOMATO_UI_CAPTURE;pngSha256=(Get-FileHash -LiteralPath $env:TOMATO_UI_CAPTURE).Hash;proof=$statePath;state=$state}
 }
}finally{foreach($key in $keys){$value=if($null -eq $saved[$key]){[NullString]::Value}else{$saved[$key]};[Environment]::SetEnvironmentVariable($key,$value,'Process')}}
[pscustomobject]@{status='passed';label=$Label;guiSha256=$hash;nativeChildViewport=$true;osInputUsed=$false;savedHostsContacted=$false;captures=$captures}|ConvertTo-Json -Depth 12|Set-Content (Join-Path $probe 'validation.json')
Write-Output ('Native export checks passed: '+$Label)
