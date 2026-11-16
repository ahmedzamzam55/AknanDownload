# Stages binaries, browser extensions, and bundled tools into build-cache\stage
# for Inno Setup (dot-sourced by build-windows.ps1).

param(
    [string]$Root = (Split-Path -Parent $PSScriptRoot),
    [string]$DistDir = "",
    [string]$CacheDir = "",
    [string]$StageDir = ""
)

$ErrorActionPreference = "Stop"
if (-not $DistDir) { $DistDir = Join-Path $Root "dist" }
if (-not $CacheDir) { $CacheDir = Join-Path $Root "build-cache" }
if (-not $StageDir) { $StageDir = Join-Path $CacheDir "stage" }

if (-not (Get-Command Step -ErrorAction SilentlyContinue)) {
    function Step($text) { Write-Host ""; Write-Host "==> $text" -ForegroundColor Cyan }
}

# The SHA-256 of a file from a checksum list ("<sha256>  <name>" lines), checked.
function Assert-Sha256($file, $sumsUrl, $name) {
    $text = (Invoke-WebRequest $sumsUrl -UseBasicParsing).Content
    if ($text -is [byte[]]) { $text = [Text.Encoding]::UTF8.GetString($text) }
    $want = ($text -split "`n" | Where-Object { $_ -match "^([0-9a-fA-F]{64})\s+\*?$([regex]::Escape($name))\s*$" } |
        ForEach-Object { ($_ -split '\s+')[0] } | Select-Object -First 1)
    $got = (Get-FileHash $file -Algorithm SHA256).Hash
    if (-not $want -or $got -ne $want.ToUpper()) {
        Remove-Item -Force $file -ErrorAction SilentlyContinue
        throw "$name does not match its published SHA-256"
    }
}

function Get-XpiVersion([string]$path) {
    try {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $zip = [System.IO.Compression.ZipFile]::OpenRead($path)
        try {
            $entry = $zip.GetEntry("manifest.json")
            if (-not $entry) { return $null }
            $reader = New-Object System.IO.StreamReader($entry.Open())
            try { return ($reader.ReadToEnd() | ConvertFrom-Json).version } finally { $reader.Close() }
        } finally { $zip.Dispose() }
    } catch { return $null }
}

Step "Preparing the files for the setup"
$tools = Join-Path $StageDir "tools"
if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
New-Item -ItemType Directory -Force $DistDir, $StageDir, $tools, $CacheDir | Out-Null
# what older builds left in dist: only the setup stays there
foreach ($old in @("AknanDownload.exe", "ffmpeg.exe", "yt-dlp.exe")) {
    Remove-Item (Join-Path $DistDir $old) -Force -ErrorAction SilentlyContinue
}
foreach ($old in @("extension", "portable", "store", "redist")) {
    if (Test-Path "$DistDir\$old") { Remove-Item -Recurse -Force "$DistDir\$old" }
}

Copy-Item (Join-Path $Root "target\release\AknanDownload.exe") $StageDir -Force
# the helper the browser starts to talk to the program (no port)
Copy-Item (Join-Path $Root "target\release\aknan-host.exe") $StageDir -Force
$extSrc = Join-Path $Root "extension"
foreach ($name in @("extension", "extension-firefox")) {
    $to = Join-Path $StageDir $name
    if (Test-Path $to) { Remove-Item -Recurse -Force $to }
    Copy-Item -Recurse $extSrc $to
    Remove-Item -Recurse -Force "$to\test" -ErrorAction SilentlyContinue
}
# Firefox wants its own manifest; the others ignore this file
Copy-Item -Force "$StageDir\extension-firefox\manifest.firefox.json" "$StageDir\extension-firefox\manifest.json"
Remove-Item "$StageDir\extension-firefox\manifest.firefox.json" -Force
Remove-Item "$StageDir\extension\manifest.firefox.json" -Force -ErrorAction SilentlyContinue
Compress-Archive -Path "$StageDir\extension\*" -DestinationPath (Join-Path $DistDir "AknanDownload-Extension-Chrome.zip") -Force
Compress-Archive -Path "$StageDir\extension-firefox\*" -DestinationPath (Join-Path $DistDir "AknanDownload-Extension-Firefox.zip") -Force

