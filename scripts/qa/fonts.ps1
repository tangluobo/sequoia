param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Harness)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid probe label'}
$probe=Join-Path $workspace ('target/font-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($probe)
$harnessPath=(Resolve-Path -LiteralPath $Harness).Path
if(!$harnessPath.StartsWith((Join-Path $workspace 'target/debug/deps'),[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($harnessPath) -notmatch '^ui_capture-[a-f0-9]+\.exe$'){throw 'Expected owned UI capture test harness'}
$profile=Join-Path $probe 'profile';[void][IO.Directory]::CreateDirectory($profile)
$rows=@();for($i=0;$i -lt 40;$i++){$rows+=@{id=('font-fixture-'+$i);name=('清晰度测试 连接 ABCabc123 '+$i);type='SSH'}}
@{connections=$rows;groups=@()}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $profile 'connections.json') -Encoding utf8
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$env:TOMATO_DATA_DIR=$profile;$env:WGPU_BACKEND='vulkan';$env:TOMATO_UI_PREVIEW=$null
try {
 foreach($dpi in @('1','1.25','1.5','2')) {foreach($mode in @('baseline','sharp')) {
  $case=$mode+'-'+$dpi.Replace('.','-')
  $env:TOMATO_UI_CAPTURE=Join-Path $probe ($case+'.png');$env:TOMATO_UI_CAPTURE_PPP=$dpi
  $env:TOMATO_UI_CAPTURE_BASELINE_FONTS=if($mode -eq 'baseline'){'1'}else{$null}
  $process=Start-Process -FilePath $harnessPath -ArgumentList @('--ignored','--exact','capture_offscreen','--nocapture') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($case+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($case+'.stderr.log'))
  try {if(!$process.WaitForExit(30000)){throw 'Own font render timed out'};if($process.ExitCode -ne 0){throw ('Font render failed: '+$case)};if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Own framebuffer missing'}}finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
 }}
 if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared legacy configuration changed'}
 @{harnessSha256=(Get-FileHash -LiteralPath $harnessPath).Hash;scales=@(1,1.25,1.5,2);sharedConfigurationUnchanged=$true;sharedSha256=$before;savedHostsContacted=$false;nativeMonitorDpiTransitionsVerified=$false}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $probe 'identity.json') -Encoding utf8
} finally {foreach($key in @('TOMATO_DATA_DIR','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_PPP','TOMATO_UI_CAPTURE_BASELINE_FONTS','WGPU_BACKEND')){[Environment]::SetEnvironmentVariable($key,$null,'Process')}}
