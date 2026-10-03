use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageSummary {
    pub id: String,
    pub repository: String,
    pub tag: String,
    pub size: String,
    pub created_at: String,
}

impl ImageSummary {
    pub fn full_name(&self) -> String {
        format!("{}:{}", self.repository, self.tag)
    }
}
