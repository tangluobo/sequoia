param([Parameter(Mandatory=$true)][string]$Label,[int[]]$Pages=0..3)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=Join-Path $workspace 'target/debug/tomato.exe';$sha=(Get-FileHash -LiteralPath $source).Hash
$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $sha -or (Get-FileHash -LiteralPath $source).Hash -ne $sha){throw 'GUI changed while snapshotting'}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$captures=@()
foreach($page in $Pages){
    $profile=Join-Path $workspace ('target/tomato-admin-fixture-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($profile)
    $env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP;$env:TOMATO_DATA_DIR=$profile
    $env:TOMATO_UI_CAPTURE=Join-Path $workspace ('target/ui-database-admin-'+$Label+'-'+$page+'.png')
    $env:TOMATO_UI_CAPTURE_FRAMES='1';$env:TOMATO_UI_PREVIEW='database_admin';$env:TOMATO_ADMIN_MODE=if($page -ge 2){'edit'}else{'create'};$env:TOMATO_ADMIN_PAGE=[string]($page % 2);$env:WGPU_BACKEND='vulkan'
    $process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($page.ToString()+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($page.ToString()+'.stderr.log'))
    try{for($attempt=0;$attempt -lt 3 -and !$process.HasExited;$attempt++){[void]$process.WaitForExit(30000)};if(!$process.HasExited){throw 'Own fixture timed out'};if($process.ExitCode -ne 0){throw 'Own fixture failed'}}finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
    if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Own framebuffer missing'}
    $state=Get-Content ([IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'state.json')) -Raw|ConvertFrom-Json
    $expectedWidth=if($page -ge 2){641}else{450};$expectedHeight=if($page -ge 2){284}else{289}
    if(!$state.ready -or $state.clientWidth -ne $expectedWidth -or $state.clientHeight -ne $expectedHeight -or $state.databaseRequested -or $state.systemClipboardWritten){throw 'Transfer fixture differs'}
    Add-Type -AssemblyName System.Drawing;$image=[Drawing.Bitmap]::new($env:TOMATO_UI_CAPTURE)
    try{if($image.Width -ne $expectedWidth -or $image.Height -ne $expectedHeight){throw 'Wrong viewport captured'}}finally{$image.Dispose()}
    $captures+=[pscustomobject]@{page=$page;path=$env:TOMATO_UI_CAPTURE;sha256=(Get-FileHash -LiteralPath $env:TOMATO_UI_CAPTURE).Hash}
}
if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared config changed'}
[pscustomobject]@{label=$Label;guiSha256=$sha;nativeChildViewports=$true;captures=$captures;sharedConfigurationUnchanged=$true;savedHostsContacted=$false;allVisualInteractionsVerified=$false}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $probe 'validation.json')
Get-Content (Join-Path $probe 'validation.json')
