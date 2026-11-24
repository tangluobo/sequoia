param([ValidateSet('start','stop')][string]$Mode='start',[string]$Label='native-268')
$ErrorActionPreference='Stop'
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid fixture label'}
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$root=Join-Path $workspace ('target/mq-fixtures/'+$Label)
$identity=Join-Path $root 'identity.json'
if($Mode -eq 'stop'){
    $state=Get-Content -LiteralPath $identity -Raw|ConvertFrom-Json
    foreach($entry in $state.processes){
        $process=Get-CimInstance Win32_Process -Filter ('ProcessId='+$entry.pid)
        if($process){
            if($process.ExecutablePath -ne $state.java -or !($process.CommandLine.Contains($root.Replace('\','/')) -or $process.CommandLine.Contains($root))){throw 'Refusing to stop an unverified process'}
            Stop-Process -Id $entry.pid
        }
    }
    return
}
if(Test-Path -LiteralPath $root){throw 'Use a fresh fixture label'}
[void][IO.Directory]::CreateDirectory($root)
$java=(Resolve-Path -LiteralPath 'D:/App/Java/jdk-21.0.12/bin/java.exe').Path
$library=Join-Path $workspace 'target/mq-fixtures/dependencies/lib/*'
if(!(Test-Path -LiteralPath (Join-Path $workspace 'target/mq-fixtures/dependencies/lib/kafka_2.13-3.9.1.jar'))){throw 'Resolve isolated Maven fixture dependencies first'}
function Reserve-Port { $listener=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0);$listener.Start();$port=$listener.LocalEndpoint.Port;$listener.Stop();return $port }
$kafka=Reserve-Port;$controller=Reserve-Port;$names=Reserve-Port;$broker=Reserve-Port;$ha=Reserve-Port
while(@($kafka,$controller,$names,$broker,($broker+1),($broker-2)).Count -ne (@($kafka,$controller,$names,$broker,($broker+1),($broker-2))|Select-Object -Unique).Count){$broker=Reserve-Port}
$slash=$root.Replace('\','/')
$kafkaConfig=Join-Path $root 'kafka.properties';$rocketConfig=Join-Path $root 'broker.properties';$namesConfig=Join-Path $root 'names.properties'
@"
process.roles=broker,controller
node.id=1
controller.quorum.voters=1@127.0.0.1:$controller
listeners=PLAINTEXT://127.0.0.1:$kafka,CONTROLLER://127.0.0.1:$controller
advertised.listeners=PLAINTEXT://127.0.0.1:$kafka
controller.listener.names=CONTROLLER
listener.security.protocol.map=CONTROLLER:PLAINTEXT,PLAINTEXT:PLAINTEXT
inter.broker.listener.name=PLAINTEXT
log.dirs=$slash/kafka-data
num.partitions=2
offsets.topic.replication.factor=1
offsets.topic.num.partitions=1
offsets.load.buffer.size=1048576
log.index.size.max.bytes=1048576
log.segment.delete.delay.ms=0
transaction.state.log.replication.factor=1
transaction.state.log.min.isr=1
group.initial.rebalance.delay.ms=0
num.network.threads=2
num.io.threads=2
log.segment.bytes=8388608
log.retention.bytes=16777216
socket.request.max.bytes=16777216
"@ | Set-Content -LiteralPath $kafkaConfig -Encoding utf8NoBOM
@"
listenPort=$names
"@|Set-Content -LiteralPath $namesConfig -Encoding utf8NoBOM
@"
brokerClusterName=TomatoOwnedFixture
brokerName=owned-broker
brokerId=0
namesrvAddr=127.0.0.1:$names
listenPort=$broker
brokerIP1=127.0.0.1
brokerIP2=127.0.0.1
brokerRole=ASYNC_MASTER
flushDiskType=ASYNC_FLUSH
autoCreateTopicEnable=true
storePathRootDir=$slash/rocket-data
storePathCommitLog=$slash/rocket-data/commitlog
haListenPort=$ha
mappedFileSizeCommitLog=16777216
mappedFileSizeConsumeQueue=300000
maxHashSlotNum=5000
maxIndexNum=20000
transientStorePoolEnable=false
diskMaxUsedSpaceRatio=99
sendMessageThreadPoolNums=2
pullMessageThreadPoolNums=2
queryMessageThreadPoolNums=2
adminBrokerThreadPoolNums=2
clientManageThreadPoolNums=2
consumeManageThreadPoolNums=2
"@|Set-Content -LiteralPath $rocketConfig -Encoding utf8NoBOM
$processes=@()
function Launch([string]$Name,[string[]]$Arguments){
    $argumentFile=Join-Path $root ($Name+'.args')
    $heap=if($Name -eq 'kafka'){'-Xmx512m'}else{'-Xmx256m'}
    $arguments=@('-Xms96m',$heap,'-XX:MaxDirectMemorySize=128m',('-Duser.home="'+$slash+'/profile"'),('-Drocketmq.home.dir="'+$slash+'"'),'-cp',('"'+$library.Replace('\','/')+'"'))+$Arguments
    [IO.File]::WriteAllLines($argumentFile,$arguments,[Text.UTF8Encoding]::new($false))
    $p=Start-Process -FilePath $java -ArgumentList ('@"'+$argumentFile+'"') -WorkingDirectory $root -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $root ($Name+'.stdout.log')) -RedirectStandardError (Join-Path $root ($Name+'.stderr.log'))
    return [pscustomobject]@{pid=$p.Id;name=$Name;process=$p}
}
try{
    $uuid=[Convert]::ToBase64String([Guid]::NewGuid().ToByteArray()).TrimEnd('=').Replace('+','-').Replace('/','_')
    # A valid URL-base64 cluster ID may start with '-'. Keep it in the same
    # argument as the option so argparse cannot interpret it as another flag.
    $format=Launch 'format' @('kafka.tools.StorageTool','format',('--cluster-id='+$uuid),'-c',('"'+$kafkaConfig.Replace('\','/')+'"'))
    $p=$format.process
    if(!$p.WaitForExit(180000)-or$p.ExitCode -ne 0){throw 'Kafka isolated storage format failed'}
    $processes+=Launch 'names' @('org.apache.rocketmq.namesrv.NamesrvStartup','-c',('"'+$namesConfig.Replace('\','/')+'"'))
    $processes+=Launch 'broker' @('org.apache.rocketmq.broker.BrokerStartup','-c',('"'+$rocketConfig.Replace('\','/')+'"'))
    $processes+=Launch 'kafka' @('kafka.Kafka',('"'+$kafkaConfig.Replace('\','/')+'"'))
}finally{
    @{java=$java;root=$root;processes=@($processes|Select-Object pid,name);kafka="127.0.0.1:$kafka";rocketmq="127.0.0.1:$names";broker="127.0.0.1:$broker";savedHostsContacted=$false}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $identity
}
Get-Content -LiteralPath $identity

