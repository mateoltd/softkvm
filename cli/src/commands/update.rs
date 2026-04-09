use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

const REPO: &str = "mateoltd/softkvm";
const REPO_URL: &str = "https://github.com/mateoltd/softkvm.git";
const BINARIES: &[&str] = &["softkvm", "softkvm-orchestrator", "softkvm-agent"];

pub async fn run(dev: bool, no_push: bool) -> Result<()> {
    let install_dir = detect_install_dir();
    let current = env!("CARGO_PKG_VERSION");

    println!("softkvm update");
    println!("  installed: v{current}");
    println!("  install dir: {}", install_dir.display());
    println!();

    if dev {
        println!("building from source (--dev)");
        build_from_source(&install_dir)?;
    } else {
        match check_latest_release(current)? {
            Some((version, url)) => {
                println!("new release available: {version}");
                download_and_install(&url, &install_dir)?;
            }
            None => {
                println!("already up to date (v{current})");
                println!("use --dev to rebuild from source");
                return Ok(());
            }
        }
    }

    // rebuild setup wizard if a bundler is available
    rebuild_setup_wizard(&install_dir);

    println!();
    println!("binaries updated");

    // push update to connected agents via orchestrator IPC
    if !no_push {
        push_to_agents(dev).await;
    }

    // restart local daemons
    restart_daemons();

    Ok(())
}

fn detect_install_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var("LOCALAPPDATA")
            .map(|d| PathBuf::from(d).join("softkvm").join("bin"))
            .unwrap_or_else(|_| PathBuf::from("C:\\softkvm\\bin"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME")
            .map(|h| PathBuf::from(h).join(".softkvm").join("bin"))
            .unwrap_or_else(|_| PathBuf::from("/usr/local/bin"))
    }
}

fn target_triple() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "x86_64-apple-darwin";
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "aarch64-apple-darwin";
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "x86_64-unknown-linux-gnu";
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "aarch64-unknown-linux-gnu";
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return "x86_64-pc-windows-msvc";
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    return "aarch64-pc-windows-msvc";
}

// check GitHub releases for a newer version
fn check_latest_release(current: &str) -> Result<Option<(String, String)>> {
    println!("checking GitHub releases...");

    let output = Command::new(curl_bin())
        .args([
            "-fsSL",
            &format!("https://api.github.com/repos/{REPO}/releases/latest"),
        ])
        .output()
        .context("curl is required to check for updates")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("404") || stderr.contains("Not Found") {
            println!("no releases published yet");
            return Ok(None);
        }
        bail!("failed to check releases: {stderr}");
    }

    let body = String::from_utf8_lossy(&output.stdout);
    let tag = extract_json_string(&body, "tag_name").unwrap_or_default();
    if tag.is_empty() {
        println!("no releases found");
        return Ok(None);
    }

    let version = tag.trim_start_matches('v');
    if version == current {
        return Ok(None);
    }

    let target = target_triple();
    let ext = if cfg!(target_os = "windows") {
        "zip"
    } else {
        "tar.gz"
    };
    let url =
        format!("https://github.com/{REPO}/releases/download/{tag}/softkvm-{tag}-{target}.{ext}");

    Ok(Some((tag.to_string(), url)))
}

fn download_and_install(url: &str, install_dir: &PathBuf) -> Result<()> {
    std::fs::create_dir_all(install_dir)?;
    println!("downloading {url}");

    if cfg!(target_os = "windows") {
        let temp = std::env::temp_dir().join("softkvm-update.zip");
        let status = Command::new(curl_bin())
            .args(["-fsSL", "-o", &temp.to_string_lossy(), url])
            .status()
            .context("download failed")?;
        if !status.success() {
            bail!("download failed");
        }

        rename_running_binaries(install_dir);

        let status = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Expand-Archive -Force -Path '{}' -DestinationPath '{}'",
                    temp.display(),
                    install_dir.display()
                ),
            ])
            .status()
            .context("extraction failed")?;
        let _ = std::fs::remove_file(&temp);
        if !status.success() {
            bail!("extraction failed");
        }
    } else {
        let status = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "curl -fsSL '{}' | tar xz -C '{}'",
                url,
                install_dir.display()
            ))
            .status()
            .context("download failed")?;
        if !status.success() {
            bail!("download and extraction failed");
        }
    }

    println!("installed to {}", install_dir.display());
    Ok(())
}

