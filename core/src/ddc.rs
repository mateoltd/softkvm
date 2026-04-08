// DDC/CI abstraction layer.
// In production this wraps ddc-hi; we keep it behind a trait for testability
// and because ddc-hi doesn't compile on WSL/Linux without i2c-dev.
// The real implementation is behind feature flags added when we integrate ddc-hi.

use crate::error::Result;
use crate::protocol::MonitorInfo;

/// VCP code for input source selection.
pub const VCP_INPUT_SOURCE: u8 = 0x60;

/// VCP code for brightness (luminance).
pub const VCP_BRIGHTNESS: u8 = 0x10;

/// Trait abstracting DDC/CI monitor operations.
/// Implementations exist per-platform (Windows, macOS, Linux).
pub trait DdcController: Send + Sync {
    /// Enumerate all monitors with DDC/CI support.
    fn enumerate_monitors(&self) -> Result<Vec<MonitorInfo>>;

    /// Get the current input source VCP value for a monitor.
    fn get_input_source(&self, monitor_id: &str) -> Result<u16>;

    /// Set the input source VCP value for a monitor.
    fn set_input_source(&self, monitor_id: &str, value: u16) -> Result<()>;

    /// Read a raw VCP feature value.
    fn get_vcp_feature(&self, monitor_id: &str, code: u8) -> Result<u16>;

    /// Write a raw VCP feature value.
    fn set_vcp_feature(&self, monitor_id: &str, code: u8, value: u16) -> Result<()>;
}