# ffmpeg: our own small build (tools\ffmpeg\build.sh; GitHub builds it for
# every release). A ready one in tools\ffmpeg\win64 is used when it is there;
# the big public build of earlier versions is not used any more.
$ours = Join-Path $Root "tools\ffmpeg\win64"
if (Test-Path "$ours\ffmpeg.exe") {
    foreach ($f in @("ffmpeg.exe", "ffmpeg-LICENSE.txt", "lame-COPYING.txt", "openh264-LICENSE.txt", "ffmpeg-version.txt")) {
        if (Test-Path "$ours\$f") { Copy-Item "$ours\$f" $CacheDir -Force }
    }
} elseif ((Test-Path "$CacheDir\ffmpeg.exe") -and ((Get-Item "$CacheDir\ffmpeg.exe").Length -gt 20MB)) {
    Remove-Item "$CacheDir\ffmpeg.exe", "$CacheDir\ffmpeg-version.txt" -Force -ErrorAction SilentlyContinue
    Write-Host "The old big ffmpeg was removed. Put our small ffmpeg.exe in tools\ffmpeg\win64 (it comes with the source zip, or from a GitHub build)." -ForegroundColor Yellow
}
if (-not (Test-Path "$CacheDir\ffmpeg.exe")) {
    Write-Host "ffmpeg.exe not found in tools\ffmpeg\win64 or build-cache." -ForegroundColor Yellow
}

# yt-dlp (finds video links): downloaded once into build-cache, the app
# keeps it updated by itself afterwards
if (-not (Test-Path "$CacheDir\yt-dlp.exe")) {
    Step "Downloading yt-dlp (finds the links of videos)"
    try {
        $yt = Invoke-RestMethod "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest" -Headers @{ "User-Agent" = "AknanDownload-build" }
        $ytExe = $yt.assets | Where-Object { $_.name -eq "yt-dlp.exe" } | Select-Object -First 1
        $ytSums = $yt.assets | Where-Object { $_.name -eq "SHA2-256SUMS" } | Select-Object -First 1
        Invoke-WebRequest $ytExe.browser_download_url -OutFile "$CacheDir\yt-dlp.exe"
        Assert-Sha256 "$CacheDir\yt-dlp.exe" $ytSums.browser_download_url "yt-dlp.exe"
    } catch {
        Write-Host "Could not download yt-dlp now. Aknan Download gets it by itself later." -ForegroundColor Yellow
    }
}

# Visual C++ runtime: the program already carries it (static CRT, see
# .cargo\config.toml). Optional: put Microsoft's vc_redist.x64.exe into
# build-cache yourself and the installer also installs it where it is missing.
$redist = Join-Path $CacheDir "redist"
New-Item -ItemType Directory -Force $redist | Out-Null
if (Test-Path "$CacheDir\vc_redist.x64.exe") { Copy-Item "$CacheDir\vc_redist.x64.exe" $redist -Force }
Copy-Item (Join-Path $Root "THIRD-PARTY-NOTICES.txt") $StageDir -Force
Copy-Item (Join-Path $Root "LICENSE") (Join-Path $StageDir "LICENSE.txt") -Force
Copy-Item (Join-Path $Root "crates\app\icon.ico") (Join-Path $StageDir "aknan.ico") -Force

# the Firefox add-on signed by Mozilla for this version (stays in Firefox for good)
$extVersion = (Get-Content (Join-Path $Root "extension\manifest.firefox.json") -Raw | ConvertFrom-Json).version
# Firefox keeps only add-ons signed by Mozilla (an unsigned one is gone
# when Firefox closes). Only a signed add-on of exactly this version goes
# into the setup: an older one would put the old add-on back into Firefox.
# Kept here (extension-signed), or the one of the newest release when that
# release is this version; otherwise the setup has none (a release signs it).
$signedXpi = Join-Path $Root "extension-signed\aknan-download-firefox-$extVersion.xpi"
$xpiOut = Join-Path $StageDir "aknan-download-firefox.xpi"
Remove-Item $xpiOut -Force -ErrorAction SilentlyContinue
if ((Test-Path $signedXpi) -and ((Get-XpiVersion $signedXpi) -eq $extVersion)) {
    Copy-Item $signedXpi $xpiOut -Force
}
if (-not (Test-Path $xpiOut)) {
    Write-Warning "No signed Firefox add-on $extVersion yet: this setup has none (the release signs it). Firefox users of this build add it from the folder."
}
foreach ($f in @("ffmpeg.exe", "yt-dlp.exe", "ffmpeg-version.txt", "ffmpeg-LICENSE.txt", "lame-COPYING.txt", "openh264-LICENSE.txt")) {
    if (Test-Path "$CacheDir\$f") { Copy-Item "$CacheDir\$f" $tools -Force }
}
