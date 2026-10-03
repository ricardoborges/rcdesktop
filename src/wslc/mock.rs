use std::sync::Arc;
use tokio::sync::RwLock;
use async_trait::async_trait;
use crate::domain::container::{Container, ContainerState, PortMapping};
use crate::domain::compose::PROJECT_LABEL;
use crate::domain::deploy::ContainerSpec;
use crate::domain::image::ImageSummary;
use crate::domain::volume::{NetworkSummary, VolumeSummary};
use crate::domain::session::WslcSessionInfo;
use crate::domain::system::{PruneTarget, WslcSession, WslcSystemInfo};
use crate::wslc::client::WslcClient;

#[derive(Clone)]
pub struct MockWslcClient {
    containers: Arc<RwLock<Vec<Container>>>,
    images: Arc<RwLock<Vec<ImageSummary>>>,
    volumes: Arc<RwLock<Vec<VolumeSummary>>>,
    networks: Arc<RwLock<Vec<NetworkSummary>>>,
    // Simulated wslc latency for lifecycle commands (zero in tests)
    latency: std::time::Duration,
}

impl Default for MockWslcClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MockWslcClient {
    pub fn with_latency(mut self, latency: std::time::Duration) -> Self {
        self.latency = latency;
        self
    }

    pub fn new() -> Self {
        let initial_containers = vec![
            Container {
                id: "c1a2b3c4d5e6".into(),
                names: vec!["frontend-web".into()],
                image: "nginx:alpine".into(),
                command: "nginx -g 'daemon off;'".into(),
                created: "2 hours ago".into(),
                status: "Running".into(),
                state: ContainerState::Running,
                ports: vec![PortMapping {
                    host_ip: "0.0.0.0".into(),
                    host_port: 8080,
                    container_port: 80,
                    protocol: "tcp".into(),
                }],
                compose_project: Some("my-stack".into()),
            },
            Container {
                id: "d2e3f4a5b6c7".into(),
                names: vec!["backend-api".into()],
                image: "python:3.12-slim".into(),
                command: "uvicorn main:app --port 8000".into(),
                created: "3 hours ago".into(),
                status: "Running".into(),
                state: ContainerState::Running,
                ports: vec![PortMapping {
                    host_ip: "127.0.0.1".into(),
                    host_port: 8000,
                    container_port: 8000,
                    protocol: "tcp".into(),
                }],
                compose_project: Some("my-stack".into()),
            },
            Container {
                id: "e3f4a5b6c7d8".into(),
                names: vec!["db-postgres".into()],
                image: "postgres:16-alpine".into(),
                command: "docker-entrypoint.sh postgres".into(),
                created: "1 day ago".into(),
                status: "Exited (0)".into(),
                state: ContainerState::Exited(0),
                ports: vec![],
                compose_project: None,
            },
        ];

        let initial_images = vec![
            ImageSummary {
                id: "img1a2b3c".into(),
                repository: "nginx".into(),
                tag: "alpine".into(),
                size: "42.5MB".into(),
                created_at: "3 days ago".into(),
            },
            ImageSummary {
                id: "img2b3c4d".into(),
                repository: "python".into(),
                tag: "3.12-slim".into(),
                size: "148MB".into(),
                created_at: "1 week ago".into(),
            },
            ImageSummary {
                id: "img3c4d5e".into(),
                repository: "postgres".into(),
                tag: "16-alpine".into(),
                size: "379MB".into(),
                created_at: "2 weeks ago".into(),
            },
        ];

        let initial_volumes = vec![
            VolumeSummary {
                name: "pgdata_volume".into(),
                driver: "local".into(),
                scope: "local".into(),
            },
            VolumeSummary {
                name: "nginx_cache".into(),
                driver: "local".into(),
                scope: "local".into(),
            },
        ];

        Self {
            containers: Arc::new(RwLock::new(initial_containers)),
            images: Arc::new(RwLock::new(initial_images)),
            volumes: Arc::new(RwLock::new(initial_volumes)),
            networks: Arc::new(RwLock::new(
                ["bridge", "host", "none"]
                    .map(|n| NetworkSummary { id: n.into(), name: n.into(), ..Default::default() })
                    .to_vec(),
            )),
            latency: std::time::Duration::ZERO,
        }
    }
}

#[async_trait]
impl WslcClient for MockWslcClient {
    fn is_mock(&self) -> bool {
        true
    }

    async fn list_containers(&self, all: bool) -> Result<Vec<Container>, String> {
        let list = self.containers.read().await;
        if all {
            Ok(list.clone())
        } else {
            Ok(list.iter().filter(|c| c.state == ContainerState::Running).cloned().collect())
        }
    }

