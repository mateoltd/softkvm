use softkvm_core::keymap::{
    build_translation_rules, find_translation, KeyCombo, OsType, ShortcutTranslation,
    TranslationRule,
};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// events emitted by the key interceptor.
/// variants are constructed by platform-specific hooks (windows/macos cfg-gated modules)
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum KeyEvent {
    /// a combo was intercepted and translated
    Translated {
        intent: String,
        from: KeyCombo,
        to: KeyCombo,
    },
    /// interceptor started
    Started,
    /// interceptor stopped
    Stopped,
}

/// shared state for the interceptor, tracks active modifiers and the current
/// OS pair context so it knows which rules to apply
pub struct InterceptorState {
    /// which OS is running locally
    pub local_os: OsType,
    /// which OS we're currently controlling remotely (changes on screen transitions)
    pub remote_os: Option<OsType>,
    /// active translation rules for the current OS pair
    pub rules: Vec<TranslationRule>,
    /// all configured translations (used to rebuild rules on OS pair change)
    pub translations: Vec<ShortcutTranslation>,
    /// whether interception is enabled
    pub enabled: bool,
}

impl InterceptorState {
    pub fn new(local_os: OsType, translations: Vec<ShortcutTranslation>, enabled: bool) -> Self {
        Self {
            local_os,
            remote_os: None,
            rules: Vec::new(),
            translations,
            enabled,
        }
    }

    /// update the remote OS (called on screen transitions) and rebuild rules
    pub fn set_remote_os(&mut self, remote_os: Option<OsType>) {
        self.remote_os = remote_os;
        if let Some(ros) = remote_os {
            self.rules = build_translation_rules(self.local_os, ros, &self.translations);
        } else {
            self.rules.clear();
        }
    }

    /// check if a pressed combo should be translated.
    /// called by platform hook callbacks (cfg-gated)
    #[allow(dead_code)]
    pub fn translate(&self, combo: &KeyCombo) -> Option<&TranslationRule> {
        if !self.enabled || self.remote_os.is_none() {
            return None;
        }
        find_translation(combo, &self.rules)
    }
}

/// handle to control the interceptor from the main event loop
pub struct KeyInterceptor {
    state: Arc<Mutex<InterceptorState>>,
    event_rx: mpsc::Receiver<KeyEvent>,
    _event_tx: mpsc::Sender<KeyEvent>,
}

impl KeyInterceptor {
    /// create a new key interceptor for the given local OS and translations
    pub fn new(local_os: OsType, translations: Vec<ShortcutTranslation>, enabled: bool) -> Self {
        let state = Arc::new(Mutex::new(InterceptorState::new(
            local_os,
            translations,
            enabled,
        )));
        let (event_tx, event_rx) = mpsc::channel(64);
        Self {
            state,
            event_rx,
            _event_tx: event_tx,
        }
    }

    /// start the OS-level keyboard hook in a background thread
    pub fn start(&self) -> anyhow::Result<()> {
        let state = Arc::clone(&self.state);
        let tx = self._event_tx.clone();

        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                if let Err(e) = win_hook::run(state, tx) {
                    tracing::error!(error = %e, "windows keyboard hook failed");
                }
            });
        }

        #[cfg(target_os = "macos")]
        {
            std::thread::spawn(move || {
                if let Err(e) = mac_hook::run(state, tx) {
                    tracing::error!(error = %e, "macos keyboard hook failed");
                }
            });
        }

        #[cfg(target_os = "linux")]
        {
            let _ = (state, tx);
            anyhow::bail!(
                "key interceptor is not supported on Linux. \
                 keyboard remapping requires Windows or macOS"
            );
        }

        #[cfg(not(target_os = "linux"))]
        {
            tracing::info!("key interceptor started");
            Ok(())
        }
    }

    /// update the remote OS context (call on screen transitions)
    pub fn set_remote_os(&self, remote_os: Option<OsType>) {
        if let Ok(mut state) = self.state.lock() {
            state.set_remote_os(remote_os);
            if let Some(os) = remote_os {
                tracing::debug!(remote_os = %os, rules = state.rules.len(), "key interceptor rules updated");
            } else {
                tracing::debug!("key interceptor: controlling local machine, rules cleared");
            }
        }
    }

    /// toggle interception on/off
    pub fn set_enabled(&self, enabled: bool) {
        if let Ok(mut state) = self.state.lock() {
            state.enabled = enabled;
        }
    }

    /// receive the next key event (translated combos, start/stop notifications)
    pub async fn recv(&mut self) -> Option<KeyEvent> {
        self.event_rx.recv().await
    }

    /// get the current state for inspection
    #[allow(dead_code)]
    pub fn state(&self) -> Arc<Mutex<InterceptorState>> {
        Arc::clone(&self.state)
    }
}

