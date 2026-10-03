use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::future::Future;
use std::time::Duration;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel, Weak};

use crate::domain::container::ContainerState;
use crate::wslc::client::WslcClient;

// The Slint-generated types are made available via crate's include_modules or module imports.
use crate::{ContainerItem, ImageItem, MainWindow, SessionInfoItem, StackItem, VolumeItem};

// Stacks the user collapsed; survives the periodic model rebuilds.
static COLLAPSED_STACKS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

// In-flight lifecycle operations ("Starting…" etc.), keyed by container id / stack name.
static PENDING_CONTAINERS: Mutex<BTreeMap<String, &'static str>> = Mutex::new(BTreeMap::new());
static PENDING_STACKS: Mutex<BTreeMap<String, &'static str>> = Mutex::new(BTreeMap::new());

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

                let pending = PENDING_CONTAINERS.lock().unwrap().clone();
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
                        let stack_str = c.compose_project.unwrap_or_default();
                        let pending_str = pending.get(&c.id).copied().unwrap_or("");
                        let id_str = c.id;

                        ContainerItem {
                            id: SharedString::from(id_str),
                            name: SharedString::from(name_str),
                            image: SharedString::from(c.image),
                            status: SharedString::from(c.status),
                            state: SharedString::from(state_str),
                            ports: SharedString::from(ports_str),
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
