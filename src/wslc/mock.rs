use std::sync::Arc;
use tokio::sync::RwLock;
use async_trait::async_trait;
use crate::domain::container::{Container, ContainerState, PortMapping};
use crate::domain::image::ImageSummary;
use crate::domain::volume::VolumeSummary;
use crate::domain::session::WslcSessionInfo;
use crate::wslc::client::WslcClient;

#[derive(Clone)]
pub struct MockWslcClient {
    containers: Arc<RwLock<Vec<Container>>>,
    images: Arc<RwLock<Vec<ImageSummary>>>,
    volumes: Arc<RwLock<Vec<VolumeSummary>>>,
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
            latency: std::time::Duration::ZERO,
        }
    }
}

#[async_trait]
impl WslcClient for MockWslcClient {
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
