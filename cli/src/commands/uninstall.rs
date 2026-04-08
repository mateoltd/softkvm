use anyhow::Result;
use std::path::PathBuf;

pub async fn run(yes: bool) -> Result<()> {
    if !yes {
        println!("this will remove softkvm, its config, and registered services.");
        println!("run with --yes to confirm, or press Ctrl+C to cancel.");

        print!("continue? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("cancelled");
            return Ok(());
        }
    }

    // stop and remove services
    remove_services();

    // remove firewall rules (Windows)
    remove_firewall_rules();

    // remove binaries
    let install_dir = install_dir();
    if install_dir.exists() {
        println!("removing {}", install_dir.display());
        let _ = std::fs::remove_dir_all(&install_dir);
    }

    // remove setup bundle
    let setup_dir = install_dir
        .parent()
        .map(|p| p.join("setup"))
        .unwrap_or_default();
    if setup_dir.exists() {
        println!("removing {}", setup_dir.display());
        let _ = std::fs::remove_dir_all(&setup_dir);
    }

    // remove config
    for path in config_paths() {
        if path.exists() {
            println!("removing {}", path.display());
            let _ = std::fs::remove_file(&path);
            // remove parent dir if empty
            if let Some(parent) = path.parent() {
                let _ = std::fs::remove_dir(parent);
            }
        }
    }

    // remove PATH entry from shell profiles (unix only)
    #[cfg(not(target_os = "windows"))]
    clean_shell_profiles();

    println!("softkvm uninstalled");
    Ok(())
}

fn install_dir() -> PathBuf {
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

fn config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    if let Ok(home) = std::env::var("HOME") {
        paths.push(PathBuf::from(&home).join("Library/Application Support/softkvm/softkvm.toml"));
    }

    #[cfg(target_os = "windows")]
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        paths.push(PathBuf::from(local).join("softkvm").join("softkvm.toml"));
    }

    #[cfg(target_os = "linux")]
    if let Ok(home) = std::env::var("HOME") {
        let xdg = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config"));
        paths.push(PathBuf::from(xdg).join("softkvm").join("softkvm.toml"));
    }

    paths
}

fn remove_services() {
    #[cfg(target_os = "macos")]
    {
        for name in &["softkvm-orchestrator", "softkvm-agent"] {
            let label = format!("dev.softkvm.{name}");
            if let Ok(home) = std::env::var("HOME") {
                let plist = PathBuf::from(&home)
                    .join("Library/LaunchAgents")
                    .join(format!("{label}.plist"));
                if plist.exists() {
                    println!("unloading {label}");
                    let _ = std::process::Command::new("launchctl")
                        .args(["unload", &plist.to_string_lossy()])
                        .output();
                    let _ = std::fs::remove_file(&plist);
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        for name in &["softkvm-orchestrator", "softkvm-agent"] {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "stop", name])
                .output();
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "disable", name])
                .output();

            let xdg = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(|h| format!("{h}/.config"))
                    .unwrap_or_default()
            });
            let unit = PathBuf::from(&xdg)
                .join("systemd/user")
                .join(format!("{name}.service"));
            if unit.exists() {
                println!("removing {}", unit.display());
                let _ = std::fs::remove_file(&unit);
            }
        }
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();
    }

    #[cfg(target_os = "windows")]
    {
        for name in &["softkvm-orchestrator", "softkvm-agent"] {
            // kill running process
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/IM", &format!("{name}.exe")])
                .output();
            // remove Run key
            let _ = std::process::Command::new("reg")
                .args([
                    "delete",
                    "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                    "/v",
                    name,
                    "/f",
                ])
                .output();
        }
    }
}

fn remove_firewall_rules() {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("netsh")
            .args([
                "advfirewall",
                "firewall",
                "delete",
                "rule",
                "name=softkvm discovery",
            ])
            .output();
        let _ = std::process::Command::new("netsh")
            .args([
                "advfirewall",
                "firewall",
                "delete",
                "rule",
                "name=softkvm agent",
            ])
            .output();
    }
}

#[cfg(not(target_os = "windows"))]
fn clean_shell_profiles() {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return,
    };

    let profiles = [
        format!("{home}/.bashrc"),
        format!("{home}/.zshrc"),
        format!("{home}/.config/fish/config.fish"),
        format!("{home}/.profile"),
    ];

    for path in &profiles {
        let path = PathBuf::from(path);
        if !path.exists() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        // remove the softkvm PATH block (comment + export line)
        let filtered: Vec<&str> = content
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                trimmed != "# softkvm" && !trimmed.contains(".softkvm/bin")
            })
            .collect();
        if filtered.len() < content.lines().count() {
            println!("cleaning {}", path.display());
            let _ = std::fs::write(&path, filtered.join("\n") + "\n");
        }
    }
}
