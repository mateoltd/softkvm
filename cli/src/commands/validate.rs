use anyhow::Result;
use std::path::Path;

pub fn run(config_path: &str) -> Result<()> {
    let path = Path::new(config_path);
    if !path.exists() {
        anyhow::bail!("config file not found: {config_path}");
    }

    match full_kvm_core::config::Config::from_file(path) {
        Ok(config) => {
            println!("configuration is valid");
            println!("  machines: {}", config.machines.len());
            println!("  monitors: {}", config.monitors.len());
            println!(
                "  keyboard auto-remap: {}",
                if config.keyboard.auto_remap { "enabled" } else { "disabled" }
            );
            println!(
                "  shortcut translations: {}",
                config.keyboard.translations.len()
            );
            Ok(())
        }
        Err(e) => {
            anyhow::bail!("configuration error: {e}");
        }
    }
}
