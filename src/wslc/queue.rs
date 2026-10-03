use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use async_trait::async_trait;

use crate::domain::container::Container;
use crate::domain::image::ImageSummary;
use crate::domain::session::WslcSessionInfo;
use crate::domain::volume::VolumeSummary;
use crate::wslc::client::WslcClient;
use crate::wslc::parser::{parse_containers, parse_images, parse_version, parse_volumes};

struct WslcRequest {
    args: Vec<String>,
    timeout: Duration,
    response: oneshot::Sender<Result<String, String>>,
}

#[derive(Clone)]
pub struct SingleLaneQueue {
    sender: mpsc::Sender<WslcRequest>,
}

impl SingleLaneQueue {
    pub fn new() -> Self {
        let (tx, mut rx) = mpsc::channel::<WslcRequest>(64);
        let bin_path = find_wslc_bin();
        let use_cmd_mode = Arc::new(AtomicBool::new(false));

        tokio::spawn(async move {
            while let Some(req) = rx.recv().await {
                let bin = bin_path.clone();
                let cmd_mode = use_cmd_mode.clone();
                let res = execute_serialized(bin, req.args, req.timeout, cmd_mode).await;
                let _ = req.response.send(res);
            }
        });

        Self { sender: tx }
    }

    pub async fn execute(&self, args: Vec<String>, timeout: Duration) -> Result<String, String> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.sender
            .send(WslcRequest {
                args,
                timeout,
                response: resp_tx,
            })
            .await
            .map_err(|e| format!("Failed to enqueue wslc request: {}", e))?;

        resp_rx
            .await
            .map_err(|e| format!("Cancelled wslc request: {}", e))?
    }
}

fn find_wslc_bin() -> PathBuf {
    if let Ok(env_path) = std::env::var("WSLC_BIN") {
        let p = PathBuf::from(env_path);
        if p.exists() {
            return p;
        }
    }

    let default_path = PathBuf::from(r"C:\Program Files\WSL\wslc.exe");
    if default_path.exists() {
        return default_path;
    }

    PathBuf::from("wslc.exe")
}

fn is_sharing_violation(output: &str) -> bool {
    let lower = output.to_lowercase();
    lower.contains("error_sharing_violation")
        || lower.contains("0x80070020")
        || lower.contains("used by another process")
        || lower.contains("outro processo")
}

async fn execute_serialized(
    bin: PathBuf,
    args: Vec<String>,
    timeout: Duration,
    use_cmd: Arc<AtomicBool>,
) -> Result<String, String> {
    let max_retries = 3;
    let mut backoff = Duration::from_millis(200);

    for attempt in 0..=max_retries {
        let is_cmd = use_cmd.load(Ordering::Relaxed);
        let mut cmd = if is_cmd {
            let mut c = Command::new("cmd.exe");
            let mut full_cmd = format!("\"{}\"", bin.display());
            for arg in &args {
                full_cmd.push(' ');
                if arg.contains(' ') || arg.contains('"') {
                    full_cmd.push_str(&format!("\"{}\"", arg.replace('"', "\\\"")));
                } else {
                    full_cmd.push_str(arg);
                }
            }
            c.args(&["/d", "/s", "/c", &full_cmd]);
            c
        } else {
            let mut c = Command::new(&bin);
            c.args(&args);
            c
        };

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let run_future = async {
            let output = cmd.output().await.map_err(|e| format!("Process exec failed: {}", e))?;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            if output.status.success() {
                Ok(stdout)
            } else {
                let err_combined = if stdout.is_empty() {
                    stderr
                } else {
                    format!("{}\n{}", stdout, stderr)
                };
                Err(err_combined)
            }
        };

        match tokio::time::timeout(timeout, run_future).await {
            Ok(Ok(stdout)) => return Ok(stdout),
            Ok(Err(err_msg)) => {
                if is_sharing_violation(&err_msg) {
                    if !is_cmd {
                        use_cmd.store(true, Ordering::Relaxed);
                    }
                    if attempt < max_retries {
                        tokio::time::sleep(backoff).await;
                        backoff *= 2;
                        continue;
                    }
                }
                return Err(err_msg);
            }
            Err(_) => return Err(format!("Command timed out after {}s", timeout.as_secs())),
        }
    }

    Err("Failed after max retries due to sharing violations".into())
}

