param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=[IO.Path]::GetFullPath($Executable)
$hash=(Get-FileHash -LiteralPath $source).Hash
$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $hash -or (Get-FileHash -LiteralPath $source).Hash -ne $hash){throw 'GUI changed while snapshotting'}
$keys=@('TEMP','TMP','TOMATO_DATA_DIR','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_UI_PREVIEW','TOMATO_QUERY_INPUT_ROUTE','WGPU_BACKEND')
$saved=@{};foreach($key in $keys){$saved[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$captures=@()
try{
 foreach($route in @('scoped','tree')){
  $profile=Join-Path $workspace ('target/tomato-query-input-fixture-'+[guid]::NewGuid().ToString('N'))
  [void][IO.Directory]::CreateDirectory($profile)
  $env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP
  $env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_PREVIEW='query_input';$env:TOMATO_QUERY_INPUT_ROUTE=$route
  $env:TOMATO_UI_CAPTURE=Join-Path $workspace ('target/ui-query-input-'+$Label+'-'+$route+'.png')
  $env:TOMATO_UI_CAPTURE_FRAMES='1';$env:TOMATO_UI_CAPTURE_DELAY_MS='10000'
  [Environment]::SetEnvironmentVariable('WGPU_BACKEND',[NullString]::Value,'Process')
  $process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($route+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($route+'.stderr.log'))
  $pidOwned=$process.Id
  try{
   for($attempt=0;$attempt -lt 5 -and !$process.HasExited;$attempt++){[void]$process.WaitForExit(30000)}
   if(!$process.HasExited){throw 'Owned query input preview timed out'}
   if($process.ExitCode -ne 0){throw 'Owned query input preview failed'}
  }finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
  $path=[IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'input.json')
  $state=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
  if(!$state.ready -or $state.route -ne $route){throw 'Query input preview incomplete'}
  foreach($frame in @(2,6,10,14,24,38)){
   $item=@($state.samples|Where-Object frame -eq $frame)
   if($item.Count -ne 1 -or !$item[0].focused -or $item[0].focusRequested -or $item[0].error){throw ('Query focus differs at frame '+$frame)}
  }
  foreach($frame in @(2,38)){
   $item=$state.samples|Where-Object frame -eq $frame
   if($item.sql -ne '' -or [Math]::Abs($item.lineHeight-17) -gt 0.1 -or $item.caret -ne 0){throw 'Blank SQL caret geometry differs'}
  }
  $typed=$state.samples|Where-Object frame -eq 6
  if($typed.sql -ne 'SEL' -or $typed.candidates -notcontains 'SELECT'){throw 'Immediate typing or completion failed'}
  if(($state.samples|Where-Object frame -eq 10).sql -ne 'SELECT'){throw 'Actual Tab completion failed'}
  if(($state.samples|Where-Object frame -eq 14).sql -ne "SELECT '中文' AS value;"){throw 'Unicode typing failed'}
  if(($state.samples|Where-Object frame -eq 24).sql -ne "SELECT '中文' AS value; -- 点击空白"){throw 'Clicking blank editor space lost input'}
  $last=$state.samples[-1]
  if($last.frame -lt 48 -or $last.sql -ne "SELECT '输入正常' AS value;" -or $last.value -ne '输入正常' -or $last.pending -ne 0 -or !$last.dirty -or $last.error){throw 'Real SQLite execution after clear and retype failed'}
  foreach($item in $state.samples){if($item.osInputUsed -or $item.savedHostsContacted -or $item.systemClipboardWritten){throw 'Preview isolation differs'}}
  if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Framebuffer missing'}
  $captures+=[pscustomobject]@{route=$route;pid=$pidOwned;normalExit=$true;png=$env:TOMATO_UI_CAPTURE;pngSha256=(Get-FileHash -LiteralPath $env:TOMATO_UI_CAPTURE).Hash;proof=$path;state=$state}
 }
}finally{foreach($key in $keys){[Environment]::SetEnvironmentVariable($key,$saved[$key],'Process')}}
[pscustomobject]@{status='passed';label=$Label;guiSha256=$hash;nativeRootViewport=$true;realSqliteActor=$true;osInputUsed=$false;savedHostsContacted=$false;physicalMouseAcceptanceClaimed=$false;captures=$captures}|ConvertTo-Json -Depth 12|Set-Content (Join-Path $workspace ('target/'+$Label+'.json'))
Write-Output ('Query input native checks passed: '+$Label)
