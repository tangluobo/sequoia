param([Parameter(Mandatory=$true)][string]$Label,[string]$Executable,[string]$Reference='target/development-settings-oracle-408/fixture.json',[string[]]$States=@('project','sdks','sdks-selected','builds','builds-selected','sdk-add','tool-add','tool-edit','tool-gradle','tool-type-change','add-directory'))
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid capture label'}
if(!$Executable){$Executable=Join-Path $workspace 'target/qa-active-200/debug/tomato.exe'}
$referencePath=if([IO.Path]::IsPathRooted($Reference)){$Reference}else{Join-Path $workspace $Reference}
$fixture=Get-Content -LiteralPath $referencePath -Raw|ConvertFrom-Json -AsHashtable
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh capture label'}
[void][IO.Directory]::CreateDirectory($probe)
$hash=(Get-FileHash -LiteralPath $Executable).Hash
$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $Executable -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $hash){throw 'GUI changed while copying'}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$keys=@('TEMP','TMP','TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_DEVELOPMENT_STATE','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','WGPU_BACKEND')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$captures=@()
try {
 foreach($state in $States) {
  if($state -notin @('project','sdks','sdks-selected','builds','builds-selected','sdk-add','tool-add','tool-edit','tool-gradle','tool-type-change','add-directory')){throw 'Unknown settings state'}
  $profile=Join-Path $workspace ('target/tomato-development-fixture-'+[guid]::NewGuid().ToString('N'))
  $project=Join-Path $profile 'project';[void][IO.Directory]::CreateDirectory($project)
  [IO.File]::WriteAllText((Join-Path $project 'pom.xml'),'<project/>')
  [IO.File]::WriteAllText((Join-Path $project 'package.json'),'{"scripts":{"dev":"node app.js"}}')
  $definition=@{};foreach($entry in $fixture.projects[0].GetEnumerator()){$definition[$entry.Key]=$entry.Value};$definition.path=$project
  $config=@{projects=@($definition);runtimes=@();buildTools=@();developmentSettingsFixture=@{runtimes=$fixture.runtimes;buildTools=$fixture.buildTools}}
  $configPath=Join-Path $profile 'development.json';$config|ConvertTo-Json -Depth 10|Set-Content -LiteralPath $configPath -Encoding utf8NoBOM
  $configHash=(Get-FileHash -LiteralPath $configPath).Hash
  $env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP
  $env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_PREVIEW='development';$env:TOMATO_DEVELOPMENT_STATE='settings-'+$state
  $env:TOMATO_UI_CAPTURE=Join-Path $probe ($state+'.png');$env:TOMATO_UI_CAPTURE_FRAMES='1';$env:TOMATO_UI_CAPTURE_DELAY_MS='6000';$env:WGPU_BACKEND='vulkan'
  $process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($state+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($state+'.stderr.log'))
  try {for($i=0;$i -lt 6 -and !$process.HasExited;$i++){[void]$process.WaitForExit(10000)};if(!$process.HasExited -or $process.ExitCode -ne 0){throw ('Settings framebuffer failed: '+$state)}}finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
  if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Missing settings framebuffer'}
  if($configHash -ne (Get-FileHash -LiteralPath $configPath).Hash){throw 'Preview persisted a draft'}
  $captures+=@{state=$state;path=$env:TOMATO_UI_CAPTURE;profile=$profile;configurationUnchanged=$true}
 }
 if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared connections changed'}
 @{guiSha256=$hash;captures=$captures;referenceFixture=$referencePath;savedHostsContacted=$false;settingsActionsDispatched=$false;sharedConfigurationUnchanged=$true;fullPixelParity=$false}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8NoBOM
 Get-Content -LiteralPath (Join-Path $probe 'validation.json')
}finally{foreach($key in $keys){if($null -eq $previous[$key]){[Environment]::SetEnvironmentVariable($key,[NullString]::Value,'Process')}else{[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}}
