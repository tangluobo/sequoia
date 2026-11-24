param([Parameter(Mandatory=$true)][string]$Label,[string]$Executable='target/qa-active-200/debug/tomato.exe',[switch]$Recheck,[string[]]$Modes=@('rows','shift','row-drag','reverse','cells','scroll','hold','batch','batch-reverse','batch-row'))
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$keys=@('TOMATO_DESIGNER_TYPE','TOMATO_DESIGNER_MENU','TOMATO_DESIGNER_SELECTION','TOMATO_DESIGNER_GRID','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_DATA_DIR','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_PREVIEW','TOMATO_DESIGNER_PAGE','WGPU_BACKEND','TEMP','TMP')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
Push-Location $workspace
try {
 $env:TOMATO_DESIGNER_TYPE=$null;$env:TOMATO_DESIGNER_MENU=$null
 $source=if([IO.Path]::IsPathRooted($Executable)){$Executable}else{Join-Path $workspace $Executable}
 $guiHash=(Get-FileHash -LiteralPath $source).Hash
 $results=@()
 foreach($mode in $Modes) {
  $env:TOMATO_DESIGNER_SELECTION=$mode
  $env:TOMATO_DESIGNER_GRID=if($mode -eq 'scroll'){'large'}else{'multi'}
  $env:TOMATO_UI_CAPTURE_DELAY_MS='10000'
  if(!$Recheck){& (Join-Path $PSScriptRoot 'table-designer.ps1') -Label ($Label+'-'+$mode) -Executable $Executable -Pages 0 -Backend auto | Out-Null}
  $path=Join-Path $workspace ('target/ui-table-designer-'+$Label+'-'+$mode+'-0.selection.json')
  $proof=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
  if($proof.mode -ne $mode -or $proof.frame -lt 150 -or $proof.editing -or $proof.dragging -or $proof.dirty -or $proof.popup -or $proof.databaseRequested -or $proof.systemClipboardWritten -or $proof.osInputUsed){throw ('Selection state failed: '+$mode)}
  if($mode -eq 'hold' -and !$proof.pointerCaptured){throw 'Held pointer was not captured by the grid'}
  if($proof.values[0][0] -ne 'id' -or $proof.values[0][1] -ne 'BIGINT' -or $proof.values[1][1] -ne 'VARCHAR' -or $proof.values[2][1] -ne 'TIMESTAMP'){throw 'Selection changed field data'}
  if($mode -eq 'scroll') {
   if($proof.fieldCount -ne 512 -or $proof.selectedRows.Count -le 15 -or $proof.selectedRows.Count -ge 512 -or !$proof.visibleFieldRows.Count -or ($proof.visibleFieldRows|Measure-Object -Minimum).Minimum -lt 10 -or $proof.visibleFieldRows.Count -ge 30){throw 'Selection did not scroll through virtual rows'}
   # Add seeds row 2's scroll target before the large fixture is populated.
   # Its first displayed selector therefore belongs to model row 2.
   if($proof.selectedRows[0] -ne 2 -or $proof.selectedRows[-1] -ne (1+$proof.selectedRows.Count)){throw 'Scrolled selection lost its pressed row or its continuity'}
   for($row=0;$row -lt 512;$row++){
    $expected=if($row -ge 2 -and $row -le $proof.selectedRows[-1]){255}else{0}
    if($proof.masks[$row] -ne $expected){throw 'Scrolled row selection has gaps'}
   }
  } else {
   $expected=switch($mode){rows {@(255,0,255)} {$_ -in @('cells','hold','batch','batch-reverse')} {@(7,7,7)} default {@(255,255,255)}}
   if($proof.fieldCount -ne 3 -or (@($proof.masks) -join ',') -ne ($expected -join ',')){throw ('Wrong selected cells: '+$mode)}
  }
  $png=Join-Path $workspace ('target/ui-table-designer-'+$Label+'-'+$mode+'-0.png')
  $capture=Get-Content -LiteralPath ('target/window-probes/'+$Label+'-'+$mode+'/validation.json') -Raw|ConvertFrom-Json
  if($capture.guiSha256 -ne $guiHash -or $capture.captures.Count -ne 1 -or (Get-FileHash -LiteralPath $png).Hash -ne $capture.captures[0].sha256){throw 'Capture or GUI identity mismatch'}
  & D:/App/msys64/ucrt64/bin/python.exe -X utf8 (Join-Path $PSScriptRoot 'designer-selection.py') $png $path
  if($LASTEXITCODE){throw ('Native selection pixels differ: '+$mode)}
  $results+=@{mode=$mode;proof=$path;capture=$png;passed=$true}
 }
 @{status='passed';guiSha256=$guiHash;scenarios=$results;recheckedExistingCaptures=[bool]$Recheck;savedHostsContacted=$false;osInputUsed=$false}|ConvertTo-Json -Depth 5|Set-Content ('target/'+$Label+'.json')
 Get-Content ('target/'+$Label+'.json')
} finally {
 foreach($key in $keys){[Environment]::SetEnvironmentVariable($key,$(if($null -eq $previous[$key]){[NullString]::Value}else{$previous[$key]}),'Process')}
 Pop-Location
}
