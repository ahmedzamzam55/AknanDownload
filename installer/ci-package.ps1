# CI packaging script for GitHub Actions (.github/workflows/build.yml).
# Expects env: AMO_JWT_ISSUER, AMO_JWT_SECRET, UPDATE_SIGNING_KEY, IS_RELEASE,
# REF_TYPE, REF_NAME, REPO, GH_TOKEN.

$ErrorActionPreference = "Stop"
Set-Location -Path (Split-Path -Parent $PSScriptRoot)

$version = ([regex]::Match((Get-Content Cargo.toml -Raw), '(?m)^version\s*=\s*"([^"]+)"')).Groups[1].Value
if ($env:REF_TYPE -eq "tag") {
    $tag = "$env:REF_NAME".TrimStart("v")
    if ($tag -ne $version) { throw "Tag v$tag does not match version $version in Cargo.toml" }
}
"VERSION=$version" | Out-File -Append $env:GITHUB_ENV

# everything for the setup is gathered here; the result is the setup only
$p = "build-cache/stage"
New-Item -ItemType Directory -Force "$p/tools", "dist" | Out-Null
Copy-Item target/release/AknanDownload.exe $p
Copy-Item target/release/aknan-host.exe $p
Copy-Item crates/app/icon.ico "$p/aknan.ico"
Copy-Item THIRD-PARTY-NOTICES.txt $p
Copy-Item LICENSE "$p/LICENSE.txt"
Copy-Item -Recurse extension "$p/extension"
Copy-Item -Recurse extension "$p/extension-firefox"
Remove-Item -Recurse -Force "$p/extension/test", "$p/extension-firefox/test"
Copy-Item -Force "$p/extension-firefox/manifest.firefox.json" "$p/extension-firefox/manifest.json"
Remove-Item -Force "$p/extension-firefox/manifest.firefox.json", "$p/extension/manifest.firefox.json"
Compress-Archive -Path "$p/extension/*" -DestinationPath "dist/AknanDownload-Extension-Chrome.zip" -Force
Compress-Archive -Path "$p/extension-firefox/*" -DestinationPath "dist/AknanDownload-Extension-Firefox.zip" -Force

# the add-on has its own version (manifest.firefox.json)
$extVersion = (Get-Content extension/manifest.firefox.json -Raw | ConvertFrom-Json).version

function Install-SignedXpi([string]$src) {
    Copy-Item $src "$p/aknan-download-firefox.xpi"
    Copy-Item $src "dist/aknan-download-firefox.xpi"
    $fx = "{`n  ""addons"": {`n    ""aknan-download@aknan.com"": {`n      ""updates"": [`n        { ""version"": ""$extVersion"", ""update_link"": ""https://github.com/$env:REPO/releases/download/v$version/aknan-download-firefox.xpi"" }`n      ]`n    }`n  }`n}`n"
    [IO.File]::WriteAllText("$PWD/dist/firefox-updates.json", $fx)
}

# Firefox keeps only add-ons signed by Mozilla; a version can be signed
# once, so this runs for a release only
if ($env:IS_RELEASE -eq "true" -and $env:AMO_JWT_ISSUER -and $env:AMO_JWT_SECRET -and -not (Test-Path "extension-signed/aknan-download-firefox-$extVersion.xpi")) {
    npx --yes web-ext@8.10.0 sign --channel=unlisted --source-dir "$p/extension-firefox" --artifacts-dir xpi-out --api-key "$env:AMO_JWT_ISSUER" --api-secret "$env:AMO_JWT_SECRET" --approval-timeout 900000
    $xpi = Get-ChildItem xpi-out -Filter *.xpi | Select-Object -First 1
    if ($xpi) {
        Install-SignedXpi $xpi.FullName
    } else { Write-Host "::warning::the Firefox add-on was not signed" }
} elseif ($env:IS_RELEASE -eq "true") { Write-Host "::warning::AMO_JWT_ISSUER / AMO_JWT_SECRET are not set: the Firefox add-on is not signed" }

# an add-on of this version already signed (by hand on addons.mozilla.org)
$signed = "extension-signed/aknan-download-firefox-$extVersion.xpi"
if (-not (Test-Path "$p/aknan-download-firefox.xpi") -and (Test-Path $signed)) {
    Install-SignedXpi $signed
}
# never an older add-on in the setup: installing it would put the
# old add-on back into Firefox. A release without the signed add-on
# of its own version is not published
if (-not (Test-Path "$p/aknan-download-firefox.xpi") -and $env:IS_RELEASE -eq "true" -and $env:AMO_JWT_ISSUER -and $env:AMO_JWT_SECRET) {
    throw "the Firefox add-on $extVersion was not signed: the release would carry no Firefox add-on"
}

# our own small ffmpeg (job "ffmpeg") and its licences
Copy-Item ffmpeg-out/* "$p/tools/"
# yt-dlp of one release, checked against that release's SHA2-256SUMS
$yt = Invoke-RestMethod "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest" -Headers @{ "User-Agent" = "AknanDownload-ci"; "Authorization" = "Bearer $env:GH_TOKEN" }
$ytExe = $yt.assets | Where-Object { $_.name -eq "yt-dlp.exe" } | Select-Object -First 1
$ytSums = $yt.assets | Where-Object { $_.name -eq "SHA2-256SUMS" } | Select-Object -First 1
if (-not $ytExe -or -not $ytSums) { throw "the yt-dlp release has no yt-dlp.exe / SHA2-256SUMS" }
Invoke-WebRequest $ytExe.browser_download_url -OutFile "$p/tools/yt-dlp.exe"
$ySumText = (Invoke-WebRequest $ytSums.browser_download_url).Content
if ($ySumText -is [byte[]]) { $ySumText = [Text.Encoding]::UTF8.GetString($ySumText) }
$yWant = ($ySumText -split "`n" | Where-Object { $_ -match "^([0-9a-fA-F]{64})\s+\*?yt-dlp\.exe\s*$" } | ForEach-Object { ($_ -split '\s+')[0] } | Select-Object -First 1)
$yGot = (Get-FileHash "$p/tools/yt-dlp.exe" -Algorithm SHA256).Hash
if (-not $yWant -or $yGot -ne $yWant.ToUpper()) { throw "yt-dlp.exe does not match its published SHA-256" }

choco install innosetup -y --no-progress
& "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" /Q "/DMyAppVersion=$version" installer\aknan.iss
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }

$hash = (Get-FileHash dist/AknanDownload-Setup.exe -Algorithm SHA256).Hash.ToLower()
# the signature the program checks before it installs an update
$sig = ""
if ($env:UPDATE_SIGNING_KEY) {
    $sig = (cargo run --quiet --release -p swiftget-engine --example sign_update -- $version $hash).Trim()
    if ($LASTEXITCODE -ne 0 -or $sig.Length -ne 128) { throw "could not sign the release" }
} elseif ($env:IS_RELEASE -eq "true") {
    throw "UPDATE_SIGNING_KEY is not set: installed copies would refuse this update"
}
$json = "{`n  ""version"": ""$version"",`n  ""url"": ""https://github.com/$env:REPO/releases/download/v$version/AknanDownload-Setup.exe"",`n  ""sha256"": ""$hash"",`n  ""sig"": ""$sig""`n}`n"
[IO.File]::WriteAllText("$PWD/dist/version.json", $json)
Get-Content dist/version.json
Compress-Archive -Path dist/AknanDownload-Setup.exe, dist/version.json, dist/AknanDownload-Extension-Chrome.zip, dist/AknanDownload-Extension-Firefox.zip -DestinationPath AknanDownload-windows.zip -Force
