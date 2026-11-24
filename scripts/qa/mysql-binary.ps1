param([Parameter(Mandatory=$true)][string]$Label,[string]$DatabaseHarness,[string]$Executable)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
Set-Location $workspace
$base=[IO.Path]::GetFullPath('target/database-fixtures')
$fixture=Get-Content (Join-Path $base 'initialized-mysql.json') -Raw|ConvertFrom-Json
if([IO.Path]::GetFullPath($fixture.Runtime) -ne (Join-Path $base 'mysql-runtime-8.4.11') -or ![IO.Path]::GetFullPath($fixture.Data).StartsWith($base+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($fixture.Data) -notmatch '^mysql-data-[a-f0-9]{32}$'){throw 'Invalid owned fixture'}
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
$probe=Join-Path $base ('probes/'+$Label)
if(Test-Path $probe){throw 'Use fresh probe'}
[void][IO.Directory]::CreateDirectory($probe)
$listener=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0)
try{$listener.Start();$port=$listener.LocalEndpoint.Port}finally{$listener.Stop()}
$process=$null
$keys=@('TOMATO_TEST_MYSQL_URL','TOMATO_BINARY_SCHEMA','TOMATO_BINARY_PORT','TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','WGPU_BACKEND')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$gui=$null
function Owned-Pids {
 $owned=@($process.Id)
 foreach($parent in @($process.Id)){
  $owned+=@(Get-CimInstance Win32_Process -Filter "ParentProcessId=$parent"|Where-Object {$_.ExecutablePath -and $_.ExecutablePath.StartsWith($base+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)}|Select-Object -ExpandProperty ProcessId)
 }
 $owned
}
try {
 $process=Start-Process -FilePath (Join-Path $fixture.Runtime 'bin/mysqld.exe') -ArgumentList @('--no-defaults',('--basedir='+$fixture.Runtime.Replace('\','/')),('--datadir='+$fixture.Data.Replace('\','/')),("--port=$port"),'--bind-address=127.0.0.1','--mysqlx=OFF','--max-connections=12','--innodb-buffer-pool-size=32M','--performance-schema=OFF','--skip-log-bin','--console') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe 'mysql.stdout.log') -RedirectStandardError (Join-Path $probe 'mysql.stderr.log')
 $ready=$false
 for($i=0;$i -lt 60;$i++){
  if($process.HasExited){throw 'Owned mysql failed'}
  $owner=@(Owned-Pids)
  $socket=Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction SilentlyContinue
  if($socket|Where-Object {$owner -contains [int]$_.OwningProcess}){
   & (Join-Path $fixture.Runtime 'bin/mysqladmin.exe') --no-defaults --no-login-paths --host=127.0.0.1 "--port=$port" --protocol=TCP --user=root --password= ping --silent *> (Join-Path $probe 'ready.log')
   if($LASTEXITCODE -eq 0){$ready=$true;break}
  }
  Start-Sleep -Milliseconds 100
 }
 if(!$ready){throw 'Owned mysql not ready'}
 $sql=@"
CREATE DATABASE IF NOT EXISTS tomato_fixture;
USE tomato_fixture;
CREATE TEMPORARY TABLE typed_values(id INT, one BIT(1), nine BIT(9), thirtytwo BIT(32), wide BIT(64), payload BLOB);
INSERT INTO typed_values VALUES (1,0,0,0,0,X''),(2,1,1,1,1,X'00FF80'),(3,1,15,256,255,X'000F'),(4,1,256,4095,18446744073709551615,X'00'),(5,NULL,NULL,NULL,NULL,NULL);
SELECT id, HEX(one), HEX(nine), HEX(thirtytwo), HEX(wide), HEX(payload), HEX(CAST(one AS BINARY)), HEX(CAST(nine AS BINARY)), HEX(CAST(thirtytwo AS BINARY)), HEX(CAST(wide AS BINARY)) FROM typed_values ORDER BY id;
"@
 & (Join-Path $fixture.Runtime 'bin/mysql.exe') --no-defaults --no-login-paths --host=127.0.0.1 "--port=$port" --protocol=TCP --user=root --password= --batch --raw "--execute=$sql" *> (Join-Path $probe 'values.tsv')
 if($LASTEXITCODE){Get-Content (Join-Path $probe 'values.tsv');throw 'Probe query failed'}
 Get-Content (Join-Path $probe 'values.tsv')
 if($DatabaseHarness){
  $copy=Join-Path $probe 'database_servers.exe'
  $harnessHash=(Get-FileHash -LiteralPath $DatabaseHarness).Hash
  Copy-Item -LiteralPath $DatabaseHarness -Destination $copy
  if((Get-FileHash -LiteralPath $copy).Hash -ne $harnessHash -or (Get-FileHash -LiteralPath $DatabaseHarness).Hash -ne $harnessHash){throw 'Harness changed'}
  $env:TOMATO_TEST_MYSQL_URL="mysql://root@127.0.0.1:$port/tomato_fixture"
  & $copy --ignored --exact binary_columns::mysql_bit_widths_binary_null_paging_writes_and_transfer_are_lossless --nocapture *> (Join-Path $probe 'binary-tests.log')
  Get-Content (Join-Path $probe 'binary-tests.log') -Tail 12
  if($LASTEXITCODE -or !(Select-String -LiteralPath (Join-Path $probe 'binary-tests.log') -Pattern '1 passed; 0 failed')){throw 'Real binary roundtrip failed'}
  foreach($case in @('query_rows::mysql_query_rows_preserve_origins_atomicity_and_binary_values','native_types::mysql_native_types_match_connector_j','transfer::mysql_streaming_copy_sync_stale_plan_and_batch_cancellation')){
   $log=Join-Path $probe ($case.Split('::')[0]+'.log')
   & $copy --ignored --exact $case --nocapture *> $log
   if($LASTEXITCODE -or !(Select-String -LiteralPath $log -Pattern '1 passed; 0 failed')){Get-Content $log -Tail 20;throw ('MySQL regression failed: '+$case)}
  }
 }
 $native=$null
 if($Executable){
  $copy=Join-Path $probe 'tomato.exe'
  $guiHash=(Get-FileHash -LiteralPath $Executable).Hash
  Copy-Item -LiteralPath $Executable -Destination $copy
  if((Get-FileHash -LiteralPath $copy).Hash -ne $guiHash -or (Get-FileHash -LiteralPath $Executable).Hash -ne $guiHash){throw 'GUI changed'}
  $schema='tomato_binary_native_'+[guid]::NewGuid().ToString('N')
  $sql=@"
CREATE DATABASE $schema;
CREATE TABLE $schema.typed_values(id INT PRIMARY KEY,flag BIT(1),mask BIT(9),wide BIT(64),fixed BINARY(4),payload_var VARBINARY(16),payload BLOB,label VARCHAR(100));
INSERT INTO $schema.typed_values VALUES(1,0,0,0,X'00010200',X'',X'','zero'),(2,1,1,1,X'0000FF00',X'00FF80',X'0080FF000A27','one'),(3,1,15,255,X'',X'0000',X'FFFFFFFF','fifteen'),(4,1,256,18446744073709551615,X'000000FF',NULL,NULL,'high'),(5,NULL,NULL,NULL,NULL,NULL,NULL,NULL);
"@
  & (Join-Path $fixture.Runtime 'bin/mysql.exe') --no-defaults --no-login-paths --host=127.0.0.1 "--port=$port" --protocol=TCP --user=root --password= "--execute=$sql" *> (Join-Path $probe 'native-setup.log')
  if($LASTEXITCODE){throw 'Native table setup failed'}
  $env:TOMATO_DATA_DIR=Join-Path ([IO.Path]::GetTempPath()) ('tomato-binary-fixture-'+[guid]::NewGuid().ToString('N'))
  [void][IO.Directory]::CreateDirectory($env:TOMATO_DATA_DIR)
  $env:TOMATO_BINARY_SCHEMA=$schema;$env:TOMATO_BINARY_PORT="$port"
  $env:TOMATO_UI_PREVIEW='binary_table';$env:TOMATO_UI_CAPTURE=Join-Path $probe 'native.png'
  $env:TOMATO_UI_CAPTURE_FRAMES='1';$env:TOMATO_UI_CAPTURE_DELAY_MS='12000';$env:WGPU_BACKEND='vulkan'
  $gui=Start-Process -FilePath $copy -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $probe 'native.stdout.log') -RedirectStandardError (Join-Path $probe 'native.stderr.log')
  if(!$gui.WaitForExit(50000)){throw 'Native table capture did not exit'}
  if($gui.ExitCode -ne 0 -or !(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Native table capture failed'}
  $proof=Get-Content -LiteralPath (Join-Path $probe 'native.binary.json') -Raw|ConvertFrom-Json
  if(!$proof.ready -or $proof.error -or $proof.total -ne 5 -or $proof.rows.Count -ne 5 -or $proof.columns.Count -ne 8 -or !$proof.databaseRequested -or !$proof.ownedLoopback -or $proof.savedHostsContacted -or $proof.configurationWritten -or $proof.osInputUsed){throw 'Native database controller did not load all rows'}
  foreach($column in 1..6){if(!$proof.columns[$column].binary){throw 'Binary column metadata was lost'}}
  $expected=@(@('00','0000','0000000000000000','00010200','',''),@('01','0001','0000000000000001','0000ff00','00ff80','0080ff000a27'),@('01','000f','00000000000000ff','00000000','0000','ffffffff'),@('01','0100','ffffffffffffffff','000000ff',$null,$null),@($null,$null,$null,$null,$null,$null))
  for($row=0;$row -lt 5;$row++){
   if($proof.rows[$row][0].kind -ne 'text' -or $proof.rows[$row][0].value -ne ($row+1).ToString()){throw 'Native row order changed'}
   for($column=1;$column -le 6;$column++){
    $cell=$proof.rows[$row][$column];$want=$expected[$row][$column-1]
    if($null -eq $want){if($cell.kind -ne 'null'){throw 'Native NULL changed'}}
    elseif($cell.kind -ne 'binary' -or [Convert]::ToHexString([byte[]]$cell.value).ToLowerInvariant() -cne $want){throw "Native binary bytes changed at $row/$column"}
   }
  }
  & (Join-Path $fixture.Runtime 'bin/mysql.exe') --no-defaults --no-login-paths --host=127.0.0.1 "--port=$port" --protocol=TCP --user=root --password= "--execute=DROP DATABASE $schema" *> (Join-Path $probe 'native-cleanup.log')
  if($LASTEXITCODE){throw 'Native table cleanup failed'}
  $native=@{guiSha256=$guiHash;capture=(Join-Path $probe 'native.png');proof=(Join-Path $probe 'native.binary.json');rows=5;binaryColumns=6;normalExit=$true}
 }
 @{port=$port;ownedFixture=$fixture.Data;savedHostsContacted=$false;native=$native}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $probe 'proof.json')
} finally {
 if($null -ne $gui){if(!$gui.HasExited){$gui.Kill($true);[void]$gui.WaitForExit(5000)};$gui.Dispose()}
 if($null -ne $process){
  if(!$process.HasExited){
   $owner=@(Owned-Pids)
   if(Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction SilentlyContinue|Where-Object {$owner -contains [int]$_.OwningProcess}){
    & (Join-Path $fixture.Runtime 'bin/mysqladmin.exe') --no-defaults --no-login-paths --host=127.0.0.1 "--port=$port" --protocol=TCP --user=root --password= shutdown *> (Join-Path $probe 'stop.log')
    [void]$process.WaitForExit(10000)
   }
   if(!$process.HasExited){$process.Kill($true)}
  }
  $process.Dispose()
 }
 foreach($key in $keys){[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}
}
