use serde_json::{json, Value};
use std::{
    collections::HashMap,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    process::{Child, ChildStdin, Command},
    sync::{mpsc, oneshot, Mutex},
};
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;
pub struct Rpc {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    pending: Pending,
    next: AtomicU64,
    pub alive: Arc<AtomicBool>,
    /// The last line Codex wrote to standard error, to say why it
    /// disconnected: short, and never one that looks like a credential.
    pub last_error: Arc<std::sync::Mutex<Option<String>>>,
}
impl Rpc {
    /// Starts Codex. `scan_root` is where the Codex Security plugin keeps the
    /// scans Orbit runs, instead of the system's temporary folder.
    pub async fn start(
        events: mpsc::Sender<Value>,
        scan_root: &std::path::Path,
    ) -> Result<Arc<Self>, String> {
        let mut cmd = Command::new("codex");
        cmd.args(["app-server", "--listen", "stdio://"]);
        cmd.env("CODEX_SECURITY_SCAN_ROOT", scan_root);
        Self::from_command(cmd, events).await
    }
    pub(crate) async fn from_command(
        mut cmd: Command,
        events: mpsc::Sender<Value>,
    ) -> Result<Arc<Self>, String> {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd.spawn().map_err(|e| {
            format!("Cannot start Codex. Install the Codex CLI and run codex login. {e}")
        })?;
        let stdin = child.stdin.take().ok_or("Missing Codex stdin")?;
        let stdout = child.stdout.take().ok_or("Missing Codex stdout")?;
        let stderr = child.stderr.take().ok_or("Missing Codex stderr")?;
        let pending: Pending = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));
        let last_error = Arc::new(std::sync::Mutex::new(None));
        let rpc = Arc::new(Self {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            pending: pending.clone(),
            next: AtomicU64::new(1),
            alive: alive.clone(),
            last_error: last_error.clone(),
        });
        // Closes when Codex's standard error does.
        let (stderr_open, stderr_closed) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let _open = stderr_open;
            let mut lines = crate::cli::Lines::new(stderr);
            // Only the last line is kept, in memory, to explain a disconnect;
            // nothing Codex writes here is logged.
            while let Some(line) = lines.next().await {
                let line = line.trim();
                if !line.is_empty() {
                    *last_error.lock().unwrap() = crate::types::credential_kinds(line)
                        .is_empty()
                        .then(|| crate::text::shorten(line, 300));
                }
            }
        });
        tokio::spawn(async move {
            let mut lines = crate::cli::Lines::new(stdout);
            while let Some(line) = lines.next().await {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if msg.get("method").is_none() {
                    if let Some(id) = msg["id"].as_u64() {
                        if let Some(tx) = pending.lock().await.remove(&id) {
                            let result = if let Some(e) = msg.get("error") {
                                Err(e["message"]
                                    .as_str()
                                    .unwrap_or("Codex request failed")
                                    .into())
                            } else {
                                Ok(msg["result"].clone())
                            };
                            let _ = tx.send(result);
                        }
                    }
                } else if events.send(msg).await.is_err() {
                    break;
                }
            }
            alive.store(false, Ordering::SeqCst);
            // A Codex that exited closes standard error too: wait for its
            // last line, so the disconnect can say why.
            let _ = tokio::time::timeout(std::time::Duration::from_secs(1), stderr_closed).await;
            for (_, tx) in pending.lock().await.drain() {
                let _ = tx.send(Err("Codex app-server disconnected.".into()));
            }
            let _ = events.send(json!({"method":"orbit/disconnected"})).await;
        });
        rpc.request(
            "initialize",
            json!({"clientInfo":{"name":"orbit","title":"Orbit","version":env!("CARGO_PKG_VERSION")}}),
        )
        .await?;
        rpc.write(json!({"method":"initialized","params":{}}))
            .await?;
        Ok(rpc)
    }
    pub async fn write(&self, message: Value) -> Result<(), String> {
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(format!("{message}\n").as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())
    }
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        self.request_timeout(method, params, 45).await
    }
    pub async fn request_timeout(
        &self,
        method: &str,
        params: Value,
        seconds: u64,
    ) -> Result<Value, String> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err("Codex is disconnected. Reconnect in Settings.".into());
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        if let Err(e) = self
            .write(json!({"id":id,"method":method,"params":params}))
            .await
        {
            self.pending.lock().await.remove(&id);
            return Err(e);
        }
        let result = tokio::time::timeout(Duration::from_secs(seconds), rx).await;
        self.pending.lock().await.remove(&id);
        result
            .map_err(|_| format!("Codex {method} timed out."))?
            .map_err(|_| "Codex connection closed.".to_string())?
    }
    pub async fn shutdown(&self) {
        self.alive.store(false, Ordering::SeqCst);
        let _ = self.stdin.lock().await.shutdown().await;
        let mut child = self.child.lock().await;
        #[cfg(unix)]
        if let Some(id) = child.id() {
            unsafe {
                libc::kill(-(id as i32), libc::SIGTERM);
            }
        }
        if tokio::time::timeout(Duration::from_secs(2), child.wait())
            .await
            .is_err()
        {
            #[cfg(unix)]
            if let Some(id) = child.id() {
                unsafe {
                    libc::kill(-(id as i32), libc::SIGKILL);
                }
            }
            let _ = child.kill().await;
        }
    }
}
impl Drop for Rpc {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(id) = self.child.get_mut().id() {
            unsafe {
                libc::kill(-(id as i32), libc::SIGKILL);
            }
        }
        let _ = self.child.get_mut().start_kill();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    const FIXTURE: &str = r#"
const readline = require('node:readline');
const send = value => process.stdout.write(JSON.stringify(value)+'\n');
let first;
readline.createInterface({input:process.stdin}).on('line',line=>{
 const m=JSON.parse(line);
 if(m.method==='initialize')send({id:m.id,result:{userAgent:'fixture'}});
 if(m.method==='first')first=m;
 if(m.method==='second'){
  send({method:'item/agentMessage/delta',params:{delta:'hello'}});
  send({id:m.id,result:{value:2}});send({id:first.id,result:{value:1}});
 }
 if(m.method==='complain'){
  process.stderr.write(m.params.lines.join('\n')+'\n');
  setTimeout(()=>process.exit(1),20);
 }
 if(m.method==='fail')send({id:m.id,error:{code:42,message:'fixture error'}});
 if(m.method==='crash')process.exit(0);
});
"#;
    pub(crate) async fn fixture() -> (Arc<Rpc>, mpsc::Receiver<Value>) {
        let (tx, rx) = mpsc::channel(32);
        let mut command = Command::new("node");
        command.args(["-e", FIXTURE]);
        (Rpc::from_command(command, tx).await.unwrap(), rx)
    }
    #[tokio::test]
    async fn routes_errors_and_out_of_order_responses() {
        let (rpc, mut events) = fixture().await;
        let (one, two) = tokio::join!(
            rpc.request("first", json!({})),
            rpc.request("second", json!({}))
        );
        assert_eq!(one.unwrap()["value"], 1);
        assert_eq!(two.unwrap()["value"], 2);
        assert_eq!(events.recv().await.unwrap()["params"]["delta"], "hello");
        assert_eq!(
            rpc.request("fail", json!({})).await.unwrap_err(),
            "fixture error"
        );
        rpc.shutdown().await;
    }
    #[tokio::test]
    async fn clears_pending_calls_on_timeout_and_process_exit() {
        let (rpc, _events) = fixture().await;
        assert!(rpc
            .request_timeout("hang", json!({}), 0)
            .await
            .unwrap_err()
            .contains("timed out"));
        assert!(rpc.pending.lock().await.is_empty());
        assert!(rpc
            .request("crash", json!({}))
            .await
            .unwrap_err()
            .contains("disconnected"));
        assert!(!rpc.alive.load(Ordering::SeqCst));
        assert!(rpc.pending.lock().await.is_empty());
        rpc.shutdown().await;
    }
    #[tokio::test]
    async fn keeps_why_codex_stopped_but_never_a_credential() {
        let token = format!("ghp_{}", "a1".repeat(18));
        for (lines, kept) in [
            (
                vec!["starting", "error: model is overloaded", ""],
                Some("error: model is overloaded"),
            ),
            (vec!["starting", &format!("bad token {token}")], None),
        ] {
            let (rpc, _events) = fixture().await;
            let _ = rpc.request("complain", json!({"lines": lines})).await;
            assert!(!rpc.alive.load(Ordering::SeqCst));
            assert_eq!(rpc.last_error.lock().unwrap().as_deref(), kept);
            rpc.shutdown().await;
        }
    }
}
