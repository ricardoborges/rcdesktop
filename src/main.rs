use std::sync::Arc;
use slint::ComponentHandle;
use rcdesktop::app::AppController;
use rcdesktop::config::AppConfig;
use rcdesktop::wslc::client::WslcClient;
use rcdesktop::wslc::mock::MockWslcClient;
use rcdesktop::wslc::queue::RealWslcClient;
use rcdesktop::MainWindow;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let config = AppConfig::default();
    let use_mock = config.mock_mode || args.iter().any(|a| a == "--mock" || a == "-m");

    let client: Arc<dyn WslcClient> = if use_mock {
        println!("[RC Desktop] Running in MOCK MODE (offline preview)");
        Arc::new(MockWslcClient::new())
    } else {
        println!("[RC Desktop] Running in LIVE WSLC MODE");
        Arc::new(RealWslcClient::new())
    };

    let main_window = MainWindow::new()?;

    // Initialize System Tray
    let _tray = match rcdesktop::tray::TrayManager::new() {
        Ok(t) => {
            println!("[RC Desktop] System tray icon initialized.");
            Some(t)
        }
        Err(e) => {
            eprintln!("[RC Desktop] Warning: Could not initialize system tray: {}", e);
            None
        }
    };

    // Wire application controller and event loops
    AppController::setup(&main_window, client);

    println!("[RC Desktop] Initialized window and controller. Starting event loop.");
    main_window.run()?;

    Ok(())
}
