use anyhow::Result;
use softkvm_core::ddc::{DdcController, VCP_BRIGHTNESS};
use softkvm_core::input_source::InputSource;
use std::io::Write;

/// identify monitors by flashing their brightness so users can tell
/// which physical screen corresponds to which DDC ID
pub async fn run(monitor_id: Option<&str>) -> Result<()> {
    let controller = create_controller();

    match monitor_id {
        Some(id) => identify_one(&*controller, id).await,
        None => identify_interactive(&*controller).await,
    }
}

async fn identify_one(controller: &dyn DdcController, monitor_id: &str) -> Result<()> {
    // read current brightness
    let original_brightness = controller.get_vcp_feature(monitor_id, VCP_BRIGHTNESS);

    match original_brightness {
        Ok(brightness) => {
            println!("identifying {monitor_id}: screen will flash dark then bright...");

            // dim to 0
            let _ = controller.set_vcp_feature(monitor_id, VCP_BRIGHTNESS, 0);
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            // flash to max
            let _ = controller.set_vcp_feature(monitor_id, VCP_BRIGHTNESS, 100);
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            // dim again
            let _ = controller.set_vcp_feature(monitor_id, VCP_BRIGHTNESS, 0);
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            // restore
            let _ = controller.set_vcp_feature(monitor_id, VCP_BRIGHTNESS, brightness);
            println!("restored {monitor_id} brightness to {brightness}");
        }
        Err(_) => {
            // brightness not readable, fall back to input switching
            println!("brightness control unavailable, trying input switch...");
            identify_via_input(controller, monitor_id).await?;
        }
    }

    Ok(())
}

// fallback: briefly switch the input source away and back
async fn identify_via_input(controller: &dyn DdcController, monitor_id: &str) -> Result<()> {
    let current = controller
        .get_input_source(monitor_id)
        .map_err(|e| anyhow::anyhow!("failed to read current input for {monitor_id}: {e}"))?;

    let current_label = InputSource::from_vcp_value(current);
    let dummy = if current == 0x02 { 0x01 } else { 0x02 };

    println!("identifying {monitor_id} (current: {current_label}), screen will blank briefly...");

    controller
        .set_input_source(monitor_id, dummy)
        .map_err(|e| anyhow::anyhow!("failed to switch monitor: {e}"))?;

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    controller
        .set_input_source(monitor_id, current)
        .map_err(|e| anyhow::anyhow!("failed to restore monitor input: {e}"))?;

    println!("restored {monitor_id} to {current_label}");
    Ok(())
}

async fn identify_interactive(controller: &dyn DdcController) -> Result<()> {
    let monitors = controller
        .enumerate_monitors()
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if monitors.is_empty() {
        println!("no monitors with DDC/CI support detected");
        return Ok(());
    }

    println!("monitors:\n");
    for (i, m) in monitors.iter().enumerate() {
        let input_str = m
            .current_input_vcp
            .map(|vcp| {
                let label = InputSource::from_vcp_value(vcp);
                format!("{label} (0x{vcp:02x})")
            })
            .unwrap_or_else(|| "unknown".into());
        println!("  [{}] {} ({})", i + 1, m.name, input_str);
        println!("      id: {}", m.id);
    }

    loop {
        println!();
        print!("enter a number to flash that monitor (or q to quit): ");
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.eq_ignore_ascii_case("q") || input.is_empty() {
            break;
        }

        match input.parse::<usize>() {
            Ok(n) if n >= 1 && n <= monitors.len() => {
                let mon = &monitors[n - 1];
                identify_one(controller, &mon.id).await?;
            }
            _ => {
                println!("invalid choice, enter 1-{} or q", monitors.len());
            }
        }
    }

    Ok(())
}

fn create_controller() -> Box<dyn DdcController> {
    softkvm_core::ddc::create_controller()
}
