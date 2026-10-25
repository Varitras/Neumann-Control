//! The desk's shortcuts from other programs: Ctrl+Shift+Alt with the window's keys, as Windows hot
//! keys on a thread of their own. Off unless chosen in the settings.

use std::sync::mpsc::Receiver;
use std::sync::Arc;

use crate::shortcuts::DeskShortcut;

/// Redraws the app; a press from another program arrives while it sleeps.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Listens for the shortcuts from other programs: Windows' hot keys in the app, a stand-in in
/// tests.
pub trait GlobalKeys {
    /// Starts listening; `None` when not one of the keys could be had.
    fn listen(&self, wake: Wake) -> Option<GlobalListener>;
}

/// The shortcuts pressed in other programs, while it lives.
pub struct GlobalListener {
    pub pressed: Receiver<DeskShortcut>,
    stop: Option<Box<dyn FnOnce() + Send>>,
}

impl GlobalListener {
    pub fn new(pressed: Receiver<DeskShortcut>, stop: Box<dyn FnOnce() + Send>) -> GlobalListener {
        GlobalListener {
            pressed,
            stop: Some(stop),
        }
    }
}

impl Drop for GlobalListener {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

#[cfg(windows)]
pub use windows_keys::WindowsGlobalKeys;

#[cfg(windows)]
mod windows_keys {
    use std::sync::mpsc;
    use std::thread;

    use eframe::egui::Key;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, VK_DOWN,
        VK_UP,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetMessageW, PostThreadMessageW, MSG, WM_HOTKEY, WM_QUIT,
    };

    use super::{GlobalKeys, GlobalListener, Wake};
    use crate::shortcuts::SHORTCUT_KEYS;

    pub struct WindowsGlobalKeys;

    /// With Shift: Windows reports AltGr as Ctrl+Alt, so Ctrl+Alt+M took a German keyboard's µ.
    const GLOBAL_MODIFIERS: u32 = MOD_CONTROL | MOD_SHIFT | MOD_ALT | MOD_NOREPEAT;

    /// The virtual key of a shortcut's key; letters are their capital's code.
    fn virtual_key(key: Key) -> u32 {
        match key {
            Key::ArrowUp => u32::from(VK_UP),
            Key::ArrowDown => u32::from(VK_DOWN),
            other => u32::from(other.name().as_bytes()[0]),
        }
    }

    impl GlobalKeys for WindowsGlobalKeys {
        fn listen(&self, wake: Wake) -> Option<GlobalListener> {
            let (pressed, received) = mpsc::channel();
            let (thread_id, told) = mpsc::channel();
            thread::spawn(move || {
                // A hot key belongs to the thread that registers it and arrives in its queue.
                let mut had = 0;
                for (id, (shortcut, key)) in SHORTCUT_KEYS.iter().enumerate() {
                    let registered = unsafe {
                        RegisterHotKey(
                            std::ptr::null_mut(),
                            id as i32,
                            GLOBAL_MODIFIERS,
                            virtual_key(*key),
                        )
                    };
                    if registered == 0 {
                        log::warn!(
                            "the global shortcut for {shortcut:?} is taken by another program"
                        );
                        continue;
                    }
                    had += 1;
                }
                let _ = thread_id.send((had > 0).then(|| unsafe { GetCurrentThreadId() }));
                if had == 0 {
                    return;
                }
                let mut message: MSG = unsafe { std::mem::zeroed() };
                while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
                    if message.message != WM_HOTKEY {
                        continue;
                    }
                    let Some((shortcut, _)) = SHORTCUT_KEYS.get(message.wParam) else {
                        continue;
                    };
                    if pressed.send(*shortcut).is_err() {
                        break;
                    }
                    wake();
                }
                for id in 0..SHORTCUT_KEYS.len() {
                    unsafe { UnregisterHotKey(std::ptr::null_mut(), id as i32) };
                }
            });
            let thread_id = told.recv().ok()??;
            let stop = Box::new(move || {
                unsafe { PostThreadMessageW(thread_id, WM_QUIT, 0, 0) };
            });
            Some(GlobalListener::new(received, stop))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Takes Ctrl+Shift+Alt keys of this machine for a moment, so it runs only when asked:
        /// `cargo test -p neumann-control-app -- --ignored hot_keys`.
        #[test]
        #[ignore]
        fn the_hot_keys_are_let_go_when_the_listener_goes() {
            let wake: Wake = std::sync::Arc::new(|| {});
            let first = WindowsGlobalKeys.listen(std::sync::Arc::clone(&wake));
            assert!(first.is_some(), "the keys are free");
            drop(first);
            std::thread::sleep(std::time::Duration::from_millis(200));
            assert!(WindowsGlobalKeys.listen(wake).is_some(), "let go again");
        }

        /// Windows reports AltGr as Ctrl+Alt: with Ctrl+Alt+M a German keyboard's AltGr+M (µ)
        /// muted every speaker instead of typing.
        #[test]
        fn the_global_keys_are_not_an_altgr_combination() {
            assert_ne!(GLOBAL_MODIFIERS & !MOD_NOREPEAT, MOD_CONTROL | MOD_ALT);
            assert_ne!(GLOBAL_MODIFIERS & MOD_SHIFT, 0);
        }

        /// Windows names the letter keys by their capitals.
        #[test]
        fn the_keys_are_windows_virtual_keys() {
            assert_eq!(virtual_key(Key::M), u32::from(b'M'));
            assert_eq!(virtual_key(Key::D), u32::from(b'D'));
            assert_eq!(virtual_key(Key::ArrowUp), u32::from(VK_UP));
        }
    }
}
