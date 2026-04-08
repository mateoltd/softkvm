use anyhow::Result;

pub async fn run() -> Result<()> {
    // the setup TUI is a separate Node.js binary (full-kvm-setup)
    // this command delegates to it
    println!("launching interactive setup...");
    println!("(not yet implemented -- requires full-kvm-setup binary)");
    Ok(())
}