// --- Windows: low-level keyboard hook via SetWindowsHookEx(WH_KEYBOARD_LL) ---

#[cfg(target_os = "windows")]
mod win_hook {
    use super::*;
    use softkvm_core::keymap::{combo_from_vk, key_name_to_vk, modifier_to_vk};
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
        UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
    };

    thread_local! {
        static HOOK_STATE: std::cell::RefCell<Option<(
            Arc<Mutex<InterceptorState>>,
            mpsc::Sender<KeyEvent>,
        )>> = const { std::cell::RefCell::new(None) };
        static SUPPRESS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
            let is_keydown = wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize;

            if is_keydown {
                let modifier_vks: &[u32] = &[
                    VK_LCONTROL as u32,
                    VK_RCONTROL as u32,
                    VK_LMENU as u32,
                    VK_RMENU as u32,
                    VK_LSHIFT as u32,
                    VK_RSHIFT as u32,
                    VK_LWIN as u32,
                    VK_RWIN as u32,
                ];

                let modifier_states: Vec<(u32, bool)> = modifier_vks
                    .iter()
                    .map(|&vk| (vk, GetAsyncKeyState(vk as i32) < 0))
                    .collect();

                if let Some(combo) = combo_from_vk(kb.vkCode, &modifier_states) {
                    HOOK_STATE.with(|h| {
                        if let Some((ref state, ref tx)) = *h.borrow() {
                            if let Ok(state) = state.lock() {
                                if let Some(rule) = state.translate(&combo) {
                                    SUPPRESS.with(|s| s.set(true));
                                    let _ = tx.blocking_send(KeyEvent::Translated {
                                        intent: rule.intent.clone(),
                                        from: rule.from.clone(),
                                        to: rule.to.clone(),
                                    });
                                    synthesize_combo(&rule.to);
                                }
                            }
                        }
                    });

                    if SUPPRESS.with(|s| {
                        let v = s.get();
                        s.set(false);
                        v
                    }) {
                        return 1isize;
                    }
                }
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
    }

    fn make_input(vk: u16, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn synthesize_combo(combo: &KeyCombo) {
        let main_vk = match key_name_to_vk(&combo.key) {
            Some(vk) => vk,
            None => return,
        };

        let mut inputs: Vec<INPUT> = Vec::new();

        for m in &combo.modifiers {
            inputs.push(make_input(modifier_to_vk(m) as u16, 0));
        }
        inputs.push(make_input(main_vk as u16, 0));
        inputs.push(make_input(main_vk as u16, KEYEVENTF_KEYUP));
        for m in combo.modifiers.iter().rev() {
            inputs.push(make_input(modifier_to_vk(m) as u16, KEYEVENTF_KEYUP));
        }

        unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            );
        }
    }

    pub fn run(
        state: Arc<Mutex<InterceptorState>>,
        tx: mpsc::Sender<KeyEvent>,
    ) -> anyhow::Result<()> {
        HOOK_STATE.with(|h| {
            *h.borrow_mut() = Some((state, tx.clone()));
        });

        let _ = tx.blocking_send(KeyEvent::Started);

        unsafe {
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), std::ptr::null_mut(), 0);
            if hook.is_null() {
                anyhow::bail!("SetWindowsHookExW failed");
            }

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            UnhookWindowsHookEx(hook);
        }

        Ok(())
    }
}

// --- macOS: event tap via CGEventTap for system-wide key interception ---
// requires Accessibility permissions (System Settings > Privacy & Security > Accessibility)

#[cfg(target_os = "macos")]
mod mac_hook {
    use super::*;
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use core_graphics::event::{
        CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions,
        CGEventTapPlacement, CGEventType, CallbackResult, EventField,
    };
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use softkvm_core::keymap::{combo_from_cg, key_name_to_cg_keycode, modifier_to_cg_flag};

    fn synthesize_combo(combo: &KeyCombo) -> CallbackResult {
        let Some(keycode) = key_name_to_cg_keycode(&combo.key) else {
            return CallbackResult::Keep;
        };
        let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
            return CallbackResult::Keep;
        };
        let Ok(event) = CGEvent::new_keyboard_event(source, keycode, true) else {
            return CallbackResult::Keep;
        };

        let mut flags_bits: u64 = 0;
        for m in &combo.modifiers {
            flags_bits |= modifier_to_cg_flag(m);
        }
        event.set_flags(CGEventFlags::from_bits_truncate(flags_bits));

