# RC Desktop

**RC Desktop** is a native Windows 11 desktop application written in **Rust** and **Slint**, designed as a lightweight, fluid alternative to Rancher Desktop / Docker Desktop for Microsoft's new [WSL Containers (WSLC)](https://learn.microsoft.com/windows/wsl/wsl-container) runtime.

---

## ✨ Features

- **Containers Management:** Live overview of running and stopped containers, one-click start/stop/restart/delete, live logs viewer, JSON inspect viewer, and direct shell attachment via Windows Terminal (`wt.exe`).
- **Images:** Local image repository overview, OCI image pull (`pull <image>:<tag>`), and image deletion.
- **Volumes & Networks:** Inspection of persistent storage volumes and networks managed by WSLC.
- **Single-Lane Command Queue:** Native `wslc.exe` concurrency protection using an asynchronous actor queue in Tokio. Prevents machine-wide `ERROR_SHARING_VIOLATION` (0x80070020) session store corruption with exponential backoff and retry.
- **Windows 11 Fluent UI:** Native styling using Slint's Fluent theme tokens with dark mode, responsive layouts, and zero webview/Electron overhead.
- **Session & Elevation Monitor:** Real-time visibility into the WSL utility VM status, elevation level detection (warning if running elevated), and 1-click diagnostic recovery (`wsl --shutdown`).
- **Windows System Tray:** Persistent tray icon in the Windows taskbar with quick status, WSL restart shortcut, and background management.
- **Mock Mode:** Full offline simulation mode (`--mock` or `RCDESKTOP_MOCK=1`) for local development without active WSL containers.

---

## 🚀 Getting Started

### Prerequisites
- Windows 11 with the [WSL container preview](https://learn.microsoft.com/windows/wsl/wsl-container) (`wslc.exe`).
- Rust 1.80+ (toolchain installed via `rustup`).

### Running Live with `wslc`
To launch against your local Windows 11 WSL container runtime:

```powershell
cargo run --release
```

### Running in Mock Mode (Offline Development)
To test and experiment with all UI features without needing active containers:

```powershell
cargo run -- --mock
# or
$env:RCDESKTOP_MOCK="1"; cargo run
```

---

## 🧪 Running Tests

The test suite validates domain models, regex/JSON output parsers, the single-lane queue, and end-to-end container workflows:

```powershell
cargo test
```

---

## 🏛️ Architecture

```
rcdesktop/
├── build.rs                   # Slint UI compiler script
├── ui/                        # Declarative native UI (Slint)
│   ├── app.slint              # Main window & view router
│   ├── theme.slint            # Windows 11 Fluent tokens
│   ├── components/            # Reusable widgets (sidebar, status bar, badges, buttons)
│   └── views/                 # View screens (dashboard, containers, images, volumes, diagnostics, details)
├── src/
│   ├── main.rs                # App entrypoint and runtime initialization
│   ├── app.rs                 # Bridge between Tokio background tasks and Slint event loop
│   ├── domain/                # Core domain entities (Container, Image, Volume, Session)
│   ├── wslc/                  # wslc.exe communication, single-lane queue, parser, and mock engine
│   ├── tray.rs                # Windows system tray integration
│   └── config.rs              # App configuration & mock detection
└── tests/                     # Automated unit and integration test suite
```

---

## 📄 License
MIT
