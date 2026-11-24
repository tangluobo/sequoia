param([Parameter(Mandatory=$true)][string]$Label,[string]$Executable)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
if(!$Executable){$Executable=Join-Path $workspace 'target/qa-active-200/debug/tomato.exe'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($probe)
$hash=(Get-FileHash -LiteralPath $Executable).Hash
$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $Executable -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $hash){throw 'Executable changed while copying'}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$sharedBefore=(Get-FileHash -LiteralPath $shared).Hash
$keys=@('TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_DOWNLOADER_FIXTURE','WGPU_BACKEND','TEMP','TMP')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$results=@()
$server=$null;$ed2kServer=$null
try{
 $portFile=Join-Path $probe 'server-port.txt';$payload=Join-Path $probe 'expected.bin'
 $server=Start-Process -FilePath 'D:/App/msys64/ucrt64/bin/python.exe' -ArgumentList @('-X','utf8',(Join-Path $workspace 'scripts/qa/downloader-server.py'),'--port-file',$portFile,'--payload-file',$payload) -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe 'server.stdout.log') -RedirectStandardError (Join-Path $probe 'server.stderr.log')
 for($i=0;$i -lt 100 -and !(Test-Path -LiteralPath $portFile);$i++){if($server.HasExited){throw 'Owned HTTP fixture exited'};Start-Sleep -Milliseconds 100}
 $port=[int](Get-Content -LiteralPath $portFile -Raw)
 $ed2kPortFile=Join-Path $probe 'ed2k-port.txt';$ed2kProof=Join-Path $probe 'ed2k-server.json'
 $ed2kServer=Start-Process -FilePath 'D:/App/msys64/ucrt64/bin/python.exe' -ArgumentList @('-X','utf8',(Join-Path $workspace 'scripts/qa/downloader-ed2k-server.py'),'--port-file',$ed2kPortFile,'--proof',$ed2kProof) -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe 'ed2k.stdout.log') -RedirectStandardError (Join-Path $probe 'ed2k.stderr.log')
 for($i=0;$i -lt 100 -and !(Test-Path -LiteralPath $ed2kPortFile);$i++){if($ed2kServer.HasExited){throw 'Owned eD2k fixture exited'};Start-Sleep -Milliseconds 100}
 $ed2kPort=[int](Get-Content -LiteralPath $ed2kPortFile -Raw)
 foreach($mode in @('tree','add','settings','catalog','download','ed2k')){
  $profile=Join-Path $workspace ('target/tomato-downloader-fixture-'+[guid]::NewGuid().ToString('N'))
  $downloads=Join-Path $profile 'files';[void][IO.Directory]::CreateDirectory($downloads)
  $config=Join-Path $profile 'connections.json';$history=Join-Path $profile 'downloads.json'
  [IO.File]::WriteAllText((Join-Path $profile 'fixture-url.txt'),"http://127.0.0.1:$port/fixture.bin",[Text.UTF8Encoding]::new($false))
  if($mode -eq 'ed2k'){
   [void][IO.Directory]::CreateDirectory((Join-Path $profile 'rust-ed2k'))
   @{servers=@("127.0.0.1:$ed2kPort");kad=$false;loopbackOnly=$true}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $profile 'rust-ed2k/network.json') -Encoding utf8NoBOM
   [IO.File]::WriteAllText((Join-Path $profile 'fixture-url.txt'),'ed2k://|file|fixture.bin|3|A448017AAF21D8525FC10AE87AA6729D|/',[Text.UTF8Encoding]::new($false))
  }
  $connection=@{id='owned-downloader';type='TOOL';name='下载器连接';toolType='downloader';future=@{keep=42}}
  $connectionJson=ConvertTo-Json -InputObject @($connection) -Depth 5
  [IO.File]::WriteAllText($config,$connectionJson,[Text.UTF8Encoding]::new($false))
  $record=@{url='http://127.0.0.1:9/legacy.bin';protocol='HTTP';fileName='旧下载记录.bin';state='CANCELLED';status='已取消';rustId='owned-row';directory=$downloads;requestedThreads=3;totalBytes=123456;downloadedBytes=1000;future=@{keep=42}}
  @{version=1;tasks=@($record);future=@{keep=43};rustDownloaderSettings=@{directory=$downloads;threads=3;concurrent=1}}|ConvertTo-Json -Depth 7|Set-Content -LiteralPath $history -Encoding utf8NoBOM
  $configBefore=(Get-FileHash -LiteralPath $config).Hash;$historyBefore=(Get-FileHash -LiteralPath $history).Hash
  $env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP
  $env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_PREVIEW='downloader_saved';$env:TOMATO_DOWNLOADER_FIXTURE=$mode
  $env:TOMATO_UI_CAPTURE=Join-Path $probe ($mode+'.png');$env:TOMATO_UI_CAPTURE_FRAMES='1';$env:TOMATO_UI_CAPTURE_DELAY_MS=if($mode -eq 'download'){'10000'}else{'6000'};$env:WGPU_BACKEND='vulkan'
  $process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($mode+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($mode+'.stderr.log'))
  try{for($i=0;$i -lt 5 -and !$process.HasExited;$i++){[void]$process.WaitForExit(10000)};if(!$process.HasExited -or $process.ExitCode -ne 0){throw ('Downloader capture/exit failed: '+$mode)}}finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
  if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Missing native downloader framebuffer'}
  $proof=Get-Content -LiteralPath ([IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'downloader.json')) -Raw|ConvertFrom-Json
  if($proof.mode -ne $mode -or !$proof.historyLoaded -or !$proof.toolOpened -or $proof.initializationFailureShown -or $proof.osInputUsed){throw 'Downloader did not initialize through its native controls'}
  if($mode -ne 'catalog' -and !$proof.treeDoubleClicked){throw 'Saved tool was not opened through the tree'}
  if($mode -in @('add','settings') -and !$proof.nativeChild){throw 'Missing native downloader dialog'}
  if($configBefore -ne (Get-FileHash -LiteralPath $config).Hash){throw 'Opening downloader modified fixture connections'}
  if($mode -notin @('download','ed2k') -and $historyBefore -ne (Get-FileHash -LiteralPath $history).Hash){throw 'Opening downloader modified fixture history'}
  if($mode -in @('download','ed2k')){
   $expectedHash=if($mode -eq 'ed2k'){'BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD'}else{(Get-FileHash -LiteralPath $payload).Hash}
   if(!$proof.downloadCompleted -or (Get-FileHash -LiteralPath (Join-Path $downloads 'fixture.bin')).Hash -ne $expectedHash){throw 'Native download bytes differ'}
   $stored=Get-Content -LiteralPath $history -Raw|ConvertFrom-Json
   if($stored.tasks.Count -ne 2 -or $stored.future.keep -ne 43 -or @($stored.tasks|Where-Object {$_.rustId -eq 'owned-row' -and $_.future.keep -eq 42}).Count -ne 1){throw 'Native download lost legacy history/unknown fields'}
  }
  $results+=@{mode=$mode;path=$env:TOMATO_UI_CAPTURE;profile=$profile;normalExit=$true;historyUnchanged=($mode -notin @('download','ed2k'));connectionsUnchanged=$true;realHttpDownloadVerified=($mode -eq 'download');realEd2kDownloadVerified=($mode -eq 'ed2k');proof=$proof}
 }
 if($sharedBefore -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared connections changed'}
 $ed2kEvidence=Get-Content -LiteralPath $ed2kProof -Raw|ConvertFrom-Json
 if($ed2kEvidence.errors.Count -or $ed2kEvidence.bytesSent -ne 3 -or $ed2kEvidence.peerHellos -lt 1 -or $ed2kEvidence.sourceQueries -lt 1 -or $ed2kEvidence.active -ne 0){throw 'eD2k protocol or socket-release verification failed'}
 @{guiSha256=$hash;captures=$results;savedHostsContacted=$false;sharedConfigurationUnchanged=$true;osInputUsed=$false;ed2kProtocol=$ed2kEvidence;realHttpDownloadCoveredBy='app/tests/tool_migration.rs; both saved and catalog control flows';javaFullPixelParityClaimed=$false}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8NoBOM
 Get-Content -LiteralPath (Join-Path $probe 'validation.json') -TotalCount 10
}finally{foreach($owned in @($server,$ed2kServer)){if($null -ne $owned){if(!$owned.HasExited){$owned.Kill()};$owned.Dispose()}};foreach($key in $keys){if($null -eq $previous[$key]){[Environment]::SetEnvironmentVariable($key,[NullString]::Value,'Process')}else{[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}}
