param(
 [Parameter(Mandatory=$true)][string]$Label,
 [Parameter(Mandatory=$true)][string]$Executable,
 [switch]$SquareCorners
)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=(Resolve-Path -LiteralPath $Executable).Path
if(!$source.StartsWith((Join-Path $workspace 'target/'),[StringComparison]::OrdinalIgnoreCase)){throw 'Expected an owned build'}
$sha=(Get-FileHash -LiteralPath $source).Hash
$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $sha){throw 'Executable changed during copy'}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$keys=@('TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_DESIGNER_PAGE','TOMATO_DESIGNER_MENU','WGPU_BACKEND')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$captures=@()
try {
 foreach($page in @(7,8)){foreach($mode in @('item','background')){
  $case=([string]$page)+'-'+$mode
  $profile=Join-Path $probe ('tomato-designer-fixture-'+$case);[void][IO.Directory]::CreateDirectory($profile)
  $env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_PREVIEW='table_designer';$env:TOMATO_DESIGNER_PAGE=[string]$page;$env:TOMATO_DESIGNER_MENU=$mode
  $env:TOMATO_UI_CAPTURE=Join-Path $probe ($case+'.png');$env:TOMATO_UI_CAPTURE_FRAMES='1';$env:TOMATO_UI_CAPTURE_DELAY_MS='5000';$env:WGPU_BACKEND='vulkan'
  $process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($case+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($case+'.stderr.log'))
  try{
   if(!$process.WaitForExit(30000) -and !$process.WaitForExit(30000)){throw ('Owned menu capture timed out: '+$case)}
   if($process.ExitCode -ne 0 -or !(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw ('Owned menu capture failed: '+$case)}
  }finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
  $measurement=[IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'pixels.json')
  [string[]]$cornerArgs=if($SquareCorners){'--square-corners'}
  & D:/App/msys64/ucrt64/bin/python.exe -X utf8 (Join-Path $PSScriptRoot 'measure-object-menu.py') $env:TOMATO_UI_CAPTURE --output $measurement @cornerArgs
  if($LASTEXITCODE){throw ('Menu framebuffer geometry differs: '+$case)}
  $menu=Get-Content -LiteralPath $measurement -Raw|ConvertFrom-Json
  $state=Get-Content ([IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'state.json')) -Raw|ConvertFrom-Json
  if(!$menu.nativeFramebufferMeasured -or $state.databaseRequested -or $state.systemClipboardWritten -or !$state.ready){throw 'Unexpected fixture state'}
  $captures+=@{state=$case;path=$env:TOMATO_UI_CAPTURE;menu=$menu;normalExit=$true}
 }}
 if((Get-FileHash -LiteralPath $shared).Hash -ne $before){throw 'Shared configuration changed'}
 @{guiSha256=$sha;captures=$captures;sharedConfigurationUnchanged=$true;sharedSha256=$before;savedHostsContacted=$false;nativeWindowVerified=$true;nativePointerTargetVerified=$false;fullPixelParity=$false}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $probe 'validation.json')
}finally{foreach($key in $keys){if($null -eq $previous[$key]){[Environment]::SetEnvironmentVariable($key,[NullString]::Value,'Process')}else{[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}}
