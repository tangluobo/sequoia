param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Executable,[ValidateSet('menu','session','global','copy','fonts','docker','docker_tree','storage','storage_error','storage_form','ports')][string[]]$Modes=@('menu','session','global','copy'),[string]$Harness)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($probe)
$gui=(Resolve-Path -LiteralPath $Executable).Path
if($Harness){$harness=(Resolve-Path -LiteralPath $Harness).Path}else{$harness=(Get-ChildItem -LiteralPath (Join-Path $workspace 'target/qa-active-200/debug/deps') -Filter 'ssh_workspace-*.exe'|Sort-Object LastWriteTime -Descending|Select-Object -First 1).FullName}
if(!$harness){throw 'Build the workspace tests first'}
$guiHash=(Get-FileHash -LiteralPath $gui).Hash
$harnessHash=(Get-FileHash -LiteralPath $harness).Hash
$copy=Join-Path $probe 'tomato.exe';$test=Join-Path $probe 'ssh_workspace.exe'
Copy-Item -LiteralPath $gui -Destination $copy
Copy-Item -LiteralPath $harness -Destination $test
if((Get-FileHash -LiteralPath $copy).Hash -ne $guiHash -or (Get-FileHash -LiteralPath $test).Hash -ne $harnessHash){throw 'Snapshot changed'}
$keys=@('TEMP','TMP','TOMATO_SSH_CAPTURE_EXE','TOMATO_SSH_CAPTURE_PATH','TOMATO_SSH_CAPTURE_HARNESS','TOMATO_SSH_TAB_FIXTURE','TOMATO_TEST_ASSERT_CURSOR_BLINK','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_UI_CAPTURE_FRAMES','WGPU_BACKEND')
$old=@{};foreach($key in $keys){$old[$key]=[Environment]::GetEnvironmentVariable($key)}
$protected=@('connections.json','global.json','development.json','ddns.json')|ForEach-Object{Join-Path $env:USERPROFILE ('.tomato/'+$_)}
$before=@($protected|ForEach-Object{@{path=$_;sha256=(Get-FileHash -LiteralPath $_).Hash}})
$results=@()
try {
 $env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP
 $env:TOMATO_SSH_CAPTURE_EXE=$copy;$env:TOMATO_SSH_CAPTURE_HARNESS=$null;$env:TOMATO_TEST_ASSERT_CURSOR_BLINK=$null
 $env:TOMATO_UI_CAPTURE_DELAY_MS='8000';$env:TOMATO_UI_CAPTURE_FRAMES='1';$env:WGPU_BACKEND=$null
 foreach($mode in $Modes){
  $env:TOMATO_UI_CAPTURE_DELAY_MS=if($mode.StartsWith('storage')){'16000'}else{'8000'}
  $env:TOMATO_SSH_TAB_FIXTURE=$mode;$env:TOMATO_SSH_CAPTURE_PATH=Join-Path $probe ($mode+'.png')
  & $test capture_native_and_original_ssh_tools --exact --ignored --nocapture *> (Join-Path $probe ($mode+'.log'))
  if($LASTEXITCODE){Get-Content (Join-Path $probe ($mode+'.log')) -Tail 20;throw ('Native SSH fixture failed: '+$mode)}
  $proof=Get-Content (Join-Path $probe ($mode+'.ssh-tab.json')) -Raw|ConvertFrom-Json
  if(!$proof.sessionReady -or $proof.osInputUsed -or $proof.mode -ne $mode){throw 'Missing connected terminal/UI proof'}
  if($mode -eq 'ports'){
   $portTitle=@($proof.texts|Where-Object text -eq '端口监听')
   $portHeader=@($proof.texts|Where-Object text -eq '协议')
   $portStatus=@($proof.texts|Where-Object text -eq '共 1 个监听端口（总计 2）')
   if($portTitle.Count -ne 1 -or $portHeader.Count -ne 1 -or $portStatus.Count -ne 1){throw 'Original port title/header/filtered status missing'}
   $portHeaderTop=($portHeader[0].y+$portHeader[0].height/2)-($portTitle[0].y+$portTitle[0].height/2)+3
   if([Math]::Abs($portHeaderTop-53) -gt 0.75){throw ('Port header differs from original Java y=53: '+$portHeaderTop)}
  }
  if($mode -eq 'fonts'){
   $fontRows=@($proof.textRows|Where-Object{$_.text -match '(?i)mono|consol|courier|menlo|dejavu|liberation|sarasa|cascadia|jetbrains'})
   if(!$proof.fontArrow -or $fontRows.Count -lt 3 -or @($fontRows|Where-Object{$_.rows -ne 1}).Count){throw 'Font popup did not open with single-line installed font names'}
  }
  $results+=@{mode=$mode;path=$env:TOMATO_SSH_CAPTURE_PATH;sha256=(Get-FileHash -LiteralPath $env:TOMATO_SSH_CAPTURE_PATH).Hash;sessionReady=$proof.sessionReady;exitCode=0}
 }
 foreach($entry in $before){if((Get-FileHash -LiteralPath $entry.path).Hash -ne $entry.sha256){throw 'Shared configuration changed'}}
 $result=@{guiSha256=$guiHash;harnessSha256=$harnessHash;scenarios=$results;realLoopbackSsh=$true;sharedConfigurationUnchanged=$true;savedHostsContacted=$false;osInputUsed=$false}
 $result|ConvertTo-Json -Depth 5|Set-Content (Join-Path $probe 'validation.json')
 $result|ConvertTo-Json -Depth 5
} finally {foreach($key in $keys){[Environment]::SetEnvironmentVariable($key,$old[$key])}}
