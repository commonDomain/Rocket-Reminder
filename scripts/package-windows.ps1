param(
    [string]$OutputDirectory = [Environment]::GetFolderPath("Desktop"),
    [switch]$SkipDesktopCopy
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

function Resolve-CommandPath {
    param(
        [string]$Name,
        [string[]]$Candidates
    )

    foreach ($candidate in $Candidates) {
        if ($candidate -and (Test-Path -LiteralPath $candidate)) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }

    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    throw "Cannot find required command: $Name"
}

$npm = Resolve-CommandPath "npm.cmd" @(
    "D:\program\nodejs\npm.cmd",
    "$env:ProgramFiles\nodejs\npm.cmd",
    "${env:ProgramFiles(x86)}\nodejs\npm.cmd"
)

$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if (Test-Path -LiteralPath $cargoBin) {
    $env:PATH = "$cargoBin;$env:PATH"
}

Write-Host "Building Rocket Reminder installer..."
Write-Host "Repository: $repoRoot"
Write-Host "npm: $npm"

& $npm run tauri:build

if ($LASTEXITCODE -ne 0) {
    throw "Tauri build failed with exit code $LASTEXITCODE"
}

$installer = Join-Path $repoRoot "src-tauri\target\release\bundle\nsis\Rocket Reminder_0.1.0_x64-setup.exe"
if (!(Test-Path -LiteralPath $installer)) {
    throw "Installer was not found: $installer"
}

$installerItem = Get-Item -LiteralPath $installer
$hash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash

Write-Host "Installer built:"
Write-Host "  $($installerItem.FullName)"
Write-Host "  Bytes: $($installerItem.Length)"
Write-Host "  SHA256: $hash"

if (!$SkipDesktopCopy) {
    New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
    $target = Join-Path $OutputDirectory $installerItem.Name
    Copy-Item -LiteralPath $installerItem.FullName -Destination $target -Force
    $targetHash = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash
    Write-Host "Copied installer:"
    Write-Host "  $target"
    Write-Host "  SHA256: $targetHash"
}
