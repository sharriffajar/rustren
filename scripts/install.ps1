$ErrorActionPreference = "Stop"

$Repo = "sharriffajar/rustren"
$Asset = "rustren-windows-x86_64.zip"
$Url = "https://github.com/$Repo/releases/latest/download/$Asset"
$InstallDir = if ($env:RUSTREN_INSTALL_DIR) { $env:RUSTREN_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "rustren\bin" }

$TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("rustren-install-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null

try {
    $ZipPath = Join-Path $TmpDir $Asset
    Write-Host "Downloading $Asset from latest release..."
    Invoke-WebRequest -Uri $Url -OutFile $ZipPath -UseBasicParsing

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Expand-Archive -Path $ZipPath -DestinationPath $TmpDir -Force
    Copy-Item -Path (Join-Path $TmpDir "rustren.exe") -Destination (Join-Path $InstallDir "rustren.exe") -Force

    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not $UserPath) { $UserPath = "" }
    if ($UserPath -notlike "*$InstallDir*") {
        $NewPath = if ($UserPath) { "$UserPath;$InstallDir" } else { $InstallDir }
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
        $env:Path = "$env:Path;$InstallDir"
        Write-Host "Added $InstallDir to your user PATH (restart your terminal if needed)."
    }

    Write-Host "Installed rustren to $(Join-Path $InstallDir 'rustren.exe')"

    # Install agent skills for Claude Code / AI coding agents (disable with RUSTREN_NO_SKILLS=1)
    if ($env:RUSTREN_NO_SKILLS -ne "1") {
        try {
            $SkillDir = Join-Path $HOME ".claude\skills\rustren"
            $SmSubDir = Join-Path $SkillDir "skills\simple-motion"
            $SmTopDir = Join-Path $HOME ".claude\skills\simple-motion"
            $SkillUrl = "https://raw.githubusercontent.com/$Repo/main/SKILL.md"
            $SmUrl = "https://raw.githubusercontent.com/$Repo/main/skills/simple-motion/SKILL.md"
            New-Item -ItemType Directory -Path $SmSubDir -Force | Out-Null
            New-Item -ItemType Directory -Path $SmTopDir -Force | Out-Null
            Invoke-WebRequest -Uri $SkillUrl -OutFile (Join-Path $SkillDir "SKILL.md") -UseBasicParsing
            Invoke-WebRequest -Uri $SmUrl -OutFile (Join-Path $SmSubDir "SKILL.md") -UseBasicParsing
            Copy-Item -Path (Join-Path $SmSubDir "SKILL.md") -Destination (Join-Path $SmTopDir "SKILL.md") -Force
            Write-Host "Installed agent skills to $SkillDir and $SmTopDir"
        } catch {
            # Best-effort; do not fail binary install if offline/rate-limited
        }
    }
    if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) {
        Write-Host ""
        Write-Host "Note: ffmpeg was not found in PATH (required for MP4 encoding)."
        Write-Host "  winget install Gyan.FFmpeg"
    }

    & (Join-Path $InstallDir "rustren.exe") --version
} finally {
    Remove-Item -Path $TmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
