use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::future::Future;
use std::time::Duration;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel, Weak};

use crate::domain::container::ContainerState;
use crate::domain::compose::{load_env, parse_compose, ComposeProject};
use crate::domain::deploy::{parse_lines, parse_run_command, quote_args, ContainerSpec};
use crate::rcompose;
use crate::wslc::client::WslcClient;
use crate::wslc::stack::{deploy_project as deploy_stack, remove_project};

// The Slint-generated types are made available via crate's include_modules or module imports.
use crate::{
    ContainerItem, DeployForm, ImageItem, MainWindow, NetworkItem, PortLink, SessionInfoItem, StackItem, VolumeItem,
    WslcInfoItem,
};
use crate::domain::system::PruneTarget;

// Stacks the user collapsed; survives the periodic model rebuilds.
static COLLAPSED_STACKS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

// In-flight lifecycle operations ("Starting…" etc.), keyed by container id / stack name.
static PENDING_CONTAINERS: Mutex<BTreeMap<String, &'static str>> = Mutex::new(BTreeMap::new());
static PENDING_STACKS: Mutex<BTreeMap<String, &'static str>> = Mutex::new(BTreeMap::new());
static PENDING_IMAGES: Mutex<BTreeMap<String, &'static str>> = Mutex::new(BTreeMap::new());
// Number of user-visible refreshes in flight (drives the global loading indicator)
static REFRESHING: AtomicUsize = AtomicUsize::new(0);
static PENDING_NETWORKS: Mutex<BTreeMap<String, &'static str>> = Mutex::new(BTreeMap::new());
static PENDING_VOLUMES: Mutex<BTreeMap<String, &'static str>> = Mutex::new(BTreeMap::new());

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
                Self::refresh_visible(weak_window.clone(), client.clone()).await;
                Self::load_wslc_info(weak_window.clone(), client.clone()).await;

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
                    Self::refresh_visible(weak_window, client).await;
                });
            });
        }

        // 3. Start Container Callback
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_start_container(move |id| {
                Self::run_pending(
                    &weak_window,
                    &client,
                    vec![id.to_string()],
                    None,
                    "Starting…",
                    |client, ids| async move {
                        for id in ids {
                            let _ = client.start_container(&id).await;
                        }
                    },
                );
            });
        }

        // 4. Stop Container Callback
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_stop_container(move |id| {
                Self::run_pending(
                    &weak_window,
                    &client,
                    vec![id.to_string()],
                    None,
                    "Stopping…",
                    |client, ids| async move {
                        for id in ids {
                            let _ = client.stop_container(&id).await;
                        }
                    },
                );
            });
        }

        // 5. Restart Container Callback
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_restart_container(move |id| {
                Self::run_pending(
                    &weak_window,
                    &client,
                    vec![id.to_string()],
                    None,
                    "Restarting…",
                    |client, ids| async move {
                        for id in ids {
                            let _ = client.restart_container(&id).await;
                        }
                    },
                );
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

        // 9a. Open published port in the default browser
        {
            window.on_open_url(|url| {
                let url = url.to_string();
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return;
                }
                std::thread::spawn(move || {
                    let _ = std::process::Command::new("rundll32.exe")
                        .args(["url.dll,FileProtocolHandler", &url])
                        .spawn();
                });
            });
        }

        // 9b. Stack Callbacks (compose projects)
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_start_stack(move |name| {
                let Some(w) = weak_window.upgrade() else { return };
                let ids = Self::stack_container_ids(&w, &name, false);
                Self::run_pending(
                    &weak_window,
                    &client,
                    ids,
                    Some(name.to_string()),
                    "Starting…",
                    |client, ids| async move {
                        for id in ids {
                            let _ = client.start_container(&id).await;
                        }
                    },
                );
            });
        }
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_stop_stack(move |name| {
                let Some(w) = weak_window.upgrade() else { return };
                let ids = Self::stack_container_ids(&w, &name, true);
                Self::run_pending(
                    &weak_window,
                    &client,
                    ids,
                    Some(name.to_string()),
                    "Stopping…",
                    |client, ids| async move {
                        for id in ids {
                            let _ = client.stop_container(&id).await;
                        }
                    },
                );
            });
        }
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_remove_stack(move |name| {
                let Some(w) = weak_window.upgrade() else { return };
                let ids = Self::stack_container_ids(&w, &name, true)
                    .into_iter()
                    .chain(Self::stack_container_ids(&w, &name, false))
                    .collect();
                let name = name.to_string();
                let weak = weak_window.clone();
                Self::run_pending(
                    &weak_window,
                    &client,
                    ids,
                    Some(name.clone()),
                    "Removing…",
                    move |client, _ids| async move {
                        if let Err(e) = remove_project(client.as_ref(), &name, |_| {}).await {
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                w.set_details_modal_title(format!("Removing stack {} failed", name).into());
                                w.set_details_modal_content(e.into());
                                w.set_details_modal_open(true);
                            });
                        }
                    },
                );
            });
        }
        {
            let weak_window = weak_window.clone();
            window.on_toggle_stack(move |name| {
                let Some(w) = weak_window.upgrade() else { return };
                let collapsed = {
                    let mut set = COLLAPSED_STACKS.lock().unwrap();
                    if !set.remove(name.as_str()) {
                        set.insert(name.to_string());
                    }
                    set.contains(name.as_str())
                };
                // Update in place so the list keeps its scroll position
                let stacks = w.get_stacks_list();
                for i in 0..stacks.row_count() {
                    if let Some(mut s) = stacks.row_data(i) {
                        if s.name == name {
                            s.collapsed = collapsed;
                            stacks.set_row_data(i, s);
                        }
                    }
                }
            });
        }

        // 9c. Deploy: form, docker run command and compose stacks
        window.on_preview_deploy(|form| Self::spec_from_form(&form).command_line().into());
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_deploy_container(move |form| {
                Self::deploy_spec(&weak_window, &client, Self::spec_from_form(&form));
            });
        }

        window.on_preview_run_command(|cmd| match parse_run_command(&cmd) {
            Ok((spec, warnings)) => {
                let mut out = spec.command_line();
                for w in warnings {
                    out.push_str(&format!("\n⚠ {}", w));
                }
                out.into()
            }
            Err(e) => format!("⚠ {}", e).into(),
        });
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_deploy_run_command(move || {
                let Some(w) = weak_window.upgrade() else { return };
                match parse_run_command(&w.get_run_command()) {
                    Ok((spec, _)) => Self::deploy_spec(&weak_window, &client, spec),
                    Err(e) => w.set_deploy_error(e.into()),
                }
            });
        }

        window.on_preview_compose(|yaml, project, file| Self::compose_preview(&yaml, &project, &file).into());
        {
            let weak_window = weak_window.clone();
            window.on_open_compose_file(move || {
                let Some(w) = weak_window.upgrade() else { return };
                let Some(path) = rfd::FileDialog::new()
                    .set_title("Open a Compose file")
                    .add_filter("Compose files", &["yml", "yaml"])
                    .add_filter("All files", &["*"])
                    .pick_file()
                else {
                    return;
                };
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        w.set_compose_yaml(text.into());
                        w.set_compose_file(path.display().to_string().into());
                        w.set_deploy_error(SharedString::default());
                        w.set_deploy_log(SharedString::default());
                    }
                    Err(e) => w.set_deploy_error(format!("Cannot read {}: {}", path.display(), e).into()),
                }
            });
        }
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_deploy_compose(move || {
                let Some(w) = weak_window.upgrade() else { return };
                let (yaml, project, file) = (w.get_compose_yaml(), w.get_compose_project(), w.get_compose_file());
                w.set_deploy_error(SharedString::default());
                w.set_deploy_log(SharedString::default());

                if client.is_mock() {
                    Self::deploy_compose_builtin(&weak_window, &client, &yaml, &project, &file);
                    return;
                }
                let Some(bin) = rcompose::locate() else {
                    w.set_rcompose_path(SharedString::default());
                    w.set_deploy_error("rcompose is not installed. Install it to deploy stacks.".into());
                    return;
                };
                let target = match rcompose::ComposeTarget::prepare(&yaml, &project, &file, &rcompose::stacks_root()) {
                    Ok(t) => t,
                    Err(e) => {
                        w.set_deploy_error(e.into());
                        return;
                    }
                };
                w.set_deploy_busy(true);

                let client = client.clone();
                let weak_window = weak_window.clone();
                tokio::spawn(async move {
                    let log = Self::deploy_logger(&weak_window);
                    log(format!("$ rcompose {}", quote_args(&target.up_args())));
                    let result = target.up(&bin, &log).await;
                    match &result {
                        Ok(()) => log("✓ Stack is up".to_string()),
                        Err(e) => log(format!("✗ {}", e)),
                    }
                    let _ = weak_window.upgrade_in_event_loop(move |w| {
                        w.set_deploy_busy(false);
                        if let Err(e) = result {
                            w.set_deploy_error(format!("Deploy failed: {}", e).into());
                        }
                    });
                    Self::refresh_data(weak_window, client).await;
                });
            });
        }

        // 9d. rcompose detection and installation
        window.set_rcompose_path(
            if client.is_mock() {
                "built-in (mock mode)".to_string()
            } else {
                rcompose::locate().map(|p| p.display().to_string()).unwrap_or_default()
            }
            .into(),
        );
        {
            let weak_window = weak_window.clone();
            window.on_install_rcompose(move || {
                let Some(w) = weak_window.upgrade() else { return };
                w.set_rcompose_installing(true);
                w.set_deploy_error(SharedString::default());
                w.set_deploy_log(SharedString::default());

                let weak_window = weak_window.clone();
                tokio::spawn(async move {
                    let log = Self::deploy_logger(&weak_window);
                    let result = rcompose::install(&log).await;
                    match &result {
                        Ok(p) => log(format!("✓ rcompose installed at {}", p.display())),
                        Err(e) => log(format!("✗ {}", e)),
                    }
                    let _ = weak_window.upgrade_in_event_loop(move |w| {
                        w.set_rcompose_installing(false);
                        match result {
                            Ok(p) => w.set_rcompose_path(p.display().to_string().into()),
                            Err(e) => w.set_deploy_error(e.into()),
                        }
                    });
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
                PENDING_IMAGES.lock().unwrap().insert(id_str.clone(), "Removing…");
                if let Some(w) = weak_window.upgrade() {
                    Self::apply_image_pending(&w);
                }
                tokio::spawn(async move {
                    let _ = client.remove_image(&id_str).await;
                    PENDING_IMAGES.lock().unwrap().remove(&id_str);
                    Self::refresh_data(weak_window.clone(), client).await;
                    // Clears the spinner even when the refresh failed
                    let _ = weak_window.upgrade_in_event_loop(|w| Self::apply_image_pending(&w));
                });
            });
        }

        // 11b. Remove Volume Callback (confirmed in the UI first)
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_remove_volume(move |name| {
                let client = client.clone();
                let weak_window = weak_window.clone();
                let name = name.to_string();
                PENDING_VOLUMES.lock().unwrap().insert(name.clone(), "Removing…");
                if let Some(w) = weak_window.upgrade() {
                    Self::apply_volume_pending(&w);
                }
                tokio::spawn(async move {
                    let result = client.remove_volume(&name).await;
                    PENDING_VOLUMES.lock().unwrap().remove(&name);
                    Self::refresh_data(weak_window.clone(), client).await;
                    let _ = weak_window.upgrade_in_event_loop(move |w| {
                        // Clears the spinner even when the refresh failed
                        Self::apply_volume_pending(&w);
                        if let Err(e) = result {
                            w.set_details_modal_title(format!("Deleting volume {} failed", name).into());
                            w.set_details_modal_content(e.into());
                            w.set_details_modal_open(true);
                        }
                    });
                });
            });
        }

        // 11c. Remove Network Callback (confirmed in the UI first)
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_remove_network(move |name| {
                let client = client.clone();
                let weak_window = weak_window.clone();
                let name = name.to_string();
                PENDING_NETWORKS.lock().unwrap().insert(name.clone(), "Removing…");
                if let Some(w) = weak_window.upgrade() {
                    Self::apply_network_pending(&w);
                }
                tokio::spawn(async move {
                    let result = client.remove_network(&name).await;
                    PENDING_NETWORKS.lock().unwrap().remove(&name);
                    Self::refresh_data(weak_window.clone(), client).await;
                    let _ = weak_window.upgrade_in_event_loop(move |w| {
                        // Clears the spinner even when the refresh failed
                        Self::apply_network_pending(&w);
                        if let Err(e) = result {
                            w.set_details_modal_title(format!("Deleting network {} failed", name).into());
                            w.set_details_modal_content(e.into());
                            w.set_details_modal_open(true);
                        }
                    });
                });
            });
        }

        // 11d. WSLC page: system info, settings and cleanup
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_refresh_wslc_info(move || {
                tokio::spawn(Self::load_wslc_info(weak_window.clone(), client.clone()));
            });
        }
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_open_wslc_settings(move || {
                let client = client.clone();
                let weak_window = weak_window.clone();
                tokio::spawn(async move {
                    if let Err(e) = client.open_settings().await {
                        let _ = weak_window.upgrade_in_event_loop(move |w| {
                            w.set_details_modal_title("Opening wslc settings failed".into());
                            w.set_details_modal_content(e.into());
                            w.set_details_modal_open(true);
                        });
                    }
                });
            });
        }
        {
            let client = client.clone();
            let weak_window = weak_window.clone();
            window.on_prune(move |key| {
                let Some(target) = PruneTarget::from_key(&key) else { return };
                let Some(w) = weak_window.upgrade() else { return };
                w.set_prune_pending(key.clone());
                w.set_prune_output(SharedString::default());

                let client = client.clone();
                let weak_window = weak_window.clone();
                tokio::spawn(async move {
                    let output = match client.prune(target).await {
                        Ok(out) if out.trim().is_empty() => "Done, nothing was removed.".to_string(),
                        Ok(out) => out.trim().to_string(),
                        Err(e) => format!("Failed: {}", e.trim()),
                    };
                    let _ = weak_window.upgrade_in_event_loop(move |w| {
                        w.set_prune_pending(SharedString::default());
                        w.set_prune_output(output.into());
                    });
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

    /// Refresh triggered by the user (or the first load): shows the global
    /// loading indicator while it runs. Background polls stay silent.
    async fn refresh_visible(weak_window: Weak<MainWindow>, client: Arc<dyn WslcClient>) {
        REFRESHING.fetch_add(1, Ordering::SeqCst);
        let _ = weak_window.upgrade_in_event_loop(|w| w.set_refreshing(true));
        Self::refresh_data(weak_window.clone(), client).await;
        let still_busy = REFRESHING.fetch_sub(1, Ordering::SeqCst) > 1;
        let _ = weak_window.upgrade_in_event_loop(move |w| {
            w.set_loading(false);
            w.set_refreshing(still_busy);
        });
    }

    /// Fetches each section in turn and pushes it to the UI as soon as it
    /// arrives, so a slow command doesn't hold back the others.
    async fn refresh_data(weak_window: Weak<MainWindow>, client: Arc<dyn WslcClient>) {
        let containers_res = client.list_containers(true).await;
        let _ = weak_window.upgrade_in_event_loop(move |w| {
            // Update Containers
            if let Ok(containers) = containers_res {
                let running_count = containers
                    .iter()
                    .filter(|c| c.state == ContainerState::Running)
                    .count() as i32;
                let total_count = containers.len() as i32;

                let pending = PENDING_CONTAINERS.lock().unwrap().clone();
                let items: Vec<ContainerItem> = containers
                    .into_iter()
                    .map(|c| {
                        let port_links: Vec<PortLink> = c
                            .ports
                            .iter()
                            .map(|p| PortLink {
                                label: SharedString::from(format!("{}:{}", p.host_port, p.container_port)),
                                url: if p.protocol.eq_ignore_ascii_case("udp") {
                                    SharedString::new()
                                } else {
                                    SharedString::from(format!("http://localhost:{}", p.host_port))
                                },
                            })
                            .collect();

                        let state_str = match c.state {
                            ContainerState::Running => "Running",
                            ContainerState::Exited(_) => "Exited",
                            ContainerState::Created => "Created",
                            ContainerState::Restarting => "Starting",
                            ContainerState::Paused => "Warning",
                            ContainerState::Unknown(_) => "Exited",
                        };

                        let name_str = c.primary_name().to_string();
                        let stack_str = c.compose_project.unwrap_or_default();
                        let pending_str = pending.get(&c.id).copied().unwrap_or("");
                        let id_str = c.id;

                        ContainerItem {
                            id: SharedString::from(id_str),
                            name: SharedString::from(name_str),
                            image: SharedString::from(c.image),
                            status: SharedString::from(c.status),
                            state: SharedString::from(state_str),
                            ports: ModelRc::new(VecModel::from(port_links)),
                            stack: SharedString::from(stack_str),
                            pending: SharedString::from(pending_str),
                        }
                    })
                    .collect();

                let (stacks, standalone) = Self::group_by_stack(items);
                w.set_stacks_list(ModelRc::new(VecModel::from(stacks)));
                w.set_standalone_containers(ModelRc::new(VecModel::from(standalone)));
                w.set_running_containers_count(running_count);
                w.set_total_containers_count(total_count);
            }

        });

        let images_res = client.list_images().await;
        let _ = weak_window.upgrade_in_event_loop(move |w| {
            // Update Images
            if let Ok(images) = images_res {
                w.set_total_images_count(images.len() as i32);
                let pending = PENDING_IMAGES.lock().unwrap().clone();
                let items: Vec<ImageItem> = images
                    .into_iter()
                    .map(|i| ImageItem {
                        pending: pending.get(&i.id).copied().unwrap_or("").into(),
                        id: SharedString::from(i.id),
                        repository: SharedString::from(i.repository),
                        tag: SharedString::from(i.tag),
                        size: SharedString::from(i.size),
                        created: SharedString::from(i.created_at),
                    })
                    .collect();
                w.set_images_list(ModelRc::new(VecModel::from(items)));
            }

        });

        let volumes_res = client.list_volumes().await;
        let _ = weak_window.upgrade_in_event_loop(move |w| {
            // Update Volumes
            if let Ok(volumes) = volumes_res {
                w.set_total_volumes_count(volumes.len() as i32);
                let pending = PENDING_VOLUMES.lock().unwrap().clone();
                let items: Vec<VolumeItem> = volumes
                    .into_iter()
                    .map(|v| VolumeItem {
                        pending: pending.get(&v.name).copied().unwrap_or("").into(),
                        name: SharedString::from(v.name),
                        driver: SharedString::from(v.driver),
                        scope: SharedString::from(v.scope),
                    })
                    .collect();
                w.set_volumes_list(ModelRc::new(VecModel::from(items)));
            }

        });

        let networks_res = client.list_networks().await;
        let _ = weak_window.upgrade_in_event_loop(move |w| {
            // Update Networks
            if let Ok(networks) = networks_res {
                w.set_total_networks_count(networks.len() as i32);
                let pending = PENDING_NETWORKS.lock().unwrap().clone();
                let items: Vec<NetworkItem> = networks
                    .into_iter()
                    .map(|n| NetworkItem {
                        pending: pending.get(&n.name).copied().unwrap_or("").into(),
                        builtin: matches!(n.name.as_str(), "bridge" | "host" | "none"),
                        id: SharedString::from(n.id),
                        name: SharedString::from(n.name),
                        driver: SharedString::from(n.driver),
                        scope: SharedString::from(n.scope),
                        stack: SharedString::from(n.compose_project.unwrap_or_default()),
                    })
                    .collect();
                w.set_networks_list(ModelRc::new(VecModel::from(items)));
            }
        });

        let session_res = client.get_session_info().await;
        let _ = weak_window.upgrade_in_event_loop(move |w| {
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

    /// Splits containers into compose stacks (sorted by name) and standalone containers.
    pub fn group_by_stack(items: Vec<ContainerItem>) -> (Vec<StackItem>, Vec<ContainerItem>) {
        let collapsed = COLLAPSED_STACKS.lock().unwrap();
        let pending = PENDING_STACKS.lock().unwrap();
        let mut groups: BTreeMap<SharedString, Vec<ContainerItem>> = BTreeMap::new();
        let mut standalone = Vec::new();
        for item in items {
            if item.stack.is_empty() {
                standalone.push(item);
            } else {
                groups.entry(item.stack.clone()).or_default().push(item);
            }
        }

        let stacks = groups
            .into_iter()
            .map(|(name, containers)| StackItem {
                running: containers.iter().filter(|c| c.state == "Running").count() as i32,
                total: containers.len() as i32,
                collapsed: collapsed.contains(name.as_str()),
                pending: pending.get(name.as_str()).copied().unwrap_or("").into(),
                containers: ModelRc::new(VecModel::from(containers)),
                name,
            })
            .collect();
        (stacks, standalone)
    }

    /// Marks containers (and optionally their stack) as busy so the UI shows a
    /// spinner, runs `op` in the background, then clears the state and refreshes.
    fn run_pending<F, Fut>(
        weak_window: &Weak<MainWindow>,
        client: &Arc<dyn WslcClient>,
        ids: Vec<String>,
        stack: Option<String>,
        label: &'static str,
        op: F,
    ) where
        F: FnOnce(Arc<dyn WslcClient>, Vec<String>) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        {
            let mut pending = PENDING_CONTAINERS.lock().unwrap();
            for id in &ids {
                pending.insert(id.clone(), label);
            }
        }
        if let Some(name) = &stack {
            PENDING_STACKS.lock().unwrap().insert(name.clone(), label);
        }
        if let Some(w) = weak_window.upgrade() {
            Self::apply_pending(&w);
        }

        let client = client.clone();
        let weak_window = weak_window.clone();
        tokio::spawn(async move {
            op(client.clone(), ids.clone()).await;
            {
                let mut pending = PENDING_CONTAINERS.lock().unwrap();
                for id in &ids {
                    pending.remove(id);
                }
            }
            if let Some(name) = &stack {
                PENDING_STACKS.lock().unwrap().remove(name);
            }
            Self::refresh_data(weak_window.clone(), client).await;
            // Also covers a failed refresh, which leaves the old models in place
            let _ = weak_window.upgrade_in_event_loop(|w| Self::apply_pending(&w));
        });
    }

    /// Patches the pending image labels into the current model without rebuilding it.
    fn apply_image_pending(w: &MainWindow) {
        let pending = PENDING_IMAGES.lock().unwrap();
        let model = w.get_images_list();
        for i in 0..model.row_count() {
            if let Some(mut img) = model.row_data(i) {
                let label = pending.get(img.id.as_str()).copied().unwrap_or("");
                if img.pending != label {
                    img.pending = label.into();
                    model.set_row_data(i, img);
                }
            }
        }
    }

    /// Fetches `wslc info` for the WSLC page.
    async fn load_wslc_info(weak_window: Weak<MainWindow>, client: Arc<dyn WslcClient>) {
        let _ = weak_window.upgrade_in_event_loop(|w| w.set_wslc_info_loading(true));
        let result = client.system_info().await;
        let _ = weak_window.upgrade_in_event_loop(move |w| {
            w.set_wslc_info_loading(false);
            match result {
                Ok(i) => {
                    let sessions = match i.sessions.len() {
                        0 => "None".to_string(),
                        n => format!(
                            "{} ({})",
                            n,
                            i.sessions.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")
                        ),
                    };
                    w.set_wslc_info(WslcInfoItem {
                        version: i.version.into(),
                        session_manager: i.session_manager_version.into(),
                        sessions: sessions.into(),
                        kernel: i.kernel_version.into(),
                        windows: i.windows_version.into(),
                        direct3d: i.direct3d_version.into(),
                        dxcore: i.dxcore_version.into(),
                        settings_file: i.settings_file.into(),
                    });
                }
                Err(e) => {
                    let mut info = w.get_wslc_info();
                    info.version = format!("Unavailable: {}", e.lines().next().unwrap_or("")).into();
                    w.set_wslc_info(info);
                }
            }
        });
    }

    /// Patches the pending network labels into the current model without rebuilding it.
    fn apply_network_pending(w: &MainWindow) {
        let pending = PENDING_NETWORKS.lock().unwrap();
        let model = w.get_networks_list();
        for i in 0..model.row_count() {
            if let Some(mut net) = model.row_data(i) {
                let label = pending.get(net.name.as_str()).copied().unwrap_or("");
                if net.pending != label {
                    net.pending = label.into();
                    model.set_row_data(i, net);
                }
            }
        }
    }

    /// Patches the pending volume labels into the current model without rebuilding it.
    fn apply_volume_pending(w: &MainWindow) {
        let pending = PENDING_VOLUMES.lock().unwrap();
        let model = w.get_volumes_list();
        for i in 0..model.row_count() {
            if let Some(mut vol) = model.row_data(i) {
                let label = pending.get(vol.name.as_str()).copied().unwrap_or("");
                if vol.pending != label {
                    vol.pending = label.into();
                    model.set_row_data(i, vol);
                }
            }
        }
    }

    /// Patches the pending labels into the current models without rebuilding them.
    fn apply_pending(w: &MainWindow) {
        let pending = PENDING_CONTAINERS.lock().unwrap();
        let pending_stacks = PENDING_STACKS.lock().unwrap();
        let patch = |model: &ModelRc<ContainerItem>| {
            for i in 0..model.row_count() {
                if let Some(mut c) = model.row_data(i) {
                    let label = pending.get(c.id.as_str()).copied().unwrap_or("");
                    if c.pending != label {
                        c.pending = label.into();
                        model.set_row_data(i, c);
                    }
                }
            }
        };

        patch(&w.get_standalone_containers());
        let stacks = w.get_stacks_list();
        for i in 0..stacks.row_count() {
            if let Some(mut s) = stacks.row_data(i) {
                patch(&s.containers);
                let label = pending_stacks.get(s.name.as_str()).copied().unwrap_or("");
                if s.pending != label {
                    s.pending = label.into();
                    stacks.set_row_data(i, s);
                }
            }
        }
    }

    /// Validates and runs a single container, showing errors on the deploy
    /// page; on success goes back to the container list.
    fn deploy_spec(weak_window: &Weak<MainWindow>, client: &Arc<dyn WslcClient>, spec: ContainerSpec) {
        let Some(w) = weak_window.upgrade() else { return };
        if let Err(e) = spec.validate() {
            w.set_deploy_error(e.into());
            return;
        }
        w.set_deploy_error(SharedString::default());
        w.set_deploy_busy(true);

        let client = client.clone();
        let weak_window = weak_window.clone();
        tokio::spawn(async move {
            let result = client.run_container(&spec).await;
            let ok = result.is_ok();
            let _ = weak_window.upgrade_in_event_loop(move |w| {
                w.set_deploy_busy(false);
                match result {
                    // Back to the list, where the new container shows up
                    Ok(_) => w.set_active_tab(1),
                    Err(e) => w.set_deploy_error(format!("Deploy failed: {}", e.trim()).into()),
                }
            });
            if ok {
                Self::refresh_data(weak_window, client).await;
            }
        });
    }

    /// Appends lines to the deploy log on the UI thread.
    fn deploy_logger(weak_window: &Weak<MainWindow>) -> impl Fn(String) + Send + Sync + 'static {
        let weak_window = weak_window.clone();
        move |line: String| {
            let _ = weak_window.upgrade_in_event_loop(move |w| {
                let mut text = w.get_deploy_log().to_string();
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&line);
                w.set_deploy_log(text.into());
            });
        }
    }

    /// Mock mode only: deploys with the built-in compose translation so the
    /// offline preview never reaches the real wslc through rcompose.
    fn deploy_compose_builtin(
        weak_window: &Weak<MainWindow>,
        client: &Arc<dyn WslcClient>,
        yaml: &str,
        project: &str,
        file: &str,
    ) {
        let Some(w) = weak_window.upgrade() else { return };
        let project = match Self::parse_compose_input(yaml, project, file) {
            Ok(p) => p,
            Err(e) => {
                w.set_deploy_error(e.into());
                return;
            }
        };
        w.set_deploy_busy(true);

        let client = client.clone();
        let weak_window = weak_window.clone();
        tokio::spawn(async move {
            let log = Self::deploy_logger(&weak_window);
            log(format!("Deploying stack {}", project.name));
            let result = deploy_stack(client.as_ref(), &project, &log).await;
            match &result {
                Ok(()) => log(format!("✓ Stack {} is up", project.name)),
                Err(e) => log(format!("✗ {}", e)),
            }
            let _ = weak_window.upgrade_in_event_loop(move |w| {
                w.set_deploy_busy(false);
                if let Err(e) = result {
                    w.set_deploy_error(format!("Deploy failed: {}", e).into());
                }
            });
            Self::refresh_data(weak_window, client).await;
        });
    }

    /// What "Deploy the stack" will run, plus the services found in the file.
    pub fn compose_preview(yaml: &str, project: &str, file: &str) -> String {
        let doc: serde_yaml::Value = match serde_yaml::from_str(yaml) {
            Ok(d) => d,
            Err(e) => return format!("⚠ Invalid YAML: {}", e),
        };
        let services: Vec<String> = doc
            .get("services")
            .and_then(|s| s.as_mapping())
            .map(|m| m.keys().filter_map(|k| k.as_str().map(String::from)).collect())
            .unwrap_or_default();
        if services.is_empty() {
            return "⚠ The file has no services.".to_string();
        }

        let project = project.trim();
        let mut args = Vec::new();
        if file.is_empty() {
            let name = if project.is_empty() {
                doc.get("name").and_then(|n| n.as_str()).unwrap_or("").trim().to_string()
            } else {
                project.to_string()
            };
            if name.is_empty() {
                return "⚠ Enter a stack name, or add a top-level name: to the file.".to_string();
            }
            let path = rcompose::stacks_root().join(&name).join("compose.yaml");
            args.extend(["-f".to_string(), path.display().to_string(), "-p".to_string(), name]);
        } else {
            args.extend(["-f".to_string(), file.to_string()]);
            if !project.is_empty() {
                args.extend(["-p".to_string(), project.to_string()]);
            }
        }
        args.extend(["up".to_string(), "-d".to_string()]);

        let mut lines = vec![format!("$ rcompose {}", quote_args(&args)), String::new()];
        lines.push(format!("{} service(s):", services.len()));
        lines.extend(services.iter().map(|s| format!("  • {}", s)));
        lines.join("\n")
    }

    /// Parses the compose editor contents; `file` (if any) anchors relative
    /// paths, `.env` and the default project name.
    pub fn parse_compose_input(yaml: &str, project: &str, file: &str) -> Result<ComposeProject, String> {
        let base_dir = Some(std::path::Path::new(file))
            .filter(|_| !file.is_empty())
            .and_then(|f| f.parent());
        parse_compose(yaml, project, base_dir, &load_env(base_dir))
    }

    pub fn spec_from_form(form: &DeployForm) -> ContainerSpec {
        ContainerSpec {
            name: form.name.to_string(),
            image: form.image.to_string(),
            command: form.command.to_string(),
            ports: parse_lines(&form.ports),
            env: parse_lines(&form.env),
            volumes: parse_lines(&form.volumes),
            network: form.network.to_string(),
            cpus: form.cpus.to_string(),
            memory: form.memory.to_string(),
            pull_always: form.pull_always,
            auto_remove: form.auto_remove,
            tty: form.tty,
            start: form.start,
            ..Default::default()
        }
    }

    /// IDs of a stack's containers that are (or are not) currently running.
    fn stack_container_ids(w: &MainWindow, name: &str, running: bool) -> Vec<String> {
        w.get_stacks_list()
            .iter()
            .filter(|s| s.name == name)
            .flat_map(|s| s.containers.iter().collect::<Vec<_>>())
            .filter(|c| (c.state == "Running") == running)
            .map(|c| c.id.to_string())
            .collect()
    }
}
