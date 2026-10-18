//! What reviewer CLIs run in print mode have in common: one process per
//! turn, read as a stream of JSON lines. Each CLI's flags and stream are its
//! own dialect (see `crate::claude` and `crate::antigravity`); starting,
//! stopping and reading the process are shared here.
use serde_json::Value;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::{mpsc, oneshot},
};

/// Lines of standard error kept to explain a failed run.
const STDERR_LINES: usize = 20;
/// How long a process may take to exit after reporting its result.
#[cfg(not(test))]
const EXIT_GRACE: Duration = Duration::from_secs(10);
#[cfg(test)]
const EXIT_GRACE: Duration = Duration::from_secs(1);

/// Reads a process's output line by line. Bytes that aren't UTF-8 are
/// replaced rather than ending the stream, so one odd line can't hide the
/// lines after it.
pub struct Lines<R> {
    reader: BufReader<R>,
    buf: Vec<u8>,
}
impl<R: AsyncRead + Unpin> Lines<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
            buf: Vec::new(),
        }
    }
    /// The next line without its line ending, or None at the end.
    pub async fn next(&mut self) -> Option<String> {
        self.buf.clear();
        match self.reader.read_until(b'\n', &mut self.buf).await {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                if self.buf.ends_with(b"\n") {
                    self.buf.pop();
                    if self.buf.ends_with(b"\r") {
                        self.buf.pop();
                    }
                }
                Some(String::from_utf8_lossy(&self.buf).into_owned())
            }
        }
    }
}

/// How to start a CLI; tests start a fake.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Launcher {
    pub program: PathBuf,
    pub prefix: Vec<String>,
}
impl Launcher {
    pub fn new(program: &str) -> Self {
        Self {
            program: program.into(),
            prefix: vec![],
        }
    }
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.prefix);
        cmd
    }
    /// Runs a short command, such as `--version`, and returns its output.
    /// `missing` explains how to install the CLI when it cannot start.
    pub async fn output(&self, args: &[&str], name: &str, missing: &str) -> Result<String, String> {
        let mut cmd = self.command();
        cmd.args(args).stdin(Stdio::null()).kill_on_drop(true);
        let out = tokio::time::timeout(Duration::from_secs(30), cmd.output())
            .await
            .map_err(|_| format!("{name} {} timed out.", args.join(" ")))?
            .map_err(|e| format!("{missing} {e}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!(
                "{name} {} failed: {}",
                args.join(" "),
                stderr.trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}

/// What a turn is for, which decides its tools and how its output is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnKind {
    /// The review itself; its result is the report.
    Review,
    /// Restating the review as findings data.
    Extraction,
    /// Restating the findings and classifying the previous round's, which
    /// may need a look at the code.
    ExtractionWithTools,
    /// A developer's question.
    FollowUp,
}

/// One turn of a review's conversation.
#[derive(Clone, Debug)]
pub struct Turn {
    pub kind: TurnKind,
    pub cwd: PathBuf,
    /// The conversation. Claude's is chosen by Orbit on the first turn;
    /// Antigravity's is reported by the first turn, so it is empty there.
    pub session: String,
    pub resume: bool,
    pub model: String,
    pub effort: String,
    pub prompt: String,
    pub schema: Option<Value>,
    /// Orbit's dev container tool, for a review turn whose repository lets
    /// the reviewer run commands there.
    pub tool: Option<crate::boxtool::Server>,
}

/// What one turn's process reported, in the order it happened.
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    /// A finished message from the reviewer itself (not a sub-agent).
    Text {
        item: String,
        text: String,
    },
    CommandStarted {
        tool: String,
        command: String,
    },
    CommandOutput(String),
    /// The turn finished. `text` is its final answer, `structured` the
    /// answer to a schema, and `denials` the tool calls that were refused.
    Result {
        ok: bool,
        text: String,
        structured: Option<Value>,
        denials: Vec<String>,
    },
    /// The process ended without a result.
    Exited(String),
}

/// The conversation's start, as the CLI reported it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Init {
    pub model: String,
    /// The conversation's id, when the CLI reports it.
    pub session: Option<String>,
    /// How the CLI will treat tool calls that need approval.
    pub permission_mode: Option<String>,
}

