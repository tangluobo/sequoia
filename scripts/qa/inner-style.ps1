param(
 [Parameter(Mandatory=$true)][string]$Label,
 [Parameter(Mandatory=$true)][string]$Executable,
 [string[]]$Cases=@('designer-0','designer-multi','designer-1','designer-2','designer-3','designer-4','designer-5','designer-6','kafka','kafka_narrow','rocketmq','rocketmq_narrow','script_info','script_result','information'),
 [ValidateRange(2000,10000)][int]$CaptureDelayMs=5000
)
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
$keys=@('TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_LAYOUT_STATE','TOMATO_DESIGNER_PAGE','TOMATO_DESIGNER_GRID','TOMATO_WORKBENCH_STATE','WGPU_BACKEND','TOMATO_RENDER_DIAGNOSTICS','TOMATO_CAPTURE_TRACE')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$captures=@()
try {
 foreach($case in $Cases){
  $designer=$case -match '^designer-([0-6]|multi)$';$mq=$case -match '^(kafka|rocketmq)(_narrow)?$';$sql=$case -in @('script_info','script_result','information')
  if(!$designer -and !$mq -and !$sql){throw ('Invalid case: '+$case)}
  $prefix=if($designer){'tomato-designer-fixture-'}elseif($mq){'tomato-layout-fixture-'}else{'tomato-sql-workbench-fixture-'}
  $profile=Join-Path $probe ($prefix+$case);[void][IO.Directory]::CreateDirectory($profile)
  $env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_CAPTURE=Join-Path $probe ($case+'.png');$env:TOMATO_UI_CAPTURE_FRAMES='1';$env:TOMATO_UI_CAPTURE_DELAY_MS=[string]$CaptureDelayMs;$env:WGPU_BACKEND='vulkan'
  $env:TOMATO_UI_PREVIEW=if($designer){'table_designer'}elseif($mq){'layout_alignment'}else{'sql_workbench'}
  $env:TOMATO_DESIGNER_PAGE=if($case -eq 'designer-multi'){'0'}elseif($designer){$case.Substring(9)}else{$null}
  $env:TOMATO_DESIGNER_GRID=if($case -eq 'designer-multi'){'multi'}else{$null}
  $env:TOMATO_LAYOUT_STATE=if($mq){$case}else{$null}
  $env:TOMATO_WORKBENCH_STATE=if($sql -and $case -ne 'information'){$case}else{$null}
  $env:TOMATO_RENDER_DIAGNOSTICS='1';$env:TOMATO_CAPTURE_TRACE='1'
  $process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($case+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($case+'.stderr.log'))
  try {
   if(!$process.WaitForExit(30000) -and !$process.WaitForExit(30000)){throw ('Owned capture timed out: '+$case)}
   if($process.ExitCode -ne 0 -or !(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw ('Owned capture failed: '+$case)}
  } finally {if(!$process.HasExited){$process.Kill()};$process.Dispose()}
  $captures+=@{state=$case;path=$env:TOMATO_UI_CAPTURE;sha256=(Get-FileHash -LiteralPath $env:TOMATO_UI_CAPTURE).Hash}
 }
 if((Get-FileHash -LiteralPath $shared).Hash -ne $before){throw 'Shared configuration changed'}
 @{captures=$captures;guiSha256=$sha;sharedSha256=$before;sharedConfigurationUnchanged=$true;savedHostsContacted=$false;inertFixtures=$true}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $probe 'validation.json')
} finally {foreach($key in $keys){if($null -eq $previous[$key]){[Environment]::SetEnvironmentVariable($key,[NullString]::Value,'Process')}else{[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}}
