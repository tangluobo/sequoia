param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'));$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if($Label -notmatch '^[a-z0-9-]+$' -or (Test-Path -LiteralPath $probe)){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($probe);$exe=Join-Path $probe 'import_source_fixture.exe';Copy-Item -LiteralPath $Executable -Destination $exe
$sourceRoot=Join-Path $workspace 'target/window-probes/import-formats-dev-1474'
$csvState=Get-Content -LiteralPath (Join-Path $sourceRoot 'run-csv.state.json') -Raw|ConvertFrom-Json
$accessState=Get-Content -LiteralPath (Join-Path $sourceRoot 'run-accdb.state.json') -Raw|ConvertFrom-Json
$odbcState=Get-Content -LiteralPath (Join-Path $workspace 'target/window-probes/import-odbc-dev-1476/run-odbc.state.json') -Raw|ConvertFrom-Json
$inputs=@{csv=Join-Path ([IO.Path]::GetDirectoryName($csvState.database)) 'input.csv';accdb=Join-Path ([IO.Path]::GetDirectoryName($accessState.database)) 'input.accdb';odbc=Join-Path ([IO.Path]::GetDirectoryName($odbcState.database)) 'input.csv'}
$before=@{};foreach($key in $inputs.Keys){$before[$key]=(Get-FileHash -LiteralPath $inputs[$key]).Hash}
$saved=@{TEMP=$env:TEMP;TMP=$env:TMP;SOURCE=$env:TOMATO_IMPORT_SOURCE_EXE};$checks=@()
try{
 [Environment]::SetEnvironmentVariable('TOMATO_IMPORT_SOURCE_EXE',[NullString]::Value,'Process')
 foreach($case in @(@('csv','repeat'),@('accdb','repeat'),@('odbc','repeat'),@('accdb','cancel'),@('odbc','cancel'))){
  $name=$case[0]+'-'+$case[1];$cache=Join-Path $probe ('tomato-import-owned-cache-'+$name);[void][IO.Directory]::CreateDirectory($cache);$env:TEMP=$cache;$env:TMP=$cache
  $stdout=Join-Path $probe ($name+'.json');$stderr=Join-Path $probe ($name+'.stderr.log')
  $process=Start-Process -FilePath $exe -ArgumentList @($case[0],$inputs[$case[0]],$case[1]) -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr;$handle=$process.Handle
  try{for($attempt=0;$attempt -lt 6 -and !$process.HasExited;$attempt++){[void]$process.WaitForExit(10000)};if(!$process.HasExited){throw ('Source lifecycle timed out: '+$name)};$process.WaitForExit();if($process.ExitCode -ne 0){Get-Content -LiteralPath $stderr -Tail 10;throw ('Source lifecycle failed: '+$name)}}finally{if(!$process.HasExited){$process.Kill($true)};$process.Dispose()}
  $result=Get-Content -LiteralPath $stdout -Raw|ConvertFrom-Json;if($result.status -ne 'passed' -or !$result.cacheEmpty -or (Get-ChildItem -LiteralPath $cache).Count -ne 0){throw ('Source cache survived: '+$name)}
  $checks+=,$result
 }
}finally{$env:TEMP=$saved.TEMP;$env:TMP=$saved.TMP;$env:TOMATO_IMPORT_SOURCE_EXE=$saved.SOURCE}
foreach($key in $inputs.Keys){if((Get-FileHash -LiteralPath $inputs[$key]).Hash -ne $before[$key]){throw 'Source file changed'}}
[pscustomobject]@{status='passed';harnessSha256=(Get-FileHash -LiteralPath $exe).Hash;checks=$checks;sourceFilesUnchanged=$true;savedHostsContacted=$false}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $probe 'validation.json')
Write-Output ('Source lifecycle passed: '+$Label)
