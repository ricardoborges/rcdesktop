use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerState {
    Running,
    Exited(i32),
    Created,
    Restarting,
    Paused,
    Unknown(String),
}

impl ContainerState {
    pub fn from_str_loose(s: &str) -> Self {
        let lower = s.to_lowercase();
        if lower.starts_with("up") || lower.contains("running") {
            ContainerState::Running
        } else if lower.starts_with("exited") {
            let code = s.split('(')
                .nth(1)
                .and_then(|p| p.split(')').next())
                .and_then(|c| c.parse::<i32>().ok())
                .unwrap_or(0);
            ContainerState::Exited(code)
        } else if lower.starts_with("created") {
            ContainerState::Created
        } else if lower.starts_with("restarting") {
            ContainerState::Restarting
        } else if lower.starts_with("paused") {
            ContainerState::Paused
        } else {
            ContainerState::Unknown(s.to_string())
        }
    }

    pub fn as_display_str(&self) -> &str {
        match self {
            ContainerState::Running => "Running",
            ContainerState::Exited(_) => "Exited",
            ContainerState::Created => "Created",
            ContainerState::Restarting => "Restarting",
            ContainerState::Paused => "Paused",
            ContainerState::Unknown(s) => s.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortMapping {
    pub host_ip: String,
    pub host_port: u16,
    pub container_port: u16,
    pub protocol: String,
}

impl PortMapping {
    pub fn parse(s: &str) -> Option<Self> {
        // Formats: 0.0.0.0:8080->80/tcp or 8080:80/tcp or 8080->80/tcp
        let parts: Vec<&str> = s.split("->").collect();
        if parts.len() != 2 {
            return None;
        }
        let host_part = parts[0].trim();
        let cont_part = parts[1].trim();

        let (host_ip, host_port_str) = if let Some(idx) = host_part.rfind(':') {
            (&host_part[..idx], &host_part[idx + 1..])
        } else {
            ("0.0.0.0", host_part)
        };
        let host_port: u16 = host_port_str.parse().ok()?;

        let (cont_port_str, protocol) = if let Some(idx) = cont_part.find('/') {
            (&cont_part[..idx], &cont_part[idx + 1..])
        } else {
            (cont_part, "tcp")
        };
        let container_port: u16 = cont_port_str.parse().ok()?;

        Some(PortMapping {
            host_ip: host_ip.to_string(),
            host_port,
            container_port,
            protocol: protocol.to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Container {
    pub id: String,
    pub names: Vec<String>,
    pub image: String,
    pub command: String,
    pub created: String,
    pub status: String,
    pub state: ContainerState,
    pub ports: Vec<PortMapping>,
    pub compose_project: Option<String>,
}

impl Container {
    pub fn primary_name(&self) -> &str {
        self.names.first().map(|s| s.trim_start_matches('/')).unwrap_or(&self.id)
    }
}
