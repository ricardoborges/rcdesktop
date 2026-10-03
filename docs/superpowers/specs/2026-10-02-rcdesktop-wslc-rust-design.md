# RC Desktop (Rancher Desktop clone for WSLC in Rust) - Technical Design Document

- **Date:** 2026-10-02
- **Author:** Ricardo Borges & AI Assistant
- **Target Platform:** Windows 11 (WSL Containers / `wslc.exe`)
- **Technology Stack:** Rust (Edition 2021/2024), Slint UI (Fluent Design), Tokio Async Runtime

---

## 1. Executive Summary

**RC Desktop** is a lightweight, high-performance desktop application built in Rust designed to manage WSL Containers (`wslc`) on Windows 11. It provides an intuitive GUI and system tray experience inspired by Rancher Desktop / Docker Desktop, but tailored specifically to Microsoft's native WSL container runtime.

### Key Objectives
1. **Windows 11 Native Integration:** Native Fluent Design with Dark/Light mode via Slint, system tray support, and terminal launching with Windows Terminal (`wt.exe`).
2. **Concurrency Safety:** Prevent `ERROR_SHARING_VIOLATION` (0x80070020) by enforcing a strict single-lane command queue with automatic backoff and retry.
3. **Session & Elevation Awareness:** Accurately track user elevation (normal vs. administrator) and WSL utility VM status.
4. **Developer Friendliness:** Full mock client mode (`RCDESKTOP_MOCK=1`) for offline UI testing and unit-testing without requiring an active WSL environment.

---

## 2. Architecture & Modules

The application is structured into clearly separated layers:

```
rcdesktop/
├── Cargo.toml
├── build.rs                   # Slint UI build script
├── ui/                        # Declarative Slint UI definitions
│   ├── app.slint              # Main shell with sidebar navigation
│   ├── theme.slint            # Windows 11 Fluent design tokens & styles
│   ├── components/            # Reusable UI components
│   │   ├── button.slint
│   │   ├── badge.slint
│   │   ├── modal.slint
│   │   └── status_bar.slint
│   └── views/                 # View screens
│       ├── dashboard.slint
│       ├── containers.slint
│       ├── images.slint
│       ├── volumes.slint
│       └── details.slint      # Logs and Inspect modal
├── src/
│   ├── main.rs                # Process initialization, runtime setup, entrypoint
│   ├── app.rs                 # Bridges Tokio async channel events to Slint event loop
│   ├── domain/                # Pure domain data models
│   │   ├── mod.rs
│   │   ├── container.rs       # Container, PortMapping, ContainerState
│   │   ├── image.rs           # ImageSummary, PullProgress
│   │   ├── volume.rs          # VolumeSummary, NetworkSummary
│   │   └── session.rs         # WslcSessionInfo, ElevationStatus, HealthStatus
│   ├── wslc/                  # WSLC communication & execution engine
│   │   ├── mod.rs
│   │   ├── client.rs          # WslcClient trait (RealWslcClient & MockWslcClient)
│   │   ├── queue.rs           # SingleLaneQueue actor with retry & error handling
│   │   ├── parser.rs          # Resilient parser for JSON and tabular fallback
│   │   └── mock.rs            # Deterministic mock dataset
│   ├── tray.rs                # Windows system tray integration
│   └── config.rs              # App settings, polling interval, theme preference
└── tests/
    └── parser_tests.rs        # Unit and integration tests for output parsing
```

---

## 3. Communication Engine (`wslc`) & Concurrency Model

### 3.1 The Concurrency Gate (Single-Lane Queue)
`wslc.exe` uses file-based locks for its session database. Multiple concurrent invocations fail with `ERROR_SHARING_VIOLATION`. To avoid this:
- All commands directed to `wslc.exe` are dispatched as messages to a dedicated Tokio worker task over a bounded MPSC channel.
- The worker executes exactly one command at a time.
- If an execution fails with `ERROR_SHARING_VIOLATION`, the worker applies an exponential backoff retry (e.g., 200ms, 400ms, 800ms; max 3 retries) before declaring an error.

### 3.2 Console Inheritance Mode
In GUI applications lacking an attached console window, `wslc.exe` can misbehave when accessing session handles. The engine supports:
- **Direct Mode:** Direct process spawning (`wslc.exe <args>`).
- **Cmd Fallback Mode:** Spawning through `cmd.exe /d /s /c "wslc.exe <args>"` to provide console handle inheritance.
- Auto-detection: If direct mode yields console-related sharing violations, the engine automatically switches to `cmd` mode and caches this decision.

### 3.3 Output Parsing
Output formats from `wslc.exe` are parsed with a fallback ladder:
1. Attempt JSON parsing (`--format json` or subcommands that output JSON).
2. Fall back to column-based regex parser with normalization for whitespace, status flags, and port mappings.

---

## 4. UI Specification (Slint)

### 4.1 Theme & Design
- Uses Slint's fluent styling matching Windows 11 aesthetics.
- Supports Dark mode (default) and Light mode.
- Accent colors align with modern container tools (cool blues/cyans, crisp status greens, warning ambers, error reds).

### 4.2 Views & Screens
1. **Dashboard & Session Bar:**
   - Real-time indicator: Running, Utility VM Starting, or Session Locked.
   - Session elevation indicator (Warning banner if running in Administrator context).
   - Quick counters: Running/Total Containers, Total Images, Disk Volumes.
2. **Containers View:**
   - Table displaying: Name, ID, Image, Status, Ports, and Action buttons.
   - Quick actions: Start, Stop, Restart, Remove.
   - Contextual actions: View Logs, Inspect JSON, Launch Shell in Windows Terminal (`wt.exe`).
   - Filter bar: Filter by name or status (All, Running, Stopped).
3. **Images View:**
   - Table of local container images (Repository, Tag, Size, Image ID).
   - "Pull Image" dialog with image reference input and live spinner.
   - "Run Image" wizard (container name, published ports, volume mounts, env vars).
4. **Volumes & Networks View:**
   - List and delete orphaned volumes.
   - Inspect active container networks.
5. **Logs & Inspect View:**
   - Modal drawer showing streaming or snapshot logs.
   - Formatted syntax-highlighted JSON inspection view.

---

## 5. System Tray & Lifecycle

- Minimize to tray on close (configurable via settings).
- Tray icon context menu:
  - Application Status (`WSLC: X active containers`).
  - Open Dashboard.
  - Restart WSL (`wsl --shutdown`).
  - Exit.

---

## 6. Testing & Quality Assurance

- **Mock Implementation:** Full `MockWslcClient` implementing `WslcClient` enables 100% of UI features without requiring `wslc.exe`.
- **Unit Tests:**
  - Parsing stdout of various `wslc list`, `wslc images`, and `wslc volume` versions.
  - Queue serialization: verify that concurrent requests are sequenced and results match request IDs.
  - Error translation: verify that exit codes and error messages map to user-friendly diagnostic guidance.

---

## 7. Non-Goals for V1 / MVP
- Kubernetes (k3s) cluster provisioning (reserved for v2).
- Direct Docker socket forwarding proxy daemon (reserved for v2).
- Container image building via Dockerfile directly in-app (can be invoked via terminal).
