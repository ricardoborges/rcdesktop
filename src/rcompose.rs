//! Integration with `rcompose`, the Compose implementation for wslc that ships
//! alongside RC Desktop. Stacks are deployed by running `rcompose up -d`.
//!
//! The binary is looked up next to `rcdesktop.exe` first (the bundled copy),
//! then in `RCOMPOSE_BIN`, the per-user install folder and `PATH`. When it is
//! missing, [`install`] fetches the latest release from GitHub into the same
//! folder the official `install.ps1` uses.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;

const EXE: &str = "rcompose.exe";
const INSTALL_SCRIPT: &str = include_str!("rcompose_install.ps1");
// Keeps rcompose/powershell from flashing a console window
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Per-user install folder, same as the official installer.
pub fn install_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs").join("rcompose"))
}

/// Finds rcompose.exe, preferring the copy bundled with RC Desktop.
pub fn locate() -> Option<PathBuf> {
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(EXE)));
    let from_env = std::env::var_os("RCOMPOSE_BIN").map(PathBuf::from);
    let installed = install_dir().map(|d| d.join(EXE));
    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join(EXE)).collect::<Vec<_>>())
        .unwrap_or_default();

    bundled
        .into_iter()
        .chain(from_env)
        .chain(installed)
        .chain(on_path)
        .find(|p| p.is_file())
}

/// Downloads and installs the latest rcompose release (checksum verified).
/// Progress lines go to `log`. Returns the installed binary.
pub async fn install(log: impl Fn(String)) -> Result<PathBuf, String> {
    let dir = install_dir().ok_or("LOCALAPPDATA is not set")?;
    let script = std::env::temp_dir().join(format!("rcdesktop-install-rcompose-{}.ps1", std::process::id()));
    std::fs::write(&script, INSTALL_SCRIPT).map_err(|e| format!("Cannot write installer: {}", e))?;

    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .env("RCOMPOSE_INSTALL_DIR", &dir);
    let result = run_streaming(cmd, &log).await;
    let _ = std::fs::remove_file(&script);
    result.map_err(|e| format!("Installing rcompose failed: {}", e))?;

    let exe = dir.join(EXE);
    if exe.is_file() {
        Ok(exe)
    } else {
        Err(format!("Installer finished but {} was not found", exe.display()))
    }
}

/// Where and how to run `rcompose` for the compose editor contents.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposeTarget {
    /// Compose file passed with `-f`
    pub file: PathBuf,
    /// Working directory; relative paths and `.env` resolve from here
    pub dir: PathBuf,
    /// `-p` value, when one was given or had to be chosen
    pub project: Option<String>,
    /// File written only for this run, removed afterwards
    pub temporary: bool,
}

impl ComposeTarget {
    /// Picks the file rcompose should read:
    /// - opened from disk and unchanged: that file, in place;
    /// - opened from disk and edited: a hidden copy next to it, so relative
    ///   paths and `.env` still resolve, removed after the run;
    /// - typed or pasted: saved under `stacks_root/<project>/compose.yaml`.
    pub fn prepare(yaml: &str, project: &str, file: &str, stacks_root: &Path) -> Result<Self, String> {
        let project = project.trim();
        let project = (!project.is_empty()).then(|| project.to_string());

        if !file.is_empty() {
            let original = PathBuf::from(file);
            let dir = original.parent().map(Path::to_path_buf).unwrap_or_default();
            let unchanged = std::fs::read_to_string(&original).map(|t| t == yaml).unwrap_or(false);
            if unchanged {
                return Ok(Self { file: original, dir, project, temporary: false });
            }
            let stem = original.file_stem().and_then(|s| s.to_str()).unwrap_or("compose");
            let copy = dir.join(format!(".{}.rcdesktop.yaml", stem));
            std::fs::write(&copy, yaml).map_err(|e| format!("Cannot write {}: {}", copy.display(), e))?;
            // The copy must not change the default project name (the folder name)
            return Ok(Self { file: copy, dir, project, temporary: true });
        }

        let name = project
            .clone()
            .or_else(|| top_level_name(yaml))
            .ok_or("Enter a stack name, or add a top-level name: to the file.")?;
        let dir = stacks_root.join(&name);
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {}", dir.display(), e))?;
        let path = dir.join("compose.yaml");
        std::fs::write(&path, yaml).map_err(|e| format!("Cannot write {}: {}", path.display(), e))?;
        Ok(Self { file: path, dir, project: Some(name), temporary: false })
    }

    /// Arguments for `rcompose up -d`.
    pub fn up_args(&self) -> Vec<String> {
        let mut args = vec!["-f".to_string(), self.file.display().to_string()];
        if let Some(p) = &self.project {
            args.extend(["-p".to_string(), p.clone()]);
        }
        args.extend(["up".to_string(), "-d".to_string()]);
        args
    }

    /// Runs `rcompose up -d`, streaming its output to `log`.
    pub async fn up(&self, bin: &Path, log: impl Fn(String)) -> Result<(), String> {
        let mut cmd = Command::new(bin);
        cmd.args(self.up_args()).current_dir(&self.dir);
        let result = run_streaming(cmd, &log).await;
        if self.temporary {
            let _ = std::fs::remove_file(&self.file);
        }
        result
    }
}

/// Default folder for stacks typed into the editor.
pub fn stacks_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("rcdesktop")
        .join("stacks")
}

/// The `name:` key at the top of a compose file, if any.
fn top_level_name(yaml: &str) -> Option<String> {
    let doc: serde_yaml::Value = serde_yaml::from_str(yaml).ok()?;
    let name = doc.get("name")?.as_str()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Runs a command, forwarding stdout and stderr line by line (without ANSI
/// colors). Fails with the last output line when the exit code is non-zero.
async fn run_streaming(mut cmd: Command, log: &impl Fn(String)) -> Result<(), String> {
    cmd.env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd.spawn().map_err(|e| format!("Cannot start: {}", e))?;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    forward_lines(child.stdout.take(), tx.clone());
    forward_lines(child.stderr.take(), tx);

    let mut last = String::new();
    while let Some(line) = rx.recv().await {
        if !line.trim().is_empty() {
            last = line.clone();
            log(line);
        }
    }

    let status = child.wait().await.map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else if last.is_empty() {
        Err(format!("exited with {}", status))
    } else {
        Err(last)
    }
}

fn forward_lines<R: AsyncRead + Unpin + Send + 'static>(
    stream: Option<R>,
    tx: tokio::sync::mpsc::UnboundedSender<String>,
) {
    let Some(stream) = stream else { return };
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx.send(strip_ansi(&line));
        }
    });
}

/// Removes ANSI escape sequences (colors, cursor moves).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                // CSI: parameters until a final byte in @..~
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}
