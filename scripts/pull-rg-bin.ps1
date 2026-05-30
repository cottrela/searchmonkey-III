$ErrorActionPreference = 'Stop'

$Version = '15.1.0'
$Target = 'x86_64-pc-windows-msvc'
$BaseUrl = "https://github.com/BurntSushi/ripgrep/releases/download/$Version"
$OutDir = Join-Path $PSScriptRoot '..\src-tauri\binaries'
$TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) 'searchmonkey-rg'
$Archive = Join-Path $TmpDir "ripgrep-$Version-$Target.zip"
$ExtractDir = Join-Path $TmpDir "ripgrep-$Version-$Target"
$Output = Join-Path $OutDir "rg-$Target.exe"

if (Test-Path -LiteralPath $TmpDir) {
  Remove-Item -LiteralPath $TmpDir -Recurse -Force
}

New-Item -ItemType Directory -Force -Path $OutDir, $TmpDir | Out-Null

Invoke-WebRequest `
  -Uri "$BaseUrl/ripgrep-$Version-$Target.zip" `
  -OutFile $Archive

Expand-Archive -LiteralPath $Archive -DestinationPath $TmpDir -Force
Copy-Item -LiteralPath (Join-Path $ExtractDir 'rg.exe') -Destination $Output -Force

& $Output --version
Write-Host "Downloaded ripgrep $Version sidecar to $Output"
