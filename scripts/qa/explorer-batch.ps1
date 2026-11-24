param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(Mandatory=$true)][string]$Label,
    [int]$Timeout = 0,
    [switch]$Progress,
    [double]$DownloadDelay = 0
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$runner = Join-Path $PSScriptRoot 'explorer-drag.py'
$python = 'D:/App/msys64/ucrt64/bin/python.exe'
if (-not [IO.File]::Exists($python)) { $python = (Get-Command python.exe -ErrorAction Stop).Source }
$results = @()
foreach ($view in @('LIST', 'ICON', 'COLUMN')) {
    $case = $Label + '-' + $view.ToLowerInvariant()
    Write-Host ('Batch acceptance: ' + $view + ' / select all four files and drag ONCE to Explorer.') -ForegroundColor Cyan
    $extra = @()
    if ($Progress) { $extra += '--progress' }
    & $python -X utf8 $runner --executable $Executable --label $case --batch --view $view --download-delay $DownloadDelay --timeout $Timeout @extra
    if ($LASTEXITCODE -ne 0) { throw ('Actual Explorer batch acceptance did not pass: ' + $view) }
    $path = Join-Path $repo ('target/window-probes/' + $case + '/validation.json')
    $proof = Get-Content -LiteralPath $path -Raw -Encoding utf8 | ConvertFrom-Json
    if ($proof.status -ne 'passed' -or -not $proof.batch -or $proof.initialView -ne $view) { throw 'Unexpected batch proof' }
    $results += [ordered]@{view=$view;proof=$path;guiSha256=$proof.guiSha256}
}
$output = Join-Path $repo ('target/' + $Label + '-all-views.json')
[ordered]@{status='passed';actualHumanExplorerBatch=$true;savedHostsContacted=$false;osInputInjected=$false;views=$results} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $output -Encoding utf8
Write-Host 'All three actual Explorer batch cases passed.' -ForegroundColor Green
