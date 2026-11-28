# Builds the Windows installer (Inno Setup) from the portable zip:
#
#   packaging/windows/build-setup.ps1 <portable zip> <version>
#
# Produces Pairnox-<version>-windows-x86_64-setup[-unsigned].exe in the
# current directory. The files are the ones of the zip (already signed when
# a certificate is set up); the installer itself is signed with signtool under
# the same WINDOWS_CERT_PFX / WINDOWS_CERT_PASSWORD rules as the zip.
param(
    [Parameter(Mandatory = $true)][string]$Zip,
    [Parameter(Mandatory = $true)][string]$Version
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "../..")
$work = Join-Path ([System.IO.Path]::GetTempPath()) ("pnxr-setup-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force $work | Out-Null
Expand-Archive $Zip $work
$app = Join-Path $work "Pairnox"

$iscc = (Get-Command iscc -ErrorAction SilentlyContinue).Source
if (-not $iscc) {
    foreach ($candidate in "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe") {
        if (Test-Path $candidate) { $iscc = $candidate; break }
    }
}
if (-not $iscc) { throw "Inno Setup (iscc) is not installed" }

$signed = [bool]($env:WINDOWS_CERT_PFX -and $env:WINDOWS_CERT_PASSWORD)
$suffix = if ($signed) { "" } else { "-unsigned" }
$name = "Pairnox-$Version-windows-x86_64-setup$suffix"
$out = (Get-Location).Path
$numeric = ($Version -replace '^v', '') -replace '[^0-9.].*$', ''
& $iscc "/DAppVersion=$numeric" "/DSourceDir=$app" "/DOutDir=$out" "/DOutName=$name" (Join-Path $PSScriptRoot "pairnox.iss")
if ($LASTEXITCODE -ne 0) { throw "iscc failed" }
$exe = Join-Path $out "$name.exe"
if (-not (Test-Path $exe)) { throw "the installer was not written" }

if ($signed) {
    $pfx = Join-Path $work "cert.pfx"
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERT_PFX))
    signtool sign /f $pfx /p $env:WINDOWS_CERT_PASSWORD /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 $exe
    if ($LASTEXITCODE -ne 0) { throw "signtool failed" }
    Remove-Item $pfx
}
Write-Host "wrote $exe"
Remove-Item -Recurse -Force $work
