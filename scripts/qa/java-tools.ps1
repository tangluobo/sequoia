param([Parameter(Mandatory=$true)][string]$Label,[Parameter(Mandatory=$true)][string]$Template)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid oracle label'}
$probe=Join-Path $workspace ('target/oracle-tools-'+$Label)
if(!(Test-Path -LiteralPath (Join-Path $probe 'classes/ToolPaneOracle.class'))){throw 'Compile current owned oracle first'}
$templatePath=(Resolve-Path -LiteralPath $Template).Path
$templateRoot=[IO.Path]::GetDirectoryName($templatePath)
$base=[IO.File]::ReadAllLines($templatePath)
$classAt=[Array]::IndexOf($base,'ToolPaneOracle')
if($classAt -lt 0){throw 'Expected tool oracle argument template'}
$prefix=$base[0..($classAt-1)]|ForEach-Object {$_.Replace($templateRoot.Replace('\','/'),$probe.Replace('\','/'))}
[void][IO.Directory]::CreateDirectory((Join-Path $probe 'profile'))
$source=Join-Path $probe 'source.png';Copy-Item -LiteralPath (Join-Path $workspace 'crates/services/tests/fixtures/icons/source-2.png') -Destination $source
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
foreach($case in @(@('shortcut',0),@('shortcut',1),@('shortcut',2),@('icon',0),@('icon',1),@('downloader',0),@('downloader',1),@('downloader',2))) {
 $name=$case[0]+'-'+$case[1];$image=Join-Path $probe ($name+'.png');$arguments=Join-Path $probe ($name+'.args')
 $lines=@($prefix)+@('-Dtomato.oracle.height=846','ToolPaneOracle',$case[0],[string]$case[1],('"'+$image.Replace('\','/')+'"'),('"'+$source.Replace('\','/')+'"'))
 [IO.File]::WriteAllLines($arguments,$lines,[Text.UTF8Encoding]::new($false))
 $process=Start-Process -FilePath 'D:/App/Java/graalvm-jdk-25.0.3+9.1/bin/java.exe' -ArgumentList ('@'+$arguments) -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($name+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($name+'.stderr.log'))
 try {if(!$process.WaitForExit(45000)){throw ('Owned Java scene timed out: '+$name)};if($process.ExitCode -ne 0){throw ('Java scene failed: '+$name)};if(!(Test-Path -LiteralPath $image)){throw 'Missing owned scene snapshot'}}finally{if(!$process.HasExited){$process.Kill()};$process.Dispose()}
}
if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared legacy configuration changed'}
$sources=@('crates/services/tests/java/ToolPaneOracle.java');$original='D:/Data/Git/tangluobo/tomata/src/main/java/com/tangluobo/tomato/module/tools'
$sources+=@('DesktopShortcutPane.java','ImageBackgroundRemoverPane.java','IconImageProcessor.java','DownloaderPane.java')|ForEach-Object {Join-Path $original $_}
$identities=@();foreach($path in $sources){$hash=Get-FileHash -LiteralPath $path;$identities+=@{path=$hash.Path;sha256=$hash.Hash}}
@{sources=$identities;ownedSceneOnly=$true;isolatedPreferences=$true;sharedConfigurationUnchanged=$true;sharedSha256=$before;saveOrDownloadInvoked=$false}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $probe 'identity.json') -Encoding utf8
