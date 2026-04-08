use anyhow::Result;

pub async fn run() -> Result<()> {
    // the setup TUI is a separate Node.js binary (softkvm-setup)
    // this command delegates to it
    println!("launching interactive setup...");
    println!("(not yet implemented -- requires softkvm-setup binary)");
    Ok(())
}
