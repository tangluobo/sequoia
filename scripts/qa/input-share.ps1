param([Parameter(Mandatory=$true)][string]$Executable,[Parameter(Mandatory=$true)][string]$Label)
$ErrorActionPreference='Stop'
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid owned probe label'}
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$root=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $root){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($root)
$program=(Resolve-Path -LiteralPath $Executable).Path
if(!$program.StartsWith((Join-Path $workspace 'target/'),[StringComparison]::OrdinalIgnoreCase)){throw 'Expected workspace candidate'}
$profile=Join-Path $root 'isolated-profile'
[void][IO.Directory]::CreateDirectory($profile)
[IO.File]::WriteAllText((Join-Path $profile 'connections.json'),'{"connections":[],"groups":[]}',[Text.UTF8Encoding]::new($false))
$keys=@('TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','WGPU_BACKEND')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json'
$before=if(Test-Path -LiteralPath $shared){(Get-FileHash -LiteralPath $shared).Hash}else{''}
$sha=(Get-FileHash -LiteralPath $program).Hash
$captures=@()
try {
 $env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_CAPTURE_FRAMES='4';$env:TOMATO_UI_CAPTURE_DELAY_MS='2500';$env:WGPU_BACKEND='vulkan'
 foreach($case in @('input_share','input_share_connected')) {
  $env:TOMATO_UI_PREVIEW=$case;$env:TOMATO_UI_CAPTURE=Join-Path $root ($case+'.png')
  $process=Start-Process -FilePath $program -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $root ($case+'.stdout.log')) -RedirectStandardError (Join-Path $root ($case+'.stderr.log'))
  try {
   if(!$process.WaitForExit(60000)){throw ('Owned capture timed out: '+$case)}
   if($process.ExitCode -ne 0){throw ('Owned capture failed: '+$case)}
  } finally {if(!$process.HasExited){$process.Kill()};$process.Dispose()}
  $image=Join-Path $root ($case+'-03.png')
  if(!(Test-Path -LiteralPath $image)){throw ('Missing framebuffer: '+$case)}
  $captures+=@{case=$case;image=$image;syntheticNodes=$case.EndsWith('_connected')}
 }
 $after=if(Test-Path -LiteralPath $shared){(Get-FileHash -LiteralPath $shared).Hash}else{''}
 if($before -ne $after){throw 'Shared connection file changed'}
 if($sha -ne (Get-FileHash -LiteralPath $program).Hash){throw 'Candidate changed during capture'}
 @{guiSha256=$sha;captures=$captures;sharedConfigurationUnchanged=$true;savedHostsContacted=$false;systemInputHooksInstalled=$false;physicalMultiHostAcceptance=$false}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $root 'validation.json') -Encoding utf8
 $root
} finally {foreach($key in $keys){[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}
