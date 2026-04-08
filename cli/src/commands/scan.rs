use anyhow::Result;
use softkvm_core::ddc::DdcController;
use softkvm_core::input_source::InputSource;

pub async fn run(json: bool) -> Result<()> {
    let controller = create_controller();

    let monitors = controller
        .enumerate_monitors()
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if json {
        let json_monitors: Vec<serde_json::Value> = monitors
            .iter()
            .map(|m| {
                let input_label = m
                    .current_input_vcp
                    .map(|vcp| InputSource::from_vcp_value(vcp).to_string());

                serde_json::json!({
                    "id": m.id,
                    "name": m.name,
                    "manufacturer": m.manufacturer,
                    "model": m.model,
                    "serial": m.serial,
                    "ddc_supported": m.ddc_supported,
                    "current_input_vcp": m.current_input_vcp.map(|v| format!("0x{v:02x}")),
                    "current_input": input_label,
                })
            })
            .collect();

        println!("{}", serde_json::to_string_pretty(&json_monitors)?);
    } else {
        if monitors.is_empty() {
            println!("no monitors with DDC/CI support detected");
            println!();
            println!("troubleshooting:");
            println!("  - ensure your monitor supports DDC/CI (check OSD settings)");
            println!("  - try a different cable (DisplayPort and HDMI typically work)");
            println!("  - on Linux, ensure i2c-dev is loaded: sudo modprobe i2c-dev");
            return Ok(());
        }

        let ddc_count = monitors.iter().filter(|m| m.ddc_supported).count();
        if ddc_count == monitors.len() {
            println!("found {} monitor(s) with DDC/CI support:\n", monitors.len());
        } else {
            println!(
                "found {} monitor(s), {} with DDC/CI support:\n",
                monitors.len(),
                ddc_count
            );
        }

        let unavailable_count = monitors.iter().filter(|m| !m.ddc_supported).count();
        if unavailable_count > 0 && ddc_count > 0 {
            println!(
                "note: monitors showing another machine's input may not respond\n\
                 to DDC/CI reads. switch them to this machine's input and re-scan\n\
                 if a monitor appears as unavailable.\n"
            );
        }

        for (i, m) in monitors.iter().enumerate() {
            let input_str = match m.current_input_vcp {
                Some(vcp) => {
                    let label = InputSource::from_vcp_value(vcp);
                    format!("{label} (0x{vcp:02x})")
                }
                None => "unknown".to_string(),
            };

            let ddc_status = if m.ddc_supported {
                "healthy"
            } else {
                "unavailable"
            };

            println!("  [{}] {}", i + 1, m.name);
            println!("    id:            {}", m.id);
            println!("    manufacturer:  {}", m.manufacturer);
            println!("    model:         {}", m.model);
            println!("    serial:        {}", m.serial);
            println!("    current input: {}", input_str);
            println!("    DDC/CI:        {}", ddc_status);
            println!();
        }
    }

    Ok(())
}

fn create_controller() -> Box<dyn DdcController> {
    softkvm_core::ddc::create_controller()
}
