param([Parameter(Mandatory=$true)][string]$Label,[string]$Harness,[string]$Executable,[string[]]$Cases=@('main-fonts','shortcut-0','shortcut-1','shortcut-2','icon-empty','icon-preview','downloader','downloader-add','downloader-settings'))
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid probe label'}
if($Cases|Where-Object {$_ -notin @('main-fonts','shortcut-0','shortcut-1','shortcut-2','icon-empty','icon-preview','downloader','downloader-add','downloader-settings')}){throw 'Unexpected capture case'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($probe)
if($Executable){
 $source=(Resolve-Path -LiteralPath $Executable).Path
 if(!$source.StartsWith((Join-Path $workspace 'target/'),[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($source) -ne 'tomato.exe'){throw 'Expected owned GUI binary'}
 $guiHash=(Get-FileHash -LiteralPath $source).Hash;$program=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $program
 if((Get-FileHash -LiteralPath $program).Hash -ne $guiHash -or (Get-FileHash -LiteralPath $source).Hash -ne $guiHash){throw 'GUI changed while copying'}
 $arguments=@();$harnessHash=$null
}else{
 $program=(Resolve-Path -LiteralPath $Harness).Path
 if(!$program.StartsWith((Join-Path $workspace 'target/debug/deps'),[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($program) -notmatch '^ui_capture-[a-f0-9]+\.exe$'){throw 'Expected owned UI capture test harness'}
 $arguments=@('--ignored','--exact','capture_desktop','--nocapture');$harnessHash=(Get-FileHash -LiteralPath $program).Hash;$guiHash=$null
}
$profile=Join-Path $probe 'profile';[void][IO.Directory]::CreateDirectory($profile)
$rows=@();for($i=0;$i -lt 40;$i++){$rows+=@{id=('font-fixture-'+$i);name=('清晰度测试 连接 ABCabc123 '+$i);type='SSH'}}
@{connections=$rows;groups=@()}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $profile 'connections.json') -Encoding utf8
$source=Join-Path $probe 'source.png';Copy-Item -LiteralPath (Join-Path $workspace 'crates/services/tests/fixtures/icons/source-2.png') -Destination $source
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$keys=@('TOMATO_DATA_DIR','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_PREVIEW','TOMATO_UI_PREVIEW_TAB','TOMATO_UI_PREVIEW_DIALOG','TOMATO_UI_PREVIEW_PATH','WGPU_BACKEND');$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$captures=@()
try {
 $env:TOMATO_DATA_DIR=$profile;$env:WGPU_BACKEND='vulkan'
 foreach($case in $Cases) {
  $env:TOMATO_UI_PREVIEW_TAB=$null;$env:TOMATO_UI_PREVIEW_DIALOG=$null;$env:TOMATO_UI_PREVIEW_PATH=$null
  if($case -eq 'main-fonts'){$env:TOMATO_UI_PREVIEW=$null}elseif($case.StartsWith('shortcut-')){$env:TOMATO_UI_PREVIEW='desktop_shortcut';$env:TOMATO_UI_PREVIEW_TAB=$case.Substring(9)}elseif($case.StartsWith('icon-')){$env:TOMATO_UI_PREVIEW='icon_tool';if($case -eq 'icon-preview'){$env:TOMATO_UI_PREVIEW_PATH=$source}}else{$env:TOMATO_UI_PREVIEW='downloader';if($case -ne 'downloader'){$env:TOMATO_UI_PREVIEW_DIALOG=$case.Substring(11)}}
  $env:TOMATO_UI_CAPTURE=Join-Path $probe ($case+'.png')
  $env:TOMATO_UI_CAPTURE_FRAMES=if($case -eq 'icon-preview'){'8'}else{'1'}
  $start=@{FilePath=$program;WindowStyle='Hidden';PassThru=$true;RedirectStandardOutput=(Join-Path $probe ($case+'.stdout.log'));RedirectStandardError=(Join-Path $probe ($case+'.stderr.log'))};if($arguments.Count -gt 0){$start.ArgumentList=$arguments}
  $process=Start-Process @start
  try {if(!$process.WaitForExit(45000)){throw ('Owned tool capture timed out: '+$case)};if($process.ExitCode -ne 0){throw ('Tool capture failed: '+$case)};if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Missing own framebuffer'}}finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
  $image=if($case -eq 'icon-preview'){Join-Path $probe 'icon-preview-07.png'}else{$env:TOMATO_UI_CAPTURE}
  if(!(Test-Path -LiteralPath $image)){throw 'Missing last owned frame'}
  $captures+=@{case=$case;path=$image;sha256=(Get-FileHash -LiteralPath $image).Hash}
 }
 if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared legacy configuration changed'}
 @{guiSha256=$guiHash;harnessSha256=$harnessHash;captures=$captures;sharedConfigurationUnchanged=$true;sharedSha256=$before;savedHostsContacted=$false;fullVisualParityVerified=$false}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8
} finally {foreach($key in $keys){if($null -eq $previous[$key]){[Environment]::SetEnvironmentVariable($key,[NullString]::Value,'Process')}else{[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}}
