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

/// m1ddc-based DDC controller for Apple Silicon Macs where ddc-hi
/// cannot read VCP features through USB-C/DP adapters.
/// shells out to the m1ddc CLI (brew install m1ddc).
#[cfg(target_os = "macos")]
pub mod m1ddc_backend {
    use super::*;
    use std::collections::HashMap;
    use std::process::Command;
    use std::sync::Mutex;

    pub struct M1DdcController {
        uuid_map: Mutex<HashMap<String, String>>,
    }

    impl Default for M1DdcController {
        fn default() -> Self {
            Self::new()
        }
    }

    impl M1DdcController {
        pub fn new() -> Self {
            Self {
                uuid_map: Mutex::new(HashMap::new()),
            }
        }

        /// check if m1ddc is installed and usable
        pub fn is_available() -> bool {
            Command::new("m1ddc")
                .arg("display")
                .arg("list")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }

        fn run(args: &[&str]) -> std::result::Result<String, String> {
            let output = Command::new("m1ddc")
                .args(args)
                .output()
                .map_err(|e| format!("failed to run m1ddc: {e}"))?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                Err(if stderr.is_empty() {
                    format!("m1ddc exited with {}", output.status)
                } else {
                    stderr
                })
            }
        }

        /// parse "m1ddc display list" output
        /// format: [N] DisplayName (UUID)
        fn parse_display_list(output: &str) -> Vec<(String, String)> {
            let mut displays = Vec::new();
            for line in output.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix('[') {
                    if let Some(bracket_end) = rest.find(']') {
                        let after = rest[bracket_end + 1..].trim();
                        if let (Some(paren_start), Some(paren_end)) =
                            (after.rfind('('), after.rfind(')'))
                        {
                            let name = after[..paren_start].trim().to_string();
                            let uuid = after[paren_start + 1..paren_end].to_string();
                            displays.push((name, uuid));
                        }
                    }
                }
            }
            displays
        }

        fn vcp_to_feature(code: u8) -> Result<&'static str> {
            match code {
                0x10 => Ok("luminance"),
                0x12 => Ok("contrast"),
                0x60 => Ok("input-alt"),
                _ => Err(crate::error::SoftKvmError::Ddc(format!(
                    "m1ddc does not support VCP 0x{code:02x}"
                ))),
            }
        }

        fn uuid_for(&self, monitor_id: &str) -> Result<String> {
            self.uuid_map
                .lock()
                .unwrap()
                .get(monitor_id)
                .cloned()
                .ok_or_else(|| crate::error::SoftKvmError::MonitorNotFound(monitor_id.into()))
        }
    }

    impl DdcController for M1DdcController {
        fn enumerate_monitors(&self) -> Result<Vec<MonitorInfo>> {
            let output =
                Self::run(&["display", "list"]).map_err(|e| crate::error::SoftKvmError::Ddc(e))?;

            let displays = Self::parse_display_list(&output);
            let mut monitors = Vec::new();
            let mut uuid_map = self.uuid_map.lock().unwrap();
            uuid_map.clear();

            for (name, uuid) in displays {
                let current_input = Self::run(&["display", &uuid, "get", "input-alt"])
                    .ok()
                    .and_then(|v| v.parse::<u16>().ok());

                let id = format!("m1ddc:{uuid}");
                uuid_map.insert(id.clone(), uuid);

                let display_name = if name == "(null)" || name.is_empty() {
                    "Unknown".to_string()
                } else {
                    name
                };

                monitors.push(MonitorInfo {
                    id: id.clone(),
                    name: display_name.clone(),
                    manufacturer: "Unknown".into(),
                    model: display_name,
                    serial: "Unknown".into(),
                    current_input_vcp: current_input,
                    ddc_supported: current_input.is_some(),
                });
            }

            Ok(monitors)
        }

        fn get_input_source(&self, monitor_id: &str) -> Result<u16> {
            let uuid = self.uuid_for(monitor_id)?;
            let output = Self::run(&["display", &uuid, "get", "input-alt"])
                .map_err(|e| crate::error::SoftKvmError::Ddc(format!("m1ddc get input: {e}")))?;
            output.parse::<u16>().map_err(|e| {
                crate::error::SoftKvmError::Ddc(format!("m1ddc parse input value '{output}': {e}"))
            })
        }

        fn set_input_source(&self, monitor_id: &str, value: u16) -> Result<()> {
            let uuid = self.uuid_for(monitor_id)?;
            let val = value.to_string();
            Self::run(&["display", &uuid, "set", "input-alt", &val])
                .map_err(|e| crate::error::SoftKvmError::Ddc(format!("m1ddc set input: {e}")))?;
            Ok(())
        }

        fn get_vcp_feature(&self, monitor_id: &str, code: u8) -> Result<u16> {
            let uuid = self.uuid_for(monitor_id)?;
            let feature = Self::vcp_to_feature(code)?;
            let output = Self::run(&["display", &uuid, "get", feature]).map_err(|e| {
                crate::error::SoftKvmError::Ddc(format!("m1ddc get {feature}: {e}"))
            })?;
            output.parse::<u16>().map_err(|e| {
                crate::error::SoftKvmError::Ddc(format!(
                    "m1ddc parse {feature} value '{output}': {e}"
                ))
            })
        }

        fn set_vcp_feature(&self, monitor_id: &str, code: u8, value: u16) -> Result<()> {
            let uuid = self.uuid_for(monitor_id)?;
            let feature = Self::vcp_to_feature(code)?;
            let val = value.to_string();
            Self::run(&["display", &uuid, "set", feature, &val]).map_err(|e| {
                crate::error::SoftKvmError::Ddc(format!("m1ddc set {feature}: {e}"))
            })?;
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_parse_display_list() {
            let output = r#"[1] LG ULTRAGEAR+ (F0DB7A7E-6804-4F3B-8687-200C7CBCE0C8)
[2] LC27G5xT (48D766BF-A3F7-4D70-9574-6347F69C9DE1)
[3] (null) (BB7A124B-B761-4DE2-B597-7E1A432EAAA2)"#;

            let displays = M1DdcController::parse_display_list(output);
            assert_eq!(displays.len(), 3);
            assert_eq!(displays[0].0, "LG ULTRAGEAR+");
            assert_eq!(displays[0].1, "F0DB7A7E-6804-4F3B-8687-200C7CBCE0C8");
            assert_eq!(displays[1].0, "LC27G5xT");
            assert_eq!(displays[2].0, "(null)");
        }

        #[test]
        fn test_parse_empty_output() {
            let displays = M1DdcController::parse_display_list("");
            assert!(displays.is_empty());
        }

        #[test]
        fn test_vcp_to_feature() {
            assert_eq!(M1DdcController::vcp_to_feature(0x10).unwrap(), "luminance");
            assert_eq!(M1DdcController::vcp_to_feature(0x12).unwrap(), "contrast");
            assert_eq!(M1DdcController::vcp_to_feature(0x60).unwrap(), "input-alt");
            assert!(M1DdcController::vcp_to_feature(0x99).is_err());
        }
    }
}