        CallbackResult::Replace(event)
    }

    pub fn run(
        state: Arc<Mutex<InterceptorState>>,
        tx: mpsc::Sender<KeyEvent>,
    ) -> anyhow::Result<()> {
        let state_clone = Arc::clone(&state);
        let tx_clone = tx.clone();

        let tap = CGEventTap::new(
            CGEventTapLocation::Session,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            vec![CGEventType::KeyDown],
            move |_proxy, _type, event| {
                let keycode =
                    event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
                let flags = event.get_flags();

                if let Ok(state) = state_clone.lock() {
                    if let Some(combo) = combo_from_cg(keycode, flags.bits()) {
                        if let Some(rule) = state.translate(&combo) {
                            let _ = tx_clone.blocking_send(KeyEvent::Translated {
                                intent: rule.intent.clone(),
                                from: rule.from.clone(),
                                to: rule.to.clone(),
                            });
                            return synthesize_combo(&rule.to);
                        }
                    }
                }

                CallbackResult::Keep
            },
        )
        .map_err(|_| {
            anyhow::anyhow!(
                "failed to create CGEventTap. \
                 grant Accessibility permissions in System Settings > Privacy & Security > Accessibility"
            )
        })?;

        let _ = tx.blocking_send(KeyEvent::Started);

        let source = tap.mach_port().create_runloop_source(0)
            .map_err(|_| anyhow::anyhow!("failed to create run loop source from event tap"))?;
        let run_loop = CFRunLoop::get_current();
        run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
        tap.enable();
        CFRunLoop::run_current();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use softkvm_core::keymap::{default_translations, Modifier};

    #[test]
    fn test_interceptor_state_no_remote() {
        let state = InterceptorState::new(OsType::MacOS, default_translations(), true);
        let combo = KeyCombo {
            modifiers: vec![Modifier::Meta],
            key: "tab".into(),
        };
        // no remote OS set, should not translate
        assert!(state.translate(&combo).is_none());
    }

    #[test]
    fn test_interceptor_state_mac_to_windows() {
        let mut state = InterceptorState::new(OsType::MacOS, default_translations(), true);
        state.set_remote_os(Some(OsType::Windows));

        let combo = KeyCombo {
            modifiers: vec![Modifier::Meta],
            key: "tab".into(),
        };
        let result = state.translate(&combo);
        assert!(result.is_some());
        assert_eq!(result.unwrap().intent, "app_switcher");
        assert_eq!(result.unwrap().to.modifiers, vec![Modifier::Alt]);
        assert_eq!(result.unwrap().to.key, "tab");
    }

    #[test]
    fn test_interceptor_state_disabled() {
        let mut state = InterceptorState::new(OsType::MacOS, default_translations(), false);
        state.set_remote_os(Some(OsType::Windows));

        let combo = KeyCombo {
            modifiers: vec![Modifier::Meta],
            key: "tab".into(),
        };
        // disabled, should not translate
        assert!(state.translate(&combo).is_none());
    }

    #[test]
    fn test_interceptor_state_switch_remote() {
        let mut state = InterceptorState::new(OsType::MacOS, default_translations(), true);

        // controlling windows
        state.set_remote_os(Some(OsType::Windows));
        assert!(!state.rules.is_empty());

        // back to local (no remote)
        state.set_remote_os(None);
        assert!(state.rules.is_empty());

        // same OS pair, no translations needed
        state.set_remote_os(Some(OsType::MacOS));
        assert!(state.rules.is_empty());
    }

    #[test]
    fn test_interceptor_quit_app_translation() {
        let mut state = InterceptorState::new(OsType::MacOS, default_translations(), true);
        state.set_remote_os(Some(OsType::Windows));

        // meta+q -> alt+F4
        let combo = KeyCombo {
            modifiers: vec![Modifier::Meta],
            key: "q".into(),
        };
        let result = state.translate(&combo).unwrap();
        assert_eq!(result.intent, "quit_app");
        assert_eq!(result.to.modifiers, vec![Modifier::Alt]);
        assert_eq!(result.to.key, "F4");
    }

    #[test]
    fn test_key_interceptor_new() {
        let interceptor = KeyInterceptor::new(OsType::MacOS, default_translations(), true);

        // set remote OS
        interceptor.set_remote_os(Some(OsType::Windows));

        // verify state updated
        let state = interceptor.state();
        let guard = state.lock().unwrap();
        assert_eq!(guard.remote_os, Some(OsType::Windows));
        assert!(!guard.rules.is_empty());

        // disable
        drop(guard);
        interceptor.set_enabled(false);
        let guard = state.lock().unwrap();
        assert!(!guard.enabled);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_key_interceptor_fails_on_linux() {
        let interceptor = KeyInterceptor::new(OsType::Linux, default_translations(), true);
        let result = interceptor.start();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not supported on Linux"));
    }
}
