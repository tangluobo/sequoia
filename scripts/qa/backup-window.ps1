param([Parameter(Mandatory=$true)][string]$Label,[ValidateRange(0,3)][int]$Tab=0,[switch]$Restore,[switch]$Question,[ValidateSet('complete','error')][string]$Alert,[string]$Executable)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
if($Restore -and $Tab -gt 1){throw 'Restore has two tabs'}
if($Question -and $Tab -ne 0){throw 'Confirmation has no tabs'}
if($Alert -and ($Question -or $Restore -or $Tab -ne 0)){throw 'Alert preview must be used on its own'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=if($Executable){(Resolve-Path -LiteralPath $Executable).Path}else{Join-Path $workspace 'target/debug/tomato.exe'};$sha=(Get-FileHash -LiteralPath $source).Hash
$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $sha -or (Get-FileHash -LiteralPath $source).Hash -ne $sha){throw 'GUI changed while snapshotting'}
$profile=Join-Path $workspace ('target/tomato-backup-fixture-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($profile)
$env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP;$env:TOMATO_DATA_DIR=$profile
$env:TOMATO_UI_CAPTURE=Join-Path $workspace ('target/ui-backup-'+$Label+'.png');$env:TOMATO_UI_CAPTURE_FRAMES='4';$env:TOMATO_UI_PREVIEW=if($Alert){'backup_'+$Alert}elseif($Question){'restore_confirm'}elseif($Restore){'restore'}else{'backup'};$env:TOMATO_UI_BACKUP_TAB=[string]$Tab;$env:WGPU_BACKEND='vulkan'
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe 'stdout.log') -RedirectStandardError (Join-Path $probe 'stderr.log')
try{for($attempt=0;$attempt -lt 3 -and !$process.HasExited;$attempt++){[void]$process.WaitForExit(30000)};if(!$process.HasExited){throw 'Own fixture timed out'};if($process.ExitCode -ne 0){throw 'Own fixture failed'}}finally{if(!$process.HasExited){$process.Kill()}}
if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Own framebuffer missing'}
$state=Get-Content ([IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'state.json')) -Raw|ConvertFrom-Json
$width=if($Alert){418}elseif($Question){360}elseif($Restore){620}else{720};$height=if($Alert){101}elseif($Question){142}elseif($Restore){480}else{580}
if(!$state.ready -or $state.clientWidth -ne $width -or $state.clientHeight -ne $height -or $state.tab -ne $Tab -or $state.objectCount -ne 5 -or $state.selectedCount -ne 0 -or $state.databaseRequested -or $state.systemClipboardWritten){throw 'Backup fixture differs'}
$expected=if($Question){@(@(190,109,75,23),@(275,109,75,23))}elseif($Restore){@(@(420,442,100,28),@(530,442,80,28))}else{@(@(530,542,90,28),@(630,542,80,28))}
if($Alert){$expected=,@(333,68,75,23)}
if($state.buttons.Count -ne $expected.Count){throw 'Unexpected dialog button count'}
for($index=0;$index -lt $expected.Count;$index++){for($axis=0;$axis -lt 4;$axis++){if([Math]::Abs($state.buttons[$index][$axis]-$expected[$index][$axis]) -gt .01){throw 'Original bottom button geometry differs'}}}
Add-Type -AssemblyName System.Drawing;$image=[Drawing.Bitmap]::new($env:TOMATO_UI_CAPTURE)
try{if($image.Width -ne $width -or $image.Height -ne $height){throw 'Wrong viewport captured'}}finally{$image.Dispose()}
if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared config changed'}
[pscustomobject]@{label=$Label;guiSha256=$sha;state=$state;nativeChildViewport=$true;sharedConfigurationUnchanged=$true;savedHostsContacted=$false;allVisualInteractionsVerified=$false}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $workspace ('target/validation-visual-'+$Label+'.json'))
Get-Content (Join-Path $workspace ('target/validation-visual-'+$Label+'.json'))
