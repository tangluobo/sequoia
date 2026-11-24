param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=(Resolve-Path -LiteralPath $Executable).Path
$sha=(Get-FileHash -LiteralPath $source).Hash
$exe=Join-Path $probe 'tomato.exe'
Copy-Item -LiteralPath $source -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $sha -or (Get-FileHash -LiteralPath $source).Hash -ne $sha){throw 'GUI changed while snapshotting'}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json'
$before=(Get-FileHash -LiteralPath $shared).Hash
$saved=@{TEMP=$env:TEMP;TMP=$env:TMP;DATA=$env:TOMATO_DATA_DIR;CAPTURE=$env:TOMATO_UI_CAPTURE;PREVIEW=$env:TOMATO_UI_PREVIEW;SELECTION=$env:TOMATO_EXPORT_SELECTION;FRAMES=$env:TOMATO_UI_CAPTURE_FRAMES}
$captures=@()
try{
    foreach($mode in @('empty','partial')){
        $profile=Join-Path $probe ('tomato-export-fixture-'+[guid]::NewGuid().ToString('N'))
        [void][IO.Directory]::CreateDirectory($profile)
        $env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP
        $env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_PREVIEW='export';$env:TOMATO_EXPORT_SELECTION=$mode;$env:TOMATO_UI_CAPTURE_FRAMES='1'
        $env:TOMATO_UI_CAPTURE=Join-Path $probe ($mode+'.png')
        $process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($mode+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($mode+'.stderr.log'))
        try{
            for($attempt=0;$attempt -lt 3 -and !$process.HasExited;$attempt++){[void]$process.WaitForExit(30000)}
            if(!$process.HasExited -or $process.ExitCode -ne 0){throw 'Own export viewport failed'}
        }finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
        if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Missing native framebuffer'}
        Add-Type -AssemblyName System.Drawing
        $image=[Drawing.Bitmap]::new($env:TOMATO_UI_CAPTURE)
        try{if($image.Width -ne 500 -or $image.Height -ne 450){throw 'Wrong export viewport size'}}finally{$image.Dispose()}
        $captures+=[pscustomobject]@{mode=$mode;path=$env:TOMATO_UI_CAPTURE;sha256=(Get-FileHash -LiteralPath $env:TOMATO_UI_CAPTURE).Hash}
    }
    if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared configuration changed'}
    [pscustomobject]@{passed=$true;label=$Label;guiSha256=$sha;captures=$captures;nativeChildViewports=$true;sharedConfigurationUnchanged=$true;savedHostsContacted=$false;physicalOsInputUsed=$false;fullJavaVisualParity=$false;modelImageReviewStillRequired=$true}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $probe 'validation.json')
}finally{
    $env:TEMP=$saved.TEMP;$env:TMP=$saved.TMP;$env:TOMATO_DATA_DIR=$saved.DATA;$env:TOMATO_UI_CAPTURE=$saved.CAPTURE;$env:TOMATO_UI_PREVIEW=$saved.PREVIEW;$env:TOMATO_EXPORT_SELECTION=$saved.SELECTION;$env:TOMATO_UI_CAPTURE_FRAMES=$saved.FRAMES
}
