param(
 [Parameter(Mandatory=$true)][string]$Executable,
 [Parameter(Mandatory=$true)][string]$Label,
 [string]$OldExecutable,
 [switch]$AutomaticGraphics
)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($probe)
$deps=Join-Path $workspace 'target/qa-active-200/debug/deps'
$harness=Get-ChildItem -LiteralPath $deps -Filter 'remote_desktop-*.exe'|Sort-Object LastWriteTime -Descending|Select-Object -First 1
$storage=Get-ChildItem -LiteralPath $deps -Filter 'object_storage-*.exe'|Sort-Object LastWriteTime -Descending|Select-Object -First 1
if(!$harness -or !$storage){throw 'Build all targets first'}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json'
$before=(Get-FileHash -LiteralPath $shared).Hash
$keys=@('TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_UI_CAPTURE_FRAMES','TOMATO_CAPTURE_TRACE','TOMATO_RENDER_DIAGNOSTICS','TOMATO_RDP_CAPTURE_EXE','TOMATO_RDP_CAPTURE_MODE','TOMATO_RDP_CAPTURE_PATH','TOMATO_RDP_CAPTURE_FULLSCREEN','TOMATO_RDP_CAPTURE_CLOSE','TOMATO_RDP_CAPTURE_CLIPBOARD','TOMATO_RDP_CAPTURE_CODEC','TOMATO_RDP_CAPTURE_MOTION','TOMATO_COPY_CAPTURE_EXE','TOMATO_COPY_CAPTURE_PATH','WGPU_BACKEND')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process');Remove-Item -LiteralPath ('Env:'+$key) -ErrorAction SilentlyContinue}
$results=@();$copies=@()
try{
 $versions=,@('new',$Executable);if($OldExecutable){$versions+=,@('old',$OldExecutable)}
 foreach($version in $versions){
  $source=[IO.Path]::GetFullPath($version[1])
  if(!$source.StartsWith((Join-Path $workspace 'target/'),[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($source) -ne 'tomato.exe'){throw 'Expected own workspace GUI'}
  $directory=Join-Path $probe $version[0];[void][IO.Directory]::CreateDirectory($directory)
  $exe=Join-Path $directory 'tomato.exe';$hash=(Get-FileHash -LiteralPath $source).Hash
  Copy-Item -LiteralPath $source -Destination $exe
  if((Get-FileHash -LiteralPath $exe).Hash -ne $hash){throw 'GUI changed while copying'}
  $copies+=@{source=$source;executable=$exe;sha256=$hash}
  $env:TOMATO_CAPTURE_TRACE='1';$env:TOMATO_RENDER_DIAGNOSTICS='1';$env:WGPU_BACKEND=if($AutomaticGraphics){$null}else{'vulkan'}
  $profile=Join-Path $directory 'empty-profile';[void][IO.Directory]::CreateDirectory($profile)
  $env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_CAPTURE=Join-Path $directory 'idle.png'
  $idle=Start-Process -FilePath $exe -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $directory 'idle.stderr.log')
  $requested=$null;$deadline=[DateTime]::UtcNow.AddSeconds(30)
  try{
   while(!$idle.WaitForExit(10)){
    if(!$requested -and (Test-Path -LiteralPath (Join-Path $directory 'idle.close'))){$requested=[DateTime]::UtcNow}
    if([DateTime]::UtcNow -ge $deadline){throw 'Owned idle process failed to exit'}
   }
   if(!$requested){throw 'Idle close marker missing'}
   $elapsed=([DateTime]::UtcNow-$requested).TotalMilliseconds
   if($idle.ExitCode -ne 0 -or $elapsed -ge 3000){throw 'Idle process did not exit normally within 3 s'}
   $results+=@{version=$version[0];case='idle-root';normalExit=$true;closeToExitMillis=$elapsed}
  }finally{if(!$idle.HasExited){$idle.Kill();$idle.WaitForExit()};$idle.Dispose()}
  Remove-Item Env:TOMATO_DATA_DIR,Env:TOMATO_UI_CAPTURE -ErrorAction SilentlyContinue
  $cases=@(@('tab-root','TAB','ROOT','vulkan','0'),@('window-root','WINDOW','ROOT','vulkan','0'),@('window-child','WINDOW','WINDOW','vulkan','0'),@('minimized-child','WINDOW','MINIMIZED','vulkan','0'),@('fullscreen-child','WINDOW','WINDOW','dx12','1'))
  if($version[0] -eq 'old'){$cases=$cases[0..1]}
  foreach($case in $cases){
   $backend=if($AutomaticGraphics -and $case[3] -eq 'vulkan'){'auto'}else{$case[3]}
   $env:WGPU_BACKEND=if($backend -eq 'auto'){$null}else{$backend};$env:TOMATO_RDP_CAPTURE_EXE=$exe;$env:TOMATO_RDP_CAPTURE_MODE=$case[1]
   $env:TOMATO_RDP_CAPTURE_FULLSCREEN=$case[4];$env:TOMATO_RDP_CAPTURE_CLIPBOARD='1'
   $env:TOMATO_RDP_CAPTURE_CODEC='raw';$env:TOMATO_UI_CAPTURE_DELAY_MS='1000'
   if($case[2] -eq 'ROOT'){Remove-Item Env:TOMATO_RDP_CAPTURE_CLOSE -ErrorAction SilentlyContinue}else{$env:TOMATO_RDP_CAPTURE_CLOSE=$case[2]}
   $env:TOMATO_RDP_CAPTURE_PATH=Join-Path $directory ($case[0]+'.png')
   & $harness.FullName 'rdp_fixture::capture_native_rdp_window' --exact --ignored --nocapture --test-threads=1 *> (Join-Path $directory ($case[0]+'.test.log'))
   if($LASTEXITCODE){throw ('Native RDP shutdown failed: '+$version[0]+' '+$case[0])}
   $exit=Get-Content -LiteralPath ([IO.Path]::ChangeExtension($env:TOMATO_RDP_CAPTURE_PATH,'exit.json')) -Raw|ConvertFrom-Json
   $child=$null
   if($case[2] -ne 'ROOT'){
    $child=Get-Content -LiteralPath ([IO.Path]::ChangeExtension($env:TOMATO_RDP_CAPTURE_PATH,'child-close.json')) -Raw|ConvertFrom-Json
    if(!$child.success -or $child.closeToRemovalMillis -ge 1000){throw 'RDP child close exceeded 1 s or removed the parent'}
   }
   if(!$exit.normalExit -or $exit.exitAfterCloseObservedMs -ge 3000){throw 'RDP application close exceeded 3 s'}
   $results+=@{version=$version[0];case=$case[0];backend=$backend;exit=$exit;child=$child}
  }
  if($version[0] -eq 'new'){
   $env:TOMATO_COPY_CAPTURE_EXE=$exe;$env:TOMATO_COPY_CAPTURE_PATH=Join-Path $directory 'multipart.png'
   Remove-Item Env:TOMATO_RDP_CAPTURE_CLOSE -ErrorAction SilentlyContinue
   & $storage.FullName 'copy::cancellation::native_capture::native_copy_progress_and_app_exit_abort_a_real_upload_without_changing_shared_config' --exact --ignored --nocapture --test-threads=1 *> (Join-Path $directory 'multipart.test.log')
   if($LASTEXITCODE){throw 'Native upload shutdown failed'}
   $exit=Get-Content -LiteralPath (Join-Path $directory 'multipart.exit.json') -Raw|ConvertFrom-Json
   $results+=@{version='new';case='active-multipart-root';exit=$exit}
  }
 }
 foreach($copy in $copies){if((Get-FileHash -LiteralPath $copy.source).Hash -ne $copy.sha256 -or (Get-FileHash -LiteralPath $copy.executable).Hash -ne $copy.sha256){throw 'GUI changed during native acceptance'}}
 if((Get-FileHash -LiteralPath $shared).Hash -ne $before){throw 'Shared configuration changed'}
 @{results=$results;executables=$copies;sharedConfigurationUnchanged=$true;savedHostsContacted=$false;osInputInjected=$false;scope='native windows and normal process exits against owned loopback RDP/S3 peers, not physical input performance'}|ConvertTo-Json -Depth 12|Set-Content -LiteralPath (Join-Path $probe 'validation.json')
}finally{
 foreach($key in $keys){if($null -eq $previous[$key]){Remove-Item -LiteralPath ('Env:'+$key) -ErrorAction SilentlyContinue}else{Set-Item -LiteralPath ('Env:'+$key) -Value $previous[$key]}}
}
