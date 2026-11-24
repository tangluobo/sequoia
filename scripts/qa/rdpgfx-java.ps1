param(
 [Parameter(Mandatory=$true)][string]$Label,
 [Parameter(Mandatory=$true)][string]$JavaHome,
 [string]$Reference='D:/Data/Git/tangluobo/rdp4j/src/main/java/com/tangluobo/rdp4j'
)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid probe label'}
$probe=Join-Path $workspace ('target/rdpgfx-java-'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
$fixture=Get-Content -LiteralPath (Join-Path $workspace 'crates/services/tests/fixtures/rdpgfx-java-clearcodec.json') -Raw | ConvertFrom-Json
$harness=Join-Path $workspace 'crates/core/tests/java/RdpClearCodecOracle.java'
if((Get-FileHash -LiteralPath $harness).Hash -ne $fixture.harnessSha256){throw 'Java oracle harness changed'}
$sources=@();$sourceHashes=@{}
foreach($property in $fixture.referenceHashes.PSObject.Properties){
 $source=Join-Path $Reference ([IO.Path]::GetFileName($property.Name))
 $hash=(Get-FileHash -LiteralPath $source).Hash
 if($hash -ne $property.Value){throw ('Original Java source changed: '+$source)}
 $sources+=$source;$sourceHashes[$source]=$hash
}
[void][IO.Directory]::CreateDirectory($probe)
$inputFile=Join-Path $probe 'inputs.tsv'
$fixture.cases | ForEach-Object { @($_.name,$_.width,$_.height,$_.stream) -join "`t" } | Set-Content -LiteralPath $inputFile -Encoding utf8NoBOM
& (Join-Path $JavaHome 'bin/javac.exe') -d (Join-Path $probe 'classes') @sources $harness *> (Join-Path $probe 'compile.log')
if($LASTEXITCODE -ne 0){throw 'Original Java decoder compilation failed'}
$outputFile=Join-Path $probe 'outputs.tsv'
& (Join-Path $JavaHome 'bin/java.exe') -cp (Join-Path $probe 'classes') com.tangluobo.rdp4j.RdpClearCodecOracle $inputFile > $outputFile
if($LASTEXITCODE -ne 0){throw 'Original Java decoder failed'}
$answers=@{};Get-Content -LiteralPath $outputFile | ForEach-Object { $parts=$_ -split "`t";if($parts.Length -ne 2 -or $answers.ContainsKey($parts[0])){throw 'Invalid Java oracle output'};$answers[$parts[0]]=$parts[1] }
foreach($case in $fixture.cases){if($answers[$case.name] -cne $case.bgra){throw ('Java result differs: '+$case.name)}}
if($answers.Count -ne $fixture.cases.Count){throw 'Java oracle case count differs'}
@{cases=$answers.Count;sourceHashes=$sourceHashes;harnessSha256=(Get-FileHash -LiteralPath $harness).Hash;networkUsed=$false;originalSourcesModified=$false;allExpectedPixelsMatched=$true}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $probe 'validation.json')
Write-Output ('Original Java ClearCodec cases passed: '+$answers.Count)