/// composite controller that merges ddc-hi and m1ddc results on macOS.
/// monitors that ddc-hi detects with working DDC are used from ddc-hi.
/// monitors where ddc-hi fails are tried with m1ddc as fallback.
#[cfg(all(target_os = "macos", feature = "real-ddc"))]
pub mod composite {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    enum Backend {
        DdcHi,
        M1Ddc,
    }

    pub struct CompositeDdcController {
        ddc_hi: real::RealDdcController,
        m1ddc: Option<m1ddc_backend::M1DdcController>,
        backend_map: Mutex<HashMap<String, Backend>>,
    }

    impl CompositeDdcController {
        pub fn new() -> Self {
            let m1ddc = if m1ddc_backend::M1DdcController::is_available() {
                tracing::info!("m1ddc detected, using as fallback DDC backend");
                Some(m1ddc_backend::M1DdcController::new())
            } else {
                tracing::debug!("m1ddc not found, using ddc-hi only");
                None
            };

            Self {
                ddc_hi: real::RealDdcController::new(),
                m1ddc,
                backend_map: Mutex::new(HashMap::new()),
            }
        }
    }

    impl Default for CompositeDdcController {
        fn default() -> Self {
            Self::new()
        }
    }

    impl DdcController for CompositeDdcController {
        fn enumerate_monitors(&self) -> Result<Vec<MonitorInfo>> {
            let mut backend_map = self.backend_map.lock().unwrap();
            backend_map.clear();

            // get ddc-hi monitors
            let ddc_hi_monitors = self.ddc_hi.enumerate_monitors().unwrap_or_default();
            let mut all_monitors = Vec::new();

            for mon in &ddc_hi_monitors {
                backend_map.insert(mon.id.clone(), Backend::DdcHi);
                all_monitors.push(mon.clone());
            }

            let ddc_hi_working = ddc_hi_monitors.iter().filter(|m| m.ddc_supported).count();

            // try m1ddc for additional monitors or if ddc-hi found monitors without DDC
            if let Some(ref m1ddc) = self.m1ddc {
                let ddc_hi_broken = ddc_hi_monitors.len() - ddc_hi_working;
                if ddc_hi_broken > 0 || ddc_hi_monitors.is_empty() {
                    if let Ok(m1ddc_monitors) = m1ddc.enumerate_monitors() {
                        for mon in m1ddc_monitors {
                            if !mon.ddc_supported {
                                continue;
                            }
                            // skip if ddc-hi already has a working monitor with
                            // the same current input VCP (true duplicate)
                            let is_duplicate = mon.current_input_vcp.is_some()
                                && all_monitors.iter().any(|existing| {
                                    existing.ddc_supported
                                        && existing.current_input_vcp == mon.current_input_vcp
                                });
                            if is_duplicate {
                                continue;
                            }

                            backend_map.insert(mon.id.clone(), Backend::M1Ddc);

                            // try to replace a broken ddc-hi entry:
                            // 1) match by current_input_vcp if both have values
                            // 2) if ddc-hi entry has no VCP at all (!ddc_supported),
                            //    replace it since it's likely the same physical display
                            let replaced = all_monitors.iter().position(|existing| {
                                if existing.ddc_supported {
                                    return false;
                                }
                                // both have VCP values: must match
                                if existing.current_input_vcp.is_some()
                                    && mon.current_input_vcp.is_some()
                                {
                                    return existing.current_input_vcp == mon.current_input_vcp;
                                }
                                // ddc-hi couldn't read VCP at all: assume same display
                                existing.current_input_vcp.is_none()
                            });

                            if let Some(idx) = replaced {
                                backend_map.remove(&all_monitors[idx].id);
                                all_monitors[idx] = mon;
                            } else {
                                all_monitors.push(mon);
                            }
                        }
                    }
                }
            }

            Ok(all_monitors)
        }

