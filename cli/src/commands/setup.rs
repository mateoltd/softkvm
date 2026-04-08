use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

pub async fn run() -> Result<()> {
    let bin = find_setup_binary().ok_or_else(|| {
        anyhow::anyhow!(
            "softkvm-setup binary not found.\n\
             install it via the installer script, or build it:\n\
             cd setup && bun install && bun run build"
        )
    })?;

    let status = Command::new(&bin)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()?;

    if !status.success() {
        anyhow::bail!(
            "setup wizard exited with code {}",
            status.code().unwrap_or(-1)
        );
    }

    Ok(())
}

fn find_setup_binary() -> Option<PathBuf> {
    let name = if cfg!(target_os = "windows") {
        "softkvm-setup.exe"
    } else {
        "softkvm-setup"
    };

    // check next to the current executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // check known install directories
    let candidates: Vec<PathBuf> = if cfg!(target_os = "windows") {
        std::env::var("LOCALAPPDATA")
            .ok()
            .map(|d| vec![PathBuf::from(d).join("softkvm").join("bin").join(name)])
            .unwrap_or_default()
    } else {
        std::env::var("HOME")
            .ok()
            .map(|h| vec![PathBuf::from(h).join(".softkvm").join("bin").join(name)])
            .unwrap_or_default()
    };

    for c in &candidates {
        if c.exists() {
            return Some(c.clone());
        }
    }

    // check PATH
    if let Ok(output) = Command::new(if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    })
    .arg(name)
    .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let p = Path::new(&path);
            if p.exists() {
                return Some(p.to_path_buf());
            }
        }
    }

    None
}
