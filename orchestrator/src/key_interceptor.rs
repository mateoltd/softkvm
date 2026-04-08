use softkvm_core::keymap::{
    build_translation_rules, find_translation, KeyCombo, Modifier, OsType, ShortcutTranslation,
    TranslationRule,
};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// events emitted by the key interceptor
#[derive(Debug, Clone)]
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

/// shared state for the interceptor — tracks active modifiers and the current
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

    /// check if a pressed combo should be translated
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
                if let Err(e) = run_windows_hook(state, tx) {
                    tracing::error!(error = %e, "windows keyboard hook failed");
                }
            });
        }

        #[cfg(target_os = "macos")]
        {
            std::thread::spawn(move || {
                if let Err(e) = run_macos_hook(state, tx) {
                    tracing::error!(error = %e, "macos keyboard hook failed");
                }
            });
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            // on linux or other platforms, no keyboard hook — log and continue
            let _ = (state, tx);
            tracing::info!("key interceptor: no OS hook available on this platform (stub mode)");
        }

        tracing::info!("key interceptor initialized");
        Ok(())
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
    pub fn state(&self) -> Arc<Mutex<InterceptorState>> {
        Arc::clone(&self.state)
    }
}

// --- platform-specific hooks ---

/// Windows: low-level keyboard hook via SetWindowsHookEx(WH_KEYBOARD_LL)
/// intercepts key events system-wide, checks against translation rules,
/// suppresses + synthesizes translated combos when matched
#[cfg(target_os = "windows")]
fn run_windows_hook(
    state: Arc<Mutex<InterceptorState>>,
    tx: mpsc::Sender<KeyEvent>,
) -> anyhow::Result<()> {
    use std::ptr;
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    // store state in thread-local for the hook callback
    thread_local! {
        static HOOK_STATE: std::cell::RefCell<Option<(Arc<Mutex<InterceptorState>>, mpsc::Sender<KeyEvent>)>> = std::cell::RefCell::new(None);
    }

    HOOK_STATE.with(|h| {
        *h.borrow_mut() = Some((state, tx));
    });

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
            let is_keydown = wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize;

            if is_keydown {
                HOOK_STATE.with(|h| {
                    if let Some((ref state, ref tx)) = *h.borrow() {
                        if let Ok(state) = state.lock() {
                            // build current combo from pressed key + tracked modifiers
                            let combo = vk_to_combo(kb.vkCode);
                            if let Some(combo) = combo {
                                if let Some(rule) = state.translate(&combo) {
                                    // suppress original and synthesize translation
                                    synthesize_combo_windows(&rule.to);
                                    let _ = tx.blocking_send(KeyEvent::Translated {
                                        intent: rule.intent.clone(),
                                        from: rule.from.clone(),
                                        to: rule.to.clone(),
                                    });
                                    // return 1 to suppress the original key
                                    return;
                                }
                            }
                        }
                    }
                });
            }
        }
        CallNextHookEx(0, code, wparam, lparam)
    }

    unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), 0, 0);
        if hook == 0 {
            anyhow::bail!("SetWindowsHookExW failed");
        }

        // message loop required for low-level hooks
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        UnhookWindowsHookEx(hook);
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn vk_to_combo(_vk: u32) -> Option<KeyCombo> {
    // TODO: map virtual key codes to KeyCombo by checking GetAsyncKeyState
    // for modifier keys (VK_LCONTROL, VK_RCONTROL, VK_LMENU, VK_RMENU,
    // VK_LWIN, VK_RWIN, VK_SHIFT) and combining with the pressed key
    None
}

#[cfg(target_os = "windows")]
fn synthesize_combo_windows(_combo: &KeyCombo) {
    // TODO: use SendInput to synthesize the translated key combo
    // 1. press modifier keys
    // 2. press the main key
    // 3. release the main key
    // 4. release modifier keys
}

/// macOS: event tap via CGEventTap for system-wide key interception
/// requires Accessibility permissions (System Preferences > Privacy > Accessibility)
#[cfg(target_os = "macos")]
fn run_macos_hook(
    state: Arc<Mutex<InterceptorState>>,
    tx: mpsc::Sender<KeyEvent>,
) -> anyhow::Result<()> {
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use core_graphics::event::{
        CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
        EventField,
    };

    let state_clone = Arc::clone(&state);
    let tx_clone = tx.clone();

    let tap = CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsert,
        CGEventTapOptions::Default,
        vec![CGEventType::KeyDown],
        move |_proxy, _type, event| {
            let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
            let flags = event.get_flags();

            if let Ok(state) = state_clone.lock() {
                let combo = cg_to_combo(keycode, flags);
                if let Some(combo) = combo {
                    if let Some(rule) = state.translate(&combo) {
                        let _ = tx_clone.blocking_send(KeyEvent::Translated {
                            intent: rule.intent.clone(),
                            from: rule.from.clone(),
                            to: rule.to.clone(),
                        });
                        // return a new event with the translated combo
                        return synthesize_combo_macos(&rule.to);
                    }
                }
            }

            Some(event)
        },
    )
    .map_err(|_| {
        anyhow::anyhow!("failed to create CGEventTap -- check Accessibility permissions")
    })?;

    let source = tap.mach_port_source();
    let run_loop = CFRunLoop::get_current();
    run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
    tap.enable();
    CFRunLoop::run_current();

    Ok(())
}

#[cfg(target_os = "macos")]
fn cg_to_combo(_keycode: u16, _flags: u64) -> Option<KeyCombo> {
    // TODO: map CGEvent keycode + flags to KeyCombo
    // flags contains modifier state (kCGEventFlagMaskCommand, kCGEventFlagMaskControl, etc.)
    None
}

#[cfg(target_os = "macos")]
fn synthesize_combo_macos(_combo: &KeyCombo) -> Option<core_graphics::event::CGEvent> {
    // TODO: create a new CGEvent with the translated keycode + modifier flags
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use softkvm_core::keymap::default_translations;

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

        // same OS pair — no translations needed
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

    #[tokio::test]
    async fn test_key_interceptor_lifecycle() {
        let interceptor = KeyInterceptor::new(OsType::MacOS, default_translations(), true);

        // start in stub mode on linux
        assert!(interceptor.start().is_ok());

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
}
