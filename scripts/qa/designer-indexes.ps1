param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$keys=@('TOMATO_DESIGNER_INDEX','TOMATO_DESIGNER_TYPE','TOMATO_DESIGNER_SELECTION','TOMATO_DESIGNER_GRID','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_DATA_DIR','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_PREVIEW','TOMATO_DESIGNER_PAGE','WGPU_BACKEND','TEMP','TMP')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
Push-Location $workspace
try {
 $proofs=@()
 foreach($mode in @('base','type','unique','method','fields','rename')) {
  $env:TOMATO_DESIGNER_INDEX=$mode;$env:TOMATO_UI_CAPTURE_DELAY_MS='8000'
  foreach($key in @('TOMATO_DESIGNER_TYPE','TOMATO_DESIGNER_SELECTION','TOMATO_DESIGNER_GRID')){[Environment]::SetEnvironmentVariable($key,[NullString]::Value,'Process')}
  & (Join-Path $PSScriptRoot 'table-designer.ps1') -Label ($Label+'-'+$mode) -Executable $Executable -Pages 1 -Backend auto | Out-Null
  $path=Join-Path $workspace ('target/ui-table-designer-'+$Label+'-'+$mode+'-1.index.json')
  $proof=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
  if($proof.mode -ne $mode -or $proof.osInputUsed -or $proof.savedHostsContacted -or $proof.indexes.Count -ne 2){throw 'Invalid index fixture proof'}
  if($proof.indexes[1].name -ne 'uq_created' -or $proof.indexes[1].kind -ne 'Unique'){throw 'Unrelated index changed'}
  switch($mode){
   rename {if($proof.indexes[0].name -ne 'ix_renamed' -or !$proof.dirty -or $proof.editing){throw 'Index name editor did not commit'}}
   unique {if($proof.indexes[0].kind -ne 'Unique' -or !$proof.dirty){throw 'Type choice did not modify the real index'}}
   fields {
    if(!$proof.fieldsOpen -or $proof.dirty -or $proof.fieldRows.Count -ne 3 -or !$proof.fieldPopup){throw 'Opening the field picker modified the draft'}
    if([Math]::Abs($proof.fieldPopup[0]-202) -gt 2 -or [Math]::Abs($proof.fieldPopup[1]-155) -gt 2 -or $proof.fieldPopup[2] -gt 450 -or $proof.fieldPopup[3] -gt 240){throw 'Field selection must be a compact dropdown directly beneath the cell'}
    for($n=1;$n -lt $proof.fieldRows.Count;$n++){
     if([Math]::Abs($proof.fieldRows[$n].rect[0]-$proof.fieldRows[0].rect[0]) -gt 1 -or [Math]::Abs($proof.fieldRows[$n].rect[1]-$proof.fieldRows[$n-1].rect[1]-30) -gt 0.1){throw 'Field picker alignment or 30px row height changed'}
    }
   }
   default {if($proof.dirty -or $proof.indexes[0].name -ne 'ix_name'){throw 'Read-only interaction modified the draft'}}
  }
  if($mode -in @('type','method')){
   $expected=if($mode -eq 'type'){462}else{582}
   if(!$proof.popup -or [Math]::Abs($proof.popup[0]-$expected) -gt 2 -or [Math]::Abs($proof.popup[1]-155) -gt 2 -or $proof.popup[2] -gt 160){throw 'Index menu moved away from the bottom-left of the cell'}
  }
  $proofs+=@{mode=$mode;proof=$path;passed=$true;popup=$proof.popup}
 }
 @{scenarios=$proofs;savedHostsContacted=$false;osInputUsed=$false;allGlyphPixelsIdentical=$false}|ConvertTo-Json -Depth 5|Set-Content ('target/'+$Label+'.json')
 Get-Content ('target/'+$Label+'.json')
} finally {
 foreach($key in $keys){[Environment]::SetEnvironmentVariable($key,$(if($null -eq $previous[$key]){[NullString]::Value}else{$previous[$key]}),'Process')}
 Pop-Location
}