/// What a line of standard output means.
#[derive(Debug, PartialEq)]
pub enum Line {
    Init(Init),
    Output(Output),
}

/// What a line of standard error means.
#[derive(Debug, PartialEq)]
pub enum Stderr {
    Plain,
    /// A tool call the CLI refused, in words.
    Denied(String),
    /// The run must not go on; the process is killed and the turn fails
    /// with this reason.
    Fatal(String),
}

/// One CLI's stream.
pub trait Dialect: Send + 'static {
    /// The CLI's name in messages.
    const NAME: &'static str;
    /// Reads one JSON line of standard output.
    fn read(&mut self, message: &Value) -> Vec<Line>;
    /// Reads one line of standard error.
    fn stderr(line: &str) -> Stderr;
}

/// A running turn. Dropping it kills the process.
pub struct Running {
    stop: Mutex<Option<oneshot::Sender<()>>>,
}
impl Running {
    /// Kills the process and the commands it started.
    pub async fn kill(&self) {
        if let Some(stop) = self.stop.lock().unwrap().take() {
            let _ = stop.send(());
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.get_mut().unwrap().take() {
            let _ = stop.send(());
        }
    }
}
/// Owns the process until it exits or is told to stop, so the process
/// group is only signalled while the process has not been reaped.
async fn wait(
    mut child: Child,
    stop: oneshot::Receiver<()>,
    fatal: oneshot::Receiver<()>,
) -> String {
    let fatal = async {
        // A dropped sender means the run can no longer be ended this way.
        if fatal.await.is_err() {
            std::future::pending::<()>().await
        }
    };
    let status = tokio::select! {
        status = child.wait() => status,
        _ = async { tokio::select! { _ = stop => {}, _ = fatal => {} } } => {
            #[cfg(unix)]
            if let Some(id) = child.id() {
                unsafe {
                    libc::kill(-(id as i32), libc::SIGKILL);
                }
            }
            let _ = child.kill().await;
            child.wait().await
        }
    };
    match status {
        Ok(s) => format!("it exited with {s}"),
        Err(e) => e.to_string(),
    }
}

/// A started turn: its process, the conversation's start, and the rest.
pub type Started = (
    Running,
    oneshot::Receiver<Result<Init, String>>,
    mpsc::Receiver<Output>,
);

/// Starts `launcher` with `args` in `cwd`, writing `stdin` to it when given
/// and then closing it. The first value from `init` is the conversation's
/// start, or the reason it never started; the rest of the turn arrives on
/// the channel. A turn's result waits for the process to exit, so tool calls
/// refused on standard error are counted in it. A turn still running after
/// `limit` is stopped and fails.
pub fn spawn<D: Dialect>(
    launcher: &Launcher,
    args: Vec<String>,
    cwd: &Path,
    stdin: Option<String>,
    mut dialect: D,
    missing: &str,
    limit: Option<Duration>,
) -> Result<Started, String> {
    let name = D::NAME;
    let mut cmd = launcher.command();
    cmd.args(args)
        .current_dir(cwd)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().map_err(|e| format!("{missing} {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or(format!("Missing {name} stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or(format!("Missing {name} stderr"))?;
    if let Some(input) = stdin {
        let mut pipe = child.stdin.take().ok_or(format!("Missing {name} stdin"))?;
        tokio::spawn(async move {
            let _ = pipe.write_all(input.as_bytes()).await;
            let _ = pipe.shutdown().await;
        });
    }
    let tail = Arc::new(Mutex::new(VecDeque::new()));
    let fatal = Arc::new(Mutex::new(None::<String>));
    let denials = Arc::new(Mutex::new(Vec::<String>::new()));
    let (init_tx, init_rx) = oneshot::channel();
    let (tx, rx) = mpsc::channel(256);
    let (stop, stopped) = oneshot::channel();
    let (kill, killed) = oneshot::channel();
    // Ends the run, with `reason` for the failed turn: kills the process
    // group and the process, which `wait` then reaps.
    let kill = Arc::new(Mutex::new(Some(kill)));
    let end = {
        let fatal = fatal.clone();
        move |reason: String| {
            fatal.lock().unwrap().get_or_insert(reason);
            if let Some(kill) = kill.lock().unwrap().take() {
                let _ = kill.send(());
            }
        }
    };
    let mut exited = tokio::spawn(wait(child, stopped, killed));
    let timer = limit.map(|limit| {
        let end = end.clone();
        tokio::spawn(async move {
            tokio::time::sleep(limit).await;
            end(format!(
                "{name} was still working after {} minutes, so Orbit stopped it and the commands it started, and the turn failed.",
                limit.as_secs() / 60
            ));
        })
    });
    {
        let (tail, denials, end) = (tail.clone(), denials.clone(), end.clone());
        tokio::spawn(async move {
            let mut lines = Lines::new(stderr);
            while let Some(line) = lines.next().await {
                match D::stderr(&line) {
                    Stderr::Plain => {}
                    Stderr::Denied(call) => denials.lock().unwrap().push(call),
                    Stderr::Fatal(reason) => end(reason),
                }
                let mut tail = tail.lock().unwrap();
                tail.push_back(line);
                if tail.len() > STDERR_LINES {
                    tail.pop_front();
                }
            }
        });
    }
    tokio::spawn(async move {
        let mut init = Some(init_tx);
        let mut result = None;
        let mut lines = Lines::new(stdout);
        'read: while let Some(line) = lines.next().await {
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            for line in dialect.read(&message) {
                match line {
                    Line::Init(found) => {
                        if let Some(init) = init.take() {
                            let _ = init.send(Ok(found));
                        }
                    }
                    Line::Output(output @ Output::Result { .. }) => {
                        result = Some(output);
                        break 'read;
                    }
                    Line::Output(output) => {
                        if tx.send(output).await.is_err() {
                            return;
                        }
                    }
                }
            }
        }
        let status = match tokio::time::timeout(EXIT_GRACE, &mut exited).await {
            Ok(status) => status,
            // A process still running after its result may have left a
            // command running too, so the turn fails rather than going on.
            Err(_) => {
                end(format!(
                    "{name} did not exit after its answer, so Orbit stopped it and the commands it started, and the turn failed."
                ));
                exited.await
            }
        }
        .unwrap_or_else(|e| format!("Orbit lost track of it: {e}"));
        if let Some(timer) = timer {
            timer.abort();
        }
        // Standard error is read to its end once the process is gone.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let fatal = fatal.lock().unwrap().take();
        let refused = std::mem::take(&mut *denials.lock().unwrap());
        let result = match (fatal, result) {
            (
                None,
                Some(Output::Result {
                    ok,
                    text,
                    structured,
                    mut denials,
                }),
            ) => {
                denials.extend(refused);
                Some(Output::Result {
                    ok,
                    text,
                    structured,
                    denials,
                })
            }
            (fatal, _) => {
                let detail = {
                    let tail = tail.lock().unwrap();
                    tail.iter()
                        .rev()
                        .find(|l| !l.trim().is_empty())
                        .cloned()
                        .unwrap_or(status)
                };
                let reason =
                    fatal.unwrap_or_else(|| format!("{name} stopped before finishing: {detail}"));
                if let Some(init) = init.take() {
                    let _ = init.send(Err(reason.clone()));
                }
                Some(Output::Exited(reason))
            }
        };
        if let Some(init) = init.take() {
            let _ = init.send(Err(format!(
                "{name} finished without starting a conversation."
            )));
        }
        if let Some(result) = result {
            let _ = tx.send(result).await;
        }
    });
    Ok((
        Running {
            stop: Mutex::new(Some(stop)),
        },
        init_rx,
        rx,
    ))
}

/// A tool result's content as text.
pub fn result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn a_line_that_is_not_utf8_does_not_end_the_stream() {
        let input: &[u8] = b"first\r\nbad \xff byte\nlast";
        let mut lines = super::Lines::new(input);
        assert_eq!(lines.next().await.as_deref(), Some("first"));
        assert_eq!(lines.next().await.as_deref(), Some("bad \u{fffd} byte"));
        assert_eq!(lines.next().await.as_deref(), Some("last"));
        assert_eq!(lines.next().await, None);
    }
}
