# Aknan Download - one-click Windows build
# Installs what is missing (Visual Studio C++ Build Tools, Rust), builds
# AknanDownload.exe, gets ffmpeg + yt-dlp and makes dist\AknanDownload-Setup.exe.

$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot

function Step($text) { Write-Host ""; Write-Host "==> $text" -ForegroundColor Cyan }
function Fail($text) {
    Write-Host ""
    Write-Host "ERROR: $text" -ForegroundColor Red
    Write-Host "Check the error message above and re-run build-windows.ps1." -ForegroundColor Yellow
    if (-not $env:NO_PAUSE) { Read-Host "Press Enter to close" }
    exit 1
}
function Refresh-Path {
    $env:Path = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" +
                [System.Environment]::GetEnvironmentVariable("Path", "User") + ";" +
                "$env:USERPROFILE\.cargo\bin"
}

Write-Host "Aknan Download build" -ForegroundColor Green
Write-Host "The first run downloads a few GB and can take 20-40 minutes. Later runs are fast."

if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
    Write-Host "Note: winget not found; using already installed tools." -ForegroundColor Yellow
}

# ---------------------------------------------------------------- 1. C++ build tools
Step "Checking Visual Studio C++ Build Tools"
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$hasVc = $false
if (Test-Path $vswhere) {
    $vc = & $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($vc) { $hasVc = $true }
}
if (-not $hasVc) {
    Step "Installing Visual Studio C++ Build Tools (big download, please wait; accept the admin prompt)"
    winget install --id Microsoft.VisualStudio.2022.BuildTools -e --accept-package-agreements --accept-source-agreements `
        --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne 3010) { Fail "Installing the C++ Build Tools failed (code $LASTEXITCODE)." }
} else {
    Write-Host "Found."
}

# ---------------------------------------------------------------- 2. Rust
Step "Checking Rust"
Refresh-Path
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Step "Installing Rust"
    winget install --id Rustlang.Rustup -e --accept-package-agreements --accept-source-agreements
    Refresh-Path
    if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) { Fail "Rust was installed but not found. Restart the computer and run this again." }
}
rustup default stable-x86_64-pc-windows-msvc | Out-Null
cargo --version

# the version comes from Cargo.toml ([workspace.package] version = "...")
$version = ([regex]::Match((Get-Content (Join-Path $PSScriptRoot "Cargo.toml") -Raw), '(?m)^version\s*=\s*"([^"]+)"')).Groups[1].Value
if (-not $version) { $version = "1.0.0" }
Write-Host "Version $version"

# ---------------------------------------------------------------- 3. build
# a running copy would lock the files we replace
Get-Process AknanDownload, SwiftGet, aknan-host -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
$ffmpegHashSrc = if (Test-Path "tools\ffmpeg\win64\ffmpeg.exe") { "tools\ffmpeg\win64\ffmpeg.exe" } elseif (Test-Path "build-cache\ffmpeg.exe") { "build-cache\ffmpeg.exe" } else { $null }
if ($ffmpegHashSrc -and -not $env:AKNAN_FFMPEG_SHA256) {
    $env:AKNAN_FFMPEG_SHA256 = (Get-FileHash $ffmpegHashSrc -Algorithm SHA256).Hash.ToLowerInvariant()
}
Step "Building AknanDownload.exe (first time: 5-15 minutes)"
cargo build --release
if ($LASTEXITCODE -ne 0) { Fail "The build failed. Scroll up to see the first red 'error' lines." }

# ---------------------------------------------------------------- 4. package
# The result is ONE file: dist\AknanDownload-Setup.exe (+ dist\version.json
# for the GitHub release). Everything that goes into the setup is gathered
# first in build-cache\stage, which is not part of the result:
#   AknanDownload.exe, tools\yt-dlp.exe + ffmpeg.exe, extension, extension-firefox
$Root = $PSScriptRoot
$dist = Join-Path $Root "dist"
$cache = Join-Path $Root "build-cache"
$stage = Join-Path $cache "stage"
. (Join-Path $PSScriptRoot "installer\stage-files.ps1") -Root $Root -DistDir $dist -CacheDir $cache -StageDir $stage

# ---------------------------------------------------------------- 4b. signing (optional)
# With a code signing certificate antivirus programs and Windows SmartScreen
# trust the program. Put it in signing\aknan.pfx and its password in
# signing\password.txt - everything is then signed automatically.
$pfx = Join-Path $PSScriptRoot "signing\aknan.pfx"
$signtool = $null
if (Test-Path $pfx) {
    $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "x64" } | Select-Object -First 1 -ExpandProperty FullName
}
function Sign-File($file) {
    if (-not $signtool) { return }
    $pw = (Get-Content (Join-Path $PSScriptRoot "signing\password.txt") -Raw).Trim()
    & $signtool sign /fd SHA256 /f $pfx /p $pw /tr "http://timestamp.digicert.com" /td SHA256 /d "Aknan Download" $file | Out-Null
    if ($LASTEXITCODE -eq 0) { Write-Host "Signed $file" } else { Write-Host "Could not sign $file" -ForegroundColor Yellow }
}
if ($signtool) {
    Step "Signing AknanDownload.exe"
    Sign-File "$stage\AknanDownload.exe"
    Sign-File "$stage\aknan-host.exe"
}


# ---------------------------------------------------------------- 5. installer
Step "Creating the installer (AknanDownload-Setup.exe)"
function Find-Iscc {
    @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
      "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
      "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
}
$iscc = Find-Iscc
if (-not $iscc) {
    Write-Host "Installing Inno Setup (makes the installer)..."
    winget install --id JRSoftware.InnoSetup -e --accept-package-agreements --accept-source-agreements | Out-Null
    $iscc = Find-Iscc
}
$setup = Join-Path $dist "AknanDownload-Setup.exe"
if ($iscc) {
    # Antivirus / OneDrive often lock a new .exe for a moment while Inno Setup
    # writes its icon into it ("EndUpdateResource failed"). So: build in a
    # temp folder, retry a few times, and as a last resort build without the
    # installer's own icon.
    $out = Join-Path $env:TEMP "aknan-setup-build"
    New-Item -ItemType Directory -Force $out | Out-Null
    $iss = Join-Path $PSScriptRoot "installer\aknan.iss"
    $ok = $false
    foreach ($attempt in 1..3) {
        Remove-Item (Join-Path $out "AknanDownload-Setup.exe") -Force -ErrorAction SilentlyContinue
        $flags = @("/Q", "/O$out", "/DMyAppVersion=$version")
        & $iscc @flags $iss
        if ($LASTEXITCODE -eq 0 -and (Test-Path (Join-Path $out "AknanDownload-Setup.exe"))) { $ok = $true; break }
        Write-Host "Retrying (the antivirus was probably scanning the file)..." -ForegroundColor Yellow
        Start-Sleep -Seconds 3
    }
    if ($ok) {
        Copy-Item (Join-Path $out "AknanDownload-Setup.exe") $setup -Force
        Sign-File $setup
        # version.json: the program reads it from the latest GitHub release
        # (or official website) to update itself (version + SHA-256 + Ed25519 sig)
        $hash = (Get-FileHash $setup -Algorithm SHA256).Hash.ToLower()
        $keyFile = Join-Path $PSScriptRoot "signing\update-signing-key.txt"
        if (-not $env:UPDATE_SIGNING_KEY -and (Test-Path $keyFile)) {
            $env:UPDATE_SIGNING_KEY = (Get-Content $keyFile -Raw).Trim()
        }
        $sig = ""
        if ($env:UPDATE_SIGNING_KEY) {
            $sig = (& cargo run --quiet --release -p swiftget-engine --example sign_update -- $version $hash).Trim()
            if ($LASTEXITCODE -ne 0 -or $sig.Length -ne 128) {
                Fail "Could not sign version.json with UPDATE_SIGNING_KEY"
            }
        } else {
            Write-Warning "UPDATE_SIGNING_KEY / signing\update-signing-key.txt missing; version.json will be unsigned."
        }
        $json = "{`n  ""version"": ""$version"",`n  ""url"": ""AknanDownload-Setup.exe"",`n  ""sha256"": ""$hash"",`n  ""sig"": ""$sig""`n}`n"
        [IO.File]::WriteAllText((Join-Path $dist "version.json"), $json)
    } else {
        Fail "Could not create the setup. Tip: move this folder out of Desktop/OneDrive (e.g. to C:\Aknan) and run BUILD.bat again."
    }
} else {
    Fail "Inno Setup is not available, so the setup could not be made. Install Inno Setup 6 and run BUILD.bat again."
}

Write-Host ""
Write-Host "DONE!" -ForegroundColor Green
if (Test-Path $setup) {
    Write-Host ""
    Write-Host "  >> Double-click  dist\AknanDownload-Setup.exe  to install Aknan Download <<" -ForegroundColor Green
    Write-Host "     (Start Menu folder, desktop shortcut, starts with Windows, uninstall from Settings > Apps)"
}
if (Test-Path "$dist\version.json") {
    Write-Host ""
    Write-Host "Also in $dist : version.json (it goes into the GitHub release together with the setup)"
}
if (-not $env:NO_PAUSE) {
    Start-Process explorer.exe $dist
    Read-Host "Press Enter to close"
}
