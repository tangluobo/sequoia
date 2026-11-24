param(
    [Parameter(Mandatory=$true)][string]$OldExecutable,
    [Parameter(Mandatory=$true)][string]$NewExecutable,
    [Parameter(Mandatory=$true)][string]$Label
)
$ErrorActionPreference = 'Stop'
if ($Label -notmatch '^[a-z0-9-]+$') { throw 'Invalid probe label' }
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$directory = Join-Path $workspace ('target/window-probes/' + $Label)
if (Test-Path -LiteralPath $directory) { throw 'Use a fresh probe label' }
[void][IO.Directory]::CreateDirectory($directory)
$shared = Join-Path $env:USERPROFILE '.tomato/connections.json'
$before = if (Test-Path -LiteralPath $shared) { (Get-FileHash -LiteralPath $shared).Hash } else { $null }
$previousBackend = $env:WGPU_BACKEND
$previousTemp = $env:TEMP
$previousTmp = $env:TMP
$samples = @()
try {
    $env:TEMP = Join-Path $workspace 'target/build-temp'
    $env:TMP = $env:TEMP
    $copies = @{}
    foreach ($spec in @(@('old', $OldExecutable), @('new', $NewExecutable))) {
        $source = (Resolve-Path -LiteralPath $spec[1]).Path
        $hash = (Get-FileHash -LiteralPath $source).Hash
        $destination = Join-Path $directory $spec[0]
        [void][IO.Directory]::CreateDirectory($destination)
        $copy = Join-Path $destination 'tomato.exe'
        Copy-Item -LiteralPath $source -Destination $copy
        if ((Get-FileHash -LiteralPath $copy).Hash -ne $hash -or
            (Get-FileHash -LiteralPath $source).Hash -ne $hash) { throw 'Executable changed while snapshotting' }
        $copies[$spec[0]] = [pscustomobject]@{ executable=$copy; source=$source; sha256=$hash }
    }
    foreach ($backend in @('auto', 'vulkan')) {
        $env:WGPU_BACKEND = if ($backend -eq 'auto') { $null } else { 'vulkan' }
        foreach ($version in @('old', 'new')) {
            $output = Join-Path $directory ($version + '-' + $backend + '.json')
            & (Join-Path $workspace 'packaging/measure-memory.ps1') -Executable $copies[$version].executable -WarmupSeconds 15 -SampleSeconds 10 -Output $output
            if (!(Test-Path -LiteralPath $output)) { throw 'Missing memory result' }
            $samples += [pscustomobject]@{
                version=$version
                backend=$backend
                executable=$copies[$version]
                measurement=(Get-Content -LiteralPath $output -Raw | ConvertFrom-Json)
            }
        }
    }
    foreach ($copy in $copies.Values) {
        if ((Get-FileHash -LiteralPath $copy.executable).Hash -ne $copy.sha256 -or
            (Get-FileHash -LiteralPath $copy.source).Hash -ne $copy.sha256) { throw 'Executable changed during probe' }
    }
    $after = if (Test-Path -LiteralPath $shared) { (Get-FileHash -LiteralPath $shared).Hash } else { $null }
    if ($before -ne $after) { throw 'Shared configuration changed during probe' }
    [pscustomobject]@{
        label=$Label
        samples=$samples
        sharedConfigurationUnchanged=$true
        savedHostsContacted=$false
        workload='Idle native window with its own empty temporary profile; not an active-session memory bound'
    } | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $directory 'validation.json')
} finally {
    $env:WGPU_BACKEND = $previousBackend
    $env:TEMP = $previousTemp
    $env:TMP = $previousTmp
}
