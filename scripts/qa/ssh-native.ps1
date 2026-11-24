param([Parameter(Mandatory=$true)][string]$Label,[string]$Executable)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid probe label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($probe)
$gui=if($Executable){(Resolve-Path -LiteralPath $Executable).Path}else{Join-Path $workspace 'target/debug/tomato.exe'}
if(!$gui.StartsWith((Join-Path $workspace 'target/'),[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($gui) -ne 'tomato.exe'){throw 'Expected owned GUI binary'}
$harness=(Get-ChildItem (Join-Path $workspace 'target/debug/deps') -Filter 'ssh_workspace-*.exe'|Sort-Object LastWriteTime -Descending|Select-Object -First 1).FullName
if(!$harness){throw 'Build the workspace harness first'}
$guiHash=(Get-FileHash -LiteralPath $gui).Hash
$harnessHash=(Get-FileHash -LiteralPath $harness).Hash
$guiCopy=Join-Path $probe 'tomato.exe';$harnessCopy=Join-Path $probe 'ssh_workspace.exe'
Copy-Item -LiteralPath $gui -Destination $guiCopy
Copy-Item -LiteralPath $harness -Destination $harnessCopy
if((Get-FileHash -LiteralPath $gui).Hash -ne $guiHash -or (Get-FileHash -LiteralPath $guiCopy).Hash -ne $guiHash -or (Get-FileHash -LiteralPath $harness).Hash -ne $harnessHash -or (Get-FileHash -LiteralPath $harnessCopy).Hash -ne $harnessHash){throw 'Fixture changed while snapshotting'}
$env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP
$env:TOMATO_SSH_CAPTURE_EXE=$guiCopy
$env:TOMATO_SSH_CAPTURE_PATH=Join-Path $workspace ('target/ui-ssh-'+$Label+'.png')
$env:TOMATO_TEST_ASSERT_CURSOR_BLINK='1';$env:WGPU_BACKEND='vulkan'
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json'
$before=(Get-FileHash -LiteralPath $shared).Hash
& $harnessCopy capture_native_and_original_ssh_tools --exact --ignored --nocapture *> (Join-Path $probe 'tests.log')
if($LASTEXITCODE -ne 0){Get-Content (Join-Path $probe 'tests.log') -Tail 25;throw 'Own SSH native fixture failed'}
$match=[regex]::Match([IO.File]::ReadAllText((Join-Path $probe 'tests.log')),'Idle cursor foreground pixel counts: \[([0-9, ]+)\]')
if(!$match.Success){throw 'Missing actual cursor observations'}
$counts=@($match.Groups[1].Value.Split(',')|ForEach-Object {[int]$_.Trim()})
if($counts.Count -ne 8 -or (($counts|Measure-Object -Maximum).Maximum-($counts|Measure-Object -Minimum).Minimum) -lt 64){throw 'Cursor did not visibly blink'}
if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared configuration changed'}
[pscustomobject]@{label=$Label;guiSha256=$guiHash;harnessSha256=$harnessHash;cursorForegroundPixels=$counts;realLoopbackSsh=$true;sharedConfigurationUnchanged=$true;savedHostsContacted=$false;allDesktopGesturesVerified=$false}|ConvertTo-Json -Depth 4|Set-Content (Join-Path $workspace ('target/validation-native-'+$Label+'.json'))
Get-Content (Join-Path $workspace ('target/validation-native-'+$Label+'.json'))
