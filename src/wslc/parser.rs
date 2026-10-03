use crate::domain::container::{Container, ContainerState, PortMapping};
use crate::domain::image::ImageSummary;
use crate::domain::volume::VolumeSummary;
use serde_json::Value;

pub fn parse_version(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("wslc version") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 3 {
                return Some(parts[2].to_string());
            }
        }
    }
    None
}

pub fn parse_containers(raw: &str) -> Vec<Container> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // Try parsing as JSON first
    if (trimmed.starts_with('[') && trimmed.ends_with(']'))
        || (trimmed.starts_with('{') && trimmed.ends_with('}'))
    {
        if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
            if let Some(arr) = parsed.as_array() {
                return arr.iter().filter_map(parse_container_json_item).collect();
            } else if parsed.is_object() {
                if let Some(c) = parse_container_json_item(&parsed) {
                    return vec![c];
                }
            }
        }
    }

    // Fallback: tabular parser
    parse_containers_tabular(trimmed)
}

fn parse_container_json_item(val: &Value) -> Option<Container> {
    let id = val.get("Id").or_else(|| val.get("ID"))?.as_str()?.to_string();
    let names = val.get("Names")
        .and_then(|n| n.as_array())
        .map(|arr| arr.iter().filter_map(|s| s.as_str().map(String::from)).collect())
        .unwrap_or_else(|| {
            val.get("Name")
                .and_then(|n| n.as_str())
                .map(|s| vec![s.to_string()])
                .unwrap_or_default()
        });

    let image = val.get("Image").and_then(|i| i.as_str()).unwrap_or("").to_string();
    let command = val.get("Command").and_then(|c| c.as_str()).unwrap_or("").to_string();
    let status = val.get("Status").and_then(|s| s.as_str()).unwrap_or("").to_string();
    let state_str = val.get("State").and_then(|s| s.as_str()).unwrap_or(&status);
    let state = ContainerState::from_str_loose(state_str);

    let mut ports = Vec::new();
    if let Some(ports_arr) = val.get("Ports").and_then(|p| p.as_array()) {
        for p in ports_arr {
            if let (Some(pub_p), Some(priv_p)) = (
                p.get("PublicPort").and_then(|v| v.as_u64()),
                p.get("PrivatePort").and_then(|v| v.as_u64()),
            ) {
                ports.push(PortMapping {
                    host_ip: p.get("IP").and_then(|s| s.as_str()).unwrap_or("0.0.0.0").to_string(),
                    host_port: pub_p as u16,
                    container_port: priv_p as u16,
                    protocol: p.get("Type").and_then(|s| s.as_str()).unwrap_or("tcp").to_string(),
                });
            }
        }
    }

    Some(Container {
        id,
        names,
        image,
        command,
        created: "".into(),
        status,
        state,
        ports,
        compose_project: None,
    })
}

fn parse_containers_tabular(raw: &str) -> Vec<Container> {
    let lines: Vec<&str> = raw.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    let mut containers = Vec::new();
    let header_line = lines[0];
    if !header_line.contains("CONTAINER ID") && !header_line.contains("ID") {
        return Vec::new();
    }

    for line in &lines[1..] {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        let id = parts[0].to_string();
        let image = parts[1].to_string();
        let name = parts.last().unwrap_or(&"unknown").to_string();

        let mut ports = Vec::new();
        for part in &parts {
            if part.contains("->") || (part.contains('/') && part.chars().next().map_or(false, |c| c.is_ascii_digit())) {
                if let Some(pm) = PortMapping::parse(part) {
                    ports.push(pm);
                }
            }
        }

        let status_str = if trimmed.to_lowercase().contains("up ") {
            "Running"
        } else if trimmed.to_lowercase().contains("exited") {
            "Exited"
        } else {
            "Created"
        };

        containers.push(Container {
            id,
            names: vec![name],
            image,
            command: "".into(),
            created: "".into(),
            status: status_str.to_string(),
            state: ContainerState::from_str_loose(status_str),
            ports,
            compose_project: None,
        });
    }

    containers
}

pub fn parse_images(raw: &str) -> Vec<ImageSummary> {
    let lines: Vec<&str> = raw.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    let mut images = Vec::new();
    for line in &lines[1..] {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let repository = parts[0].to_string();
            let tag = parts[1].to_string();
            let id = parts[2].to_string();
            let size = parts.last().unwrap_or(&"0MB").to_string();
            images.push(ImageSummary {
                id,
                repository,
                tag,
                size,
                created_at: "".into(),
            });
        }
    }
    images
}

pub fn parse_volumes(raw: &str) -> Vec<VolumeSummary> {
    let lines: Vec<&str> = raw.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    let mut volumes = Vec::new();
    for line in &lines[1..] {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let driver = parts[0].to_string();
            let name = parts[1].to_string();
            volumes.push(VolumeSummary {
                name,
                driver,
                scope: "local".into(),
            });
        }
    }
    volumes
}
