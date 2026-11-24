function Get-CurrentHarness {
 param([Parameter(Mandatory=$true)][string]$Pattern)
 $workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
 $target=if([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)){'target'}else{$env:CARGO_TARGET_DIR}
 $target=[IO.Path]::GetFullPath($target,$workspace)
 $candidate=Get-ChildItem -LiteralPath (Join-Path $target 'debug/deps') -Filter $Pattern|Where-Object Length -gt 1000000|Sort-Object LastWriteTimeUtc -Descending|Select-Object -First 1
 if(!$candidate){throw ('Build current harness first: '+$Pattern)}
 Push-Location $workspace
 try {
  $roots=@('crates/core/src','crates/services/src');if($Pattern -like 'tomato_ui_egui*'){$roots+='crates/ui-egui/src'}
  $inputs=@(& rg --files @roots -g '*.rs' -g '*.json')+@('Cargo.toml','Cargo.lock','crates/core/Cargo.toml','crates/services/Cargo.toml')
  if($Pattern -like 'tomato_ui_egui*'){$inputs+='crates/ui-egui/Cargo.toml'}
  if($Pattern -like 'cloud-*'){$inputs+=@('crates/services/tests/cloud.rs','crates/services/tests/support/cloud_fixture.rs')}
  if($Pattern -like 'mq-*'){$inputs+=@('crates/services/tests/mq.rs','crates/services/tests/support/mq_seed.rs')}
  if($LASTEXITCODE -ne 0){throw 'Could not inspect harness inputs'}
  foreach($inputPath in $inputs){if((Get-Item -LiteralPath $inputPath).LastWriteTimeUtc -gt $candidate.LastWriteTimeUtc){throw ('Harness predates current Rust input '+$inputPath+'; finish the workspace build first')}}
 }finally{Pop-Location}
 return $candidate
}
