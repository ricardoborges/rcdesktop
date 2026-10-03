#[derive(Debug, Clone)]
pub struct AppConfig {
    pub poll_interval_ms: u64,
    pub mock_mode: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 2500,
            mock_mode: std::env::var("RCDESKTOP_MOCK").map(|v| v == "1").unwrap_or(false),
        }
    }
}
