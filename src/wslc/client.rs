use async_trait::async_trait;
use crate::domain::container::Container;
use crate::domain::deploy::ContainerSpec;
use crate::domain::image::ImageSummary;
use crate::domain::volume::{NetworkSummary, VolumeSummary};
use crate::domain::session::WslcSessionInfo;
use crate::domain::system::{PruneTarget, WslcSystemInfo};

#[async_trait]
pub trait WslcClient: Send + Sync {
    /// Offline preview client: external tools (rcompose) must not touch real wslc.
    fn is_mock(&self) -> bool {
        false
    }
    async fn list_containers(&self, all: bool) -> Result<Vec<Container>, String>;
    async fn start_container(&self, id: &str) -> Result<(), String>;
    async fn stop_container(&self, id: &str) -> Result<(), String>;
    async fn restart_container(&self, id: &str) -> Result<(), String>;
    async fn remove_container(&self, id: &str) -> Result<(), String>;
    /// Creates (and, if `spec.start`, starts) a container; returns its id.
    async fn run_container(&self, spec: &ContainerSpec) -> Result<String, String>;
    async fn get_logs(&self, id: &str, tail: usize) -> Result<String, String>;
    async fn inspect_container(&self, id: &str) -> Result<String, String>;
    async fn list_images(&self) -> Result<Vec<ImageSummary>, String>;
    async fn pull_image(&self, image: &str) -> Result<(), String>;
    async fn remove_image(&self, id: &str) -> Result<(), String>;
    async fn list_volumes(&self) -> Result<Vec<VolumeSummary>, String>;
    async fn list_networks(&self) -> Result<Vec<NetworkSummary>, String>;
    async fn remove_network(&self, name: &str) -> Result<(), String>;
    async fn create_network(&self, name: &str, labels: &[String]) -> Result<(), String>;
    async fn create_volume(&self, name: &str, labels: &[String]) -> Result<(), String>;
    async fn remove_volume(&self, name: &str) -> Result<(), String>;
    async fn get_session_info(&self) -> Result<WslcSessionInfo, String>;
    async fn system_info(&self) -> Result<WslcSystemInfo, String>;
    /// Runs a prune and returns what wslc printed (removed items, reclaimed space).
    async fn prune(&self, target: PruneTarget) -> Result<String, String>;
    /// Opens the wslc settings file in the default editor.
    async fn open_settings(&self) -> Result<(), String>;
}
