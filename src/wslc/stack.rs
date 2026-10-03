use crate::domain::compose::ComposeProject;
use crate::domain::container::ContainerState;
use crate::wslc::client::WslcClient;

/// Brings a compose project up: creates its networks and volumes if missing,
/// then (re)creates each service's container in dependency order.
/// Progress lines go to `log`; stops at the first failure.
pub async fn deploy_project(
    client: &dyn WslcClient,
    project: &ComposeProject,
    log: impl Fn(String),
) -> Result<(), String> {
    let labels = project.labels();

    if !project.networks.is_empty() {
        let existing = client.list_networks().await?;
        for net in &project.networks {
            if existing.iter().any(|n| n.name == *net) {
                log(format!("Network {} already exists", net));
            } else {
                log(format!("Creating network {}", net));
                client
                    .create_network(net, &labels)
                    .await
                    .map_err(|e| format!("Creating network {}: {}", net, e.trim()))?;
            }
        }
    }

    if !project.volumes.is_empty() {
        let existing = client.list_volumes().await?;
        for vol in &project.volumes {
            if existing.iter().any(|v| v.name == *vol) {
                log(format!("Volume {} already exists", vol));
            } else {
                log(format!("Creating volume {}", vol));
                client
                    .create_volume(vol, &labels)
                    .await
                    .map_err(|e| format!("Creating volume {}: {}", vol, e.trim()))?;
            }
        }
    }

    let containers = client.list_containers(true).await?;
    for svc in &project.services {
        let name = &svc.spec.name;
        // Like `compose up --force-recreate`: replace the old container
        if let Some(old) = containers.iter().find(|c| c.primary_name() == name.as_str()) {
            log(format!("Recreating {}", name));
            if old.state == ContainerState::Running {
                client
                    .stop_container(&old.id)
                    .await
                    .map_err(|e| format!("Stopping {}: {}", name, e.trim()))?;
            }
            client
                .remove_container(&old.id)
                .await
                .map_err(|e| format!("Removing {}: {}", name, e.trim()))?;
        }

        log(format!("Starting {} ({})", name, svc.spec.image));
        let id = client
            .run_container(&svc.spec)
            .await
            .map_err(|e| format!("Service {}: {}", svc.name, e.trim()))?;
        log(format!("  ✓ {} started {}", svc.name, id.chars().take(12).collect::<String>()));
    }
    Ok(())
}

/// Takes a stack down like `compose down`: stops and removes its containers,
/// then removes the networks it created. Volumes are kept, so data survives.
/// Returns how many containers were removed.
pub async fn remove_project(
    client: &dyn WslcClient,
    project: &str,
    log: impl Fn(String),
) -> Result<usize, String> {
    let containers: Vec<_> = client
        .list_containers(true)
        .await?
        .into_iter()
        .filter(|c| c.compose_project.as_deref() == Some(project))
        .collect();

    for c in &containers {
        let name = c.primary_name();
        if c.state == ContainerState::Running {
            log(format!("Stopping {}", name));
            client
                .stop_container(&c.id)
                .await
                .map_err(|e| format!("Stopping {}: {}", name, e.trim()))?;
        }
        log(format!("Removing {}", name));
        client
            .remove_container(&c.id)
            .await
            .map_err(|e| format!("Removing {}: {}", name, e.trim()))?;
    }

    for net in client.list_networks().await? {
        if net.compose_project.as_deref() == Some(project) {
            log(format!("Removing network {}", net.name));
            client
                .remove_network(&net.name)
                .await
                .map_err(|e| format!("Removing network {}: {}", net.name, e.trim()))?;
        }
    }
    Ok(containers.len())
}
