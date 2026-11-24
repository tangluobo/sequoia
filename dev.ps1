param([Parameter(ValueFromRemainingArguments = $true)][string[]]$CargoArguments)

$ErrorActionPreference = 'Stop'
$cargoDirectory = Join-Path $env:USERPROFILE '.cargo\bin'
$cargoExecutable = Join-Path $cargoDirectory 'cargo.exe'
if (!(Test-Path -LiteralPath $cargoExecutable)) {
    $cargoExecutable = (Get-Command cargo -ErrorAction Stop).Source
}
$originalPath = $env:PATH
$originalTemp = $env:TEMP
$originalTmp = $env:TMP
try {
    $env:PATH = "$cargoDirectory;$originalPath"
    $videoAssembler = Join-Path $PSScriptRoot 'target/build-tools/nasm-3.01'
    if (Test-Path -LiteralPath (Join-Path $videoAssembler 'nasm.exe')) {
        $env:PATH = "$videoAssembler;$env:PATH"
    }
    $buildTarget = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
    if (![IO.Path]::IsPathRooted($buildTarget)) { $buildTarget = Join-Path $PSScriptRoot $buildTarget }
    $buildTemp = Join-Path $buildTarget 'build-temp'
    [void](New-Item -ItemType Directory -Path $buildTemp -Force)
    $env:TEMP = $buildTemp
    $env:TMP = $buildTemp
    Push-Location $PSScriptRoot
    try {
        if (!$CargoArguments -or $CargoArguments.Count -eq 0) {
            & $cargoExecutable run -p tomato
        } else {
            & $cargoExecutable @CargoArguments
        }
        $result = $LASTEXITCODE
    } finally { Pop-Location }
} finally {
    $env:PATH = $originalPath
    $env:TEMP = $originalTemp
    $env:TMP = $originalTmp
}
exit $result
