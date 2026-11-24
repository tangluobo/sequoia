param([Parameter(Mandatory=$true)][string]$Label,[switch]$Verify)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid checkpoint label'}
$directory=Join-Path $workspace ('target/checkpoints/'+$Label)
if($Verify){
 $manifest=Get-Content -LiteralPath (Join-Path $directory 'production-sources.json') -Raw|ConvertFrom-Json
 foreach($entry in $manifest){$path=Join-Path $workspace $entry.path;if(!(Test-Path -LiteralPath $path) -or (Get-FileHash -LiteralPath $path).Hash -ne $entry.sha256){throw ('Production source changed: '+$entry.path)}}
 [pscustomobject]@{checkpoint=$Label;productionSourceFiles=$manifest.Count;unchanged=$true}
 return
}
if(Test-Path -LiteralPath $directory){throw 'Use a fresh checkpoint label'}
[void][IO.Directory]::CreateDirectory($directory)
Push-Location $workspace
try {
 $paths=@(& rg --files --hidden -g '!target/**' -g '!.git/**' -g '!dist/**' -g '!out/**' -g '!node_modules/**'|Sort-Object)
 if($LASTEXITCODE -ne 0){throw 'Source enumeration failed'}
 $all=@();$production=@()
 foreach($path in $paths){$item=Get-Item -LiteralPath $path;$entry=[ordered]@{path=$path.Replace('\','/');sha256=(Get-FileHash -LiteralPath $path).Hash;bytes=$item.Length};$all+=$entry
  if($entry.path -match '^(Cargo\.(toml|lock)|\.cargo/|apps/|crates/|vendor/|xtask/|server/)' -and $entry.path -notmatch '/tests/'){ $production+=$entry }
 }
 $all|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $directory 'sources.json') -Encoding utf8
 $production|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $directory 'production-sources.json') -Encoding utf8
 [pscustomobject]@{checkpoint=$Label;allSourceFiles=$all.Count;productionSourceFiles=$production.Count}
} finally {Pop-Location}
