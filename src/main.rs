// GUI subsystem: no console window in release builds (debug keeps it for logs)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use slint::{CloseRequestResponse, ComponentHandle};
use rcdesktop::app::AppController;
use rcdesktop::config::AppConfig;
use rcdesktop::settings::{self, autostart, AUTOSTART_ARG};
use rcdesktop::single_instance;
use rcdesktop::tray;
use rcdesktop::wslc::client::WslcClient;
use rcdesktop::wslc::mock::MockWslcClient;
use rcdesktop::wslc::queue::RealWslcClient;
use rcdesktop::MainWindow;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let config = AppConfig::default();
    let use_mock = config.mock_mode || args.iter().any(|a| a == "--mock" || a == "-m");
    let from_autostart = args.iter().any(|a| a == AUTOSTART_ARG);

    // A second launch just brings the running instance's window back.
    // The mock preview runs side by side with a real instance.
    let instance = if use_mock {
        None
    } else {
        match single_instance::acquire() {
            Some(i) => Some(i),
            None => {
                println!("[RC Desktop] Already running; showing the existing window.");
                return Ok(());
            }
        }
    };

    let client: Arc<dyn WslcClient> = if use_mock {
        println!("[RC Desktop] Running in MOCK MODE (offline preview)");
        Arc::new(MockWslcClient::new().with_latency(std::time::Duration::from_millis(1200)))
    } else {
        println!("[RC Desktop] Running in LIVE WSLC MODE");
        Arc::new(RealWslcClient::new())
    };

    let main_window = MainWindow::new()?;

    if let Err(e) = tray::install(&main_window) {
        eprintln!("[RC Desktop] Warning: Could not initialize system tray: {}", e);
    }
    if let Some(instance) = &instance {
        let weak = main_window.as_weak();
        instance.listen(move || tray::show_window(&weak));
    }
    // Keep the startup entry pointing at this exe if the folder moved
    autostart::refresh();

    // Closing hides to the tray (when enabled and the tray is up); Quit in
    // the tray menu ends the app
    main_window.window().on_close_requested(|| {
        if !(settings::get().close_to_tray && tray::is_installed()) {
            let _ = slint::quit_event_loop();
        }
        CloseRequestResponse::HideWindow
    });

    // Wire application controller and event loops
    AppController::setup(&main_window, client);

    let start_hidden = from_autostart && settings::get().start_minimized && tray::is_installed();
    if !start_hidden {
        main_window.show()?;
    }
    println!("[RC Desktop] Initialized window and controller. Starting event loop.");
    slint::run_event_loop_until_quit()?;

    Ok(())
}
