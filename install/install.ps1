$ErrorActionPreference = "Stop"

$Repo = "mateoltd/softkvm"
$InstallDir = "$env:LOCALAPPDATA\softkvm\bin"
$RepoUrl = "https://github.com/$Repo.git"

function Info($msg)  { Write-Host "▸ $msg" -ForegroundColor Green }
function Warn($msg)  { Write-Host "▸ $msg" -ForegroundColor Yellow }
function Error($msg) { Write-Host "▸ $msg" -ForegroundColor Red }

function Main {
    Write-Host ""
    Write-Host "softkvm installer" -NoNewline
    Write-Host ""
    Write-Host ""

    $script:Target = Detect-Platform
    if (-not $script:Target) { return }
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

    $installed = $false
    if (Try-ReleaseInstall) {
        Info "installed from release"
        $installed = $true
    }
    elseif (Try-SourceInstall) {
        Info "built from source"
        $installed = $true
    }

    if (-not $installed) {
        Error "installation failed"
        Write-Host ""
        Write-Host "  manual install: https://github.com/$Repo#build-from-source"
        return
    }

    Install-Deskflow
    Register-Path
    Write-Host ""
    Info "installed to $InstallDir"
    Run-PostInstall
}

function Detect-Platform {
    $Arch = $env:PROCESSOR_ARCHITECTURE
    switch ($Arch) {
        "AMD64" { $t = "x86_64-pc-windows-msvc" }
        "ARM64" { $t = "aarch64-pc-windows-msvc" }
        default { Error "unsupported architecture: $Arch"; return $null }
    }
    Info "platform: $t"
    return $t
}

function Try-ReleaseInstall {
    try {
        $release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest" -ErrorAction Stop
        $latest = $release.tag_name

        # check if already at this version
        if (Test-Path "$InstallDir\softkvm.exe") {
            $current = & "$InstallDir\softkvm.exe" --version 2>$null
            if ($current -match $latest.TrimStart("v")) {
                Info "already up to date ($latest)"
                return $true
            }
        }

        $url = "https://github.com/$Repo/releases/download/$latest/softkvm-$latest-$script:Target.zip"
        Info "downloading $latest for $script:Target"

        $tempZip = "$env:TEMP\softkvm-$([guid]::NewGuid().ToString('N').Substring(0,8)).zip"
        Invoke-WebRequest -Uri $url -OutFile $tempZip -ErrorAction Stop
        Expand-Archive -Path $tempZip -DestinationPath $InstallDir -Force
        Remove-Item $tempZip -ErrorAction SilentlyContinue
        return $true
    }
    catch {
        Warn "no releases found, falling back to source build"
        return $false
    }
}

