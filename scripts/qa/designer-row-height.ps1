param([Parameter(Mandatory=$true)][string]$Label,[string]$Executable='target/qa-active-200/debug/tomato.exe')
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$keys=@('TOMATO_DESIGNER_TYPE','TOMATO_DESIGNER_GRID','TOMATO_UI_CAPTURE_DELAY_MS','TOMATO_DATA_DIR','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_PREVIEW','TOMATO_DESIGNER_PAGE','WGPU_BACKEND','TEMP','TMP')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
Push-Location $workspace
try {
 $env:TOMATO_DESIGNER_TYPE=$null;$env:TOMATO_DESIGNER_GRID='multi';$env:TOMATO_UI_CAPTURE_DELAY_MS='4000'
 & scripts/qa/table-designer.ps1 -Label ($Label+'-static') -Executable $Executable -Pages 0 -Backend auto | Out-Null
 & scripts/qa/designer-types.ps1 -Label $Label -Executable $Executable | Out-Null
 $baseline='target/ui-table-designer-'+$Label+'-static-0.png'
 $states=@('open','current','filter','commit','cancel','blur','text')|ForEach-Object {'target/ui-table-designer-'+$Label+'-'+$_+'-0.png'}
 & D:/App/msys64/ucrt64/bin/python.exe -X utf8 scripts/qa/designer-row-height.py $baseline @states --proof ('target/'+$Label+'-geometry.json')
 if($LASTEXITCODE){throw 'Native row geometry changed while editing'}
} finally {
 foreach($key in $keys){[Environment]::SetEnvironmentVariable($key,$(if($null -eq $previous[$key]){[NullString]::Value}else{$previous[$key]}),'Process')}
 Pop-Location
}
