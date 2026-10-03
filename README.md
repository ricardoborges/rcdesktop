# RC Desktop

A small Windows app for managing containers running on [WSL Containers](https://learn.microsoft.com/windows/wsl/wsl-container) (`wslc.exe`).

I wanted something like Docker Desktop or Rancher Desktop for wslc, without dragging a whole Electron app along. So this is Rust + [Slint](https://slint.dev): it starts fast, uses little memory, and just calls `wslc.exe` underneath.

It's early and built for my own use, so expect rough edges.

## What it does

- Lists containers, with compose projects grouped into stacks you can start/stop at once
- Start, stop, restart and remove containers; view logs and `inspect` output
- Opens a shell in a container through Windows Terminal
- Lists, pulls and removes images; lists volumes
- Shows the WSL session state and warns you if the app is running elevated
- Sits in the system tray, with a shortcut to `wsl --shutdown` when things get stuck

One detail worth knowing: wslc doesn't like being called concurrently. Two calls at the same time can fail with `ERROR_SHARING_VIOLATION` (0x80070020). The app runs every command through a single queue and retries with backoff when that happens.

## Running it

You need Windows 11 with wslc installed and a recent Rust toolchain (1.80+).

```powershell
cargo run --release
```

No wslc around, or just working on the UI? There's a mock mode with fake data:

```powershell
cargo run -- --mock
# or
$env:RCDESKTOP_MOCK="1"; cargo run
```

Tests:

```powershell
cargo test
```

## Code layout

- `ui/` – Slint files: the main window, theme, components and one file per view
- `src/wslc/` – everything that talks to `wslc.exe`: the command queue, output parsing and the mock client
- `src/domain/` – plain types for containers, images, volumes and the session
- `src/app.rs` – glue between the Tokio worker and the Slint event loop
- `src/tray.rs` – system tray icon

## License

MIT
