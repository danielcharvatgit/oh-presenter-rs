# Downloads ffmpeg.exe and ffplay.exe (a GPL build from gyan.dev) into
# packaging\windows\ffmpeg\, so the installer includes video support:
#   powershell -ExecutionPolicy Bypass -File packaging\windows\get-ffmpeg.ps1
#   iscc packaging\windows\oh-presenter.iss
$ErrorActionPreference = "Stop"
$dest = Join-Path $PSScriptRoot "ffmpeg"
$zip = Join-Path $env:TEMP "ffmpeg-release-essentials.zip"
$tmp = Join-Path $env:TEMP "oh-presenter-ffmpeg"
Invoke-WebRequest "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip" -OutFile $zip
if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force }
Expand-Archive $zip -DestinationPath $tmp
New-Item -ItemType Directory -Force $dest | Out-Null
$bin = Get-ChildItem $tmp -Recurse -Filter ffmpeg.exe | Select-Object -First 1
Copy-Item (Join-Path $bin.DirectoryName "ffmpeg.exe") $dest -Force
Copy-Item (Join-Path $bin.DirectoryName "ffplay.exe") $dest -Force
$license = Get-ChildItem $tmp -Recurse -Filter LICENSE* | Select-Object -First 1
if ($license) { Copy-Item $license.FullName (Join-Path $dest "LICENSE.txt") -Force }
Remove-Item $zip, $tmp -Recurse -Force
Write-Host "ffmpeg.exe and ffplay.exe are in $dest"
