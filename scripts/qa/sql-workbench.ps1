param([Parameter(Mandatory=$true)][string]$Label)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=Join-Path $workspace 'target/debug/tomato.exe';$sha=(Get-FileHash -LiteralPath $source).Hash
$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $sha -or (Get-FileHash -LiteralPath $source).Hash -ne $sha){throw 'GUI changed while snapshotting'}
$profile=Join-Path $workspace ('target/tomato-sql-workbench-fixture-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($profile)
$env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP;$env:TOMATO_DATA_DIR=$profile
$env:TOMATO_UI_CAPTURE=Join-Path $workspace ('target/ui-sql-workbench-'+$Label+'.png');$env:TOMATO_UI_CAPTURE_FRAMES='4';$env:TOMATO_UI_PREVIEW='sql_workbench';$env:WGPU_BACKEND='vulkan'
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe 'stdout.log') -RedirectStandardError (Join-Path $probe 'stderr.log')
try{for($attempt=0;$attempt -lt 3 -and !$process.HasExited;$attempt++){[void]$process.WaitForExit(30000)};if(!$process.HasExited){throw 'Own fixture timed out'};if($process.ExitCode -ne 0){throw 'Own fixture failed'}}finally{if(!$process.HasExited){$process.Kill()}}
if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Own framebuffer missing'}
$state=Get-Content ([IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'state.json')) -Raw|ConvertFrom-Json
if(!$state.ready -or $state.clientWidth -ne 896 -or $state.clientHeight -ne 700 -or $state.rowCount -ne 5 -or !$state.documentUnchanged -or $state.databaseRequested -or $state.systemClipboardWritten){throw 'Workbench fixture differs'}
if([Math]::Abs($state.textX-40) -gt .01 -or [Math]::Abs($state.textY-37) -gt .01 -or [Math]::Abs($state.divider-.6) -gt .001){throw 'Measured original editor geometry differs'}
Add-Type -AssemblyName System.Drawing;$image=[Drawing.Bitmap]::new($env:TOMATO_UI_CAPTURE)
try{if($image.Width -ne 896 -or $image.Height -ne 700){throw 'Wrong viewport captured'}}finally{$image.Dispose()}
if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared config changed'}
[pscustomobject]@{label=$Label;guiSha256=$sha;state=$state;nativeChildViewport=$true;sharedConfigurationUnchanged=$true;savedHostsContacted=$false;allVisualInteractionsVerified=$false}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $workspace ('target/validation-visual-'+$Label+'.json'))
Get-Content (Join-Path $workspace ('target/validation-visual-'+$Label+'.json'))