fn build_from_source(install_dir: &PathBuf) -> Result<()> {
    if Command::new("git").arg("--version").output().is_err() {
        bail!("git is required to build from source");
    }
    if Command::new("cargo").arg("--version").output().is_err() {
        bail!("cargo is required (install from https://rustup.rs)");
    }

    let build_dir = std::env::temp_dir().join(format!("softkvm-build-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&build_dir);

    println!("cloning repository...");
    let status = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            REPO_URL,
            &build_dir.to_string_lossy(),
        ])
        .status()
        .context("git clone failed")?;
    if !status.success() {
        bail!("git clone failed");
    }

    println!("building (release mode)...");
    let status = Command::new("cargo")
        .args([
            "build",
            "--release",
            "--manifest-path",
            &build_dir.join("Cargo.toml").to_string_lossy(),
            "--workspace",
            "--features",
            "softkvm-orchestrator/real-ddc,softkvm-cli/real-ddc",
        ])
        .status()
        .context("cargo build failed")?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&build_dir);
        bail!("cargo build failed");
    }

    std::fs::create_dir_all(install_dir)?;
    rename_running_binaries(install_dir);

    let ext = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };

    println!("copying binaries...");
    let mut copied = 0;
    for bin in BINARIES {
        let src = build_dir
            .join("target")
            .join("release")
            .join(format!("{bin}{ext}"));
        if src.exists() {
            let dest = install_dir.join(format!("{bin}{ext}"));
            std::fs::copy(&src, &dest).with_context(|| format!("failed to copy {bin}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
            }
            println!("  {bin}");
            copied += 1;
        } else {
            eprintln!("  warning: {bin} not found in build output");
        }
    }

    let _ = std::fs::remove_dir_all(&build_dir);

    if copied == 0 {
        bail!("no binaries were produced by the build");
    }
    println!(
        "{copied} binary(ies) installed to {}",
        install_dir.display()
    );
    Ok(())
}

fn rebuild_setup_wizard(install_dir: &PathBuf) {
    let setup_dir = install_dir
        .parent()
        .map(|p| p.join("setup"))
        .unwrap_or_default();
    if !setup_dir.join("setup.mjs").exists() {
        return;
    }
    println!("note: setup wizard may need rebuilding (re-run the installer)");
}

#[cfg(target_os = "windows")]
fn rename_running_binaries(install_dir: &PathBuf) {
    for bin in BINARIES {
        let path = install_dir.join(format!("{bin}.exe"));
        let old = install_dir.join(format!("{bin}.exe.old"));
        if path.exists() {
            let _ = std::fs::rename(&path, &old);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn rename_running_binaries(_install_dir: &PathBuf) {}

async fn push_to_agents(dev: bool) {
    println!();
    println!("pushing update to connected agents...");

    match send_ipc_push(dev).await {
        Ok(resp) => {
            if resp.contains("error") {
                eprintln!("  agent push response: {resp}");
            } else {
                println!("  update pushed to agents");
            }
        }
        Err(e) => {
            eprintln!("  could not reach orchestrator: {e}");
            eprintln!("  update agents manually: softkvm update [--dev] on each machine");
        }
    }
}

async fn send_ipc_push(dev: bool) -> Result<String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "push_update",
        "params": { "dev": dev },
        "id": 1
    });
    let mut line = serde_json::to_string(&request)?;
    line.push('\n');

    #[cfg(target_os = "windows")]
    {
        let addr: std::net::SocketAddr = "127.0.0.1:24803".parse()?;
        let mut stream = tokio::net::TcpStream::connect(addr).await?;
        stream.write_all(line.as_bytes()).await?;
        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response).await?;
        Ok(response)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut stream = tokio::net::UnixStream::connect("/tmp/softkvm/ipc.sock").await?;
        stream.write_all(line.as_bytes()).await?;
        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response).await?;
        Ok(response)
    }
}

fn restart_daemons() {
    println!();

    #[cfg(target_os = "macos")]
    {
        for name in &["softkvm-orchestrator", "softkvm-agent"] {
            let label = format!("dev.softkvm.{name}");
            let uid = Command::new("id")
                .arg("-u")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|_| "501".into());
            let status = Command::new("launchctl")
                .args(["kickstart", "-k", &format!("gui/{uid}/{label}")])
                .status();
            match status {
                Ok(s) if s.success() => println!("restarted {name}"),
                _ => {}
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        for name in &["softkvm-orchestrator", "softkvm-agent"] {
            let status = Command::new("systemctl")
                .args(["--user", "restart", name])
                .status();
            match status {
                Ok(s) if s.success() => println!("restarted {name}"),
                _ => {}
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        for name in BINARIES {
            if *name == "softkvm" {
                continue;
            }
            let _ = Command::new("taskkill")
                .args(["/F", "/IM", &format!("{name}.exe")])
                .output();
        }
        // clean up .old files
        let install_dir = detect_install_dir();
        for bin in BINARIES {
            let old = install_dir.join(format!("{bin}.exe.old"));
            let _ = std::fs::remove_file(&old);
        }
        println!("daemons stopped. restart them or they will start at next login.");
    }

    println!();
    println!("update complete");
}

// curl.exe on Windows (avoid PowerShell alias), curl on Unix
fn curl_bin() -> &'static str {
    if cfg!(target_os = "windows") {
        "curl.exe"
    } else {
        "curl"
    }
}

// minimal JSON string extractor to avoid pulling in a full JSON parser for the GitHub API
fn extract_json_string<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let pattern = format!("\"{key}\"");
    let pos = json.find(&pattern)?;
    let rest = &json[pos + pattern.len()..];
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(&rest[..end])
}
