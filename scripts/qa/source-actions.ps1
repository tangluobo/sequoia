param(
    [Parameter(Mandatory=$true)][string]$Label,
    [Parameter(Mandatory=$true)][string]$JavaRoot,
    [Parameter(Mandatory=$true)][string]$Jdk,
    [Parameter(Mandatory=$true)][string]$GsonJar
)
$ErrorActionPreference='Stop'
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid probe label'}
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$original=(Resolve-Path -LiteralPath $JavaRoot).Path
$gson=(Resolve-Path -LiteralPath $GsonJar).Path
$java=Join-Path $Jdk 'bin/java.exe'
$javac=Join-Path $Jdk 'bin/javac.exe'
if(!(Test-Path -LiteralPath $java) -or !(Test-Path -LiteralPath $javac)){throw 'JDK tools missing'}
$probe=Join-Path $workspace ('target/source-audits/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
if(([IO.Path]::GetFullPath($probe)+[IO.Path]::DirectorySeparatorChar).StartsWith($original.TrimEnd('\','/')+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Probe must stay outside original source'}
$classes=Join-Path $probe 'classes'
[void][IO.Directory]::CreateDirectory($classes)
$source=Join-Path $workspace 'crates/core/tests/java/SourceActionInventory.java'
$sourceHash=(Get-FileHash -LiteralPath $source).Hash
$gsonHash=(Get-FileHash -LiteralPath $gson).Hash
& $javac '--enable-preview' '--release' '25' '-encoding' 'UTF-8' '-cp' $gson '-d' $classes $source *> (Join-Path $probe 'compile.log')
if($LASTEXITCODE -ne 0){throw 'Source inventory compiler failed; inspect compile.log'}
$output=Join-Path $probe 'actions.json'
& $java '-Xmx768m' '--enable-preview' '-cp' ($classes+';'+$gson) 'SourceActionInventory' $original $output *> (Join-Path $probe 'run.log')
if($LASTEXITCODE -ne 0){throw 'Original source parsing failed; inspect run.log'}
if((Get-FileHash -LiteralPath $source).Hash -ne $sourceHash -or (Get-FileHash -LiteralPath $gson).Hash -ne $gsonHash){throw 'Inventory inputs changed'}
$inventory=Get-Content -LiteralPath $output -Raw | ConvertFrom-Json
if(!$inventory.parseOnly -or $inventory.applicationStarted -or $inventory.configurationWritten -or $inventory.savedHostsContacted){throw 'Unexpected inventory mode'}
foreach($file in $inventory.files){
    foreach($item in @($file.methods)+@($file.bindings)+@($file.controls)){
        if($item.reviewStatus -ne 'unreviewed'){throw 'Inventory cannot claim migration coverage'}
    }
}
$validation=[ordered]@{
    label=$Label;generatorSha256=$sourceHash;gsonSha256=$gsonHash
    inventorySha256=(Get-FileHash -LiteralPath $output).Hash
    sourceFiles=$inventory.sourceFiles;methods=$inventory.methods
    bindings=$inventory.bindings;controls=$inventory.controls
    parseOnly=$true;allEntriesUnreviewed=$true
    applicationStarted=$false;configurationWritten=$false;savedHostsContacted=$false
}
$validation | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8
$validation | ConvertTo-Json -Depth 4
