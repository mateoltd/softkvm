$ErrorActionPreference = "Stop"

$Repo = "mateoltd/full-kvm"
$InstallDir = "$env:LOCALAPPDATA\full-kvm\bin"

function Main {
    Write-Host "full-kvm installer"
    Write-Host ""

    # detect architecture
    $Arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    switch ($Arch) {
        "X64"   { $Target = "x86_64-pc-windows-msvc" }
        "Arm64" { $Target = "aarch64-pc-windows-msvc" }
        default { Write-Error "unsupported architecture: $Arch"; exit 1 }
    }

    Write-Host "detected: $Target"

    # get latest release
    $Release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
    $Latest = $Release.tag_name
    Write-Host "latest version: $Latest"

    # check if already installed
    if (Test-Path "$InstallDir\full-kvm.exe") {
        $Current = & "$InstallDir\full-kvm.exe" --version 2>$null
        if ($Current -match $Latest.TrimStart("v")) {
            Write-Host "already up to date ($Latest)"
            Run-Setup
            return
        }
        Write-Host "updating from $Current to $Latest"
    }

    # download and extract
    $Url = "https://github.com/$Repo/releases/download/$Latest/full-kvm-$Latest-$Target.zip"
    Write-Host "downloading $Url"

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $TempZip = "$env:TEMP\full-kvm.zip"
    Invoke-WebRequest -Uri $Url -OutFile $TempZip
    Expand-Archive -Path $TempZip -DestinationPath $InstallDir -Force
    Remove-Item $TempZip

    # add to PATH
    $CurrentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($CurrentPath -notlike "*full-kvm*") {
        [Environment]::SetEnvironmentVariable("PATH", "$InstallDir;$CurrentPath", "User")
        Write-Host "added $InstallDir to user PATH"
    }
    $env:PATH = "$InstallDir;$env:PATH"

    Write-Host "installed to $InstallDir"
    Run-Setup
}

function Run-Setup {
    $SetupBin = "$InstallDir\full-kvm-setup.exe"
    if (Test-Path $SetupBin) {
        Write-Host ""
        & $SetupBin
    }
}

Main
