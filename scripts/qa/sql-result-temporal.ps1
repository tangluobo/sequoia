param([Parameter(Mandatory=$true)][string]$Label,[ValidateSet("date","time","datetime")][string]$Kind="datetime",[string]$Executable)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid probe label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=if($Executable){(Resolve-Path -LiteralPath $Executable).Path}else{Join-Path $workspace 'target/debug/tomato.exe'}
$sha=(Get-FileHash -LiteralPath $source).Hash
$exe=Join-Path $probe 'tomato.exe'
Copy-Item -LiteralPath $source -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $sha -or (Get-FileHash -LiteralPath $source).Hash -ne $sha){throw 'GUI changed while snapshotting'}
$profile=Join-Path $workspace ('target/tomato-sql-results-fixture-'+[guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($profile)
$keys=@('TEMP','TMP','TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','WGPU_BACKEND')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
try {
$env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP
$env:TOMATO_DATA_DIR=$profile
$env:TOMATO_UI_CAPTURE=Join-Path $workspace ('target/ui-sql-results-'+$Label+'.png')
$env:TOMATO_UI_CAPTURE_FRAMES='4';$env:TOMATO_UI_PREVIEW=('sql_results_'+$Kind);$env:WGPU_BACKEND='vulkan'
$env:TOMATO_UI_CAPTURE_DELAY_MS='5000'
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
$dialog=Get-Content -LiteralPath ([IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'temporal.json')) -Raw|ConvertFrom-Json
$expectedHeight=switch($Kind){'date'{224};'time'{83};default{262}}
if(!$dialog.ready -or $dialog.clientWidth -ne 260 -or $dialog.clientHeight -ne $expectedHeight -or $dialog.okX -ne 209 -or $dialog.okY -ne ($expectedHeight-34)){throw 'Original temporal popup geometry differs'}
if($Kind -ne 'time' -and $dialog.date -ne '2024-02-29'){throw 'Original selected date differs'}
if($Kind -ne 'date' -and ($dialog.time -join ',') -ne '12,34,56'){throw 'Original time parser differs'}
if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Own temporal framebuffer was not captured'}
Add-Type -AssemblyName System.Drawing
$image=[Drawing.Bitmap]::new($env:TOMATO_UI_CAPTURE)
try {
    if($image.Width -ne 260 -or $image.Height -ne $expectedHeight){throw 'Captured wrong temporal viewport'}
    foreach($sample in @(@(0,0,153),@(5,5,255))){$pixel=$image.GetPixel($sample[0],$sample[1]);if($pixel.R -ne $sample[2] -or $pixel.G -ne $sample[2] -or $pixel.B -ne $sample[2]){throw 'Original temporal panel border or fill differs'}}
    if($Kind -ne 'time'){$pixel=$image.GetPixel(150,159);if($pixel.R -ne 53 -or $pixel.G -ne 146 -or $pixel.B -ne 203){throw 'Original selected calendar day fill differs'}}
}finally{$image.Dispose()}
if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared configuration changed'}
[pscustomobject]@{label=$Label;guiSha256=$sha;state=$state;dialog=$dialog;injectedEditableMetadata=$true;nativeChildViewport=$true;realDatabaseActor=$true;sharedConfigurationUnchanged=$true;systemClipboardWritten=$false;allVisualInteractionsVerified=$false}|ConvertTo-Json -Depth 6|Set-Content (Join-Path $workspace ('target/validation-visual-'+$Label+'.json'))
Get-Content (Join-Path $workspace ('target/validation-visual-'+$Label+'.json'))
} finally {foreach($key in $keys){if($null -eq $previous[$key]){[Environment]::SetEnvironmentVariable($key,[NullString]::Value,'Process')}else{[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}}
