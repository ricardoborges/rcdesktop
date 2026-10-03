use async_trait::async_trait;
use crate::domain::container::Container;
use crate::domain::image::ImageSummary;
use crate::domain::volume::VolumeSummary;
use crate::domain::session::WslcSessionInfo;

#[async_trait]
pub trait WslcClient: Send + Sync {
    async fn list_containers(&self, all: bool) -> Result<Vec<Container>, String>;
    async fn start_container(&self, id: &str) -> Result<(), String>;
    async fn stop_container(&self, id: &str) -> Result<(), String>;
    async fn restart_container(&self, id: &str) -> Result<(), String>;
    async fn remove_container(&self, id: &str) -> Result<(), String>;
    async fn get_logs(&self, id: &str, tail: usize) -> Result<String, String>;
    async fn inspect_container(&self, id: &str) -> Result<String, String>;
    async fn list_images(&self) -> Result<Vec<ImageSummary>, String>;
    async fn pull_image(&self, image: &str) -> Result<(), String>;
    async fn remove_image(&self, id: &str) -> Result<(), String>;
    async fn list_volumes(&self) -> Result<Vec<VolumeSummary>, String>;
    async fn get_session_info(&self) -> Result<WslcSessionInfo, String>;
}