/// Switch a monitor's input source with retry logic.
pub fn switch_with_retry(
    controller: &dyn DdcController,
    monitor_id: &str,
    target_vcp: u16,
    skip_if_current: bool,
    retry_count: u32,
    retry_delay_ms: u64,
) -> Result<bool> {
    // Check current input if skip_if_current is enabled
    if skip_if_current {
        match controller.get_input_source(monitor_id) {
            Ok(current) if current == target_vcp => {
                tracing::debug!(
                    monitor = monitor_id,
                    input = target_vcp,
                    "monitor already on target input, skipping"
                );
                return Ok(false); // No switch needed
            }
            Ok(_) => {} // Different input, proceed with switch
            Err(e) => {
                tracing::warn!(
                    monitor = monitor_id,
                    error = %e,
                    "failed to read current input, proceeding with switch anyway"
                );
            }
        }
    }

    let mut last_error = None;
    for attempt in 0..retry_count {
        match controller.set_input_source(monitor_id, target_vcp) {
            Ok(()) => {
                tracing::info!(
                    monitor = monitor_id,
                    input = format!("0x{:02x}", target_vcp),
                    attempt = attempt + 1,
                    "monitor input switched"
                );
                return Ok(true);
            }
            Err(e) => {
                tracing::warn!(
                    monitor = monitor_id,
                    attempt = attempt + 1,
                    max_attempts = retry_count,
                    error = %e,
                    "DDC switch failed, retrying"
                );
                last_error = Some(e);
                if attempt + 1 < retry_count {
                    std::thread::sleep(std::time::Duration::from_millis(retry_delay_ms));
                }
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| crate::error::SoftKvmError::Ddc("no retry attempts made".into())))
}

/// real DDC controller wrapping the ddc-hi crate
#[cfg(feature = "real-ddc")]
pub mod real {
    use super::*;
    use ddc_hi::Ddc;

    #[derive(Default)]
    pub struct RealDdcController;

    impl RealDdcController {
        pub fn new() -> Self {
            Self
        }

        /// build a stable monitor ID from ddc-hi display info
        fn monitor_id(display: &ddc_hi::Display) -> String {
            let mfg = display.info.manufacturer_id.as_deref().unwrap_or("UNK");
            let model = display.info.model_name.as_deref().unwrap_or("UNK");
            let serial = display.info.serial_number.as_deref().unwrap_or("UNK");
            format!("{mfg}:{model}:{serial}")
        }

        /// count how many fields have real (non-placeholder) values
        fn metadata_score(mon: &MonitorInfo) -> u8 {
            let mut score = 0u8;
            if mon.manufacturer != "Unknown" && mon.manufacturer != "UNK" {
                score += 1;
            }
            if mon.model != "Unknown" && mon.model != "UNK" {
                score += 1;
            }
            if mon.serial != "Unknown" && mon.serial != "UNK" {
                score += 1;
            }
            if mon.name != "Unknown" {
                score += 1;
            }
            score
        }

        /// remove duplicate monitors that represent the same physical display
        /// enumerated through different backends
        fn deduplicate(monitors: &mut Vec<MonitorInfo>) {
            let mut keep = vec![true; monitors.len()];
            for i in 0..monitors.len() {
                if !keep[i] {
                    continue;
                }
                for j in (i + 1)..monitors.len() {
                    if !keep[j] {
                        continue;
                    }
                    // same current input on two entries means same physical monitor
                    let same_input = monitors[i].current_input_vcp.is_some()
                        && monitors[i].current_input_vcp == monitors[j].current_input_vcp;
                    if !same_input {
                        continue;
                    }
                    // keep the one with more metadata
                    let score_i = Self::metadata_score(&monitors[i]);
                    let score_j = Self::metadata_score(&monitors[j]);
                    if score_i >= score_j {
                        keep[j] = false;
                    } else {
                        keep[i] = false;
                        break;
                    }
                }
            }
            let mut idx = 0;
            monitors.retain(|_| {
                let k = keep[idx];
                idx += 1;
                k
            });
        }

        /// find a display by our composite ID
        fn find_display(monitor_id: &str) -> Result<ddc_hi::Display> {
            for display in ddc_hi::Display::enumerate() {
                if Self::monitor_id(&display) == monitor_id {
                    return Ok(display);
                }
            }
            Err(crate::error::SoftKvmError::MonitorNotFound(
                monitor_id.into(),
            ))
        }
    }

    impl DdcController for RealDdcController {
        fn enumerate_monitors(&self) -> Result<Vec<MonitorInfo>> {
            let mut monitors = Vec::new();
            for mut display in ddc_hi::Display::enumerate() {
                let id = Self::monitor_id(&display);
                let name = display
                    .info
                    .model_name
                    .clone()
                    .unwrap_or_else(|| "Unknown".into());
                let manufacturer = display
                    .info
                    .manufacturer_id
                    .clone()
                    .unwrap_or_else(|| "Unknown".into());
                let model = display
                    .info
                    .model_name
                    .clone()
                    .unwrap_or_else(|| "Unknown".into());
                let serial = display
                    .info
                    .serial_number
                    .clone()
                    .unwrap_or_else(|| "Unknown".into());

                // try to read current input source
                let current_input_vcp = match display.handle.get_vcp_feature(VCP_INPUT_SOURCE) {
                    Ok(val) => Some(val.value() as u16),
                    Err(e) => {
                        tracing::debug!(
                            monitor = %id,
                            error = %e,
                            "failed to read VCP 0x60"
                        );
                        None
                    }
                };

                let ddc_supported = current_input_vcp.is_some();

                monitors.push(MonitorInfo {
                    id,
                    name,
                    manufacturer,
                    model,
                    serial,
                    current_input_vcp,
                    ddc_supported,
                });
            }

            // ddc-hi can enumerate the same physical monitor through multiple
            // backends (e.g. Monitor Configuration API and I2C on Windows).
            // deduplicate by current_input_vcp, keeping the entry with the
            // most metadata (non-UNK fields)
            Self::deduplicate(&mut monitors);

            Ok(monitors)
        }

        fn get_input_source(&self, monitor_id: &str) -> Result<u16> {
            let mut display = Self::find_display(monitor_id)?;
            let val = display
                .handle
                .get_vcp_feature(VCP_INPUT_SOURCE)
                .map_err(|e| {
                    crate::error::SoftKvmError::Ddc(format!(
                        "failed to read input source for {monitor_id}: {e}"
                    ))
                })?;
            Ok(val.value() as u16)
        }

        fn set_input_source(&self, monitor_id: &str, value: u16) -> Result<()> {
            let mut display = Self::find_display(monitor_id)?;
            display
                .handle
                .set_vcp_feature(VCP_INPUT_SOURCE, value)
                .map_err(|e| {
                    crate::error::SoftKvmError::Ddc(format!(
                        "failed to set input source for {monitor_id}: {e}"
                    ))
                })
        }

        fn get_vcp_feature(&self, monitor_id: &str, code: u8) -> Result<u16> {
            let mut display = Self::find_display(monitor_id)?;
            let val = display.handle.get_vcp_feature(code).map_err(|e| {
                crate::error::SoftKvmError::Ddc(format!(
                    "failed to read VCP 0x{code:02x} for {monitor_id}: {e}"
                ))
            })?;
            Ok(val.value() as u16)
        }

        fn set_vcp_feature(&self, monitor_id: &str, code: u8, value: u16) -> Result<()> {
            let mut display = Self::find_display(monitor_id)?;
            display.handle.set_vcp_feature(code, value).map_err(|e| {
                crate::error::SoftKvmError::Ddc(format!(
                    "failed to set VCP 0x{code:02x} for {monitor_id}: {e}"
                ))
            })
        }
    }
}

/// stub DDC controller for testing and platforms where ddc-hi isn't available
#[cfg(any(test, feature = "stub-ddc"))]
pub mod stub {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    pub struct StubDdcController {
        pub monitors: Mutex<HashMap<String, u16>>,
        pub fail_count: Mutex<u32>,
    }

    impl Default for StubDdcController {
        fn default() -> Self {
            Self::new()
        }
    }

    impl StubDdcController {
        pub fn new() -> Self {
            let mut monitors = HashMap::new();
            monitors.insert("TEST:MON:001".into(), 0x0f); // DP1
            Self {
                monitors: Mutex::new(monitors),
                fail_count: Mutex::new(0),
            }
        }
    }

    impl DdcController for StubDdcController {
        fn enumerate_monitors(&self) -> Result<Vec<MonitorInfo>> {
            let monitors = self.monitors.lock().unwrap();
            Ok(monitors
                .iter()
                .map(|(id, vcp)| MonitorInfo {
                    id: id.clone(),
                    name: "Test Monitor".into(),
                    manufacturer: "TEST".into(),
                    model: "MON".into(),
                    serial: "001".into(),
                    current_input_vcp: Some(*vcp),
                    ddc_supported: true,
                })
                .collect())
        }

        fn get_input_source(&self, monitor_id: &str) -> Result<u16> {
            let monitors = self.monitors.lock().unwrap();
            monitors
                .get(monitor_id)
                .copied()
                .ok_or_else(|| crate::error::SoftKvmError::MonitorNotFound(monitor_id.into()))
        }

        fn set_input_source(&self, monitor_id: &str, value: u16) -> Result<()> {
            let mut fail_count = self.fail_count.lock().unwrap();
            if *fail_count > 0 {
                *fail_count -= 1;
                return Err(crate::error::SoftKvmError::Ddc("simulated failure".into()));
            }

            let mut monitors = self.monitors.lock().unwrap();
            if monitors.contains_key(monitor_id) {
                monitors.insert(monitor_id.to_string(), value);
                Ok(())
            } else {
                Err(crate::error::SoftKvmError::MonitorNotFound(
                    monitor_id.into(),
                ))
            }
        }

        fn get_vcp_feature(&self, monitor_id: &str, _code: u8) -> Result<u16> {
            let monitors = self.monitors.lock().unwrap();
            monitors
                .get(monitor_id)
                .copied()
                .ok_or_else(|| crate::error::SoftKvmError::MonitorNotFound(monitor_id.into()))
        }

        fn set_vcp_feature(&self, monitor_id: &str, _code: u8, _value: u16) -> Result<()> {
            let monitors = self.monitors.lock().unwrap();
            if monitors.contains_key(monitor_id) {
                Ok(())
            } else {
                Err(crate::error::SoftKvmError::MonitorNotFound(
                    monitor_id.into(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stub::StubDdcController;

    #[test]
    fn test_switch_success() {
        let controller = StubDdcController::new();
        let switched = switch_with_retry(&controller, "TEST:MON:001", 0x11, false, 3, 10).unwrap();
        assert!(switched);
        assert_eq!(controller.get_input_source("TEST:MON:001").unwrap(), 0x11);
    }

    #[test]
    fn test_switch_skip_if_current() {
        let controller = StubDdcController::new();
        // Monitor is already on 0x0f
        let switched = switch_with_retry(&controller, "TEST:MON:001", 0x0f, true, 3, 10).unwrap();
        assert!(!switched); // No switch needed
    }

    #[test]
    fn test_switch_retry_on_failure() {
        let controller = StubDdcController::new();
        *controller.fail_count.lock().unwrap() = 2; // Fail first 2 attempts
        let switched = switch_with_retry(&controller, "TEST:MON:001", 0x11, false, 3, 10).unwrap();
        assert!(switched); // Succeeds on 3rd attempt
    }

    #[test]
    fn test_switch_all_retries_fail() {
        let controller = StubDdcController::new();
        *controller.fail_count.lock().unwrap() = 5; // Fail all attempts
        let result = switch_with_retry(&controller, "TEST:MON:001", 0x11, false, 3, 10);
        assert!(result.is_err());
    }
}
