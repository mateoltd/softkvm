use anyhow::Result;
use softkvm_core::ddc::DdcController;
use softkvm_core::input_source::InputSource;

/// identify a monitor by briefly switching its input away, causing it to
/// show "no signal" for a couple seconds, then switching back
pub async fn run(monitor_id: &str) -> Result<()> {
    let controller = create_controller();

    let current = controller
        .get_input_source(monitor_id)
        .map_err(|e| anyhow::anyhow!("failed to read current input for {monitor_id}: {e}"))?;

    let current_label = InputSource::from_vcp_value(current);

    // pick a dummy input that differs from current to cause "no signal"
    // VGA2 (0x02) is unlikely to be connected on modern systems
    let dummy = if current == 0x02 { 0x01 } else { 0x02 };

    println!(
        "identifying {monitor_id} (current: {current_label}), the screen will blank briefly..."
    );

    // switch away
    controller
        .set_input_source(monitor_id, dummy)
        .map_err(|e| anyhow::anyhow!("failed to switch monitor: {e}"))?;

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // switch back
    controller
        .set_input_source(monitor_id, current)
        .map_err(|e| anyhow::anyhow!("failed to restore monitor input: {e}"))?;

    println!("restored {monitor_id} to {current_label}");

    Ok(())
}

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
