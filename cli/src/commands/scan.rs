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

        println!("found {} monitor(s) with DDC/CI support:\n", monitors.len());

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

// real-ddc takes priority when both features are enabled (default includes stub-ddc)
#[cfg(feature = "real-ddc")]
fn create_controller() -> Box<dyn DdcController> {
    Box::new(softkvm_core::ddc::real::RealDdcController::new())
}

#[cfg(all(not(feature = "real-ddc"), feature = "stub-ddc"))]
fn create_controller() -> Box<dyn DdcController> {
    Box::new(softkvm_core::ddc::stub::StubDdcController::new())
}

#[cfg(all(not(feature = "real-ddc"), not(feature = "stub-ddc")))]
fn create_controller() -> Box<dyn DdcController> {
    compile_error!("enable either the real-ddc or stub-ddc feature");
}
