param([Parameter(Mandatory=$true)][string]$Label,[ValidateSet('prompt','settings')][string]$Kind='prompt')
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=Join-Path $workspace 'target/debug/tomato.exe';$sha=(Get-FileHash -LiteralPath $source).Hash
$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $sha -or (Get-FileHash -LiteralPath $source).Hash -ne $sha){throw 'GUI changed while snapshotting'}
$profile=Join-Path $workspace ('target/tomato-ai-sql-fixture-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($profile)
$env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP;$env:TOMATO_DATA_DIR=$profile
$env:TOMATO_UI_CAPTURE=Join-Path $workspace ('target/ui-sql-ai-'+$Label+'.png');$env:TOMATO_UI_CAPTURE_FRAMES='4';$env:TOMATO_UI_PREVIEW='ai_sql_'+$Kind;$env:WGPU_BACKEND='vulkan'
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$sharedGlobal=Join-Path $env:USERPROFILE '.tomato/global.json';$globalBefore=if(Test-Path -LiteralPath $sharedGlobal){(Get-FileHash -LiteralPath $sharedGlobal).Hash}else{$null}
$process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe 'stdout.log') -RedirectStandardError (Join-Path $probe 'stderr.log')
try{for($attempt=0;$attempt -lt 3 -and !$process.HasExited;$attempt++){[void]$process.WaitForExit(30000)};if(!$process.HasExited){throw 'Own fixture timed out'};if($process.ExitCode -ne 0){throw 'Own fixture failed'}}finally{if(!$process.HasExited){$process.Kill()}}
if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Own framebuffer missing'}
$state=Get-Content ([IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'dialog.json')) -Raw|ConvertFrom-Json
$height=if($Kind -eq 'settings'){292}else{212}
if(!$state.ready -or $state.clientWidth -ne 560 -or $state.clientHeight -ne $height -or $state.okX -ne 390 -or $state.okY -ne ($height-33) -or $state.httpRequested -or $state.settingsSaved){throw 'Original AI dialog geometry or isolation differs'}
if($state.okWidth -ne 75 -or $state.okHeight -ne 24){throw 'Original dialog button size differs'}
$expected=[Collections.Generic.List[double[]]]::new();if($Kind -eq 'settings'){foreach($bounds in @(@(78,47,474,23),@(78,82,474,23),@(78,117,474,23),@(78,152,71,23))){$expected.Add([double[]]$bounds)}}else{$expected.Add([double[]]@(4,43,552,82))}
if($state.fields.Count -ne $expected.Count){throw 'Missing measured editor bounds'}
for($index=0;$index -lt $expected.Count;$index++){for($part=0;$part -lt 4;$part++){if([Math]::Abs($state.fields[$index][$part]-$expected[$index][$part]) -gt 0.01){throw ('Original editor geometry differs: '+$index)}}}
Add-Type -AssemblyName System.Drawing;$image=[Drawing.Bitmap]::new($env:TOMATO_UI_CAPTURE)
try{if($image.Width -ne 560 -or $image.Height -ne $height){throw 'Wrong viewport captured'};foreach($point in @(@(400,38,200),@(400,39,244),@(10,($height-1),244))){$pixel=$image.GetPixel($point[0],$point[1]);if([Math]::Abs($pixel.R-$point[2]) -gt 1 -or $pixel.R -ne $pixel.G -or $pixel.G -ne $pixel.B){throw 'Original AI dialog background differs'}}}finally{$image.Dispose()}
if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared config changed'}
$globalAfter=if(Test-Path -LiteralPath $sharedGlobal){(Get-FileHash -LiteralPath $sharedGlobal).Hash}else{$null};if($globalBefore -ne $globalAfter){throw 'Shared global settings changed'}
if(Test-Path -LiteralPath (Join-Path $profile 'global.json')){throw 'Fixture unexpectedly saved settings'}
[pscustomobject]@{label=$Label;guiSha256=$sha;kind=$Kind;state=$state;nativeChildViewport=$true;sharedConfigurationUnchanged=$true;sharedGlobalConfigurationUnchanged=$true;httpRequested=$false;systemClipboardWritten=$false;allVisualInteractionsVerified=$false}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $workspace ('target/validation-visual-'+$Label+'.json'))
Get-Content (Join-Path $workspace ('target/validation-visual-'+$Label+'.json'))
