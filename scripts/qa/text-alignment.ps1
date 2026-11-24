param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(Mandatory=$true)][string]$Label
)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if($Label -notmatch '^[a-z0-9-]+$'){throw 'Invalid probe label'}
$probe=Join-Path $workspace ('target/window-probes/'+$Label)
if(Test-Path -LiteralPath $probe){throw 'Use a fresh probe label'}
[void][IO.Directory]::CreateDirectory($probe)
$source=(Resolve-Path -LiteralPath $Executable).Path
if(!$source.StartsWith((Join-Path $workspace 'target/'),[StringComparison]::OrdinalIgnoreCase)){throw 'Expected owned build'}
$sha=(Get-FileHash -LiteralPath $source).Hash
$program=Join-Path $probe 'tomato.exe'
Copy-Item -LiteralPath $source -Destination $program
if((Get-FileHash -LiteralPath $program).Hash -ne $sha){throw 'GUI changed while snapshotting'}
$profile=Join-Path $probe 'profile'
[void][IO.Directory]::CreateDirectory($profile)
$document=Join-Path $probe 'alignment.md'
@'
# 标题 Heading `code中文`

普通文字 Ag **加粗 Bold中文** *斜体 Italic中文* `code中文` [链接 link](https://example.invalid) 尾部文字。

中英文 ABCabc123 **粗体中文 gjpq** *Italic gyp* `inline_code(123)` 同一行基线。

> 引用普通文字 **加粗 Bold中文** `code中文` 和 [链接](https://example.invalid)。

| 表头中文 | Header Bold |
| --- | --- |
| 普通 **加粗** `code中文` | 第二列 *斜体 Italic* |
| 下一行文字 | `code` 正常文字 |

```rust
let 中文 = "value"; // 注释
let text = "ABCabc123";
```

1. 普通 **加粗中文 Bold** `code中文` 列表
2. 第二项 *Italic中文* 和普通文字
'@ | Set-Content -LiteralPath $document -Encoding utf8
$shared=Join-Path $env:USERPROFILE '.tomato/connections.json'
$before=(Get-FileHash -LiteralPath $shared).Hash
$keys=@('TEMP','TMP','TOMATO_DATA_DIR','TOMATO_UI_PREVIEW','TOMATO_UI_PREVIEW_PATH','TOMATO_UI_CAPTURE','TOMATO_UI_CAPTURE_FRAMES','TOMATO_UI_CAPTURE_DELAY_MS','WGPU_BACKEND')
$previous=@{};foreach($key in $keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process')}
$captures=@()
try {
    $env:TEMP=Join-Path $workspace 'target/build-temp';$env:TMP=$env:TEMP
    $env:TOMATO_DATA_DIR=$profile;$env:TOMATO_UI_PREVIEW_PATH=$document
    $env:TOMATO_UI_CAPTURE_FRAMES='4';$env:TOMATO_UI_CAPTURE_DELAY_MS='2000'
    $env:WGPU_BACKEND='vulkan'
    foreach($case in @('markdown','markdown_find')){
        $env:TOMATO_UI_PREVIEW=$case;$env:TOMATO_UI_CAPTURE=Join-Path $probe ($case+'.png')
        $process=Start-Process -FilePath $program -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $probe ($case+'.stdout.log')) -RedirectStandardError (Join-Path $probe ($case+'.stderr.log'))
        try {
            if(!$process.WaitForExit(45000)){throw ('Owned capture timed out: '+$case)}
            if($process.ExitCode -ne 0){throw ('Owned capture failed: '+$case)}
        } finally {
            if(!$process.HasExited){$process.Kill();[void]$process.WaitForExit(5000)}
            $process.Dispose()
        }
        $image=Join-Path $probe ($case+'-03.png')
        if(!(Test-Path -LiteralPath $image)){throw ('Missing final framebuffer: '+$case)}
        $captures+=@{case=$case;path=$image;sha256=(Get-FileHash -LiteralPath $image).Hash;normalExit=$true}
    }
    if((Get-FileHash -LiteralPath $source).Hash -ne $sha){throw 'Source executable changed'}
    if((Get-FileHash -LiteralPath $shared).Hash -ne $before){throw 'Shared configuration changed'}
    @{guiSha256=$sha;captures=$captures;sharedConfigurationUnchanged=$true;savedHostsContacted=$false;systemClipboardWritten=$false;nativeWindow=$true}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $probe 'validation.json') -Encoding utf8
} finally {
    foreach($key in $keys){[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}
}
