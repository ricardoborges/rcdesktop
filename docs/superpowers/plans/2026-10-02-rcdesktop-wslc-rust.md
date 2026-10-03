# RC Desktop (Rancher Desktop clone for WSLC in Rust) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a native Windows 11 desktop application in Rust + Slint that replicates the core capabilities of Rancher Desktop / Docker Desktop for Windows WSL Containers (`wslc`).

**Architecture:** Clean architecture with domain models, a single-lane actor queue (`SingleLaneQueue`) managing `wslc.exe` invocations to prevent `ERROR_SHARING_VIOLATION`, a resilient output parser, native Slint UI with Fluent theme, and Windows tray integration.

**Tech Stack:** Rust 2021, Slint UI 1.9+, Tokio, Serde, Tray-icon, Windows-sys.

**Spec:** [docs/superpowers/specs/2026-10-02-rcdesktop-wslc-rust-design.md](file:///d:/dev/github/ricardoborges/rcdesktop/docs/superpowers/specs/2026-10-02-rcdesktop-wslc-rust-design.md)

## Global Constraints

- OS: Windows 11 x64, PowerShell shell.
- Toolchain: `cargo` / `rustc` 1.92+.
- `wslc` location: `C:\Program Files\WSL\wslc.exe` or resolved via PATH.
- No concurrent `wslc` calls — all commands must funnel through the single-lane queue.
- Support `RCDESKTOP_MOCK=1` environment variable for development without active WSL containers.

---

### Task 1: Scaffolding, Cargo.toml & Slint Build Setup

**Files:**
- Create: `Cargo.toml`
- Create: `build.rs`
- Create: `.gitignore`
- Create: `src/main.rs`
- Create: `ui/app.slint`

**Interfaces:**
- Produces: Compilable Rust project with Slint build step and minimal running window.

- [ ] **Step 1: Create .gitignore**

```gitignore
/target
Cargo.lock
**/*.rs.bk
.idea
.vscode
```

- [ ] **Step 2: Create Cargo.toml**

```toml
[package]
name = "rcdesktop"
version = "0.1.0"
edition = "2021"
authors = ["Ricardo Borges"]
description = "Rancher Desktop clone for Windows 11 WSL Containers (WSLC) in Rust"

[dependencies]
slint = { version = "1.9", default-features = true, features = ["compat-1-0", "backend-winit"] }
tokio = { version = "1.43", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
regex = "1.11"
tray-icon = "0.19"
windows-sys = { version = "0.59", features = [
    "Win32_Security",
    "Win32_System_Threading",
    "Win32_Foundation",
    "Win32_UI_WindowsAndMessaging"
] }

[build-dependencies]
slint-build = "1.9"
```

- [ ] **Step 3: Create build.rs**

```rust
fn main() {
    slint_build::compile("ui/app.slint").expect("Slint build failed");
}
```

- [ ] **Step 4: Create initial ui/app.slint**

```slint
import { Button } from "std-widgets.slint";

export component MainWindow inherits Window {
    title: "RC Desktop - WSLC Manager";
    min-width: 900px;
    min-height: 600px;
    preferred-width: 1024px;
    preferred-height: 720px;

    Text {
        text: "RC Desktop (WSLC)";
        font-size: 24px;
        horizontal-alignment: center;
        vertical-alignment: center;
    }
}
```

- [ ] **Step 5: Create minimal src/main.rs**

```rust
slint::include_modules!();

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let main_window = MainWindow::new()?;
    main_window.run()?;
    Ok(())
}
```

- [ ] **Step 6: Verify compilation and run test**

Run: `cargo check`
Expected: PASS with 0 errors.

- [ ] **Step 7: Commit**

```powershell
git add .gitignore Cargo.toml build.rs ui/app.slint src/main.rs
git commit -m "chore: scaffold rcdesktop rust project with slint build"
```

---

### Task 2: Domain Models

**Files:**
- Create: `src/domain/mod.rs`
- Create: `src/domain/container.rs`
- Create: `src/domain/image.rs`
- Create: `src/domain/volume.rs`
- Create: `src/domain/session.rs`
- Test: `tests/domain_tests.rs`

**Interfaces:**
- Produces:
  - `Container`: `id`, `name`, `image`, `status`, `ports`, `created`
  - `ImageSummary`: `id`, `repository`, `tag`, `size`, `created`
  - `VolumeSummary`: `name`, `driver`, `scope`
  - `NetworkSummary`: `id`, `name`, `driver`
  - `WslcSessionInfo`: `session_id`, `is_elevated`, `is_healthy`, `status_message`, `version`

- [ ] **Step 1: Write domain tests**

Create `tests/domain_tests.rs`:
```rust
use rcdesktop::domain::container::{Container, ContainerState, PortMapping};
use rcdesktop::domain::session::WslcSessionInfo;

#[test]
fn test_container_state_parsing() {
    let state = ContainerState::from_str_loose("Up 2 hours");
    assert_eq!(state, ContainerState::Running);

    let state = ContainerState::from_str_loose("Exited (0) 10 minutes ago");
    assert_eq!(state, ContainerState::Exited(0));

    let state = ContainerState::from_str_loose("Created");
    assert_eq!(state, ContainerState::Created);
}

#[test]
fn test_port_mapping_parse() {
    let port = PortMapping::parse("0.0.0.0:8080->80/tcp");
    assert!(port.is_some());
    let p = port.unwrap();
    assert_eq!(p.host_port, 8080);
    assert_eq!(p.container_port, 80);
}

#[test]
fn test_session_elevation_warning() {
    let s = WslcSessionInfo {
        session_id: "default".into(),
        is_elevated: true,
        is_healthy: true,
        status_message: "Active".into(),
        version: "5.0.1.1".into(),
    };
    assert!(s.is_elevated);
}
```

- [ ] **Step 2: Update Cargo.toml to expose lib**

Add to `Cargo.toml`:
```toml
[lib]
name = "rcdesktop"
path = "src/lib.rs"

[[bin]]
name = "rcdesktop"
path = "src/main.rs"
```

Create `src/lib.rs`:
```rust
pub mod domain;
pub mod wslc;
pub mod config;
```

- [ ] **Step 3: Implement domain models**

Create `src/domain/container.rs`:
```rust
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
        // Formats like: 0.0.0.0:8080->80/tcp or 8080:80/tcp or 8080->80/tcp
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
```

Create `src/domain/image.rs`:
```rust
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
```

Create `src/domain/volume.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeSummary {
    pub name: String,
    pub driver: String,
    pub scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkSummary {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
}
```

Create `src/domain/session.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WslcSessionInfo {
    pub session_id: String,
    pub is_elevated: bool,
    pub is_healthy: bool,
    pub status_message: String,
    pub version: String,
}

impl Default for WslcSessionInfo {
    fn default() -> Self {
        Self {
            session_id: "unknown".into(),
            is_elevated: false,
            is_healthy: false,
            status_message: "Initializing...".into(),
            version: "unknown".into(),
        }
    }
}
```

Create `src/domain/mod.rs`:
```rust
pub mod container;
pub mod image;
pub mod volume;
pub mod session;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test domain_tests`
Expected: PASS (3 passed, 0 failed).

- [ ] **Step 5: Commit**

```powershell
git add src/domain/ tests/domain_tests.rs src/lib.rs Cargo.toml
git commit -m "feat(domain): implement core models for container, image, volume, session"
```

---

### Task 3: Parser Layer for `wslc` Output

**Files:**
- Create: `src/wslc/parser.rs`
- Test: `tests/parser_tests.rs`

**Interfaces:**
- Consumes: Raw `stdout`/`stderr` strings from `wslc.exe`.
- Produces:
  - `parse_containers(output: &str) -> Vec<Container>`
  - `parse_images(output: &str) -> Vec<ImageSummary>`
  - `parse_volumes(output: &str) -> Vec<VolumeSummary>`
  - `parse_version(output: &str) -> Option<String>`
  - `parse_session_info(output: &str) -> WslcSessionInfo`

- [ ] **Step 1: Write parser tests with sample wslc outputs**

Create `tests/parser_tests.rs`:
```rust
use rcdesktop::domain::container::ContainerState;
use rcdesktop::wslc::parser::{parse_containers, parse_images, parse_version};

#[test]
fn test_parse_containers_json() {
    let json_output = r#"[
        {
            "Id": "c1a2b3c4d5e6",
            "Names": ["/web-server"],
            "Image": "nginx:alpine",
            "Command": "nginx -g 'daemon off;'",
            "Created": 1710000000,
            "State": "running",
            "Status": "Up 2 hours",
            "Ports": [{"PublicPort": 8080, "PrivatePort": 80, "Type": "tcp", "IP": "0.0.0.0"}]
        }
    ]"#;
    let list = parse_containers(json_output);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "c1a2b3c4d5e6");
    assert_eq!(list[0].primary_name(), "web-server");
    assert_eq!(list[0].state, ContainerState::Running);
    assert_eq!(list[0].ports.len(), 1);
    assert_eq!(list[0].ports[0].host_port, 8080);
}

#[test]
fn test_parse_containers_tabular() {
    let tabular_output = "CONTAINER ID   IMAGE          COMMAND                  CREATED         STATUS         PORTS                  NAMES\n\
c1a2b3c4d5e6   nginx:alpine   \"nginx -g 'daemon of…\"   2 hours ago     Up 2 hours     0.0.0.0:8080->80/tcp   web-server\n\
f9e8d7c6b5a4   redis:7        \"docker-entrypoint.s…\"   5 hours ago     Exited (0)     6379/tcp               cache-db\n";
    let list = parse_containers(tabular_output);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, "c1a2b3c4d5e6");
    assert_eq!(list[0].image, "nginx:alpine");
    assert_eq!(list[0].primary_name(), "web-server");
    assert_eq!(list[0].state, ContainerState::Running);
    assert_eq!(list[1].id, "f9e8d7c6b5a4");
    assert_eq!(list[1].state, ContainerState::Exited(0));
}

#[test]
fn test_parse_version() {
    let out = "wslc version 5.0.1.1\ncommit: deadbeef\n";
    assert_eq!(parse_version(out), Some("5.0.1.1".to_string()));
}

#[test]
fn test_parse_images_tabular() {
    let out = "REPOSITORY   TAG       IMAGE ID       CREATED        SIZE\n\
nginx        alpine    9a8b7c6d5e4f   3 days ago     42.5MB\n\
redis        latest    1a2b3c4d5e6f   2 weeks ago    117MB\n";
    let images = parse_images(out);
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].repository, "nginx");
    assert_eq!(images[0].tag, "alpine");
    assert_eq!(images[0].size, "42.5MB");
}
```

- [ ] **Step 2: Implement parser logic in src/wslc/parser.rs**

Create `src/wslc/parser.rs`:
```rust
use crate::domain::container::{Container, ContainerState, PortMapping};
use crate::domain::image::ImageSummary;
use crate::domain::volume::VolumeSummary;
use crate::domain::session::WslcSessionInfo;
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

    // Fallback: tabular regex
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
            if let (Some(pub_p), Some(priv_p)) = (p.get("PublicPort").and_then(|v| v.as_u64()), p.get("PrivatePort").and_then(|v| v.as_u64())) {
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
```

Create `src/wslc/mod.rs`:
```rust
pub mod parser;
pub mod client;
pub mod queue;
pub mod mock;
```

- [ ] **Step 3: Run parser tests**

Run: `cargo test --test parser_tests`
Expected: PASS (4 passed, 0 failed).

- [ ] **Step 4: Commit**

```powershell
git add src/wslc/parser.rs src/wslc/mod.rs tests/parser_tests.rs
git commit -m "feat(wslc): add resilient output parsers for containers, images, and version"
```

---

### Task 4: SingleLaneQueue & WslcClient with Retry Logic

**Files:**
- Create: `src/wslc/client.rs`
- Create: `src/wslc/queue.rs`
- Create: `src/wslc/mock.rs`
- Test: `tests/queue_tests.rs`

**Interfaces:**
- Produces:
  - `trait WslcClient: Send + Sync` with async methods:
    - `list_containers(&self, all: bool) -> Result<Vec<Container>, String>`
    - `start_container(&self, id: &str) -> Result<(), String>`
    - `stop_container(&self, id: &str) -> Result<(), String>`
    - `restart_container(&self, id: &str) -> Result<(), String>`
    - `remove_container(&self, id: &str, force: bool) -> Result<(), String>`
    - `get_logs(&self, id: &str, tail: usize) -> Result<String, String>`
    - `inspect_container(&self, id: &str) -> Result<String, String>`
    - `list_images(&self) -> Result<Vec<ImageSummary>, String>`
    - `pull_image(&self, image: &str) -> Result<(), String>`
    - `remove_image(&self, id: &str) -> Result<(), String>`
    - `list_volumes(&self) -> Result<Vec<VolumeSummary>, String>`
    - `get_session_info(&self) -> Result<WslcSessionInfo, String>`

- [ ] **Step 1: Write queue tests**

Create `tests/queue_tests.rs`:
```rust
use rcdesktop::wslc::mock::MockWslcClient;
use rcdesktop::wslc::client::WslcClient;

#[tokio::test]
async fn test_mock_client_containers_crud() {
    let client = MockWslcClient::new();
    let initial = client.list_containers(true).await.expect("List failed");
    assert!(!initial.is_empty());

    let target_id = &initial[0].id;
    client.stop_container(target_id).await.expect("Stop failed");

    let updated = client.list_containers(true).await.expect("List failed");
    let stopped = updated.iter().find(|c| &c.id == target_id).unwrap();
    assert_eq!(stopped.status, "Exited");

    client.start_container(target_id).await.expect("Start failed");
    let running_list = client.list_containers(true).await.expect("List failed");
    let started = running_list.iter().find(|c| &c.id == target_id).unwrap();
    assert_eq!(started.status, "Running");
}

#[tokio::test]
async fn test_mock_client_images_pull_and_remove() {
    let client = MockWslcClient::new();
    client.pull_image("redis:7-alpine").await.expect("Pull failed");

    let images = client.list_images().await.expect("Images failed");
    assert!(images.iter().any(|i| i.repository == "redis" && i.tag == "7-alpine"));

    let target = images.iter().find(|i| i.repository == "redis").unwrap();
    client.remove_image(&target.id).await.expect("Remove failed");

    let after_removal = client.list_images().await.expect("Images failed");
    assert!(!after_removal.iter().any(|i| i.id == target.id));
}
```

- [ ] **Step 2: Implement WslcClient trait in src/wslc/client.rs**

Create `src/wslc/client.rs`:
```rust
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
    async fn remove_container(&self, id: &str, force: bool) -> Result<(), String>;
    async fn get_logs(&self, id: &str, tail: usize) -> Result<String, String>;
    async fn inspect_container(&self, id: &str) -> Result<String, String>;
    async fn list_images(&self) -> Result<Vec<ImageSummary>, String>;
    async fn pull_image(&self, image: &str) -> Result<(), String>;
    async fn remove_image(&self, id: &str) -> Result<(), String>;
    async fn list_volumes(&self) -> Result<Vec<VolumeSummary>, String>;
    async fn get_session_info(&self) -> Result<WslcSessionInfo, String>;
}
```
*(Add `async-trait = "0.1"` to Cargo.toml)*

- [ ] **Step 3: Implement MockWslcClient in src/wslc/mock.rs**

Create `src/wslc/mock.rs`:
```rust
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
}

impl MockWslcClient {
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
        let mut list = self.containers.write().await;
        if let Some(c) = list.iter_mut().find(|c| c.id == id || c.primary_name() == id) {
            c.status = "Exited".into();
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

    async fn remove_container(&self, id: &str, _force: bool) -> Result<(), String> {
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
            "[2026-10-02 23:25:01] Container {} listening on port...\n[2026-10-02 23:25:05] Ready to accept connections\n[2026-10-02 23:25:10] GET /health 200 OK",
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
        imgs.push(ImageSummary {
            id: format!("img_{:x}", imgs.len() + 100),
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
```

- [ ] **Step 4: Implement SingleLaneQueue & RealWslcClient in src/wslc/queue.rs**

Create `src/wslc/queue.rs` implementing serialized commands to `wslc.exe`:
- MPSC worker processing one `WslcRequest` at a time.
- Automatic backoff on `ERROR_SHARING_VIOLATION`.
- Fallback execution through `cmd.exe /d /s /c` if direct console handles fail.
- Output parsed via `crate::wslc::parser`.

- [ ] **Step 5: Run tests**

Run: `cargo test --test queue_tests`
Expected: PASS (2 passed, 0 failed).

- [ ] **Step 6: Commit**

```powershell
git add src/wslc/ Cargo.toml tests/queue_tests.rs
git commit -m "feat(wslc): implement single lane queue, mock client, and real wslc client"
```

---

### Task 5: Slint UI Theme & Component Library

**Files:**
- Create: `ui/theme.slint`
- Create: `ui/components/badge.slint`
- Create: `ui/components/button.slint`
- Create: `ui/components/status_bar.slint`
- Create: `ui/components/sidebar.slint`

**Interfaces:**
- Produces: Fluent-styled UI components matching Windows 11 dark/light aesthetic.

- [ ] **Step 1: Create ui/theme.slint**

Define colors (`bg-primary`, `bg-card`, `accent-blue`, `text-primary`, `text-secondary`, `status-green`, `status-red`, `border-color`), typography, and spacing tokens.

- [ ] **Step 2: Create badge.slint and button.slint**

Provide rounded status badges (`Running`, `Exited`, `Created`) and styled action buttons with hover and active states.

- [ ] **Step 3: Create status_bar.slint**

Displays:
- Connection dot (🟢 Healthy / 🟡 Starting / 🔴 Error)
- Session name & version tag
- Elevation alert badge if running elevated
- Refresh indicator

- [ ] **Step 4: Create sidebar.slint**

Navigation items with icons/glyphs:
- 📊 Dashboard
- 📦 Containers
- 🖼️ Images
- 💾 Volumes
- ⚙️ Diagnostics

- [ ] **Step 5: Verify build with `cargo check`**

Run: `cargo check`
Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add ui/
git commit -m "feat(ui): add fluent theme and reusable slint components"
```

---

### Task 6: Slint Views (Dashboard, Containers, Images, Volumes, Logs)

**Files:**
- Create: `ui/views/dashboard.slint`
- Create: `ui/views/containers.slint`
- Create: `ui/views/images.slint`
- Create: `ui/views/volumes.slint`
- Create: `ui/views/details.slint`
- Modify: `ui/app.slint`

**Interfaces:**
- Exposes properties and callbacks to Rust:
  - `in-out property <[ContainerItem]> containers-list`
  - `in-out property <[ImageItem]> images-list`
  - `in-out property <[VolumeItem]> volumes-list`
  - `in-out property <SessionInfoItem> session-info`
  - `callback start-container(string)`
  - `callback stop-container(string)`
  - `callback restart-container(string)`
  - `callback remove-container(string)`
  - `callback view-logs(string)`
  - `callback inspect-container(string)`
  - `callback pull-image(string)`
  - `callback remove-image(string)`
  - `callback refresh-all()`

- [ ] **Step 1: Implement containers view with action buttons**
- [ ] **Step 2: Implement images view with pull dialog**
- [ ] **Step 3: Implement volumes view**
- [ ] **Step 4: Implement dashboard summary view**
- [ ] **Step 5: Implement details/logs modal**
- [ ] **Step 6: Assemble all views into ui/app.slint**
- [ ] **Step 7: Verify compile with `cargo check`**

Run: `cargo check`
Expected: PASS.

- [ ] **Step 8: Commit**

```powershell
git add ui/
git commit -m "feat(ui): implement all view screens and dialogs in slint"
```

---

### Task 7: Controller & Background Event Loop Integration

**Files:**
- Create: `src/app.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `WslcClient` (either `MockWslcClient` if `RCDESKTOP_MOCK=1` or `RealWslcClient`).
- Produces: Slint event loop controller with reactive UI updates and periodic polling.

- [ ] **Step 1: Create src/app.rs bridging Slint and WslcClient**
- [ ] **Step 2: Wire UI callbacks (start, stop, pull, logs) to async calls**
- [ ] **Step 3: Implement periodic polling with backoff**
- [ ] **Step 4: Run application in Mock mode**

Run: `$env:RCDESKTOP_MOCK="1"; cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```powershell
git add src/app.rs src/main.rs
git commit -m "feat(app): bridge slint event loop with async wslc background worker"
```

---

### Task 8: Windows System Tray & Shell Integration

**Files:**
- Create: `src/tray.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `tray-icon` crate.
- Produces:
  - System Tray icon on Windows 11 taskbar notification area.
  - Context menu: "Open RC Desktop", "WSLC Status", "Restart WSL", "Quit".
  - Double-click restores window from minimized state.

- [ ] **Step 1: Implement tray initialization in src/tray.rs**
- [ ] **Step 2: Handle tray events in event loop**
- [ ] **Step 3: Add Windows Terminal integration (wt.exe) for container shell exec**
- [ ] **Step 4: Verify build on Windows**

Run: `cargo check`
Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/tray.rs src/main.rs
git commit -m "feat(tray): implement windows system tray and terminal launcher"
```

---

### Task 9: Verification, End-to-End Test & Smoke Check

**Files:**
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Run complete automated test suite**

Run: `cargo test`
Expected: All unit and integration tests PASS.

- [ ] **Step 2: Smoke test Mock execution**

Run: `cargo build`
Expected: Build successfully creates `target/debug/rcdesktop.exe`.

- [ ] **Step 3: Commit and final documentation update**

```powershell
git add tests/ README.md
git commit -m "docs: finalize v1 implementation and usage instructions"
```
