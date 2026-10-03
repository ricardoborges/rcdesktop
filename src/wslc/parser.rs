use crate::domain::container::{Container, ContainerState, PortMapping};
use crate::domain::image::ImageSummary;
use crate::domain::system::{WslcSession, WslcSystemInfo};
use crate::domain::volume::{NetworkSummary, VolumeSummary};
use serde_json::Value;

/// Accepts `wslc 3.0.1.0` (current) and `wslc version 3.0.1.0`.
pub fn parse_version(raw: &str) -> Option<String> {
    raw.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        if parts.next()? != "wslc" {
            return None;
        }
        let next = parts.next()?;
        let version = if next == "version" { parts.next()? } else { next };
        version.starts_with(|c: char| c.is_ascii_digit()).then(|| version.to_string())
    })
}

/// Parses `wslc info --format json`.
pub fn parse_system_info(raw: &str) -> Option<WslcSystemInfo> {
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    let text = |v: Option<&Value>| match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    let client = v.get("Client")?;
    let server = v.get("Server");
    let c = |k: &str| text(client.get(k));
    Some(WslcSystemInfo {
        version: c("Version"),
        kernel_version: c("KernelVersion"),
        windows_version: c("WindowsVersion"),
        direct3d_version: c("Direct3DVersion"),
        dxcore_version: c("DxCoreVersion"),
        settings_file: c("SettingsFile"),
        session_manager_version: text(server.and_then(|s| s.get("SessionManagerVersion"))),
        sessions: server
            .and_then(|s| s.get("Sessions"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|s| WslcSession {
                        id: text(s.get("ID")),
                        name: text(s.get("Name")),
                        creator_pid: text(s.get("CreatorPid")),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
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

    // JSON lines (`list --format json` prints one object per line)
    if trimmed.starts_with('{') {
        let items: Vec<Container> = trimmed
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
            .filter_map(|v| parse_container_json_item(&v))
            .collect();
        if !items.is_empty() {
            return items;
        }
    }

    // Fallback: tabular parser
    parse_containers_tabular(trimmed)
}

fn parse_container_json_item(val: &Value) -> Option<Container> {
    let id = val.get("Id").or_else(|| val.get("ID"))?.as_str()?.to_string();
    let names = match val.get("Names") {
        Some(Value::Array(arr)) => arr.iter().filter_map(|s| s.as_str().map(String::from)).collect(),
        Some(Value::String(s)) => s.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect(),
        _ => val
            .get("Name")
            .and_then(|n| n.as_str())
            .map(|s| vec![s.to_string()])
            .unwrap_or_default(),
    };

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
    } else if let Some(ports_str) = val.get("Ports").and_then(|p| p.as_str()) {
        // `list --format json` style: "127.0.0.1:9090->80/tcp, ..."
        ports.extend(ports_str.split(',').filter_map(|p| PortMapping::parse(p.trim())));
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
        compose_project: parse_compose_project(val),
    })
}

// Compose project label; `Labels` is an object (inspect style) or a
// "k=v,k=v" string (`ps --format json` style).
fn parse_compose_project(val: &Value) -> Option<String> {
    const KEY: &str = "com.docker.compose.project";
    let project = match val.get("Labels")? {
        Value::Object(map) => map.get(KEY)?.as_str()?.to_string(),
        Value::String(s) => s
            .split(',')
            .find_map(|kv| kv.trim().strip_prefix(KEY)?.strip_prefix('='))?
            .to_string(),
        _ => return None,
    };
    (!project.is_empty()).then_some(project)
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

/// Networks from `wslc network list --format json`: one JSON object per
/// line (or a JSON array). Falls back to the table (ID, NAME, DRIVER, SCOPE).
pub fn parse_networks(raw: &str) -> Vec<NetworkSummary> {
    let trimmed = raw.trim();
    let from_json = |v: &Value| {
        let field = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        Some(NetworkSummary {
            id: field("ID"),
            name: v.get("Name")?.as_str()?.to_string(),
            driver: field("Driver"),
            scope: field("Scope"),
            compose_project: parse_compose_project(v),
        })
    };

    if trimmed.starts_with('[') {
        if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(trimmed) {
            return items.iter().filter_map(from_json).collect();
        }
    }
    if trimmed.starts_with('{') {
        return trimmed
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
            .filter_map(|v| from_json(&v))
            .collect();
    }

    trimmed
        .lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            Some(NetworkSummary {
                id: cols.first()?.to_string(),
                name: cols.get(1)?.to_string(),
                driver: cols.get(2).unwrap_or(&"").to_string(),
                scope: cols.get(3).unwrap_or(&"").to_string(),
                compose_project: None,
            })
        })
        .collect()
}
