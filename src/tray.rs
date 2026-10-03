use tray_icon::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

pub struct TrayManager {
    _tray_icon: TrayIcon,
}

impl TrayManager {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let tray_menu = Menu::new();

        let open_item = MenuItem::new("Open RC Desktop", true, None);
        let restart_wsl_item = MenuItem::new("Restart WSL (wsl --shutdown)", true, None);
        let quit_item = MenuItem::new("Quit", true, None);

        let _ = tray_menu.append(&open_item);
        let _ = tray_menu.append(&PredefinedMenuItem::separator());
        let _ = tray_menu.append(&restart_wsl_item);
        let _ = tray_menu.append(&PredefinedMenuItem::separator());
        let _ = tray_menu.append(&quit_item);

        // The app icon embedded in the exe (assets/rcdesktop.rc); the drawn
        // fallback covers builds without resources
        let icon = Icon::from_resource(1, Some((32, 32))).or_else(|_| create_default_icon())?;

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip("RC Desktop - WSLC Manager")
            .with_icon(icon)
            .build()?;

        // Listen for menu events in background
        let open_id = open_item.id().clone();
        let restart_id = restart_wsl_item.id().clone();
        let quit_id = quit_item.id().clone();

        std::thread::spawn(move || {
            let menu_channel = tray_icon::menu::MenuEvent::receiver();
            while let Ok(event) = menu_channel.recv() {
                if event.id == open_id {
                    // Open / show window if minimized
                    println!("[RC Desktop Tray] Open clicked");
                } else if event.id == restart_id {
                    println!("[RC Desktop Tray] Restart WSL clicked");
                    let _ = std::process::Command::new("wsl")
                        .arg("--shutdown")
                        .spawn();
                } else if event.id == quit_id {
                    println!("[RC Desktop Tray] Quit clicked");
                    std::process::exit(0);
                }
            }
        });

        Ok(Self { _tray_icon: tray_icon })
    }
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
