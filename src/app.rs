use std::sync::Arc;
use std::time::Duration;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel, Weak};

use crate::domain::container::ContainerState;
use crate::wslc::client::WslcClient;

// The Slint-generated types are made available via crate's include_modules or module imports.
use crate::{ContainerItem, ImageItem, MainWindow, SessionInfoItem, VolumeItem};

pub struct AppController;

impl AppController {
    pub fn setup(window: &MainWindow, client: Arc<dyn WslcClient>) {
        let weak_window = window.as_weak();

        // 1. Initial refresh & background polling task
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            tokio::spawn(async move {
                // Initial load immediately
                Self::refresh_data(weak_window.clone(), client.clone()).await;

                // Continuous adaptive polling loop
                let mut interval = tokio::time::interval(Duration::from_millis(2500));
                loop {
                    interval.tick().await;
                    if weak_window.upgrade().is_none() {
                        break;
                    }
                    Self::refresh_data(weak_window.clone(), client.clone()).await;
                }
            });
        }

        // 2. Refresh All Callback
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_refresh_all(move || {
                let client = client.clone();
                let weak_window = weak_window.clone();
                tokio::spawn(async move {
                    Self::refresh_data(weak_window, client).await;
                });
            });
        }

        // 3. Start Container Callback
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_start_container(move |id| {
                let client = client.clone();
                let weak_window = weak_window.clone();
                let id_str = id.to_string();
                tokio::spawn(async move {
                    let _ = client.start_container(&id_str).await;
                    Self::refresh_data(weak_window, client).await;
                });
            });
        }

        // 4. Stop Container Callback
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_stop_container(move |id| {
                let client = client.clone();
                let weak_window = weak_window.clone();
                let id_str = id.to_string();
                tokio::spawn(async move {
                    let _ = client.stop_container(&id_str).await;
                    Self::refresh_data(weak_window, client).await;
                });
            });
        }

        // 5. Restart Container Callback
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_restart_container(move |id| {
                let client = client.clone();
                let weak_window = weak_window.clone();
                let id_str = id.to_string();
                tokio::spawn(async move {
                    let _ = client.restart_container(&id_str).await;
                    Self::refresh_data(weak_window, client).await;
                });
            });
        }

        // 6. Remove Container Callback
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_remove_container(move |id| {
                let client = client.clone();
                let weak_window = weak_window.clone();
                let id_str = id.to_string();
                tokio::spawn(async move {
                    let _ = client.remove_container(&id_str).await;
                    Self::refresh_data(weak_window, client).await;
                });
            });
        }

        // 7. View Logs Callback
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_view_logs(move |id| {
                let client = client.clone();
                let weak_window = weak_window.clone();
                let id_str = id.to_string();
                tokio::spawn(async move {
                    let logs = client
                        .get_logs(&id_str, 100)
                        .await
                        .unwrap_or_else(|e| format!("Failed to read logs: {}", e));

                    let title = format!("Logs: {}", id_str);
                    let _ = weak_window.upgrade_in_event_loop(move |w| {
                        w.set_details_modal_title(SharedString::from(title));
                        w.set_details_modal_content(SharedString::from(logs));
                        w.set_details_modal_open(true);
                    });
                });
            });
        }

        // 8. Inspect Container Callback
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_inspect_container(move |id| {
                let client = client.clone();
                let weak_window = weak_window.clone();
                let id_str = id.to_string();
                tokio::spawn(async move {
                    let inspect_json = client
                        .inspect_container(&id_str)
                        .await
                        .unwrap_or_else(|e| format!("Failed to inspect container: {}", e));

                    let title = format!("Inspect: {}", id_str);
                    let _ = weak_window.upgrade_in_event_loop(move |w| {
                        w.set_details_modal_title(SharedString::from(title));
                        w.set_details_modal_content(SharedString::from(inspect_json));
                        w.set_details_modal_open(true);
                    });
                });
            });
        }

        // 9. Open Terminal Callback (Windows Terminal)
        {
            window.on_open_terminal(move |id| {
                let id_str = id.to_string();
                std::thread::spawn(move || {
                    let _ = std::process::Command::new("wt.exe")
                        .args(&["-w", "0", "nt", "wslc", "exec", "-it", &id_str, "sh"])
                        .spawn();
                });
            });
        }

        // 10. Pull Image Callback
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_pull_image(move |img| {
                let client = client.clone();
                let weak_window = weak_window.clone();
                let img_str = img.to_string();
                tokio::spawn(async move {
                    let _ = client.pull_image(&img_str).await;
                    Self::refresh_data(weak_window, client).await;
                });
            });
        }

        // 11. Remove Image Callback
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_remove_image(move |id| {
                let client = client.clone();
                let weak_window = weak_window.clone();
                let id_str = id.to_string();
                tokio::spawn(async move {
                    let _ = client.remove_image(&id_str).await;
                    Self::refresh_data(weak_window, client).await;
                });
            });
        }

        // 12. Restart WSL Callback
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_restart_wsl(move || {
                let client = client.clone();
                let weak_window = weak_window.clone();
                tokio::spawn(async move {
                    let _ = tokio::process::Command::new("wsl")
                        .arg("--shutdown")
                        .output()
                        .await;
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    Self::refresh_data(weak_window, client).await;
                });
            });
        }

        // 13. Run Diagnostics Callback
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_run_diagnostics(move || {
                let client = client.clone();
                let weak_window = weak_window.clone();
                tokio::spawn(async move {
                    let session = client.get_session_info().await.unwrap_or_default();
                    let diag_report = format!(
                        "=== WSLC Diagnostics Report ===\nSession ID: {}\nVersion: {}\nHealth: {}\nElevation: {}\nStatus Message: {}\nChecked at: {:?}",
                        session.session_id,
                        session.version,
                        if session.is_healthy { "Healthy" } else { "Unhealthy / Degraded" },
                        if session.is_elevated { "Elevated (Admin)" } else { "Normal (Non-Elevated)" },
                        session.status_message,
                        std::time::SystemTime::now()
                    );

                    let _ = weak_window.upgrade_in_event_loop(move |w| {
                        w.set_details_modal_title(SharedString::from("Diagnostics Report"));
                        w.set_details_modal_content(SharedString::from(diag_report));
                        w.set_details_modal_open(true);
                    });
                });
            });
        }
    }

    async fn refresh_data(weak_window: Weak<MainWindow>, client: Arc<dyn WslcClient>) {
        let containers_res = client.list_containers(true).await;
        let images_res = client.list_images().await;
        let volumes_res = client.list_volumes().await;
        let session_res = client.get_session_info().await;

        let _ = weak_window.upgrade_in_event_loop(move |w| {
            // Update Containers
            if let Ok(containers) = containers_res {
                let running_count = containers
                    .iter()
                    .filter(|c| c.state == ContainerState::Running)
                    .count() as i32;
                let total_count = containers.len() as i32;

                let items: Vec<ContainerItem> = containers
                    .into_iter()
                    .map(|c| {
                        let ports_str = c
                            .ports
                            .iter()
                            .map(|p| format!("{}:{}", p.host_port, p.container_port))
                            .collect::<Vec<_>>()
                            .join(", ");

                        let state_str = match c.state {
                            ContainerState::Running => "Running",
                            ContainerState::Exited(_) => "Exited",
                            ContainerState::Created => "Created",
                            ContainerState::Restarting => "Starting",
                            ContainerState::Paused => "Warning",
                            ContainerState::Unknown(_) => "Exited",
                        };

                        let name_str = c.primary_name().to_string();
                        let id_str = c.id;

                        ContainerItem {
                            id: SharedString::from(id_str),
                            name: SharedString::from(name_str),
                            image: SharedString::from(c.image),
                            status: SharedString::from(c.status),
                            state: SharedString::from(state_str),
                            ports: SharedString::from(ports_str),
                        }
                    })
                    .collect();

                w.set_containers_list(ModelRc::new(VecModel::from(items)));
                w.set_running_containers_count(running_count);
                w.set_total_containers_count(total_count);
            }

            // Update Images
            if let Ok(images) = images_res {
                w.set_total_images_count(images.len() as i32);
                let items: Vec<ImageItem> = images
                    .into_iter()
                    .map(|i| ImageItem {
                        id: SharedString::from(i.id),
                        repository: SharedString::from(i.repository),
                        tag: SharedString::from(i.tag),
                        size: SharedString::from(i.size),
                        created: SharedString::from(i.created_at),
                    })
                    .collect();
                w.set_images_list(ModelRc::new(VecModel::from(items)));
            }

            // Update Volumes
            if let Ok(volumes) = volumes_res {
                w.set_total_volumes_count(volumes.len() as i32);
                let items: Vec<VolumeItem> = volumes
                    .into_iter()
                    .map(|v| VolumeItem {
                        name: SharedString::from(v.name),
                        driver: SharedString::from(v.driver),
                        scope: SharedString::from(v.scope),
                    })
                    .collect();
                w.set_volumes_list(ModelRc::new(VecModel::from(items)));
            }

            // Update Session Info
            if let Ok(session) = session_res {
                w.set_session_info(SessionInfoItem {
                    session_id: SharedString::from(session.session_id),
                    is_elevated: session.is_elevated,
                    is_healthy: session.is_healthy,
                    status_message: SharedString::from(session.status_message),
                    version: SharedString::from(session.version),
                });
            }
        });
    }
}