function Try-SourceInstall {
    # native commands (git, cargo) write progress to stderr which
    # ErrorActionPreference=Stop treats as terminating errors
    $ErrorActionPreference = 'Continue'
    # need git + cargo
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
        Error "git is required to build from source"
        return $false
    }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Warn "rust not found"
        Write-Host "  install from: https://rustup.rs"
        return $false
    }

    $buildDir = Join-Path $env:TEMP "softkvm-build-$([guid]::NewGuid().ToString('N').Substring(0,8))"

    try {
        Info "cloning repository"
        $cloneOut = git clone --depth 1 --quiet $RepoUrl $buildDir 2>&1
        if ($LASTEXITCODE -ne 0) { throw "git clone failed: $cloneOut" }

        Info "building (release mode)"
        $buildOut = cargo build --release --manifest-path "$buildDir\Cargo.toml" `
            --workspace `
            --features softkvm-orchestrator/real-ddc,softkvm-cli/real-ddc 2>&1
        if ($LASTEXITCODE -ne 0) {
            $errors = $buildOut | Where-Object { $_ -match "error\[" } | Select-Object -Last 20
            throw "cargo build failed:`n$($errors -join "`n")"
        }

        Info "copying binaries"
        $copied = 0
        foreach ($bin in @("softkvm", "softkvm-orchestrator", "softkvm-agent")) {
            $src = "$buildDir\target\release\$bin.exe"
            if (Test-Path $src) {
                Copy-Item $src "$InstallDir\$bin.exe" -Force
                $copied++
            } else {
                Warn "binary not found: $bin.exe"
            }
        }
        if ($copied -eq 0) { throw "no binaries were produced by the build" }

        # bundle the setup TUI while we still have the source tree
        $setupSrc = Join-Path $buildDir "setup"
        if (Test-Path "$setupSrc\package.json") {
            $bundler = $null
            if (Get-Command bun -ErrorAction SilentlyContinue) { $bundler = "bun" }
            elseif (Get-Command npx -ErrorAction SilentlyContinue) { $bundler = "npx" }

            if ($bundler) {
                Info "bundling setup wizard"
                $setupDest = Join-Path (Split-Path $InstallDir -Parent) "setup"
                New-Item -ItemType Directory -Force -Path $setupDest | Out-Null
                try {
                    Push-Location $setupSrc
                    if ($bundler -eq "bun") {
                        bun install --silent 2>$null
                        bun build --outfile="$setupDest\setup.mjs" --target=node src/index.ts 2>$null
                    } else {
                        npm install --silent 2>$null
                        npx esbuild --bundle --platform=node --format=esm --outfile="$setupDest\setup.mjs" src/index.ts 2>$null
                    }
                }
                catch {}
                finally { Pop-Location }

                if (Test-Path "$setupDest\setup.mjs") {
                    # create a .cmd wrapper that invokes node
                    $wrapper = "@echo off`r`nnode `"%~dp0..\setup\setup.mjs`" %*"
                    Set-Content -Path "$InstallDir\softkvm-setup.cmd" -Value $wrapper -Encoding ASCII
                } else {
                    Warn "setup wizard build failed (will use manual setup)"
                }
            }
        }

        return $true
    }
    catch {
        Error "source build failed: $_"
        return $false
    }
    finally {
        if (Test-Path $buildDir) {
            Remove-Item $buildDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

function Install-Deskflow {
    # check if deskflow-core is already in PATH
    if (Get-Command deskflow-core -ErrorAction SilentlyContinue) {
        Info "deskflow-core already installed"
        return
    }

    # check known install locations
    $knownPaths = @(
        "$env:ProgramFiles\Deskflow\deskflow-core.exe"
        "${env:ProgramFiles(x86)}\Deskflow\deskflow-core.exe"
    )
    foreach ($p in $knownPaths) {
        if (Test-Path $p) {
            Info "deskflow-core found at $p"
            return
        }
    }

    Info "installing deskflow (required for mouse/keyboard sharing)"

    # try winget first
    if (Get-Command winget -ErrorAction SilentlyContinue) {
        try {
            $out = winget install --id=Deskflow.Deskflow -e --accept-package-agreements --accept-source-agreements 2>&1
            if ($LASTEXITCODE -eq 0) {
                Info "deskflow installed via winget"
                return
            }
        }
        catch {}
    }

    # try chocolatey
    if (Get-Command choco -ErrorAction SilentlyContinue) {
        try {
            choco install deskflow -y 2>$null
            if ($LASTEXITCODE -eq 0) {
                Info "deskflow installed via chocolatey"
                return
            }
        }
        catch {}
    }

    Warn "could not install deskflow automatically"
    Write-Host "  install manually: winget install --id=Deskflow.Deskflow -e"
    Write-Host "  or download from: https://github.com/deskflow/deskflow/releases"
}

function Register-Path {
    $currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($currentPath -notlike "*softkvm*") {
        [Environment]::SetEnvironmentVariable("PATH", "$InstallDir;$currentPath", "User")
        Info "added to user PATH"
    }
    $env:PATH = "$InstallDir;$env:PATH"
}

function Run-PostInstall {
    Write-Host ""
    Write-Host "scanning monitors" -ForegroundColor White
    Write-Host ""

    # detect monitors
    try {
        & "$InstallDir\softkvm.exe" scan 2>$null
        Write-Host ""
    }
    catch {
        Warn "no DDC/CI monitors detected (can be configured manually)"
        Write-Host ""
    }

    # run interactive setup
    $setupCmd = "$InstallDir\softkvm-setup.cmd"
    if (Test-Path $setupCmd) {
        & $setupCmd
    }
    else {
        Show-ManualSetup
    }
}

function Show-ManualSetup {
    Write-Host "next steps" -ForegroundColor White
    Write-Host ""
    Write-Host "  1. create a config file:"
    Write-Host "     softkvm setup          (interactive, requires bun)"
    Write-Host "     softkvm validate       (check an existing config)"
    Write-Host ""
    Write-Host "  2. start the daemon:"
    Write-Host "     softkvm-orchestrator   (on the primary machine)"
    Write-Host "     softkvm-agent          (on each secondary machine)"
    Write-Host ""
    Write-Host "  docs: https://github.com/$Repo#quick-start"
}

Main
