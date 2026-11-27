# Packages Pairnox for Windows as a zip (extract anywhere and run):
#
#   packaging/windows/build-zip.ps1 <directory with pnxr.exe and pnxr-gui.exe> <version>
#
# Produces Pairnox-<version>-windows-x86_64[-unsigned].zip in the
# current directory. Qt is bundled by windeployqt (on the PATH, from the Qt the
# window was built with). The programs are signed with signtool when
# WINDOWS_CERT_PFX (base64 of a .pfx) and WINDOWS_CERT_PASSWORD are set;
# without them the zip is named `-unsigned`.
param(
    [Parameter(Mandatory = $true)][string]$BinDir,
    [Parameter(Mandatory = $true)][string]$Version
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "../..")
$work = Join-Path ([System.IO.Path]::GetTempPath()) ("pnxr-pack-" + [guid]::NewGuid())
$app = Join-Path $work "Pairnox"
New-Item -ItemType Directory -Force $app | Out-Null

Copy-Item (Join-Path $BinDir "pnxr.exe") $app
Copy-Item (Join-Path $BinDir "pnxr-gui.exe") $app
windeployqt --release --qmldir (Join-Path $root "gui/qml") --compiler-runtime (Join-Path $app "pnxr-gui.exe")
if ($LASTEXITCODE -ne 0) { throw "windeployqt failed" }

# Licence and third-party notices (Qt, libarchive, crates).
Copy-Item (Join-Path $root "LICENSE") (Join-Path $app "LICENSE.txt")
python (Join-Path $root "scripts/third_party.py") -o (Join-Path $app "THIRD-PARTY-NOTICES.md")
if ($LASTEXITCODE -ne 0) { throw "third_party.py failed" }

$signed = $false
if ($env:WINDOWS_CERT_PFX -and $env:WINDOWS_CERT_PASSWORD) {
    $pfx = Join-Path $work "cert.pfx"
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERT_PFX))
    foreach ($exe in "pnxr.exe", "pnxr-gui.exe") {
        signtool sign /f $pfx /p $env:WINDOWS_CERT_PASSWORD /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 (Join-Path $app $exe)
        if ($LASTEXITCODE -ne 0) { throw "signtool failed" }
    }
    Remove-Item $pfx
    $signed = $true
}
$suffix = if ($signed) { "" } else { "-unsigned" }
$zip = "Pairnox-$Version-windows-x86_64$suffix.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path $app -DestinationPath $zip
Write-Host "wrote $zip"
Remove-Item -Recurse -Force $work
