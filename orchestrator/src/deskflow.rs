use softkvm_core::config::Config;
use softkvm_core::keymap::deskflow_modifier_mapping;
use softkvm_core::topology::MachineRole;
use std::fmt::Write;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

/// generate a Deskflow server text configuration from our config
pub fn generate_deskflow_config(config: &Config) -> String {
    let mut out = String::new();

    let server = config
        .machines
        .iter()
        .find(|m| m.role == MachineRole::Server)
        .expect("config validation ensures exactly one server");

    // screens section
    writeln!(out, "section: screens").unwrap();
    for machine in &config.machines {
        writeln!(out, "\t{}:", machine.name).unwrap();

        // apply modifier remapping if auto_remap is enabled
        if config.keyboard.auto_remap && machine.role == MachineRole::Client {
            let mapping = deskflow_modifier_mapping(server.os, machine.os);
            for (from, to) in &mapping {
                writeln!(out, "\t\t{from} = {to}").unwrap();
            }
        }
    }
    writeln!(out, "end").unwrap();

    // links section
    writeln!(out, "section: links").unwrap();
    for (machine_name, links) in &config.layout {
        writeln!(out, "\t{machine_name}:").unwrap();
        if let Some(ref left) = links.left {
            writeln!(out, "\t\tleft = {left}").unwrap();
        }
        if let Some(ref right) = links.right {
            writeln!(out, "\t\tright = {right}").unwrap();
        }
        if let Some(ref up) = links.up {
            writeln!(out, "\t\tup = {up}").unwrap();
        }
        if let Some(ref down) = links.down {
            writeln!(out, "\t\tdown = {down}").unwrap();
        }
    }
    writeln!(out, "end").unwrap();

    // options section
    writeln!(out, "section: options").unwrap();
    writeln!(out, "\tswitchDelay = {}", config.deskflow.switch_delay).unwrap();
    if config.deskflow.switch_double_tap > 0 {
        writeln!(
            out,
            "\tswitchDoubleTap = {}",
            config.deskflow.switch_double_tap
        )
        .unwrap();
    }
    if config.deskflow.clipboard_sharing {
        writeln!(out, "\tclipboardSharing = true").unwrap();
        writeln!(
            out,
            "\tclipboardSharingSize = {}",
            config.deskflow.clipboard_max_size_kb
        )
        .unwrap();
    } else {
        writeln!(out, "\tclipboardSharing = false").unwrap();
    }
    writeln!(out, "end").unwrap();

    out
}

/// write the generated deskflow config to a temp file and return the path
pub fn write_deskflow_config(config: &Config) -> std::io::Result<PathBuf> {
    let conf_content = generate_deskflow_config(config);
    let dir = std::env::temp_dir().join("softkvm");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("deskflow.conf");
    std::fs::write(&path, &conf_content)?;
    tracing::info!(path = %path.display(), "wrote deskflow config");
    Ok(path)
}

/// manages a deskflow-core server child process
pub struct DeskflowProcess {
    child: Child,
    config_path: PathBuf,
}

impl DeskflowProcess {
    /// spawn deskflow-core server with stdout/stderr captured
    /// returns the process handle and a receiver for stdout lines
    pub async fn spawn(config: &Config) -> anyhow::Result<(Self, mpsc::Receiver<String>)> {
        let config_path = write_deskflow_config(config)?;
        let binary = &config.deskflow.binary_path;

        tracing::info!(
            binary = binary,
            config = %config_path.display(),
            "spawning deskflow-core server"
        );

        let mut child = Command::new(binary)
            .arg("--server")
            .arg("--no-daemon")
            .arg("--config")
            .arg(&config_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        let (tx, rx) = mpsc::channel(256);

        // pipe stdout lines to the channel
        let tx_out = tx.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx_out.send(line).await.is_err() {
                    break;
                }
            }
        });

        // pipe stderr lines to the channel too (deskflow logs to both)
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).await.is_err() {
                    break;
                }
            }
        });

        Ok((Self { child, config_path }, rx))
    }

    /// wait for the process to exit and return the exit status
    #[allow(dead_code)]
    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }

    /// kill the process
    pub async fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill().await
    }

    /// clean up the temp config file
    pub fn cleanup(&self) {
        let _ = std::fs::remove_file(&self.config_path);
    }
}

impl Drop for DeskflowProcess {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_CONFIG: &str = r#"
[general]
role = "orchestrator"

[deskflow]
switch_delay = 200
clipboard_sharing = true
clipboard_max_size_kb = 2048

[[machine]]
name = "Windows-PC"
role = "server"
os = "windows"

[[machine]]
name = "MacBook"
role = "client"
os = "macos"

[layout]
"Windows-PC" = { right = "MacBook" }
"MacBook" = { left = "Windows-PC" }
"#;

    #[test]
    fn test_generate_deskflow_config() {
        let config = Config::from_toml(TEST_CONFIG).unwrap();
        let deskflow_conf = generate_deskflow_config(&config);

        assert!(deskflow_conf.contains("Windows-PC:"));
        assert!(deskflow_conf.contains("MacBook:"));

        // Windows server -> Mac client: ctrl = meta, super = ctrl
        assert!(deskflow_conf.contains("ctrl = meta"));
        assert!(deskflow_conf.contains("super = ctrl"));

        assert!(deskflow_conf.contains("right = MacBook"));
        assert!(deskflow_conf.contains("left = Windows-PC"));

        assert!(deskflow_conf.contains("switchDelay = 200"));
        assert!(deskflow_conf.contains("clipboardSharing = true"));
        assert!(deskflow_conf.contains("clipboardSharingSize = 2048"));
    }

    #[test]
    fn test_no_remapping_same_os() {
        let toml = r#"
[general]
role = "orchestrator"

[[machine]]
name = "PC1"
role = "server"
os = "windows"

[[machine]]
name = "PC2"
role = "client"
os = "windows"

[layout]
"PC1" = { right = "PC2" }
"PC2" = { left = "PC1" }
"#;
        let config = Config::from_toml(toml).unwrap();
        let deskflow_conf = generate_deskflow_config(&config);

        assert!(!deskflow_conf.contains("meta ="));
        assert!(!deskflow_conf.contains("ctrl ="));
    }

    #[test]
    fn test_write_deskflow_config() {
        let config = Config::from_toml(TEST_CONFIG).unwrap();
        let path = write_deskflow_config(&config).unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("section: screens"));
        std::fs::remove_file(&path).unwrap();
    }
}
