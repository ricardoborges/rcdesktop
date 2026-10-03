use std::cell::RefCell;

use slint::ComponentHandle;
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

use crate::settings::autostart;
use crate::MainWindow;

const TOOLTIP: &str = "RC Desktop";

thread_local! {
    // The tray lives on the UI thread; kept here so status updates can reach it
    static TRAY: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
    static AUTOSTART_ITEM: RefCell<Option<CheckMenuItem>> = const { RefCell::new(None) };
}

/// Tray icon with a menu to open the window, toggle start with Windows,
/// restart WSL and quit. Must be created on the UI thread.
pub fn install(window: &MainWindow) -> Result<(), Box<dyn std::error::Error>> {
    let menu = Menu::new();
    let open_item = MenuItem::new("Open RC Desktop", true, None);
    let autostart_item = CheckMenuItem::new("Start with Windows", true, autostart::is_enabled(), None);
    let restart_wsl_item = MenuItem::new("Restart WSL (wsl --shutdown)", true, None);
    let quit_item = MenuItem::new("Quit", true, None);

    menu.append(&open_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&autostart_item)?;
    menu.append(&restart_wsl_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit_item)?;

    // The app icon embedded in the exe (assets/rcdesktop.rc); the drawn
    // fallback covers builds without resources
    let icon = Icon::from_resource(1, Some((32, 32))).or_else(|_| create_default_icon())?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip(TOOLTIP)
        .with_icon(icon)
        .build()?;

    let (open_id, autostart_id, restart_id, quit_id) =
        (open_item.id().clone(), autostart_item.id().clone(), restart_wsl_item.id().clone(), quit_item.id().clone());
    let weak = window.as_weak();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == open_id {
            show_window(&weak);
        } else if event.id == autostart_id {
            let _ = weak.upgrade_in_event_loop(|w| {
                let enable = !autostart::is_enabled();
                match autostart::set_enabled(enable) {
                    Ok(()) => w.set_setting_autostart(enable),
                    Err(e) => {
                        w.set_details_modal_title("Start with Windows".into());
                        w.set_details_modal_content(e.into());
                        w.set_details_modal_open(true);
                    }
                }
                sync_autostart_item();
            });
        } else if event.id == restart_id {
            let _ = std::process::Command::new("wsl").arg("--shutdown").spawn();
        } else if event.id == quit_id {
            let _ = slint::invoke_from_event_loop(|| {
                let _ = slint::quit_event_loop();
            });
        }
    }));

    // Left click (or double click) on the icon brings the window back
    let weak = window.as_weak();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| match event {
        TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
        | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => show_window(&weak),
        _ => {}
    }));

    TRAY.with(|t| *t.borrow_mut() = Some(tray));
    AUTOSTART_ITEM.with(|i| *i.borrow_mut() = Some(autostart_item));
    Ok(())
}

/// Whether the tray icon is up (the window may only hide to the tray then).
pub fn is_installed() -> bool {
    TRAY.with(|t| t.borrow().is_some())
}

/// Shows and restores the main window. Callable from any thread.
pub fn show_window(weak: &slint::Weak<MainWindow>) {
    let _ = weak.upgrade_in_event_loop(|w| {
        let _ = w.show();
        w.window().set_minimized(false);
    });
}

/// Updates the tooltip with container counts. UI thread only.
pub fn set_status(running: i32, total: i32) {
    let text = format!("{}\n{} of {} containers running", TOOLTIP, running, total);
    TRAY.with(|t| {
        if let Some(tray) = t.borrow().as_ref() {
            let _ = tray.set_tooltip(Some(text));
        }
    });
}

/// Mirrors the Run entry in the tray menu's check mark. UI thread only.
pub fn sync_autostart_item() {
    AUTOSTART_ITEM.with(|i| {
        if let Some(item) = i.borrow().as_ref() {
            item.set_checked(autostart::is_enabled());
        }
    });
}

fn create_default_icon() -> Result<Icon, Box<dyn std::error::Error>> {
    // Generate a clean 32x32 RGBA icon (Cyan / Blue container logo)
    let width = 32u32;
    let height = 32u32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);

    for y in 0..height {
        for x in 0..width {
            let in_box = x >= 4 && x < 28 && y >= 4 && y < 28;
            let is_border = in_box && (x == 4 || x == 27 || y == 4 || y == 27);
            let is_cross = in_box && (x == 16 || y == 16);

            if is_border || is_cross {
                // White accent
                rgba.extend_from_slice(&[255, 255, 255, 255]);
            } else if in_box {
                // Ocean / WSL container cyan
                rgba.extend_from_slice(&[56, 189, 248, 255]);
            } else {
                // Transparent
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }

    let icon = Icon::from_rgba(rgba, width, height)?;
    Ok(icon)
}
