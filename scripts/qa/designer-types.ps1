param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$keys=@('TOMATO_DESIGNER_TYPE','TOMATO_DESIGNER_GRID','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_DATA_DIR','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_PREVIEW','TOMATO_DESIGNER_PAGE','WGPU_BACKEND','TEMP','TMP')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
Push-Location $workspace
try {
 $proofs=@()
 foreach($mode in @('open','current','filter','commit','cancel','blur','text')) {
  $env:TOMATO_DESIGNER_TYPE=$mode;$env:TOMATO_DESIGNER_GRID='multi';$env:TOMATO_UI_CAPTURE_DELAY_MS='10000'
  & (Join-Path $PSScriptRoot 'table-designer.ps1') -Label ($Label+'-'+$mode) -Executable $Executable -Pages 0 -Backend auto | Out-Null
  $path=Join-Path $workspace ('target/ui-table-designer-'+$Label+'-'+$mode+'-0.type.json')
  $proof=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
  if($proof.mode -ne $mode -or $proof.databaseRequested -or $proof.osInputUsed -or $proof.systemClipboardWritten){throw 'Invalid type fixture proof'}
  if($proof.values[1] -ne 'VARCHAR' -or $proof.values[2] -ne 'TIMESTAMP'){throw 'Underlying row was changed'}
  switch($mode) {
   text {if(!$proof.editing -or $proof.open -or $proof.editText -ne 'id_edited' -or $proof.values[0] -ne 'BIGINT'){throw 'Text cell editor did not remain active'}}
   open {if(!$proof.open -or !$proof.editing -or $proof.focused -ne 'BIGINT' -or $proof.values[0] -ne 'BIGINT' -or $proof.dirty){throw 'Single press did not open current type'}}
   current {
    if(!$proof.open -or $proof.focused -ne 'VARCHAR' -or $proof.dirty){throw 'Current type was not focused'}
    $visible=@($proof.textPositions|Where-Object {$_.text -eq 'VARCHAR' -and $_.x -ge $proof.popup[0] -and $_.y -ge $proof.popup[1] -and ($_.y+$_.height) -le ($proof.popup[1]+$proof.popup[3])})
    if(!$visible.Count){throw 'Current candidate is outside popup'}
   }
   filter {if(!$proof.open -or $proof.focused -ne 'CHAR' -or $proof.visibleCount -ne 2 -or $proof.values[0] -ne 'BIGINT' -or $proof.dirty){throw 'Typing filter committed prematurely'}}
   commit {if($proof.open -or $proof.editing -or !$proof.dirty -or $proof.values[0] -ne 'VARCHAR'){throw 'Keyboard candidate was not committed'}}
   cancel {if($proof.open -or $proof.editing -or $proof.dirty -or $proof.values[0] -ne 'BIGINT'){throw 'Escape did not restore original type'}}
   blur {if($proof.open -or $proof.editing -or !$proof.dirty -or $proof.values[0] -ne 'CUSTOM_TYPE'){throw 'Outside click did not commit typed type'}}
  }
  if($proof.open -and ([Math]::Abs($proof.popup[2]-135) -gt 1 -or $proof.popup[3] -gt 505)){throw 'Popup geometry differs from reference'}
  if($proof.open -and ([Math]::Abs($proof.popup[0]-167) -gt 1 -or [Math]::Abs($proof.popup[1]-$(if($mode -eq 'current'){150}else{120})) -gt 1)){throw 'Popup shifted away from the bottom-left of its cell'}
  $proofs+=@{mode=$mode;proof=$path;passed=$true;popup=$proof.popup}
 }
 @{scenarios=$proofs;savedHostsContacted=$false;osInputUsed=$false;allGlyphPixelsIdentical=$false}|ConvertTo-Json -Depth 5|Set-Content ('target/'+$Label+'.json')
 Get-Content ('target/'+$Label+'.json')
} finally {
 foreach($key in $keys){[Environment]::SetEnvironmentVariable($key,$(if($null -eq $previous[$key]){[NullString]::Value}else{$previous[$key]}),'Process')}
 Pop-Location
}