        fn get_input_source(&self, monitor_id: &str) -> Result<u16> {
            let map = self.backend_map.lock().unwrap();
            match map.get(monitor_id) {
                Some(Backend::M1Ddc) => self
                    .m1ddc
                    .as_ref()
                    .ok_or_else(|| crate::error::SoftKvmError::Ddc("m1ddc unavailable".into()))?
                    .get_input_source(monitor_id),
                _ => self.ddc_hi.get_input_source(monitor_id),
            }
        }

        fn set_input_source(&self, monitor_id: &str, value: u16) -> Result<()> {
            let map = self.backend_map.lock().unwrap();
            match map.get(monitor_id) {
                Some(Backend::M1Ddc) => self
                    .m1ddc
                    .as_ref()
                    .ok_or_else(|| crate::error::SoftKvmError::Ddc("m1ddc unavailable".into()))?
                    .set_input_source(monitor_id, value),
                _ => self.ddc_hi.set_input_source(monitor_id, value),
            }
        }

        fn get_vcp_feature(&self, monitor_id: &str, code: u8) -> Result<u16> {
            let map = self.backend_map.lock().unwrap();
            match map.get(monitor_id) {
                Some(Backend::M1Ddc) => self
                    .m1ddc
                    .as_ref()
                    .ok_or_else(|| crate::error::SoftKvmError::Ddc("m1ddc unavailable".into()))?
                    .get_vcp_feature(monitor_id, code),
                _ => self.ddc_hi.get_vcp_feature(monitor_id, code),
            }
        }

        fn set_vcp_feature(&self, monitor_id: &str, code: u8, value: u16) -> Result<()> {
            let map = self.backend_map.lock().unwrap();
            match map.get(monitor_id) {
                Some(Backend::M1Ddc) => self
                    .m1ddc
                    .as_ref()
                    .ok_or_else(|| crate::error::SoftKvmError::Ddc("m1ddc unavailable".into()))?
                    .set_vcp_feature(monitor_id, code, value),
                _ => self.ddc_hi.set_vcp_feature(monitor_id, code, value),
            }
        }
    }
}

/// create the best available DDC controller for the current platform.
/// on macOS with real-ddc: composite (ddc-hi + m1ddc fallback).
/// on other platforms with real-ddc: ddc-hi only.
/// with stub-ddc: stub controller.
#[cfg(feature = "real-ddc")]
pub fn create_controller() -> Box<dyn DdcController> {
    #[cfg(target_os = "macos")]
    {
        Box::new(composite::CompositeDdcController::new())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(real::RealDdcController::new())
    }
}

#[cfg(all(not(feature = "real-ddc"), feature = "stub-ddc"))]
pub fn create_controller() -> Box<dyn DdcController> {
    Box::new(stub::StubDdcController::new())
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