    async fn start_container(&self, id: &str) -> Result<(), String> {
        tokio::time::sleep(self.latency).await;
        let mut list = self.containers.write().await;
        if let Some(c) = list.iter_mut().find(|c| c.id == id || c.primary_name() == id) {
            c.status = "Running".into();
            c.state = ContainerState::Running;
            Ok(())
        } else {
            Err(format!("Container not found: {}", id))
        }
    }

    async fn stop_container(&self, id: &str) -> Result<(), String> {
        tokio::time::sleep(self.latency).await;
        let mut list = self.containers.write().await;
        if let Some(c) = list.iter_mut().find(|c| c.id == id || c.primary_name() == id) {
            c.status = "Exited (0)".into();
            c.state = ContainerState::Exited(0);
            Ok(())
        } else {
            Err(format!("Container not found: {}", id))
        }
    }

    async fn restart_container(&self, id: &str) -> Result<(), String> {
        self.stop_container(id).await?;
        self.start_container(id).await
    }

    async fn remove_container(&self, id: &str) -> Result<(), String> {
        let mut list = self.containers.write().await;
        let prev_len = list.len();
        list.retain(|c| c.id != id && c.primary_name() != id);
        if list.len() < prev_len {
            Ok(())
        } else {
            Err(format!("Container not found: {}", id))
        }
    }

    async fn run_container(&self, spec: &ContainerSpec) -> Result<String, String> {
        spec.validate()?;
        tokio::time::sleep(self.latency).await;

        let image = spec.image.trim();
        let (repo, tag) = image.rsplit_once(':').unwrap_or((image, "latest"));
        let needs_pull = spec.pull_always
            || !self.images.read().await.iter().any(|i| i.repository == repo && i.tag == tag);
        if needs_pull {
            self.pull_image(image).await?;
        }

        let mut list = self.containers.write().await;
        // Counter, not list length: ids must stay unique after removals
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let n = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id = format!("{:012x}", 0xf00d_0000_0000u64 + n);
        let name = if spec.name.trim().is_empty() {
            format!("{}_{}", repo.rsplit('/').next().unwrap_or(repo), &id[8..])
        } else {
            spec.name.trim().to_string()
        };
        if list.iter().any(|c| c.primary_name() == name) {
            return Err(format!("Conflict: the container name \"{}\" is already in use", name));
        }

        let ports = spec
            .ports
            .iter()
            .filter_map(|p| {
                let p = p.split('/').next().unwrap_or(p);
                let mut parts = p.rsplit(':');
                let container_port = parts.next()?.parse().ok()?;
                let host_port = parts.next().and_then(|h| h.parse().ok()).unwrap_or(container_port);
                Some(PortMapping {
                    host_ip: parts.next().unwrap_or("0.0.0.0").into(),
                    host_port,
                    container_port,
                    protocol: "tcp".into(),
                })
            })
            .collect();

        let (status, state) = if spec.start {
            ("Running", ContainerState::Running)
        } else {
            ("Created", ContainerState::Created)
        };
        list.push(Container {
            id: id.clone(),
            names: vec![name],
            image: image.into(),
            command: spec.command.clone(),
            created: "Just now".into(),
            status: status.into(),
            state,
            ports,
            compose_project: spec
                .labels
                .iter()
                .find_map(|l| l.strip_prefix(PROJECT_LABEL)?.strip_prefix('=').map(String::from)),
        });
        Ok(id)
    }

    async fn get_logs(&self, id: &str, _tail: usize) -> Result<String, String> {
        Ok(format!(
            "[2026-10-02 23:30:00] Container {} initialized\n[2026-10-02 23:30:02] Listening on configured endpoints...\n[2026-10-02 23:30:05] Ready to serve traffic\n[2026-10-02 23:30:15] GET /health 200 OK",
            id
        ))
    }

    async fn inspect_container(&self, id: &str) -> Result<String, String> {
        let list = self.containers.read().await;
        if let Some(c) = list.iter().find(|c| c.id == id || c.primary_name() == id) {
            serde_json::to_string_pretty(c).map_err(|e| e.to_string())
        } else {
            Err(format!("Container not found: {}", id))
        }
    }

    async fn list_images(&self) -> Result<Vec<ImageSummary>, String> {
        Ok(self.images.read().await.clone())
    }

    async fn pull_image(&self, image: &str) -> Result<(), String> {
        let parts: Vec<&str> = image.split(':').collect();
        let (repo, tag) = if parts.len() == 2 {
            (parts[0], parts[1])
        } else {
            (image, "latest")
        };
        let mut imgs = self.images.write().await;
        let next_id = format!("img_{:x}", imgs.len() + 100);
        imgs.push(ImageSummary {
            id: next_id,
            repository: repo.into(),
            tag: tag.into(),
            size: "85.2MB".into(),
            created_at: "Just now".into(),
        });
        Ok(())
    }

