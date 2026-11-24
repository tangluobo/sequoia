param([Parameter(Mandatory=$true)][string]$Label)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid probe label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=Join-Path $workspace 'target/debug/tomato.exe'
$sha=(Get-FileHash -LiteralPath $source).Hash
$exe=Join-Path $probe 'tomato.exe'
Copy-Item -LiteralPath $source -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $sha -or (Get-FileHash -LiteralPath $source).Hash -ne $sha){throw 'GUI changed while snapshotting'}
$profile=Join-Path $workspace ('target/tomato-sql-results-fixture-'+[guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($profile)
$env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP
$env:TOMATO_DATA_DIR=$profile
$env:TOMATO_UI_CAPTURE=Join-Path $workspace ('target/ui-sql-results-'+$Label+'.png')
$env:TOMATO_UI_CAPTURE_FRAMES='4';$env:TOMATO_UI_PREVIEW='sql_results_filter';$env:WGPU_BACKEND='vulkan'
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json'
$before=(Get-FileHash -LiteralPath $shared).Hash
$process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe 'stdout.log') -RedirectStandardError (Join-Path $probe 'stderr.log')
try {
    for($attempt=0;$attempt -lt 3 -and !$process.HasExited;$attempt++){[void]$process.WaitForExit(30000)}
    if(!$process.HasExited){throw 'Own result window timed out'}
    if($process.ExitCode -ne 0){Get-Content (Join-Path $probe 'stderr.log') -Tail 15;throw 'Native result window failed'}
}finally{if(!$process.HasExited){$process.Kill($true);[void]$process.WaitForExit(10000)};$process.Dispose()}
$state=Get-Content -LiteralPath ([IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'state.json')) -Raw|ConvertFrom-Json
if(!$state.ready -or $state.rows -ne 2000 -or $state.clientWidth -ne 700 -or $state.clientHeight -ne 450){throw 'Native result geometry or real row count differs'}
if(!$state.nullFirst -or $state.emptySecond -ne '' -or $state.literalThird -ne 'NULL' -or $state.lastValue -ne '第 1999 行'){throw 'Native actor lost nullable result values'}
if(($state.columnWidths -join ',') -ne '72,60,72,60'){throw 'Column widths differ from the original control'}
$dialog=Get-Content -LiteralPath ([IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'dialog.json')) -Raw|ConvertFrom-Json
if(!$dialog.ready -or $dialog.clientWidth -ne 352 -or $dialog.clientHeight -ne 154 -or $dialog.inputX -ne 181 -or $dialog.inputY -ne 78 -or $dialog.inputWidth -ne 161 -or $dialog.inputHeight -ne 23 -or $dialog.okX -ne 182 -or $dialog.okY -ne 121){throw 'Original native filter dialog geometry differs'}
if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Own filter framebuffer was not captured'}
Add-Type -AssemblyName System.Drawing
$image=[Drawing.Bitmap]::new($env:TOMATO_UI_CAPTURE)
try {
    if($image.Width -ne 352 -or $image.Height -ne 154){throw 'Captured wrong viewport'}
    foreach($point in @(@(200,0,244),@(200,67,200),@(200,68,244),@(200,110,244))){$pixel=$image.GetPixel($point[0],$point[1]);if([Math]::Abs($pixel.R-$point[2]) -gt 1 -or $pixel.R -ne $pixel.G -or $pixel.G -ne $pixel.B){throw ('Original filter dialog pixel differs at '+$point[0]+','+$point[1])}}
}finally{$image.Dispose()}
if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared configuration changed'}
[pscustomobject]@{label=$Label;guiSha256=$sha;state=$state;dialog=$dialog;nativeChildViewport=$true;realDatabaseActor=$true;sharedConfigurationUnchanged=$true;systemClipboardWritten=$false;allVisualInteractionsVerified=$false}|ConvertTo-Json -Depth 6|Set-Content (Join-Path $workspace ('target/validation-visual-'+$Label+'.json'))
Get-Content (Join-Path $workspace ('target/validation-visual-'+$Label+'.json'))
