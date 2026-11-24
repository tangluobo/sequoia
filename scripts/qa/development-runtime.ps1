param(
 [Parameter(Mandatory=$true)][string]$Label,
 [Parameter(Mandatory=$true)][string]$Node,
 [Parameter(Mandatory=$true)][string]$Jdk,
 [Parameter(Mandatory=$true)][string]$Maven,
 [Parameter(Mandatory=$true)][string]$OwnedMavenRepository,
 [string]$TargetDirectory='target/qa-active-200',
 [ValidateSet('services','ui')][string[]]$Suites=@('services','ui')
)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid QA label'}
$repository=[IO.Path]::GetFullPath($OwnedMavenRepository)
$ownedRoot=[IO.Path]::GetFullPath((Join-Path $workspace 'target'))+[IO.Path]::DirectorySeparatorChar
if(!$repository.StartsWith($ownedRoot,[StringComparison]::OrdinalIgnoreCase) -or (Split-Path $repository -Leaf) -notlike 'development-maven-fixture-*'){throw 'Use an isolated copied Maven cache under this workspace target directory'}
foreach($path in @($Node,(Join-Path $Jdk 'bin/java.exe'),(Join-Path $Maven 'bin/mvn.cmd'),$repository)){if(!(Test-Path -LiteralPath $path)){throw ('Missing runtime fixture: '+$path)}}
$target=if([IO.Path]::IsPathRooted($TargetDirectory)){$TargetDirectory}else{Join-Path $workspace $TargetDirectory}
$probe=Join-Path $workspace ('target/runtime-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh QA label'}
[void][IO.Directory]::CreateDirectory($probe)
$keys=@('TEMP','TMP','TOMATO_DEV_NODE','TOMATO_DEV_JDK','TOMATO_DEV_MAVEN','TOMATO_DEV_MAVEN_REPO','MAVEN_OPTS','MAVEN_SKIP_RC','NPM_CONFIG_USERCONFIG','NPM_CONFIG_GLOBALCONFIG','NPM_CONFIG_CACHE','NPM_CONFIG_UPDATE_NOTIFIER','NPM_CONFIG_AUDIT','NPM_CONFIG_FUND','NPM_CONFIG_OFFLINE')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
try {
 $env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP
 $env:TOMATO_DEV_NODE=$Node;$env:TOMATO_DEV_JDK=$Jdk;$env:TOMATO_DEV_MAVEN=$Maven;$env:TOMATO_DEV_MAVEN_REPO=$repository
 $env:MAVEN_OPTS='-Xmx256m';$env:MAVEN_SKIP_RC='1'
 $env:NPM_CONFIG_USERCONFIG=Join-Path $probe 'user.npmrc';$env:NPM_CONFIG_GLOBALCONFIG=Join-Path $probe 'global.npmrc'
 [IO.File]::WriteAllText($env:NPM_CONFIG_USERCONFIG,'');[IO.File]::WriteAllText($env:NPM_CONFIG_GLOBALCONFIG,'')
 $env:NPM_CONFIG_CACHE=Join-Path $probe 'npm-cache';$env:NPM_CONFIG_UPDATE_NOTIFIER='false';$env:NPM_CONFIG_AUDIT='false';$env:NPM_CONFIG_FUND='false';$env:NPM_CONFIG_OFFLINE='true'
 $evidence=@()
 foreach($suite in $Suites) {
  $crate=if($suite -eq 'services'){'tomato_services'}else{'tomato_ui_egui'}
  $binary=Get-ChildItem -LiteralPath (Join-Path $target 'debug/deps') -Filter ($crate+'-*.exe')|Sort-Object LastWriteTimeUtc -Descending|Select-Object -First 1
  if(!$binary){throw ('Missing built test binary: '+$crate)}
  $list=@(& $binary.FullName --list)
  if($LASTEXITCODE -or !($list -match 'development::.*(real_maven_compiles|actual_java_file_context_menu)')){throw 'Wrong test binary or missing genuine Maven test'}
  $log=Join-Path $probe ($crate+'.log')
  & $binary.FullName development:: --ignored --nocapture *> $log
  if($LASTEXITCODE){Get-Content -LiteralPath $log -Tail 50;throw ('Real development workflow failed: '+$crate)}
  $evidence+=@{binary=$binary.FullName;sha256=(Get-FileHash -LiteralPath $binary.FullName).Hash;log=$log;result=(Select-String -LiteralPath $log -Pattern '^test result:').Line}
 }
 @{runs=$evidence;savedHostsContacted=$false;mavenOffline=$true;npmOffline=$true}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8NoBOM
 Get-Content -LiteralPath (Join-Path $probe 'validation.json')
} finally {foreach($key in $keys){if($null -eq $previous[$key]){[Environment]::SetEnvironmentVariable($key,[NullString]::Value,'Process')}else{[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}}
