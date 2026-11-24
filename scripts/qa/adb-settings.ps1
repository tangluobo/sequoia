param(
    [Parameter(Mandatory=$true)][string]$Label,
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(Mandatory=$true)][string]$AdbFixture
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'adb-peer.ps1')
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if ($Label -notmatch '^[a-z0-9-]+$') { throw 'Invalid capture label' }
$probe = Join-Path $workspace ('target/window-probes/' + $Label)
if (Test-Path -LiteralPath $probe) { throw 'Use a fresh capture label' }
[void][IO.Directory]::CreateDirectory($probe)
$exe = (Resolve-Path -LiteralPath $Executable).Path
$fixture = (Resolve-Path -LiteralPath $AdbFixture).Path
$hash = (Get-FileHash -LiteralPath $exe).Hash
$shared = Join-Path $env:USERPROFILE '.tomato/connections.json'
$before = if (Test-Path -LiteralPath $shared) { (Get-FileHash -LiteralPath $shared).Hash }
$keys = @('TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS')
$previous = @{}
foreach ($key in $keys) { $previous[$key] = [Environment]::GetEnvironmentVariable($key, 'Process') }
$captures = @()
try {
    foreach ($state in 'adb_config','adb_png','adb_window') {
        $directory = Join-Path $probe ('tomato-device-fixture-' + $state)
        [void][IO.Directory]::CreateDirectory($directory)
        $marker = Join-Path $directory 'adb-fixture'
        $peer = $null
        try {
            if ($state -ne 'adb_config') { $peer = Start-AdbTestPeer $fixture $marker }
            $env:TOMATO_DATA_DIR = $directory
            $env:TOMATO_UI_PREVIEW = $state
            $env:TOMATO_UI_CAPTURE = Join-Path $probe "$state.png"
            $env:TOMATO_UI_CAPTURE_FRAMES = '1'
            $env:TOMATO_UI_CAPTURE_DELAY_MS = '4000'
            $process = Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe "$state.stdout.log") -RedirectStandardError (Join-Path $probe "$state.stderr.log")
            try {
                if (!$process.WaitForExit(45000) -or $process.ExitCode -ne 0) { throw "ADB capture did not exit normally for $state" }
            } finally {
                if (!$process.HasExited) { $process.Kill() }
                $process.Dispose()
            }
            if (!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)) { throw "Missing $state framebuffer" }
            if (Test-Path -LiteralPath (Join-Path $directory 'connections.json')) { throw 'Preview persisted a draft' }
            if ($state -eq 'adb_png') {
                $commands = Get-Content -LiteralPath "$marker.commands.jsonl" -Raw
                if (!$commands.Contains('screencap') -or !$commands.Contains("'keyevent' '3'")) { throw 'PNG or its control did not reach the native peer' }
            }
            if ($state -eq 'adb_window') {
                $commands = Get-Content -LiteralPath "$marker.commands.jsonl" -Raw
                $control = Get-Content -LiteralPath "$marker.scrcpy-input.jsonl" -Raw
                if (!$commands.Contains('video_codec=h264') -or !$control.Contains('[0,0,0,0,0,3,0,0,0,0,0,0,0,0]') -or !(Test-Path -LiteralPath "$marker.server-removed")) {
                    throw 'Independent window H.264, control or server-file removal did not reach the peer'
                }
            }
            $captures += @{ state=$state; path=$env:TOMATO_UI_CAPTURE; normalExit=$true }
        } finally {
            Stop-AdbTestPeer $peer
        }
    }
    $after = if (Test-Path -LiteralPath $shared) { (Get-FileHash -LiteralPath $shared).Hash }
    if ($before -ne $after) { throw 'Shared connections changed' }
    @{ guiSha256=$hash; captures=$captures; nativeTransport=$true; sharedConfigurationUnchanged=$true } |
        ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8
    Get-Content -LiteralPath (Join-Path $probe 'validation.json')
} finally {
    foreach ($key in $keys) {
        if ($null -eq $previous[$key]) {
            [Environment]::SetEnvironmentVariable($key, [NullString]::Value, 'Process')
        } else {
            [Environment]::SetEnvironmentVariable($key, $previous[$key], 'Process')
        }
    }
}
