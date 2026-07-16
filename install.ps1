# pult-desktop installer for Windows (PowerShell).
#
#   irm https://raw.githubusercontent.com/lonic-software/pult-desktop/main/install.ps1 | iex
#
# Runs the app's NSIS setup silently (a per-user install — no admin prompt),
# then adds the install directory to your user PATH so `pult-desktop` works
# from a terminal. The setup also creates the usual Start Menu shortcut.
#
# Environment overrides:
#   PULT_DESKTOP_VERSION   install a specific tag, e.g. v0.1.0 (default: latest published release)
#   PULT_DESKTOP_REPO      GitHub repo slug          (default: lonic-software/pult-desktop)

$ErrorActionPreference = "Stop"

$Repo = if ($env:PULT_DESKTOP_REPO) { $env:PULT_DESKTOP_REPO } else { "lonic-software/pult-desktop" }
$Version = if ($env:PULT_DESKTOP_VERSION) { $env:PULT_DESKTOP_VERSION } else { "latest" }

if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq "Arm64") {
    throw "no Windows ARM build yet - install from a native x64 shell, or build from source (npm run tauri build)"
}

# Resolve the release tag via the GitHub API. Releases start as drafts (CI
# publishes assets to a draft; a human publishes it), so both
# /releases/latest and /releases/tags/<tag> 404 until that happens — the
# common failure mode here, so it gets its own error rather than a bare
# download failure.
try {
    if ($Version -eq "latest") {
        $rel = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest" -UseBasicParsing
    } else {
        $TagIn = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
        $rel = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/tags/$TagIn" -UseBasicParsing
    }
    $Tag = $rel.tag_name
} catch {
    throw "no published release found for $Repo - it may still be a draft pending publish. Check https://github.com/$Repo/releases, or pass PULT_DESKTOP_VERSION for an exact tag."
}
$Num = $Tag.TrimStart("v")

$Base = "https://github.com/$Repo/releases/download/$Tag"
$Asset = "pult-desktop_${Num}_x64-setup.exe"

$Tmp = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
New-Item -ItemType Directory $Tmp | Out-Null
try {
    $Setup = Join-Path $Tmp $Asset
    Write-Host "downloading $Base/$Asset"
    try {
        Invoke-WebRequest "$Base/$Asset" -OutFile $Setup -UseBasicParsing
    } catch {
        throw "download failed - does $Tag have a $Asset asset? $Base/$Asset"
    }

    Write-Host "running the installer (silent, per-user)..."
    Start-Process -FilePath $Setup -ArgumentList "/S" -Wait

    # Tauri's NSIS setup installs per-user to %LOCALAPPDATA%\pult-desktop by
    # default. Check that first, then fall back to a bounded search of
    # LOCALAPPDATA — never Program Files, where a per-user install can't land.
    $InstallDir = Join-Path $env:LOCALAPPDATA "pult-desktop"
    $exe = Join-Path $InstallDir "pult-desktop.exe"
    if (-not (Test-Path $exe)) {
        $found = Get-ChildItem -Path $env:LOCALAPPDATA -Filter "pult-desktop.exe" `
                     -Recurse -Depth 3 -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { $exe = $found.FullName; $InstallDir = Split-Path $exe } else { $exe = $null }
    }

    if ($exe) {
        Write-Host "installed $exe"
        $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
        if ([string]::IsNullOrEmpty($UserPath)) {
            [Environment]::SetEnvironmentVariable("Path", $InstallDir, "User")
            Write-Host "added $InstallDir to your user PATH (restart your terminal, then run 'pult-desktop')"
        } elseif ($UserPath -notlike "*$InstallDir*") {
            [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
            Write-Host "added $InstallDir to your user PATH (restart your terminal, then run 'pult-desktop')"
        } else {
            Write-Host "run 'pult-desktop' to launch"
        }
    } else {
        Write-Host "installed - launch 'pult-desktop' from the Start Menu"
    }
} finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}
