//! User preferences, kept in `%APPDATA%\rcdesktop\settings.json`, and the
//! "start with Windows" entry in `HKCU\...\CurrentVersion\Run`.

use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

/// Argument the autostart entry passes, so the app can start in the tray.
pub const AUTOSTART_ARG: &str = "--autostart";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserSettings {
    /// When launched at sign-in, stay in the tray instead of opening the window
    pub start_minimized: bool,
    /// The window's close button hides it to the tray instead of quitting
    pub close_to_tray: bool,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self { start_minimized: true, close_to_tray: true }
    }
}

static CURRENT: RwLock<Option<UserSettings>> = RwLock::new(None);

fn settings_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("rcdesktop").join("settings.json"))
}

/// Current settings, read from disk on first use (defaults if missing or invalid).
pub fn get() -> UserSettings {
    if let Some(s) = CURRENT.read().unwrap().as_ref() {
        return s.clone();
    }
    let loaded = settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    *CURRENT.write().unwrap() = Some(loaded);
    CURRENT.read().unwrap().clone().unwrap_or_default()
}

/// Applies `change` and saves the result.
pub fn update(change: impl FnOnce(&mut UserSettings)) -> Result<UserSettings, String> {
    let mut s = get();
    change(&mut s);
    *CURRENT.write().unwrap() = Some(s.clone());
    let path = settings_path().ok_or("APPDATA is not set")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("Cannot save {}: {}", path.display(), e))?;
    Ok(s)
}

/// "Start with Windows", stored as a per-user Run entry.
pub mod autostart {
    use super::AUTOSTART_ARG;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "RC Desktop";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// The command the Run entry should hold for this executable.
    fn command() -> Result<String, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        Ok(format!("\"{}\" {}", exe.display(), AUTOSTART_ARG))
    }

    /// The Run entry's current command, if there is one.
    fn read() -> Option<String> {
        let (key, value) = (wide(RUN_KEY), wide(VALUE));
        let mut buf = vec![0u16; 2048];
        let mut len = (buf.len() * 2) as u32;
        let rc = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut len,
            )
        };
        if rc != ERROR_SUCCESS {
            return None;
        }
        buf.truncate((len as usize / 2).saturating_sub(1));
        Some(String::from_utf16_lossy(&buf))
    }

    pub fn is_enabled() -> bool {
        read().is_some()
    }

    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        let (key, value) = (wide(RUN_KEY), wide(VALUE));
        let rc = if enabled {
            let data = wide(&command()?);
            unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    value.as_ptr(),
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            }
        } else {
            if read().is_none() {
                return Ok(());
            }
            unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) }
        };
        if rc == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("Updating the Windows startup entry failed (error {})", rc))
        }
    }

    /// Points an existing entry at this executable again, e.g. after the
    /// portable folder was moved.
    pub fn refresh() {
        if let (Some(current), Ok(expected)) = (read(), command()) {
            if current != expected {
                let _ = set_enabled(true);
            }
        }
    }
}
