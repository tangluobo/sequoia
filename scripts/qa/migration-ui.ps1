param([Parameter(Mandatory=$true)][string]$Executable,[Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$FixtureLabel)
$ErrorActionPreference='Stop'
if($Label -notmatch '^[a-z0-9-]+$' -or $FixtureLabel -notmatch '^[a-z0-9-]+$'){throw 'Invalid owned fixture label'}
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$root=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $root){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($root)
$program=(Resolve-Path -LiteralPath $Executable).Path
if(!$program.StartsWith((Join-Path $workspace 'target/'),[StringComparison]::OrdinalIgnoreCase)){throw 'Expected owned candidate'}
$sha=(Get-FileHash -LiteralPath $program).Hash
$profile=Join-Path $root ('tomato-cloud-mq-fixture-'+[guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($profile)
[IO.File]::WriteAllText((Join-Path $profile 'connections.json'),'{"connections":[],"groups":[]}',[Text.UTF8Encoding]::new($false))
$fixture=Get-Content -LiteralPath (Join-Path $workspace ('target/mq-fixtures/'+$FixtureLabel+'/identity.json')) -Raw|ConvertFrom-Json
@{kafka=$fixture.kafka;rocketmq=$fixture.rocketmq}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $profile 'ready.json') -Encoding utf8NoBOM
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$keys=@('TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','WGPU_BACKEND','TEMP','TMP');$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$results=@()
try {
 $env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_CAPTURE_FRAMES='4';$env:TOMATO_UI_CAPTURE_DELAY_MS='2500';$env:WGPU_BACKEND='vulkan';$env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP
 foreach($case in @('table_config','kafka_groups','rocketmq_groups','kafka_batch','rocketmq_batch')) {
  $env:TOMATO_UI_PREVIEW=$case;$env:TOMATO_UI_CAPTURE=Join-Path $root ($case+'.png')
  $process=Start-Process -FilePath $program -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $root ($case+'.stdout.log')) -RedirectStandardError (Join-Path $root ($case+'.stderr.log'))
  try {
   if(!$process.WaitForExit(60000)){throw ('Own framebuffer capture timed out: '+$case)}
   if($process.ExitCode -ne 0){throw ('Own framebuffer capture failed: '+$case)}
  }finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
  $image=Join-Path $root ($case+'-03.png');if(!(Test-Path -LiteralPath $image)){throw ('Missing own framebuffer: '+$case)}
  $results+=@{case=$case;image=$image;sha256=(Get-FileHash -LiteralPath $image).Hash;normalExit=$true;syntheticBatchRows=$case.EndsWith('_batch')}
 }
 if($sha -ne (Get-FileHash -LiteralPath $program).Hash){throw 'Candidate changed during capture'}
 if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared configuration changed'}
 @{passed=$true;guiSha256=$sha;captures=$results;sharedConfigurationUnchanged=$true;savedHostsContacted=$false;systemClipboardWritten=$false;fullJavaVisualParityClaimed=$false}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $root 'validation.json') -Encoding utf8
}finally{foreach($key in $keys){[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}
