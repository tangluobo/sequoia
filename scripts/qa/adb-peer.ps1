function Start-AdbTestPeer([string]$Executable, [string]$Marker) {
    $parent = Split-Path -Parent $Marker
    if ((Split-Path -Leaf $parent) -notlike 'tomato-device-fixture-*' -or (Test-Path -LiteralPath "$Marker.port")) {
        throw 'ADB peer requires a fresh disposable profile'
    }
    $arguments = '"{0}"' -f $Marker
    $peer = Start-Process -FilePath $Executable -ArgumentList $arguments -WindowStyle Hidden -PassThru
    try {
        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        while (!(Test-Path -LiteralPath "$Marker.port")) {
            if ($peer.HasExited -or [DateTime]::UtcNow -gt $deadline) { throw 'ADB peer did not publish a loopback port' }
            Start-Sleep -Milliseconds 50
        }
        return $peer
    } catch {
        Stop-AdbTestPeer $peer
        throw
    }
}

function Stop-AdbTestPeer([Diagnostics.Process]$Peer) {
    if (!$Peer) { return }
    try {
        if (!$Peer.HasExited) { $Peer.Kill() }
        if (!$Peer.WaitForExit(5000)) { throw 'Owned ADB peer did not exit' }
    } finally {
        $Peer.Dispose()
    }
}
