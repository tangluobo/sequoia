param([Parameter(Mandatory=$true)][string]$Label,[string]$Executable,[ValidateSet('development','color_transpose')][string]$Feature='development',[string[]]$States=@('empty','editor','version','dependencies','logs'),[switch]$JavaOracle)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid label'}
if(!$Executable){$Executable=Join-Path $workspace 'target/qa-active-200/debug/tomato.exe'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh label'}
[void][IO.Directory]::CreateDirectory($probe)
$sha=(Get-FileHash -LiteralPath $Executable).Hash
$exe=Join-Path $probe 'tomato.exe';Copy-Item -LiteralPath $Executable -Destination $exe
if((Get-FileHash -LiteralPath $exe).Hash -ne $sha){throw 'GUI changed while copying'}
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json';$before=(Get-FileHash -LiteralPath $shared).Hash
$captures=@()
$keys=@('TEMP','TMP','TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_DEVELOPMENT_STATE','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','WGPU_BACKEND')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
try {
foreach($state in $States){
    $profile=Join-Path $workspace ('target/tomato-development-fixture-'+[guid]::NewGuid().ToString('N'))
    $project=Join-Path $profile 'project'
    [void][IO.Directory]::CreateDirectory((Join-Path $project 'src/main/java/demo'))
    [void][IO.Directory]::CreateDirectory((Join-Path $project 'src/main/resources'))
    [IO.File]::WriteAllText((Join-Path $project 'pom.xml'),'<project/>')
    [IO.File]::WriteAllText((Join-Path $project 'package.json'),'{"scripts":{"dev":"node app.js","build":"node build.js"}}')
    [IO.File]::WriteAllText((Join-Path $project 'src/main/java/demo/App.java'),'package demo; public class App { public static void main(String[] args) { System.out.println("Tomato fixture"); } }')
    [IO.File]::WriteAllText((Join-Path $project 'src/main/resources/application-dev.yml'),'spring.profiles.active: dev')
    $config=@{projects=@(@{name='示例项目';projectType='MAVEN_NODE';path=$project;runType='JAVA';runTarget='demo.App';activeProfiles=@();jdkId=$null;nodeId=$null});runtimes=@();buildTools=@()}
    $config|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $profile 'development.json') -Encoding utf8NoBOM
    $configHash=(Get-FileHash -LiteralPath (Join-Path $profile 'development.json')).Hash
    & git -C $project init --template= -q
    if($LASTEXITCODE){throw 'Owned project git init failed'}
    $env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP
    $env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_PREVIEW=$Feature;$env:TOMATO_DEVELOPMENT_STATE=$state
    $env:TOMATO_UI_CAPTURE=Join-Path $probe ($state+'.png');$env:TOMATO_UI_CAPTURE_FRAMES='1';$env:WGPU_BACKEND='vulkan'
    $env:TOMATO_UI_CAPTURE_DELAY_MS='7000'
    $process=Start-Process -FilePath $exe -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($state+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($state+'.stderr.log'))
    try {
        for($attempt=0;$attempt -lt 6 -and !$process.HasExited;$attempt++){[void]$process.WaitForExit(10000)}
        if(!$process.HasExited){throw 'Own development viewport timed out'}
        if($process.ExitCode -ne 0){throw 'Own development viewport failed'}
    } finally {if(!$process.HasExited){$process.Kill()};$process.Dispose()}
    if(!(Test-Path -LiteralPath $env:TOMATO_UI_CAPTURE)){throw 'Missing framebuffer'}
    $javaImage=$null
    if($JavaOracle){
        $classpath=(Get-Content (Join-Path $workspace 'target/development-oracle-370/compile.args'))[3]|ConvertFrom-Json
        $classes=Join-Path $workspace $(if($Feature -eq 'development'){'target/development-oracle-390/classes'}else{'target/color-transpose-oracle-381/classes'})
        if(!(Test-Path -LiteralPath $classes)){throw 'Compile the current Java source oracle before capture'}
        [void][IO.Directory]::CreateDirectory((Join-Path $profile '.tomato'))
        Copy-Item -LiteralPath (Join-Path $profile 'development.json') -Destination (Join-Path $profile '.tomato/development.json')
        $javaImage=Join-Path $probe ('java-'+$state+'.png')
        $oracleClass=if($Feature -eq 'development'){'com.tangluobo.tomato.module.tools.DevelopmentOracle'}else{'com.tangluobo.tomato.module.connect.view.ColorTransposeOracle'}
        $argsList=@('-Dprism.order=sw',('-Duser.home='+$profile),'-cp',($classes+';'+$classpath),$oracleClass,$state,$javaImage,(Join-Path $project 'src/main/java/demo/App.java'))
        $argsFile=Join-Path $probe ($state+'.java.args')
        $argsList|ForEach-Object{ConvertTo-Json -Compress $_}|Set-Content -LiteralPath $argsFile -Encoding utf8NoBOM
        $java=Start-Process -FilePath 'D:/App/Java/graalvm-jdk-25.0.3+9.1/bin/java.exe' -ArgumentList ('"@'+$argsFile+'"') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($state+'.java.stdout.log')) -RedirectStandardError (Join-Path $probe ($state+'.java.stderr.log'))
        try{for($attempt=0;$attempt -lt 5 -and !$java.HasExited;$attempt++){[void]$java.WaitForExit(10000)};if(!$java.HasExited -or $java.ExitCode -ne 0 -or !(Test-Path -LiteralPath $javaImage)){throw 'Original Java oracle failed'}}finally{if(!$java.HasExited){$java.Kill()};$java.Dispose()}
    }
    if($configHash -ne (Get-FileHash -LiteralPath (Join-Path $profile 'development.json')).Hash){throw 'Preview modified its legacy config'}
    $captures+=@{state=$state;profile=$profile;originalJava=$javaImage;path=$env:TOMATO_UI_CAPTURE;sha256=(Get-FileHash -LiteralPath $env:TOMATO_UI_CAPTURE).Hash}
}
if($before -ne (Get-FileHash -LiteralPath $shared).Hash){throw 'Shared config changed'}
@{guiSha256=$sha;feature=$Feature;captures=$captures;savedHostsContacted=$false;sharedConfigurationUnchanged=$true;allStatesPixelIdentical=$false}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $probe 'validation.json')
Get-Content (Join-Path $probe 'validation.json')
} finally {foreach($key in $keys){if($null -eq $previous[$key]){[Environment]::SetEnvironmentVariable($key,[NullString]::Value,'Process')}else{[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}}
