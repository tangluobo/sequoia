param(
 [Parameter(Mandatory=$true)][ValidatePattern('^[a-z0-9-]+$')][string]$Label,
 [string]$TargetDirectory='target/qa-active-200'
)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$target=[IO.Path]::GetFullPath((Join-Path $workspace $TargetDirectory))
if(!$target.StartsWith((Join-Path $workspace 'target/'),[StringComparison]::OrdinalIgnoreCase)){throw 'Expected workspace build cache'}
$dependencies=Join-Path $target 'debug/deps'
$framework=Get-ChildItem -LiteralPath $dependencies -Filter 'libeframe-*.rlib' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if(!$framework){throw 'Build the workspace with all targets first'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($probe)
$rustc=Join-Path $env:USERPROFILE '.cargo/bin/rustc.exe'
$executable=Join-Path $probe 'rdp-viewport-probe.exe'
$proof=Join-Path $probe 'validation.json'
$arguments=@('--edition=2024',(Join-Path $PSScriptRoot 'rdp-viewport.rs'),'--extern',('eframe='+$framework.FullName),'-L',('dependency='+$dependencies),'-o',$executable)
$registry=Join-Path $env:USERPROFILE '.cargo/registry/src'
foreach($index in Get-ChildItem -LiteralPath $registry -Directory){
 foreach($library in Get-ChildItem -LiteralPath $index.FullName -Directory -Filter 'windows_x86_64_msvc-*'){
  $arguments+=@('-L',('native='+$library.FullName+'/lib'))
 }
}
& $rustc @arguments *> (Join-Path $probe 'build.log')
if($LASTEXITCODE -ne 0){throw 'Owned viewport regression did not compile'}
$oldBackend=[Environment]::GetEnvironmentVariable('WGPU_BACKEND','Process')
$process=$null
try {
 Remove-Item Env:WGPU_BACKEND -ErrorAction SilentlyContinue
 $process=Start-Process -FilePath $executable -ArgumentList $proof -WorkingDirectory $workspace -WindowStyle Hidden -RedirectStandardOutput (Join-Path $probe 'stdout.log') -RedirectStandardError (Join-Path $probe 'stderr.log') -PassThru
 if(!$process.WaitForExit(45000)){throw 'Owned viewport regression timed out'}
 if($process.ExitCode -ne 0 -or !(Test-Path -LiteralPath $proof)){throw 'Owned viewport lifecycle failed; see stderr.log'}
 Get-Content -LiteralPath $proof
} finally {
 if($process){
  if(!$process.HasExited -and [string]::Equals($process.MainModule.FileName,$executable,[StringComparison]::OrdinalIgnoreCase)){$process.Kill();[void]$process.WaitForExit(10000)}
  $process.Dispose()
 }
 if($null -eq $oldBackend){Remove-Item Env:WGPU_BACKEND -ErrorAction SilentlyContinue}else{$env:WGPU_BACKEND=$oldBackend}
}
