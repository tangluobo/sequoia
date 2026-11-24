#Requires -Version 7.0
param([Parameter(Mandatory=$true)][string]$Label,[string]$Harness,[string]$UiHarness,[string]$SourceWorker,[string[]]$CleanupDatabases=@(),[string]$BackendProof,[string]$TestFilter,[string]$UiTestFilter,[ValidateRange(30,300)][int]$StartupTimeoutSeconds=45)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$base=Join-Path $workspace 'target/database-fixtures'
$guard=[IO.Path]::GetFullPath($base)+[IO.Path]::DirectorySeparatorChar
$probe=Join-Path $base ('probes/'+$Label)
if($Label -notmatch '^[a-z0-9-]+$' -or (Test-Path -LiteralPath $probe)){throw 'Use a fresh fixture label'}
[void][IO.Directory]::CreateDirectory($probe)
function Read-Fixture([string]$Name,[string]$RuntimeName){
    $fixture=Get-Content (Join-Path $base ('initialized-'+$Name+'.json')) -Raw|ConvertFrom-Json
    $expected=Join-Path $base $RuntimeName
    if([IO.Path]::GetFullPath($fixture.Runtime) -ne $expected){throw 'Unexpected database runtime'}
    if([IO.Path]::GetFileName($fixture.Data) -notmatch ('^'+$Name+'-data-[a-f0-9]{32}$')){throw 'Unexpected fixture data name'}
    if(!(Get-Item -LiteralPath $fixture.Data).FullName.StartsWith($guard,[StringComparison]::OrdinalIgnoreCase)){throw 'Data escaped fixture root'}
    $fixture
}
function Get-FreePort {
    $listener=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0)
    try{$listener.Start();$listener.LocalEndpoint.Port}finally{$listener.Stop()}
}
function Get-OwnedPids($Process){
    $owned=[Collections.Generic.HashSet[int]]::new();[void]$owned.Add($Process.Id)
    $parents=@($Process.Id)
    for($depth=0;$depth -lt 4 -and $parents.Count -gt 0;$depth++){
        $children=@(foreach($parent in $parents){Get-CimInstance Win32_Process -Filter "ParentProcessId=$parent"|Where-Object {$_.ExecutablePath -and $_.ExecutablePath.StartsWith($guard,[StringComparison]::OrdinalIgnoreCase)}})
        $parents=@(foreach($child in $children){if($owned.Add([int]$child.ProcessId)){[int]$child.ProcessId}})
    }
    foreach($entry in $owned){$entry}
}
function Wait-OwnedListener($Process,[int]$Port){
    $deadline=[DateTime]::UtcNow.AddSeconds($StartupTimeoutSeconds)
    while([DateTime]::UtcNow -lt $deadline){
        $Process.Refresh();if($Process.HasExited){throw 'Own database exited during startup'}
        $owned=@(Get-OwnedPids $Process)
        $listener=Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue
        if($listener|Where-Object {$owned -contains [int]$_.OwningProcess}){return}
        Start-Sleep -Milliseconds 100
    }
    throw ('Own database listener timed out: '+$Process.ProcessName+' port '+$Port)
}
function Wait-OwnedReady($Process,[string]$Program,[string[]]$ProbeArguments,[string]$Log){
    $deadline=[DateTime]::UtcNow.AddSeconds($StartupTimeoutSeconds)
    while([DateTime]::UtcNow -lt $deadline){
        $Process.Refresh();if($Process.HasExited){throw 'Own database exited before accepting connections'}
        & $Program @ProbeArguments *> $Log
        if($LASTEXITCODE -eq 0){return}
        Start-Sleep -Milliseconds 100
    }
    throw 'Own database is listening but did not become ready'
}
$mysql=Read-Fixture 'mysql' 'mysql-runtime-8.4.11'
$postgres=Read-Fixture 'postgres' 'postgres-runtime-17.11'
if(!$Harness){$Harness=(Get-ChildItem (Join-Path $workspace 'target/debug/deps') -Filter 'database_servers-*.exe'|Sort-Object LastWriteTime -Descending|Select-Object -First 1).FullName}
if(!$Harness){throw 'Build the workspace test harness first'}
$copy=Join-Path $probe 'database_servers.exe'
$hash=(Get-FileHash -LiteralPath $Harness).Hash
Copy-Item -LiteralPath $Harness -Destination $copy
if((Get-FileHash -LiteralPath $Harness).Hash -ne $hash -or (Get-FileHash -LiteralPath $copy).Hash -ne $hash){throw 'Harness changed while snapshotting'}
$listing=& $copy --list
$ignoredListing=& $copy --ignored --list
$backendExpected=@($ignoredListing|Where-Object {$_ -match ': test$'}).Count
if($backendExpected -lt 19){throw 'Incomplete database regression harness'}
if(($listing -match 'scopes::.*database_scopes').Count -ne 2){throw 'Harness must contain both scope regressions'}
if(($listing -match 'native_types::.*native_types_match').Count -ne 2){throw 'Harness must contain both native JDBC type regressions; wait for the current workspace build'}
if(($listing -match 'metadata::.*metadata_matches_original_service').Count -ne 2){throw 'Harness must contain both original-service metadata regressions'}
if(($listing -match 'script::.*sql_script_continues').Count -ne 2){throw 'Harness must contain both script continuation/EXPLAIN regressions'}
if(($listing -match 'query_rows::.*query_rows_preserve').Count -ne 2){throw 'Harness must contain both query-result writeback regressions'}
if(($listing -match 'backup::mysql_nb3_original_restore_native_snapshot_streaming_and_cancellation').Count -ne 1){throw 'Harness must contain the real nb3 backup regression'}
if(($listing -match 'backup_postgres::postgres_nb3_schema_types_dependencies_exact_rows_atomic_restore_and_cancellation').Count -ne 1){throw 'Harness must contain the PostgreSQL nb3 backup regression'}
if(($listing -match 'objects::.*objects_preserve_comments_composite_foreign_keys').Count -ne 2){throw 'Harness must contain both real object-browser catalog regressions'}
if(($listing -match 'transfer::.*streaming_copy_sync_stale_plan_and_batch_cancellation').Count -ne 2){throw 'Harness must contain both streaming transfer regressions'}
if(($listing -match 'transfer::mysql_postgres_cross_dialect_transfer_preserves_decimal_binary_null_and_unicode').Count -ne 1){throw 'Harness must contain the cross-dialect roundtrip'}
$uiCopy=$null;$uiHash=$null
if($UiHarness){
    $uiCopy=Join-Path $probe 'tomato_ui_tests.exe';$uiHash=(Get-FileHash -LiteralPath $UiHarness).Hash
    Copy-Item -LiteralPath $UiHarness -Destination $uiCopy
    if((Get-FileHash -LiteralPath $UiHarness).Hash -ne $uiHash -or (Get-FileHash -LiteralPath $uiCopy).Hash -ne $uiHash){throw 'UI harness changed while snapshotting'}
    $uiListing=& $uiCopy --list
    if(($uiListing -match 'app::backup_ui::server_test::mysql_native_backup_and_restore_controls_roundtrip_real_data_and_preserve_configuration').Count -ne 1){throw 'UI harness must contain the complete native backup workflow regression'}
}
$myPort=Get-FreePort;$pgPort=Get-FreePort
while($pgPort -eq $myPort){$pgPort=Get-FreePort}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json'
$sharedBefore=if(Test-Path $shared){(Get-FileHash $shared).Hash}else{$null}
$myProcess=$null;$pgProcess=$null
$savedWorker=$env:TOMATO_IMPORT_SOURCE_EXE;$savedDataDirectory=$env:TOMATO_DATA_DIR
$workerHash=$null
try{
    if(!$SourceWorker){$SourceWorker=$env:TOMATO_IMPORT_SOURCE_EXE}
    if($SourceWorker){
        $workerSource=(Resolve-Path -LiteralPath $SourceWorker).Path
        $workerHash=(Get-FileHash -LiteralPath $workerSource).Hash
        $workerCopy=Join-Path $probe 'tomato-import-source.exe'
        Copy-Item -LiteralPath $workerSource -Destination $workerCopy
        if((Get-FileHash -LiteralPath $workerSource).Hash -ne $workerHash -or (Get-FileHash -LiteralPath $workerCopy).Hash -ne $workerHash){throw 'Source worker changed while snapshotting'}
        $env:TOMATO_IMPORT_SOURCE_EXE=$workerCopy
    }elseif(!$TestFilter -and !$BackendProof){throw 'Set -SourceWorker to the current built tomato-cli or desktop executable'}
    $env:TOMATO_DATA_DIR=Join-Path $probe 'profile'
    [void][IO.Directory]::CreateDirectory($env:TOMATO_DATA_DIR)
    $basedirArg='--basedir='+$mysql.Runtime.Replace('\','/')
    $datadirArg='--datadir='+$mysql.Data.Replace('\','/')
    $portArg='--port='+$myPort
    $myProcess=Start-Process -FilePath (Join-Path $mysql.Runtime 'bin/mysqld.exe') -ArgumentList @('--no-defaults',$basedirArg,$datadirArg,$portArg,'--bind-address=127.0.0.1','--mysqlx=OFF','--max-connections=24','--innodb-buffer-pool-size=32M','--performance-schema=OFF','--skip-log-bin','--console') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe 'mysql.stdout.log') -RedirectStandardError (Join-Path $probe 'mysql.stderr.log')
    $pgProcess=Start-Process -FilePath (Join-Path $postgres.Runtime 'bin/postgres.exe') -ArgumentList @('-D',$postgres.Data,'-h','127.0.0.1','-p',$pgPort,'-c','shared_buffers=16MB','-c','max_connections=24') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe 'postgres.stdout.log') -RedirectStandardError (Join-Path $probe 'postgres.stderr.log')
    [pscustomobject]@{mysqlPid=$myProcess.Id;postgresPid=$pgProcess.Id;mysqlPort=$myPort;postgresPort=$pgPort;harnessSha256=$hash}|ConvertTo-Json|Set-Content (Join-Path $probe 'lease.json')
    Wait-OwnedListener $myProcess $myPort
    Wait-OwnedListener $pgProcess $pgPort
    Wait-OwnedReady $myProcess (Join-Path $mysql.Runtime 'bin/mysqladmin.exe') @('--no-defaults','--no-login-paths','--host=127.0.0.1',"--port=$myPort",'--protocol=TCP','--user=root','--password=','ping','--silent') (Join-Path $probe 'mysql-ready.log')
    Wait-OwnedReady $pgProcess (Join-Path $postgres.Runtime 'bin/pg_isready.exe') @('-h','127.0.0.1','-p',"$pgPort",'-U','tomato_fixture','-d','postgres') (Join-Path $probe 'postgres-ready.log')
    & (Join-Path $mysql.Runtime 'bin/mysql.exe') --no-defaults --no-login-paths --host=127.0.0.1 "--port=$myPort" --protocol=TCP --user=root --password= --execute='CREATE DATABASE IF NOT EXISTS tomato_fixture CHARACTER SET utf8mb4' *> (Join-Path $probe 'mysql-create.log')
    if($LASTEXITCODE -ne 0){throw 'Own MySQL fixture database creation failed'}
    $serviceFile=Join-Path $probe 'pg_service.conf'
    @('[tomato_fixture]','host=127.0.0.1',("port=$pgPort"),'user=tomato_fixture','dbname=postgres','sslmode=disable')|Set-Content -LiteralPath $serviceFile -Encoding utf8NoBOM
    $env:PGSERVICEFILE=$serviceFile
    $env:PGSERVICE='tomato_fixture'
    $env:PGSSLMODE='disable'
    $exists=& (Join-Path $postgres.Runtime 'bin/psql.exe') -X -h 127.0.0.1 -p $pgPort -U tomato_fixture -d postgres -v ON_ERROR_STOP=1 -t -A -c "SELECT 1 FROM pg_database WHERE datname='tomato_fixture'" 2> (Join-Path $probe 'postgres-check.log')
    if($LASTEXITCODE -ne 0){throw 'Own PostgreSQL fixture database check failed'}
    if(([string]$exists).Trim() -ne '1'){
        & (Join-Path $postgres.Runtime 'bin/psql.exe') -X -h 127.0.0.1 -p $pgPort -U tomato_fixture -d postgres -v ON_ERROR_STOP=1 -c 'CREATE DATABASE tomato_fixture' *> (Join-Path $probe 'postgres-create.log')
        if($LASTEXITCODE -ne 0){throw 'Own PostgreSQL fixture database creation failed'}
    }
    $env:TOMATO_TEST_MYSQL_URL="mysql://root@127.0.0.1:$myPort/tomato_fixture"
    $env:TOMATO_TEST_MYSQL_SCOPE_URL=$env:TOMATO_TEST_MYSQL_URL
    $env:TOMATO_TEST_POSTGRES_URL="postgres://tomato_fixture@127.0.0.1:$pgPort/tomato_fixture"
    foreach($ownedDatabase in $CleanupDatabases){
        if($ownedDatabase -notmatch '^tomato_backup_(ui_)?[a-f0-9]{32}(_restore)?$'){throw 'Cleanup only accepts exact disposable backup fixture names'}
        & (Join-Path $postgres.Runtime 'bin/psql.exe') -X -h 127.0.0.1 -p $pgPort -U tomato_fixture -d postgres -v ON_ERROR_STOP=1 -c ('DROP DATABASE IF EXISTS "'+$ownedDatabase+'"') *> (Join-Path $probe ('cleanup-'+$ownedDatabase+'.log'))
        if($LASTEXITCODE -ne 0){throw 'Owned backup fixture cleanup failed'}
    }
    if($env:TOMATO_SQL_NATIVE_ORACLE_ARGS){
        if(!$env:TOMATO_ORACLE_JDK){throw 'Set TOMATO_ORACLE_JDK for the optional JDBC oracle'}
        & (Join-Path $env:TOMATO_ORACLE_JDK 'bin/java.exe') ('@'+$env:TOMATO_SQL_NATIVE_ORACLE_ARGS) *> (Join-Path $probe 'jdbc-oracle.log')
        if($LASTEXITCODE -ne 0){Get-Content (Join-Path $probe 'jdbc-oracle.log') -Tail 20;throw 'Original JDBC getter oracle failed'}
    }
    [pscustomobject]@{mysqlPid=$myProcess.Id;postgresPid=$pgProcess.Id;mysqlPort=$myPort;postgresPort=$pgPort;harnessSha256=$hash}|ConvertTo-Json|Set-Content (Join-Path $probe 'lease.json')
    $backendLog=Join-Path $probe 'tests.log'
    if($BackendProof){
        $proofPath=(Resolve-Path -LiteralPath $BackendProof).Path
        if(!$proofPath.StartsWith((Join-Path $base 'probes/'),[StringComparison]::OrdinalIgnoreCase)){throw 'Backend evidence must stay in the owned probes directory'}
        $lease=Get-Content -LiteralPath (Join-Path $proofPath 'lease.json') -Raw|ConvertFrom-Json
        $backendLog=Join-Path $proofPath 'tests.log'
        if($lease.harnessSha256 -ne $hash -or (Get-FileHash (Join-Path $proofPath 'database_servers.exe')).Hash -ne $hash -or !(Select-String -LiteralPath $backendLog -Pattern ("test result: ok\. $backendExpected passed; 0 failed; 0 ignored"))){throw 'Backend evidence does not match this exact passing harness'}
    }else{
        if($TestFilter){& $copy $TestFilter --ignored --nocapture --test-threads=1 *> $backendLog}else{& $copy --ignored --nocapture --test-threads=1 *> $backendLog}
        if($LASTEXITCODE -ne 0){Get-Content $backendLog -Tail 45;throw 'Isolated database regressions failed'}
    }
    if($uiCopy -and !$UiTestFilter){
        foreach($kind in @('mysql','postgres')){
            $backupLog=Join-Path $probe ($kind+'-backup-ui-tests.log')
            & $uiCopy --ignored --exact ('app::backup_ui::server_test::'+$kind+'_native_backup_and_restore_controls_roundtrip_real_data_and_preserve_configuration') --nocapture *> $backupLog
            if($LASTEXITCODE -ne 0){Get-Content $backupLog -Tail 45;throw 'Native backup controls failed against the owned database'}
            if(!(Select-String -LiteralPath $backupLog -Pattern '1 passed; 0 failed')){throw 'Native backup workflow test missing'}
        }
    }
    if($uiCopy -and !$UiTestFilter){
        foreach($kind in @('mysql','postgres')){
            $designerLog=Join-Path $probe ($kind+'-designer-ui-tests.log')
            $designerTest='app::tests::designer::'+$kind+'_designer_controls_create_edit_preview_save_refresh_and_preserve_failed_draft'
            & $uiCopy --ignored --exact $designerTest --nocapture *> $designerLog
            if($LASTEXITCODE -ne 0){Get-Content -LiteralPath $designerLog -Tail 45;throw 'Native designer workflow failed against the owned database'}
            if(!(Select-String -LiteralPath $designerLog -Pattern '1 passed; 0 failed')){throw 'Native designer workflow was not present in the harness'}
        }
    }
    if($uiCopy -and !$UiTestFilter){
        foreach($kind in @('mysql','postgres')){
            $transferLog=Join-Path $probe ($kind+'-transfer-ui-tests.log')
            $transferTest='app::table_transfer::tests::'+$kind+'_native_transfer_next_preview_execute_close_and_config_unchanged'
            & $uiCopy --ignored --exact $transferTest --nocapture *> $transferLog
            if($LASTEXITCODE -ne 0){Get-Content -LiteralPath $transferLog -Tail 45;throw 'Native transfer workflow failed'}
            if(!(Select-String -LiteralPath $transferLog -Pattern '1 passed; 0 failed')){throw 'Native transfer test missing'}
        }
        foreach($kind in @('mysql','postgres')){
            $tableLog=Join-Path $probe ($kind+'-tree-table-actions-ui-tests.log')
            & $uiCopy --ignored --exact ('app::tree_objects::tests::'+$kind+'_tree_table_menus_confirm_cancel_multiselect_and_refresh_real_data') --nocapture *> $tableLog
            if($LASTEXITCODE -ne 0){Get-Content -LiteralPath $tableLog -Tail 50;throw 'Native sidebar table actions failed'}
            if(!(Select-String -LiteralPath $tableLog -Pattern '1 passed; 0 failed')){throw 'Native sidebar table action test missing'}
        }
        $mysqlTreeLog=Join-Path $probe 'mysql-tree-ui-tests.log'
        & $uiCopy --ignored --exact app::sql_tree::direct::tests::mysql_native_tree_opens_without_query_and_keeps_query_transaction_on_database_close --nocapture *> $mysqlTreeLog
        if($LASTEXITCODE -ne 0){Get-Content -LiteralPath $mysqlTreeLog -Tail 45;throw 'Native MySQL tree failed'}
        if(!(Select-String -LiteralPath $mysqlTreeLog -Pattern '1 passed; 0 failed')){throw 'Native MySQL tree test missing'}
        $treeLog=Join-Path $probe 'postgres-tree-ui-tests.log'
        & $uiCopy --ignored --exact app::sql_tree::postgres::tests::postgres_native_tree_database_scope_icons_refresh_close_and_reopen --nocapture *> $treeLog
        if($LASTEXITCODE -ne 0){Get-Content -LiteralPath $treeLog -Tail 45;throw 'Native PostgreSQL tree failed'}
        if(!(Select-String -LiteralPath $treeLog -Pattern '1 passed; 0 failed')){throw 'Native tree test missing'}
    }
    if($uiCopy -and !$UiTestFilter){
        foreach($kind in @('mysql','postgres')){
            $adminLog=Join-Path $probe ($kind+'-admin-ui-tests.log')
            & $uiCopy --ignored --exact ('app::database_admin::tests::'+$kind+'_native_database_create_edit_sql_preview_confirm_drop_and_preserve_config') --nocapture *> $adminLog
            if($LASTEXITCODE -ne 0){Get-Content -LiteralPath $adminLog -Tail 45;throw 'Native database administration failed'}
            if(!(Select-String -LiteralPath $adminLog -Pattern '1 passed; 0 failed')){throw 'Native admin test missing'}
        }
    }
    if($uiCopy -and $UiTestFilter){
        $uiLog=Join-Path $probe 'filtered-ui-tests.log'
        & $uiCopy --ignored $UiTestFilter --test-threads=1 --nocapture *> $uiLog
        if($LASTEXITCODE -ne 0){Get-Content -LiteralPath $uiLog -Tail 65;throw 'Filtered sidebar UI workflow failed'}
        if(!(Select-String -LiteralPath $uiLog -Pattern 'test result: ok\. [1-9][0-9]* passed; 0 failed')){throw 'Filtered UI tests did not run'}
    }
    $sharedAfter=if(Test-Path $shared){(Get-FileHash $shared).Hash}else{$null}
    if($sharedBefore -ne $sharedAfter){throw 'Shared connection file changed'}
    if($TestFilter){
        if(!(Select-String -LiteralPath $backendLog -Pattern 'test result: ok\. [1-9][0-9]* passed; 0 failed')){throw 'Filtered server tests did not run'}
        [pscustomobject]@{label=$Label;backendTestLog=$backendLog;testFilter=$TestFilter;uiTestFilter=$UiTestFilter;uiHarnessSha256=$uiHash;harnessSha256=$hash;mysqlVersion=$mysql.Version;postgresVersion=$postgres.Version;sharedConfigurationUnchanged=$true;savedHostsContacted=$false}|ConvertTo-Json|Set-Content (Join-Path $probe 'validation.json')
        return
    }
    [pscustomobject]@{label=$Label;backendTestLog=$backendLog;backendEvidenceInherited=[bool]$BackendProof;backendTests=$backendExpected;sourceWorkerSha256=$workerHash;mysqlVersion=$mysql.Version;postgresVersion=$postgres.Version;harnessSha256=$hash;uiHarnessSha256=$uiHash;realWorkbench=$true;realDatabaseScopes=$true;nativeJdbcTypes=$true;originalServiceMetadata=$true;scriptExecution=$true;queryResultWriteback=$true;realNb3Backup=$true;nativeBackupControls=[bool]$uiCopy;nativeDesignerControls=[bool]$uiCopy;nativePostgresDesignerControls=[bool]$uiCopy;nativeTransferControls=[bool]$uiCopy;nativePostgresTreeControls=[bool]$uiCopy;nativeMysqlTreeControls=[bool]$uiCopy;nativeDatabaseAdminControls=[bool]$uiCopy;nativeTreeTableMutationControls=[bool]$uiCopy;realStreamingTableTransfer=$true;allSessionsClosed=$true;sharedConfigurationUnchanged=$true;savedHostsContacted=$false}|ConvertTo-Json|Set-Content (Join-Path $probe 'validation.json')
    Get-Content (Join-Path $probe 'validation.json')
}finally{
    $env:TOMATO_IMPORT_SOURCE_EXE=$savedWorker;$env:TOMATO_DATA_DIR=$savedDataDirectory
    $mysqlLeaseId=if($null -ne $myProcess){$myProcess.Id}else{$null}
    # Identity checks restrict graceful shutdown and fallback termination to this lease.
    foreach($process in @($myProcess,$pgProcess)){
        if($null -eq $process){continue}
        try{
            $process.Refresh()
            if(!$process.HasExited){
                if($process.Id -eq $mysqlLeaseId){
                    $listener=Get-NetTCPConnection -State Listen -LocalPort $myPort -ErrorAction SilentlyContinue
                    $owned=@(Get-OwnedPids $process)
                    if($listener|Where-Object {$owned -contains [int]$_.OwningProcess}){
                        & (Join-Path $mysql.Runtime 'bin/mysqladmin.exe') --no-defaults --no-login-paths --host=127.0.0.1 "--port=$myPort" --protocol=TCP --user=root --password= shutdown *> (Join-Path $probe 'mysql-stop.log')
                        if($LASTEXITCODE -eq 0){[void]$process.WaitForExit(10000)}
                    }
                }else{
                    $pidFile=Join-Path $postgres.Data 'postmaster.pid'
                    if((Test-Path -LiteralPath $pidFile) -and ([IO.File]::ReadAllLines($pidFile)[0]) -eq [string]$process.Id){
                        & (Join-Path $postgres.Runtime 'bin/pg_ctl.exe') -D $postgres.Data stop -m fast -w -t 15 *> (Join-Path $probe 'postgres-stop.log')
                        if($LASTEXITCODE -eq 0){[void]$process.WaitForExit(10000)}
                    }
                }
            }
        }catch{
            $_.Exception.Message|Set-Content (Join-Path $probe ('cleanup-'+$process.Id+'.log'))
        }finally{
            $process.Refresh();if(!$process.HasExited){$process.Kill($true);[void]$process.WaitForExit(10000)};$process.Dispose()
        }
    }
}
