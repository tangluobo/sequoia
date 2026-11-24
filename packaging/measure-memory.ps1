param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [ValidateRange(1, 300)][int]$WarmupSeconds = 15,
    [ValidateRange(1, 300)][int]$SampleSeconds = 10,
    [string]$Output = '',
    [string]$LogDirectory = ''
)
$ErrorActionPreference = 'Stop'
$executablePath = (Resolve-Path -LiteralPath $Executable).Path
$probeDirectory = Join-Path ([IO.Path]::GetTempPath()) ('tomato-memory-' + [Guid]::NewGuid().ToString('N'))
[void](New-Item -ItemType Directory -Path $probeDirectory)
$previousDataDirectory = $env:TOMATO_DATA_DIR
$previousCapture = $env:TOMATO_UI_CAPTURE
$previousPreview = $env:TOMATO_UI_PREVIEW
$probeProcess = $null
$otherInstances = 0
foreach ($existingProcess in Get-Process -Name ([IO.Path]::GetFileNameWithoutExtension($executablePath)) -ErrorAction SilentlyContinue) {
    try { if (!$existingProcess.HasExited) { $otherInstances++ } }
    finally { $existingProcess.Dispose() }
}
try {
    $env:TOMATO_DATA_DIR = $probeDirectory
    $env:TOMATO_UI_CAPTURE = $null
    $env:TOMATO_UI_PREVIEW = $null
    $logging = @{}
    if ($LogDirectory) {
        $logPath = [IO.Path]::GetFullPath($LogDirectory)
        [void][IO.Directory]::CreateDirectory($logPath)
        $logging.RedirectStandardOutput = Join-Path $logPath 'stdout.log'
        $logging.RedirectStandardError = Join-Path $logPath 'stderr.log'
    }
    $probeProcess = Start-Process -FilePath $executablePath -WorkingDirectory (Split-Path -Parent $executablePath) -WindowStyle Hidden -PassThru @logging
    Start-Sleep -Seconds $WarmupSeconds
    $samples = @()
    for ($sample = 0; $sample -lt $SampleSeconds; $sample++) {
        $probeProcess.Refresh()
        if ($probeProcess.HasExited) { throw "Probe exited early: $($probeProcess.ExitCode)" }
        $samples += [pscustomobject]@{
            workingSetMiB = [Math]::Round($probeProcess.WorkingSet64 / 1MB, 2)
            privateCommitMiB = [Math]::Round($probeProcess.PrivateMemorySize64 / 1MB, 2)
            peakWorkingSetMiB = [Math]::Round($probeProcess.PeakWorkingSet64 / 1MB, 2)
        }
        Start-Sleep -Seconds 1
    }
    $result = [pscustomobject]@{
        executable = $executablePath
        measuredAtUtc = [DateTime]::UtcNow.ToString('o')
        scenario = 'fresh isolated workspace; idle; hidden launch; no connections'
        otherInstancesAtStart = $otherInstances
        warmupSeconds = $WarmupSeconds
        sampleSeconds = $SampleSeconds
        backendOverride = $(if ($null -eq [Environment]::GetEnvironmentVariable('WGPU_BACKEND','Process')) { 'automatic' } elseif ($env:WGPU_BACKEND -eq '') { '<empty>' } else { $env:WGPU_BACKEND })
        powerPreferenceOverride = $(if ($null -eq [Environment]::GetEnvironmentVariable('WGPU_POWER_PREF','Process')) { 'default' } elseif ($env:WGPU_POWER_PREF -eq '') { '<empty>' } else { $env:WGPU_POWER_PREF })
        workingSetMiB = [Math]::Round(($samples | Measure-Object workingSetMiB -Average).Average, 2)
        privateCommitMiB = [Math]::Round(($samples | Measure-Object privateCommitMiB -Average).Average, 2)
        peakWorkingSetMiB = ($samples | Measure-Object peakWorkingSetMiB -Maximum).Maximum
        samples = $samples
    }
    $json = $result | ConvertTo-Json -Depth 4
    if ($Output) { [IO.File]::WriteAllText([IO.Path]::GetFullPath($Output), $json) }
    $json
} finally {
    # Use the owned Process handle; GPU-driver teardown can outlast ten seconds.
    try {
        if ($probeProcess -and !$probeProcess.HasExited) {
            $probeProcess.Kill()
            $exited = $false
            for ($attempt = 0; $attempt -lt 6; $attempt++) {
                if ($probeProcess.WaitForExit(10000)) { $exited = $true; break }
            }
            if (!$exited) { throw 'Owned memory probe did not exit within 60 seconds' }
        }
    } finally {
        try { if ($probeProcess) { $probeProcess.Dispose() } }
        finally {
            $env:TOMATO_DATA_DIR = $previousDataDirectory
            $env:TOMATO_UI_CAPTURE = $previousCapture
            $env:TOMATO_UI_PREVIEW = $previousPreview
        }
    }
    # Keep the isolated directory for diagnosis; never remove user configuration.
}
