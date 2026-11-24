param(
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-z0-9-]+$')][string]$Label,
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(Mandatory=$true)][string]$Harness
)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$target=[IO.Path]::GetFullPath((Join-Path $workspace 'target'))+[IO.Path]::DirectorySeparatorChar
$source=[IO.Path]::GetFullPath($Executable)
$fixture=[IO.Path]::GetFullPath($Harness)
if(!$source.StartsWith($target,[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($source) -ne 'tomato.exe'){throw 'Expected workspace GUI'}
if(!$fixture.StartsWith($target,[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($fixture) -notmatch '^object_storage-[a-f0-9]+\.exe$'){throw 'Expected owned object-storage harness'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($probe)
$gui=Join-Path $probe 'tomato.exe'
$runner=Join-Path $probe 'object-fixture.exe'
$guiHash=(Get-FileHash -LiteralPath $source).Hash
$harnessHash=(Get-FileHash -LiteralPath $fixture).Hash
Copy-Item -LiteralPath $source -Destination $gui
Copy-Item -LiteralPath $fixture -Destination $runner
if((Get-FileHash -LiteralPath $gui).Hash -ne $guiHash -or (Get-FileHash -LiteralPath $runner).Hash -ne $harnessHash){throw 'Binary changed while copying'}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json'
$before=(Get-FileHash -LiteralPath $shared).Hash
$keys=@('TOMATO_IMAGE_BROWSER_MENU','TOMATO_IMAGE_BROWSER_MARKDOWN','TOMATO_IMAGE_CAPTURE_EXE','TOMATO_IMAGE_CAPTURE_PATH','TOMATO_IMAGE_BROWSER_MODE','TOMATO_UI_CAPTURE_DELAY_MS')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$captures=@()
try {
    $env:TOMATO_IMAGE_CAPTURE_EXE=$gui
    $env:TOMATO_UI_CAPTURE_DELAY_MS='2500'
    foreach($scenario in @('menu','markdown')) { foreach($mode in @('ICON','LIST','COLUMN')) {
        $env:TOMATO_IMAGE_BROWSER_MENU=if($scenario -eq 'menu'){'1'}else{'0'}; $env:TOMATO_IMAGE_BROWSER_MARKDOWN=if($scenario -eq 'markdown'){'1'}else{'0'}
        $env:TOMATO_IMAGE_BROWSER_MODE=$mode
        $env:TOMATO_IMAGE_CAPTURE_PATH=Join-Path $probe ($scenario+'-'+$mode+'.png')
        & $runner @('images::capture::native_image_preview_reads_signed_object_and_preserves_old_configuration','--ignored','--exact','--nocapture') *> (Join-Path $probe ($scenario+'-'+$mode+'.log'))
        if($LASTEXITCODE -ne 0){throw "Native $mode file browser failed"}
        if(!(Test-Path -LiteralPath $env:TOMATO_IMAGE_CAPTURE_PATH)){throw 'Missing browser framebuffer'}
        $tracePath=[IO.Path]::ChangeExtension($env:TOMATO_IMAGE_CAPTURE_PATH,'browser.json'); $trace=Get-Content -LiteralPath $tracePath -Raw|ConvertFrom-Json; if(!$trace.ready -or $trace.osInputUsed){throw 'Native controls did not complete'}
        $captures+=@{scenario=$scenario;trace=$tracePath;mode=$mode;path=$env:TOMATO_IMAGE_CAPTURE_PATH;sha256=(Get-FileHash -LiteralPath $env:TOMATO_IMAGE_CAPTURE_PATH).Hash}
    }
    }
    $unchanged=(Get-FileHash -LiteralPath $shared).Hash -eq $before
    if(!$unchanged){throw 'Shared configuration changed'}
    $proof=@{guiSha256=$guiHash;harnessSha256=$harnessHash;captures=$captures;sharedConfigurationUnchanged=$unchanged;scope='Owned loopback S3 native browser; original Java pixel parity is not asserted'}
    $proof|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8
    $proof|ConvertTo-Json -Depth 6
} finally {
    foreach($key in $keys){if($null -eq $previous[$key]){Remove-Item -LiteralPath ('Env:'+ $key) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}
}
