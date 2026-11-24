param([Parameter(Mandatory=$true)][ValidatePattern('^[a-z0-9-]+$')][string]$Label)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$probe=Join-Path $workspace ('target/rdp-codec-tests-'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh test label'}
[void][IO.Directory]::CreateDirectory($probe)
$config=Join-Path $probe 'patches.toml'
$patches=@();$inside=$false
foreach($line in Get-Content -LiteralPath (Join-Path $workspace 'Cargo.toml')){
 if($line.Trim() -eq '[patch.crates-io]'){$inside=$true;continue}
 if($inside -and $line -match '^\['){break}
 if($inside -and $line -match '^(ironrdp-(?:core|error|pdu))\s*=\s*\{\s*path\s*=\s*"([^"]+)"'){
  $name=$Matches[1];$path=[IO.Path]::GetFullPath((Join-Path $workspace $Matches[2])).Replace('\','/')
  $patches+=('[patch.crates-io."'+$name+'"]'+"`n"+'path = "'+$path+'"'+"`n")
 }
}
if($patches.Count -ne 3){throw 'Expected the product core/error/pdu patches'}
[IO.File]::WriteAllText($config,($patches -join "`n"))
$manifest=Join-Path $workspace 'vendor/ironrdp-graphics/Cargo.toml'
$lock=Join-Path $workspace 'vendor/ironrdp-graphics/Cargo.lock'
if(Test-Path -LiteralPath $lock){Copy-Item -LiteralPath $lock -Destination (Join-Path $probe 'prior-sdk.lock')}
$productLock=Join-Path $workspace 'Cargo.lock';$before=(Get-FileHash -LiteralPath $productLock).Hash
$oldTarget=$env:CARGO_TARGET_DIR
try {
 $env:CARGO_TARGET_DIR=Join-Path $workspace 'target/qa-rdpgfx-graphics-469'
 & cargo generate-lockfile --manifest-path $manifest --config $config --offline *> (Join-Path $probe 'lock.log')
 if($LASTEXITCODE -ne 0){throw 'Isolated codec lock resolution failed'}
 & cargo test --manifest-path $manifest --config $config --lib --locked --offline -j4 *> (Join-Path $probe 'tests.log')
 if($LASTEXITCODE -ne 0){throw 'Codec regression failed'}
 if((Get-FileHash -LiteralPath $productLock).Hash -ne $before){throw 'Product lock changed'}
 @{productLockUnchanged=$true;productLockSha256=$before;sdkLockSha256=(Get-FileHash -LiteralPath $lock).Hash;networkUsed=$false}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $probe 'validation.json')
 Get-Content -LiteralPath (Join-Path $probe 'tests.log') -Tail 3
} finally {
 if($null -eq $oldTarget){Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue}else{$env:CARGO_TARGET_DIR=$oldTarget}
}
