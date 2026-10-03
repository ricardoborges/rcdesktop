//! Keeps a single RC Desktop running. A second launch (Start menu, desktop
//! shortcut) signals the first one to show its window, then exits.

use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject, INFINITE};

const EVENT_NAME: &str = "Local\\RcDesktop.ShowWindow";

pub struct Instance {
    // Auto-reset event; stored as usize because raw handles aren't Send
    event: usize,
}

/// `Some` when this is the first instance; otherwise the running one has
/// been asked to show itself and `None` is returned.
pub fn acquire() -> Option<Instance> {
    let name: Vec<u16> = EVENT_NAME.encode_utf16().chain(Some(0)).collect();
    let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
    if event.is_null() {
        // Can't coordinate; behave as a normal standalone launch
        return Some(Instance { event: 0 });
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe { SetEvent(event) };
        return None;
    }
    Some(Instance { event: event as usize })
}

impl Instance {
    /// Calls `on_show` (from a background thread) whenever another launch
    /// asks this instance to show its window.
    pub fn listen(&self, on_show: impl Fn() + Send + 'static) {
        let event = self.event;
        if event == 0 {
            return;
        }
        std::thread::spawn(move || loop {
            if unsafe { WaitForSingleObject(event as _, INFINITE) } != 0 {
                break;
            }
            on_show();
        });
    }
}
