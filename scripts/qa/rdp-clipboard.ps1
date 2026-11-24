param(
    [switch]$Offline,
    [string]$TargetDirectory = '',
    [ValidatePattern('^[a-z0-9-]{1,64}$')][string]$Label = ('rdp-clipboard-' + [Guid]::NewGuid().ToString('N'))
)
$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Windows clipboard tests require Windows' }
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$directory = Join-Path $workspace ('target/native-checks/' + $Label)
if (Test-Path -LiteralPath $directory) { throw 'Choose a fresh native test label' }
[void][IO.Directory]::CreateDirectory($directory)
$rootLock = Join-Path $workspace 'Cargo.lock'
$before = (Get-FileHash -LiteralPath $rootLock).Hash
$sourceManifest = Join-Path $workspace 'vendor/ironrdp-client/Cargo.toml'
$library = (Join-Path $workspace 'vendor/ironrdp-client/src/lib.rs').Replace('\', '/')
$manifest = (Get-Content -LiteralPath $sourceManifest -Raw).Replace('path = "src/lib.rs"', ('path = "' + $library + '"'))
$manifest = [regex]::Replace($manifest, '(?m)^test = false\s*$', 'test = true')
$manifest += "`n[workspace]`n`n[profile.dev]`nopt-level=1`ndebug=0`nincremental=false`n"
$manifestPath = Join-Path $directory 'Cargo.toml'
[IO.File]::WriteAllText($manifestPath, $manifest, [Text.UTF8Encoding]::new($false))
Copy-Item -LiteralPath $rootLock -Destination (Join-Path $directory 'Cargo.lock')
$workspaceManifest = Get-Content -LiteralPath (Join-Path $workspace 'Cargo.toml') -Raw
$patch = [regex]::Match($workspaceManifest, '(?ms)^\[patch\.crates-io\][ \t]*\r?\n(?<body>.*?)(?=^\[|\z)')
if (!$patch.Success) { throw 'Workspace patch table not found' }
$patches = [regex]::Replace($patch.Value, 'path\s*=\s*"([^"]+)"', {
    param($pathMatch)
    $absolute = [IO.Path]::GetFullPath((Join-Path $workspace $pathMatch.Groups[1].Value)).Replace('\', '/')
    'path = "' + $absolute + '"'
})
$patchPath = Join-Path $directory 'patches.toml'
[IO.File]::WriteAllText($patchPath, $patches, [Text.UTF8Encoding]::new($false))
if (!$TargetDirectory) { $TargetDirectory = Join-Path $workspace 'target/rdp-native-tests' }
$log = Join-Path $directory 'tests.log'
$arguments = @('test', '--manifest-path', $manifestPath, '--config', $patchPath, '--target-dir', $TargetDirectory)
if ($Offline) { $arguments += '--offline' }
$arguments += @('--features', 'clipboard,rustls', '--lib', 'tomato_', '--', '--nocapture', '--test-threads=1')
& cargo @arguments *> $log
$code = $LASTEXITCODE
if ((Get-FileHash -LiteralPath $rootLock).Hash -ne $before) { throw 'Root Cargo.lock changed during isolated native tests' }
if ($code -ne 0) { Get-Content -LiteralPath $log -Tail 70; throw ('Native file clipboard tests failed: ' + $code) }
$text = Get-Content -LiteralPath $log -Raw
$stream = [regex]::Match($text, 'Native OLE stream verified: (\d+) bytes')
$summaries = [regex]::Matches($text, 'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored;')
if (!$stream.Success -or [long]$stream.Groups[1].Value -lt 1MB -or !$summaries.Count -or [int]$summaries[-1].Groups[1].Value -lt 7) { throw 'Missing native stream and isolated child acceptance' }
[pscustomobject]@{
    status='passed'; manifest=$manifestPath; log=$log; rootLockUnchanged=$true
    userClipboardWritten=$false; scope='Isolated TCP protocol and private Windows clipboard station; not saved hosts'
} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $directory 'validation.json') -Encoding utf8
Get-Content -LiteralPath $log -Tail 18
