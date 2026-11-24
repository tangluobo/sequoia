param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference='Stop';$global:LASTEXITCODE=0
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label);if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'};[void][IO.Directory]::CreateDirectory($probe)
$source=[IO.Path]::GetFullPath($Executable);$sha=(Get-FileHash -LiteralPath $source).Hash;$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $sha -or (Get-FileHash -LiteralPath $source).Hash -ne $sha){throw 'GUI changed during snapshot'}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=if(Test-Path -LiteralPath $shared){(Get-FileHash -LiteralPath $shared).Hash}else{$null}
$names=@('TOMATO_DATA_DIR','TOMATO_UI_CAPTURE','TOMATO_UI_PREVIEW','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_UI_CAPTURE_FRAMES','TOMATO_TREE_RENAME_TARGET','WGPU_BACKEND','TEMP','TMP');$saved=@{};foreach($name in $names){$saved[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
$captures=@()
try{
    foreach($target in @('文件.md','文件夹','测试表','测试视图')){
        $profile=Join-Path $probe ('tomato-tree-rename-fixture-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory((Join-Path $profile 'files/文件夹'));[IO.File]::WriteAllText((Join-Path $profile 'files/文件.md'),'# retained fixture bytes')
        $env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_PREVIEW='tree_rename';$env:TOMATO_TREE_RENAME_TARGET=$target;$env:TOMATO_UI_CAPTURE_DELAY_MS='2400';$env:TOMATO_UI_CAPTURE_FRAMES='1';$env:TOMATO_UI_CAPTURE=Join-Path $probe ($captures.Count.ToString()+'.png')
        $env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP;[Environment]::SetEnvironmentVariable('WGPU_BACKEND',[NullString]::Value,'Process')
        $process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($captures.Count.ToString()+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($captures.Count.ToString()+'.stderr.log'))
        try{for($attempt=0;$attempt -lt 3 -and !$process.HasExited;$attempt++){[void]$process.WaitForExit(30000)};if(!$process.HasExited){throw 'Own tree fixture timed out'};$process.Refresh();if($process.ExitCode -ne 0){throw 'Own tree fixture failed'}}finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
        if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Native tree framebuffer missing'}
        $tracePath=[IO.Path]::ChangeExtension($env:TOMATO_UI_CAPTURE,'rename.json');$trace=Get-Content -LiteralPath $tracePath -Raw|ConvertFrom-Json
        if(!$trace.inline -or !$trace.focused -or $trace.popup -or $trace.fieldHeight -ne 26 -or $trace.rowHeight -ne 35 -or $trace.osInputUsed -or $trace.syntheticPointerEvents -ne 4 -or $trace.targetLabel -ne $target){throw 'Native tree rename control differs'}
        if(Test-Path -LiteralPath (Join-Path $profile 'connections.json')){throw 'Preview must not persist fixture configuration'}
        Add-Type -AssemblyName System.Drawing;$image=[Drawing.Bitmap]::new($env:TOMATO_UI_CAPTURE);try{$size=@($image.Width,$image.Height);if($image.Width -lt 800 -or $image.Height -lt 600){throw 'Native framebuffer too small'}}finally{$image.Dispose()}
        $captures+=[pscustomobject]@{target=$target;path=$env:TOMATO_UI_CAPTURE;trace=$tracePath;size=$size;sha256=(Get-FileHash -LiteralPath $env:TOMATO_UI_CAPTURE).Hash;normalExit=$true}
    }
    $after=if(Test-Path -LiteralPath $shared){(Get-FileHash -LiteralPath $shared).Hash}else{$null};if($before -ne $after){throw 'Shared configuration changed'}
    [pscustomobject]@{label=$Label;guiSha256=$sha;captures=$captures;nativeFramebuffer=$true;actualTreeControls=$true;osInputUsed=$false;savedHostsContacted=$false;sharedConfigurationUnchanged=$true;javaPixelParityClaimed=$false}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8NoBOM
    Get-Content -LiteralPath (Join-Path $probe 'validation.json')
}finally{foreach($name in $names){if($null -eq $saved[$name]){[Environment]::SetEnvironmentVariable($name,[NullString]::Value,'Process')}else{[Environment]::SetEnvironmentVariable($name,$saved[$name],'Process')}}}
