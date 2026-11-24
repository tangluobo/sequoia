param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=(Resolve-Path -LiteralPath $Executable).Path
$hash=(Get-FileHash -LiteralPath $source).Hash
$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $hash -or (Get-FileHash -LiteralPath $source).Hash -ne $hash){throw 'GUI changed while snapshotting'}
$runner=Get-ChildItem -LiteralPath (Join-Path $workspace 'target/qa-active-200/debug/deps') -Filter 'object_storage-*.exe'|Sort-Object LastWriteTimeUtc -Descending|Select-Object -First 1
if(!$runner){throw 'Build all workspace targets first'}
$test='upload::native::capture_native_file_drop'
$available=& $runner.FullName --list
if($LASTEXITCODE -ne 0 -or !($available|Where-Object {$_ -eq ($test+': test')})){throw 'Native upload test missing from runner'}
$keys=@('TEMP','TMP','TOMATO_FILE_DROP_EXE','TOMATO_FILE_DROP_OUTPUT','TOMATO_FILE_DROP_MANUAL','WGPU_BACKEND')
$saved=@{};foreach($key in $keys){$saved[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
try{
 $env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP
 $env:TOMATO_FILE_DROP_EXE=$exe;$env:TOMATO_FILE_DROP_OUTPUT=$probe
 [Environment]::SetEnvironmentVariable('TOMATO_FILE_DROP_MANUAL',[NullString]::Value,'Process')
 [Environment]::SetEnvironmentVariable('WGPU_BACKEND',[NullString]::Value,'Process')
 & $runner.FullName --exact $test --ignored --nocapture *> (Join-Path $probe 'runner.log')
 if($LASTEXITCODE -ne 0){throw ('Native upload checks failed: '+$probe)}
}finally{foreach($key in $keys){[Environment]::SetEnvironmentVariable($key,$saved[$key],'Process')}}
if((Get-FileHash -LiteralPath $source).Hash -ne $hash -or (Get-FileHash -LiteralPath $exe).Hash -ne $hash){throw 'GUI changed during checks'}
& D:/App/msys64/ucrt64/bin/python.exe -X utf8 (Join-Path $workspace 'scripts/qa/file-drop-pixels.py') $probe *> (Join-Path $probe 'pixels.log')
if($LASTEXITCODE -ne 0){throw ('Upload framebuffer differs: '+$probe)}
$path=Join-Path $probe 'validation.json'
$proof=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
if($proof.status -ne 'passed' -or $proof.cases.Count -ne 7){throw 'Incomplete upload evidence'}
$proof|Add-Member guiSha256 $hash
$proof|Add-Member runnerSha256 (Get-FileHash -LiteralPath $runner.FullName).Hash
$proof|Add-Member physicalExplorerDragAcceptanceClaimed $false
$proof|Add-Member framebufferHashes @(Get-ChildItem -LiteralPath $probe -Filter '*.png'|ForEach-Object {[pscustomobject]@{name=$_.Name;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash}})
$proof|ConvertTo-Json -Depth 14|Set-Content -LiteralPath $path
Write-Output ('Native file upload checks passed: '+$path)
