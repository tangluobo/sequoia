param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Harness,[string]$Executable,[ValidateSet('TAB','WINDOW','FULLSCREEN')][string[]]$Modes=@('TAB','WINDOW','FULLSCREEN'),[switch]$Motion,[ValidateRange(700,10000)][int]$CaptureDelayMs=700,[ValidateSet("sparse","full","mosaic")][string]$Pattern="sparse",[ValidateSet("auto","vulkan","dx12","gl")][string]$Backend="vulkan",[ValidateSet('raw','remotefx','clearcodec','adaptive','avc420','progressive')][string]$Codec='raw',[switch]$NativeClipboard)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
$harnessPath=[IO.Path]::GetFullPath($Harness)
$harnessRoots=@('target/debug/deps/','target/qa-active-200/debug/deps/') | ForEach-Object { [IO.Path]::GetFullPath((Join-Path $workspace $_)) }
if(!($harnessRoots | Where-Object { $harnessPath.StartsWith($_,[StringComparison]::OrdinalIgnoreCase) }) -or [IO.Path]::GetFileName($harnessPath) -notmatch '^remote_desktop-[a-f0-9]+\.exe$'){throw 'Expected own RDP test harness'}
[void][IO.Directory]::CreateDirectory($probe)
$source=if($Executable){[IO.Path]::GetFullPath($Executable)}else{Join-Path $workspace 'target/debug/tomato.exe'}
if(!$source.StartsWith((Join-Path $workspace 'target/'),[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($source) -ne 'tomato.exe'){throw 'Expected own GUI binary'}
$harnessHash=(Get-FileHash -LiteralPath $harnessPath).Hash
$harnessCopy=Join-Path $probe 'rdp-fixture.exe';Copy-Item -LiteralPath $harnessPath -Destination $harnessCopy
if((Get-FileHash -LiteralPath $harnessCopy).Hash -ne $harnessHash){throw 'Harness changed while copying'}
$guiHash=(Get-FileHash -LiteralPath $source).Hash
$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $source -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $guiHash){throw 'GUI changed while copying'}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$captures=@()
$keys=@('TOMATO_RDP_CAPTURE_EXE','TOMATO_RDP_CAPTURE_MODE','TOMATO_RDP_CAPTURE_PATH','TOMATO_RDP_CAPTURE_FULLSCREEN','TOMATO_RDP_CAPTURE_MOTION','TOMATO_RDP_CAPTURE_PATTERN','TOMATO_RDP_CAPTURE_CODEC','TOMATO_RDP_CAPTURE_CLIPBOARD','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_RENDER_DIAGNOSTICS','WGPU_BACKEND')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
try {
    foreach($mode in $Modes) {
        $env:TOMATO_RDP_CAPTURE_EXE=$exe
        $env:TOMATO_RDP_CAPTURE_MODE=if($mode -eq 'TAB'){'TAB'}else{'WINDOW'}
        $env:TOMATO_RDP_CAPTURE_FULLSCREEN=if($mode -eq 'FULLSCREEN'){'1'}else{'0'}
        $env:TOMATO_RDP_CAPTURE_PATH=Join-Path $probe ($mode+'.png')
        if($Backend -eq 'auto'){Remove-Item Env:WGPU_BACKEND -ErrorAction SilentlyContinue}else{$env:WGPU_BACKEND=$Backend}
        $env:TOMATO_RENDER_DIAGNOSTICS='1'
        $env:TOMATO_RDP_CAPTURE_MOTION=if($Motion){'1'}else{[NullString]::Value}
        $env:TOMATO_UI_CAPTURE_DELAY_MS=[string]$CaptureDelayMs
        $env:TOMATO_RDP_CAPTURE_PATTERN=$Pattern
        $env:TOMATO_RDP_CAPTURE_CODEC=$Codec
        $env:TOMATO_RDP_CAPTURE_CLIPBOARD=if($NativeClipboard){'1'}else{[NullString]::Value}
        & $harnessCopy 'rdp_fixture::capture_native_rdp_window' --exact --ignored --nocapture *> (Join-Path $probe ($mode+'.log'))
        if($LASTEXITCODE -ne 0){throw ('Native RDP capture failed: '+$mode)}
        if(!(Test-Path -LiteralPath $env:TOMATO_RDP_CAPTURE_PATH)){throw 'Missing own framebuffer'}
        $selected=Select-String -LiteralPath ([IO.Path]::ChangeExtension($env:TOMATO_RDP_CAPTURE_PATH,'stderr.log')) -Pattern 'wgpu adapter: backend: (\w+),' | Select-Object -Last 1
        if(!$selected){throw 'Missing actual graphics backend proof'}
        $actual=$selected.Matches[0].Groups[1].Value
        if($Backend -ne 'auto' -and $actual -ne $Backend){throw ('Requested graphics backend did not take effect: '+$Backend+' -> '+$actual)}
        $captures+=@{mode=$mode;path=$env:TOMATO_RDP_CAPTURE_PATH;sha256=(Get-FileHash -LiteralPath $env:TOMATO_RDP_CAPTURE_PATH).Hash;actualBackend=$actual;adapter=$selected.Line}
    }
} finally {
    foreach($key in $keys){if($null -eq $previous[$key]){Remove-Item -LiteralPath ('Env:'+ $key) -ErrorAction SilentlyContinue}else{Set-Item -LiteralPath ('Env:'+ $key) -Value $previous[$key]}}
}
$after=(Get-FileHash -LiteralPath $shared).Hash
if($before -ne $after){throw 'Shared legacy config changed'}
@{codec=$Codec;nativeClipboard=[bool]$NativeClipboard;backend=$Backend;pattern=$Pattern;motion=[bool]$Motion;captureDelayMs=$CaptureDelayMs;guiSha256=$guiHash;harnessSha256=$harnessHash;captures=$captures;sharedConfigurationUnchanged=$true;sharedSha256=$after;network='owned loopback TLS/CredSSP fixture only';actualSavedHostsContacted=$false}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8