#[derive(Clone)]
pub struct RealWslcClient {
    queue: SingleLaneQueue,
}

impl RealWslcClient {
    pub fn new() -> Self {
        Self {
            queue: SingleLaneQueue::new(),
        }
    }
}

#[async_trait]
impl WslcClient for RealWslcClient {
    async fn list_containers(&self, all: bool) -> Result<Vec<Container>, String> {
        let mut args = vec!["list".to_string()];
        if all {
            args.push("-a".to_string());
        }
        // JSON carries the labels needed to group compose stacks
        args.extend(["--format".to_string(), "json".to_string()]);
        let output = self.queue.execute(args, Duration::from_secs(45)).await?;
        Ok(parse_containers(&output))
    }

    async fn start_container(&self, id: &str) -> Result<(), String> {
        self.queue
            .execute(vec!["start".to_string(), id.to_string()], Duration::from_secs(60))
            .await?;
        Ok(())
    }

    async fn stop_container(&self, id: &str) -> Result<(), String> {
        self.queue
            .execute(vec!["stop".to_string(), id.to_string()], Duration::from_secs(60))
            .await?;
        Ok(())
    }

    async fn restart_container(&self, id: &str) -> Result<(), String> {
        self.queue
            .execute(vec!["restart".to_string(), id.to_string()], Duration::from_secs(60))
            .await?;
        Ok(())
    }

    async fn remove_container(&self, id: &str) -> Result<(), String> {
        self.queue
            .execute(vec!["rm".to_string(), id.to_string()], Duration::from_secs(30))
            .await?;
        Ok(())
    }

    async fn get_logs(&self, id: &str, tail: usize) -> Result<String, String> {
        let args = vec![
            "logs".to_string(),
            "--tail".to_string(),
            tail.to_string(),
            id.to_string(),
        ];
        self.queue.execute(args, Duration::from_secs(30)).await
    }

    async fn inspect_container(&self, id: &str) -> Result<String, String> {
        self.queue
            .execute(vec!["inspect".to_string(), id.to_string()], Duration::from_secs(30))
            .await
    }

    async fn list_images(&self) -> Result<Vec<ImageSummary>, String> {
        let output = self
            .queue
            .execute(vec!["images".to_string()], Duration::from_secs(45))
            .await?;
        Ok(parse_images(&output))
    }

    async fn pull_image(&self, image: &str) -> Result<(), String> {
        self.queue
            .execute(vec!["pull".to_string(), image.to_string()], Duration::from_secs(600))
            .await?;
        Ok(())
    }

    async fn remove_image(&self, id: &str) -> Result<(), String> {
        self.queue
            .execute(vec!["rmi".to_string(), id.to_string()], Duration::from_secs(30))
            .await?;
        Ok(())
    }

    async fn list_volumes(&self) -> Result<Vec<VolumeSummary>, String> {
        let output = self
            .queue
            .execute(vec!["volume".to_string(), "list".to_string()], Duration::from_secs(30))
            .await?;
        Ok(parse_volumes(&output))
    }

    async fn get_session_info(&self) -> Result<WslcSessionInfo, String> {
        let version_out = self
            .queue
            .execute(vec!["--version".to_string()], Duration::from_secs(15))
            .await
            .unwrap_or_else(|_| "wslc version unknown".to_string());

        let version = parse_version(&version_out).unwrap_or_else(|| "5.0.1.1".to_string());

        Ok(WslcSessionInfo {
            session_id: "wslc-default".into(),
            is_elevated: false,
            is_healthy: true,
            status_message: "WSLC session active".into(),
            version,
        })
    }
}
