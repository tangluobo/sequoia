param([Parameter(Mandatory=$true)][string]$Label,[switch]$Footer)
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
$env:TOMATO_UI_CAPTURE_FRAMES='4';$env:TOMATO_UI_PREVIEW=if($Footer){'sql_results_footer'}else{'sql_results'};$env:WGPU_BACKEND='vulkan'
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
if($Footer -and !$state.footer){throw 'Footer fixture was not rendered'}
Add-Type -AssemblyName System.Drawing
$image=[Drawing.Bitmap]::new($env:TOMATO_UI_CAPTURE)
try {
    foreach($sample in @(@(80,1,245),@(80,28,204),@(80,56,224),@(80,84,224),@(300,15,255),@(300,100,255),@(699,100,244),@(15,10,244),@(15,40,244),@(87,40,224),@(279,40,224),@(5,56,255),@(5,27,204),@(5,28,255))){
        $pixel=$image.GetPixel($sample[0],$sample[1])
        if($pixel.R -ne $sample[2] -or $pixel.G -ne $sample[2] -or $pixel.B -ne $sample[2]){throw ('Styled grid pixel differs at '+$sample[0]+','+$sample[1])}
    }
    foreach($point in @(@(0,56),@(14,56),@(5,55),@(5,83))){$pixel=$image.GetPixel($point[0],$point[1]);if($pixel.R -ne 190 -or $pixel.G -ne 190 -or $pixel.B -ne 188){throw ('Original frozen selector pixel differs at '+$point[0]+','+$point[1])}}
    if($Footer){foreach($sample in @(@(300,402,215),@(300,403,243),@(300,428,215),@(300,429,250),@(300,449,224))){$pixel=$image.GetPixel($sample[0],$sample[1]);if($pixel.R -ne $sample[2] -or $pixel.G -ne $sample[2] -or $pixel.B -ne $sample[2]){throw ('Original footer pixel differs at '+$sample[0]+','+$sample[1])}}}
}finally{$image.Dispose()}
if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared configuration changed'}
[pscustomobject]@{label=$Label;guiSha256=$sha;state=$state;nativeChildViewport=$true;realDatabaseActor=$true;sharedConfigurationUnchanged=$true;systemClipboardWritten=$false;allVisualInteractionsVerified=$false}|ConvertTo-Json -Depth 6|Set-Content (Join-Path $workspace ('target/validation-visual-'+$Label+'.json'))
Get-Content (Join-Path $workspace ('target/validation-visual-'+$Label+'.json'))