    async fn remove_image(&self, id: &str) -> Result<(), String> {
        let mut imgs = self.images.write().await;
        let prev_len = imgs.len();
        imgs.retain(|i| i.id != id && i.full_name() != id);
        if imgs.len() < prev_len {
            Ok(())
        } else {
            Err(format!("Image not found: {}", id))
        }
    }

    async fn list_volumes(&self) -> Result<Vec<VolumeSummary>, String> {
        Ok(self.volumes.read().await.clone())
    }

    async fn list_networks(&self) -> Result<Vec<NetworkSummary>, String> {
        Ok(self.networks.read().await.clone())
    }

    async fn remove_network(&self, name: &str) -> Result<(), String> {
        let mut nets = self.networks.write().await;
        let prev_len = nets.len();
        nets.retain(|n| n.name != name);
        if nets.len() < prev_len {
            Ok(())
        } else {
            Err(format!("Network not found: {}", name))
        }
    }

    async fn create_network(&self, name: &str, labels: &[String]) -> Result<(), String> {
        let mut nets = self.networks.write().await;
        if nets.iter().any(|n| n.name == name) {
            return Err(format!("network with name {} already exists", name));
        }
        nets.push(NetworkSummary {
            id: name.into(),
            name: name.into(),
            driver: "bridge".into(),
            scope: "local".into(),
            compose_project: labels
                .iter()
                .find_map(|l| l.strip_prefix(PROJECT_LABEL)?.strip_prefix('=').map(String::from)),
        });
        Ok(())
    }

    async fn create_volume(&self, name: &str, _labels: &[String]) -> Result<(), String> {
        let mut vols = self.volumes.write().await;
        if !vols.iter().any(|v| v.name == name) {
            vols.push(VolumeSummary {
                name: name.into(),
                driver: "guest".into(),
                scope: "local".into(),
            });
        }
        Ok(())
    }

    async fn remove_volume(&self, name: &str) -> Result<(), String> {
        let mut vols = self.volumes.write().await;
        let prev_len = vols.len();
        vols.retain(|v| v.name != name);
        if vols.len() < prev_len {
            Ok(())
        } else {
            Err(format!("Volume not found: {}", name))
        }
    }

    async fn system_info(&self) -> Result<WslcSystemInfo, String> {
        Ok(WslcSystemInfo {
            version: "3.0.1.0".into(),
            kernel_version: "6.18.40.1-1".into(),
            windows_version: "10.0.26200".into(),
            direct3d_version: "1.611.1".into(),
            dxcore_version: "10.0.26100.1".into(),
            settings_file: r"C:\Users\mock\AppData\Local\wslc\settings.yaml".into(),
            session_manager_version: "3.0.1".into(),
            sessions: vec![WslcSession { id: "1".into(), name: "wslc-cli-mock".into(), creator_pid: "1234".into() }],
        })
    }

    async fn prune(&self, target: PruneTarget) -> Result<String, String> {
        tokio::time::sleep(self.latency).await;
        let removed: Vec<String> = match target {
            PruneTarget::Containers => {
                let mut items = self.containers.write().await;
                let (gone, kept) = items.drain(..).partition(|c| c.state != ContainerState::Running);
                *items = kept;
                gone.into_iter().map(|c: Container| c.primary_name().to_string()).collect()
            }
            PruneTarget::DanglingImages | PruneTarget::UnusedImages => {
                let used: Vec<String> = self.containers.read().await.iter().map(|c| c.image.clone()).collect();
                let mut items = self.images.write().await;
                let all = target == PruneTarget::UnusedImages;
                let (gone, kept) = items.drain(..).partition(|i| {
                    let dangling = i.tag.is_empty() || i.tag == "<none>";
                    (dangling || all) && !used.contains(&i.full_name())
                });
                *items = kept;
                gone.into_iter().map(|i: ImageSummary| i.full_name()).collect()
            }
            PruneTarget::Networks => {
                let mut items = self.networks.write().await;
                let (gone, kept) = items
                    .drain(..)
                    .partition(|n| !["bridge", "host", "none"].contains(&n.name.as_str()));
                *items = kept;
                gone.into_iter().map(|n: NetworkSummary| n.name).collect()
            }
            PruneTarget::Volumes => {
                let mut items = self.volumes.write().await;
                items.drain(..).map(|v| v.name).collect()
            }
        };
        if removed.is_empty() {
            Ok("Nothing to remove.".into())
        } else {
            Ok(format!("Deleted:\n{}", removed.join("\n")))
        }
    }

    async fn open_settings(&self) -> Result<(), String> {
        Ok(())
    }

    async fn get_session_info(&self) -> Result<WslcSessionInfo, String> {
        Ok(WslcSessionInfo {
            session_id: "default-wslc".into(),
            is_elevated: false,
            is_healthy: true,
            status_message: "WSLC session connected (Mock)".into(),
            version: "5.0.1.1".into(),
        })
    }
}
