use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::tool::{Tool, ToolContext, ToolResult};

/// Default script timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Max tool calls per script execution.
const DEFAULT_MAX_TOOL_CALLS: usize = 50;

/// Max stdout bytes to keep (head+tail strategy).
const MAX_STDOUT_BYTES: usize = 50_000;

/// Max stderr bytes to keep (head-only).
const MAX_STDERR_BYTES: usize = 10_000;

/// stdout head ratio (40%).
const STDOUT_HEAD_RATIO: f64 = 0.4;

/// stdout tail ratio (60%).
const STDOUT_TAIL_RATIO: f64 = 0.6;

/// SIGTERM→SIGKILL escalation window (seconds).
const ESCALATION_TIMEOUT_SECS: u64 = 5;

/// Secret-like environment variable substrings that get blocked.
const SECRET_SUBSTRINGS: &[&str] = &[
    "KEY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "PASSWD", "AUTH",
];

/// Safe environment variable prefixes that are allowed through.
const SAFE_ENV_PREFIXES: &[&str] = &[
    "PATH", "HOME", "USER", "LANG", "LC_", "TERM", "TMPDIR", "TMP",
    "TEMP", "SHELL", "LOGNAME", "XDG_", "PYTHONPATH", "VIRTUAL_ENV", "CONDA",
];

/// Allowed sandbox tools.
const SANDBOX_ALLOWED_TOOLS: &[&str] = &[
    "web_search", "web_extract", "read_file", "write_file",
    "search_files", "patch", "terminal",
];

/// Blocked terminal parameters in sandbox context.
const TERMINAL_BLOCKED_PARAMS: &[&str] = &[
    "background", "pty", "notify_on_complete", "watch_patterns",
];

/// Configuration for code execution sandbox.
#[derive(Debug, Clone)]
pub struct CodeExecutionConfig {
    pub timeout_secs: u64,
    pub max_tool_calls: usize,
}

impl Default for CodeExecutionConfig {
    fn default() -> Self {
        Self {
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_tool_calls: DEFAULT_MAX_TOOL_CALLS,
        }
    }
}

/// RPC request from sandbox child to parent.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RpcRequest {
    tool: String,
    args: serde_json::Value,
    seq: Option<usize>,
}

/// RPC response from parent to sandbox child.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RpcResponse {
    success: bool,
    result: String,
    error: Option<String>,
}

/// Result status of code execution.
#[derive(Debug, Clone)]
pub enum ExecutionStatus {
    Success,
    Timeout,
    Interrupted,
    Error(i32),
}

impl ExecutionStatus {
    fn to_str(&self) -> &str {
        match self {
            ExecutionStatus::Success => "success",
            ExecutionStatus::Timeout => "timeout",
            ExecutionStatus::Interrupted => "interrupted",
            ExecutionStatus::Error(_) => "error",
        }
    }
}

/// Strip ANSI escape sequences from text.
fn strip_ansi(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(c) = chars.next() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                result.push(ch);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Generate `hermes_tools.py` module content for the sandbox.
pub fn generate_hermes_tools_module(
    enabled_tools: &HashSet<String>,
    transport: &str,
    socket_path: Option<&str>,
    rpc_dir: Option<&str>,
) -> String {
    let mut tools_to_generate: Vec<&str> = SANDBOX_ALLOWED_TOOLS
        .iter()
        .filter(|t| enabled_tools.is_empty() || enabled_tools.iter().any(|s| s == *t))
        .copied()
        .collect();
    tools_to_generate.sort();

    if tools_to_generate.is_empty() {
        tools_to_generate = SANDBOX_ALLOWED_TOOLS.to_vec();
    }

    let header = if transport == "uds" {
        let sock = socket_path.unwrap_or("/tmp/hermes_rpc.sock");
        format!(
            r#"import json
import os
import socket
import shlex
import time

_RPC_SOCKET = os.environ.get("HERMES_RPC_SOCKET", "{sock}")
_seq = 0

def _connect():
    conn = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    conn.connect(_RPC_SOCKET)
    return conn

def _call(tool_name, args):
    global _seq
    _seq += 1
    conn = _connect()
    try:
        request = json.dumps({{"tool": tool_name, "args": args, "seq": _seq}}) + "\\n"
        conn.sendall(request.encode())
        buf = b""
        while True:
            chunk = conn.recv(65536)
            if not chunk:
                raise RuntimeError("Agent process disconnected")
            buf += chunk
            if buf.endswith(b"\\n"):
                break
        return json.loads(buf.decode().strip())
    finally:
        conn.close()
"#,
        )
    } else {
        let dir = rpc_dir.unwrap_or("/tmp/hermes_rpc");
        format!(
            r#"import json
import os
import shlex
import time

_RPC_DIR = r"{dir}"
_seq = 0

def _call(tool_name, args):
    global _seq
    _seq += 1
    req_file = os.path.join(_RPC_DIR, f"req_{{_seq:06}}")
    res_file = os.path.join(_RPC_DIR, f"res_{{_seq:06}}")

    tmp = req_file + ".tmp"
    with open(tmp, "w") as f:
        json.dump({{"tool": tool_name, "args": args, "seq": _seq}}, f)
    os.rename(tmp, req_file)

    deadline = time.monotonic() + 300
    poll_interval = 0.05
    while not os.path.exists(res_file):
        if time.monotonic() > deadline:
            raise RuntimeError("RPC timeout")
        time.sleep(poll_interval)
        poll_interval = min(poll_interval * 1.2, 0.25)

    with open(res_file) as f:
        result = json.loads(f.read())
    os.unlink(res_file)
    return result
"#,
        )
    };

    let helpers = r#"
def json_parse(text):
    import json
    return json.loads(text, strict=False)

def retry(fn, max_attempts=3, delay=2):
    for attempt in range(max_attempts):
        try:
            return fn()
        except Exception as e:
            if attempt == max_attempts - 1:
                raise
            time.sleep(delay * (2 ** attempt))
"#;

    let tool_sigs: &[(&str, &str, &str)] = &[
        ("web_search", "query: str, limit: int = 5", r#"{"query": query, "limit": limit}"#),
        ("web_extract", "urls: list", r#"{"urls": urls}"#),
        ("read_file", "path: str, offset: int = 1, limit: int = 500",
         r#"{"path": path, "offset": offset, "limit": limit}"#),
        ("write_file", "path: str, content: str", r#"{"path": path, "content": content}"#),
        ("search_files", "pattern: str, target: str = \"content\", path: str = \".\"",
         r#"{"pattern": pattern, "target": target, "path": path}"#),
        ("patch", "path: str, old_string: str = None, new_string: str = None, regex: bool = False",
         r#"{"path": path, "old_string": old_string, "new_string": new_string, "regex": regex}"#),
        ("terminal", "command: str, timeout: int = None, workdir: str = None",
         r#"{"command": command, "timeout": timeout, "workdir": workdir}"#),
    ];

    let mut stubs = String::new();
    for (name, sig, args_expr) in tool_sigs {
        if tools_to_generate.contains(name) {
            stubs.push_str(&format!(
                "\ndef {name}({sig}):\n    return _call(\"{name}\", {args_expr})\n",
            ));
        }
    }

    format!("{header}\n{helpers}{stubs}")
}

/// Filter environment variables for child process security.
fn filter_child_env(passthrough: &HashSet<String>) -> std::collections::HashMap<String, String> {
    let mut child_env = std::collections::HashMap::new();

    for (key, value) in std::env::vars() {
        if passthrough.contains(&key) {
            child_env.insert(key.clone(), value);
            continue;
        }

        if SECRET_SUBSTRINGS.iter().any(|pat| key.contains(pat)) {
            continue;
        }

        if SAFE_ENV_PREFIXES.iter().any(|prefix| key.starts_with(prefix)) {
            child_env.insert(key, value);
        }
    }

    child_env.insert("PYTHONDONTWRITEBYTECODE".to_string(), "1".to_string());

    if !child_env.contains_key("HOME") {
        if let Ok(home) = std::env::var("HOME") {
            child_env.insert("HOME".to_string(), home);
        }
    }

    child_env
}

/// Strip blocked parameters from terminal tool args.
fn strip_blocked_terminal_params(args: &mut serde_json::Value) {
    if let Some(obj) = args.as_object_mut() {
        for param in TERMINAL_BLOCKED_PARAMS {
            obj.remove(*param);
        }
    }
}

/// Drain a reader into a head+tail buffer (for stdout).
fn drain_head_tail<R: Read>(
    reader: R,
    head_bytes: usize,
    tail_bytes: usize,
) -> (String, String, usize) {
    let mut buf_reader = BufReader::new(reader);
    let mut head = String::new();
    let mut tail_chunks: Vec<String> = Vec::new();
    let mut tail_total: usize = 0;
    let mut head_done = false;
    let mut total: usize = 0;

    loop {
        let mut line = String::new();
        match buf_reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        total += line.len();

        if !head_done {
            if head.len() + line.len() <= head_bytes {
                head.push_str(&line);
            } else {
                head_done = true;
                let remainder = &line[head_bytes.saturating_sub(head.len())..];
                tail_total += remainder.len();
                tail_chunks.push(remainder.to_string());
                if tail_total > tail_bytes {
                    let mut to_remove = tail_total - tail_bytes;
                    while to_remove > 0 && !tail_chunks.is_empty() {
                        if tail_chunks[0].len() <= to_remove {
                            to_remove -= tail_chunks[0].len();
                            tail_total -= tail_chunks.remove(0).len();
                        } else {
                            tail_chunks[0] = tail_chunks[0][to_remove..].to_string();
                            tail_total -= to_remove;
                            to_remove = 0;
                        }
                    }
                }
            }
        } else {
            tail_total += line.len();
            tail_chunks.push(line);
            if tail_total > tail_bytes {
                let mut to_remove = tail_total - tail_bytes;
                while to_remove > 0 && !tail_chunks.is_empty() {
                    if tail_chunks[0].len() <= to_remove {
                        to_remove -= tail_chunks[0].len();
                        tail_total -= tail_chunks.remove(0).len();
                    } else {
                        tail_chunks[0] = tail_chunks[0][to_remove..].to_string();
                        tail_total -= to_remove;
                        to_remove = 0;
                    }
                }
            }
        }
    }

    let tail = tail_chunks.concat();
    (head, tail, total)
}

/// Drain a reader into a head-only buffer (for stderr).
fn drain_head_only<R: Read>(reader: R, max_bytes: usize) -> (String, usize) {
    let mut buf_reader = BufReader::new(reader);
    let mut buf = String::new();
    let mut total: usize = 0;

    loop {
        let mut line = String::new();
        match buf_reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        total += line.len();
        if buf.len() + line.len() <= max_bytes {
            buf.push_str(&line);
        } else {
            let remaining = max_bytes.saturating_sub(buf.len());
            if remaining > 0 {
                let end = line.len().min(remaining);
                buf.push_str(&line[..end]);
            }
            break;
        }
    }

    (buf, total)
}

/// UDS RPC server loop. Runs in a background thread.
fn run_uds_server(
    socket_path: &Path,
    tool_call_count: Arc<AtomicUsize>,
    max_tool_calls: usize,
    tool_callback: Option<Arc<dyn Fn(&str, Value) -> Result<String> + Send + Sync>>,
    stop_flag: Arc<AtomicBool>,
) -> Result<()> {
    use std::os::unix::net::UnixListener;

    let _ = fs::remove_file(socket_path);

    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("Failed to bind UDS socket: {}", socket_path.display()))?;

    let socket_path = socket_path.to_path_buf();

    std::thread::spawn(move || {
        while !stop_flag.load(Ordering::Relaxed) {
            let _ = listener.set_nonblocking(true);

            match listener.accept() {
                Ok((mut conn, _)) => {
                    conn.set_read_timeout(Some(Duration::from_secs(300))).ok();
                    conn.set_write_timeout(Some(Duration::from_secs(300))).ok();

                    loop {
                        if stop_flag.load(Ordering::Relaxed) {
                            break;
                        }

                        if tool_call_count.load(Ordering::Relaxed) >= max_tool_calls {
                            let resp = RpcResponse {
                                success: false,
                                result: String::new(),
                                error: Some(format!("Max tool calls ({max_tool_calls}) exceeded")),
                            };
                            if let Ok(json_str) = serde_json::to_string(&resp) {
                                let _ = conn.write_all(format!("{json_str}\n").as_bytes());
                            }
                            break;
                        }

                        let mut reader = BufReader::new(&conn);
                        let mut line = String::new();
                        match reader.read_line(&mut line) {
                            Ok(0) => break,
                            Ok(_) => {}
                            Err(_) => break,
                        }

                        let request: RpcRequest = match serde_json::from_str(&line) {
                            Ok(r) => r,
                            Err(e) => {
                                let resp = RpcResponse {
                                    success: false,
                                    result: String::new(),
                                    error: Some(format!("Invalid JSON: {e}")),
                                };
                                if let Ok(json_str) = serde_json::to_string(&resp) {
                                    let _ = conn.write_all(format!("{json_str}\n").as_bytes());
                                }
                                break;
                            }
                        };

                        tool_call_count.fetch_add(1, Ordering::Relaxed);

                        let resp = if let Some(ref cb) = tool_callback {
                            let mut args = request.args.clone();
                            strip_blocked_terminal_params(&mut args);

                            if !SANDBOX_ALLOWED_TOOLS.contains(&request.tool.as_str()) {
                                RpcResponse {
                                    success: false,
                                    result: String::new(),
                                    error: Some(format!(
                                        "Tool '{}' not allowed in sandbox",
                                        request.tool
                                    )),
                                }
                            } else {
                                match cb(&request.tool, args) {
                                    Ok(result) => RpcResponse {
                                        success: true,
                                        result,
                                        error: None,
                                    },
                                    Err(e) => RpcResponse {
                                        success: false,
                                        result: String::new(),
                                        error: Some(e.to_string()),
                                    },
                                }
                            }
                        } else {
                            RpcResponse {
                                success: false,
                                result: String::new(),
                                error: Some("No tool callback configured".to_string()),
                            }
                        };

                        if let Ok(json_str) = serde_json::to_string(&resp) {
                            let _ = conn.write_all(format!("{json_str}\n").as_bytes());
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }

        let _ = fs::remove_file(&socket_path);
    });

    Ok(())
}

/// Get Python interpreter path.
fn get_python_path() -> String {
    std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string())
}

/// Execute code using local UDS transport.
pub fn execute_code_sync(
    code: &str,
    config: &CodeExecutionConfig,
    enabled_tools: &HashSet<String>,
    passthrough: &HashSet<String>,
    _working_dir: &Path,
    tool_callback: Option<Arc<dyn Fn(&str, Value) -> Result<String> + Send + Sync>>,
) -> ToolResult {
    let start = Instant::now();

    let tmpdir = std::env::temp_dir();
    let sandbox_id = uuid::Uuid::new_v4().simple().to_string();
    let sandbox_dir = tmpdir.join(format!("hermes_sandbox_{sandbox_id}"));

    if let Err(e) = fs::create_dir_all(&sandbox_dir) {
        return ToolResult::err(format!("Failed to create sandbox directory: {e}"));
    }

    // macOS: use /tmp to avoid AF_UNIX 104-byte path limit
    let sock_tmpdir = if cfg!(target_os = "macos") {
        PathBuf::from("/tmp")
    } else {
        tmpdir.clone()
    };
    let socket_path = sock_tmpdir.join(format!("hermes_rpc_{sandbox_id}.sock"));

    // Generate hermes_tools.py
    let hermes_tools_content = generate_hermes_tools_module(
        enabled_tools,
        "uds",
        Some(&socket_path.to_string_lossy()),
        None,
    );
    if let Err(e) = fs::write(sandbox_dir.join("hermes_tools.py"), &hermes_tools_content) {
        let _ = fs::remove_dir_all(&sandbox_dir);
        return ToolResult::err(format!("Failed to write hermes_tools.py: {e}"));
    }

    // Write script.py
    if let Err(e) = fs::write(sandbox_dir.join("script.py"), code) {
        let _ = fs::remove_dir_all(&sandbox_dir);
        return ToolResult::err(format!("Failed to write script.py: {e}"));
    }

    // Set up UDS server
    let tool_call_count = Arc::new(AtomicUsize::new(0));
    let stop_flag = Arc::new(AtomicBool::new(false));

    if let Err(e) = run_uds_server(
        &socket_path,
        tool_call_count.clone(),
        config.max_tool_calls,
        tool_callback,
        stop_flag.clone(),
    ) {
        let _ = fs::remove_dir_all(&sandbox_dir);
        return ToolResult::err(format!("Failed to start UDS server: {e}"));
    }

    // Build child environment
    let mut child_env = filter_child_env(passthrough);
    child_env.insert(
        "HERMES_RPC_SOCKET".to_string(),
        socket_path.to_string_lossy().to_string(),
    );
    child_env.insert(
        "PYTHONPATH".to_string(),
        sandbox_dir.to_string_lossy().to_string(),
    );

    if let Ok(tz) = std::env::var("HERMES_TIMEZONE") {
        if !tz.trim().is_empty() {
            child_env.insert("TZ".to_string(), tz.trim().to_string());
        }
    }

    // Spawn child process
    let python = get_python_path();
    let child = Command::new(&python)
        .arg("script.py")
        .current_dir(&sandbox_dir)
        .env_clear()
        .envs(&child_env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            stop_flag.store(true, Ordering::Relaxed);
            let _ = fs::remove_file(&socket_path);
            let _ = fs::remove_dir_all(&sandbox_dir);
            return ToolResult::err(format!("Failed to spawn Python: {e}"));
        }
    };

    let timeout = Duration::from_secs(config.timeout_secs);
    let deadline = Instant::now() + timeout;

    // Take pipes before poll loop
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    // Drain in background threads
    let stdout_head_bytes = (MAX_STDOUT_BYTES as f64 * STDOUT_HEAD_RATIO).ceil() as usize;
    let stdout_tail_bytes = (MAX_STDOUT_BYTES as f64 * STDOUT_TAIL_RATIO).ceil() as usize;

    let stdout_thread = stdout_pipe.map(|pipe| {
        std::thread::spawn(move || drain_head_tail(pipe, stdout_head_bytes, stdout_tail_bytes))
    });

    let stderr_thread = stderr_pipe.map(|pipe| {
        std::thread::spawn(move || drain_head_only(pipe, MAX_STDERR_BYTES))
    });

    // Poll loop
    let mut status = ExecutionStatus::Success;
    loop {
        if let Some(exit_status) = child.try_wait().unwrap_or(None) {
            let code = exit_status.code().unwrap_or(-1);
            status = if code == 0 {
                ExecutionStatus::Success
            } else if code == 124 || code == 137 {
                ExecutionStatus::Timeout
            } else if code == 130 {
                ExecutionStatus::Interrupted
            } else {
                ExecutionStatus::Error(code)
            };
            break;
        }

        if Instant::now() > deadline {
            // SIGTERM
            let _ = std::process::Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .output();

            // Wait for escalation
            let grace = Instant::now() + Duration::from_secs(ESCALATION_TIMEOUT_SECS);
            while Instant::now() < grace {
                if child.try_wait().unwrap_or(None).is_some() {
                    status = ExecutionStatus::Timeout;
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }

            // SIGKILL if still running
            if child.try_wait().unwrap_or(None).is_none() {
                let _ = std::process::Command::new("kill")
                    .args(["-KILL", &child.id().to_string()])
                    .output();
                let _ = child.wait();
            }

            status = ExecutionStatus::Timeout;
            break;
        }

        std::thread::sleep(Duration::from_millis(200));
    }

    // Join drain threads
    let (head, tail, total_stdout) = if let Some(handle) = stdout_thread {
        handle.join().unwrap_or((String::new(), String::new(), 0))
    } else {
        (String::new(), String::new(), 0)
    };

    let (stderr_content, _total_stderr) = if let Some(handle) = stderr_thread {
        handle.join().unwrap_or((String::new(), 0))
    } else {
        (String::new(), 0)
    };

    // Clean up
    stop_flag.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(100));
    let _ = fs::remove_file(&socket_path);
    let _ = fs::remove_dir_all(&sandbox_dir);

    let duration = start.elapsed();
    let tool_calls_made = tool_call_count.load(Ordering::Relaxed);

    // Build output
    let stdout_text = if total_stdout > MAX_STDOUT_BYTES && !tail.is_empty() {
        let omitted = total_stdout - head.len() - tail.len();
        format!(
            "{}\n\n... [OUTPUT TRUNCATED - {} chars omitted] ...\n\n{}",
            head, omitted, tail
        )
    } else {
        head + &tail
    };

    let stdout_text = strip_ansi(&stdout_text);

    let output = if stderr_content.is_empty() {
        stdout_text
    } else {
        format!("{stdout_text}\n--- stderr ---\n{stderr_content}")
    };

    match status {
        ExecutionStatus::Success => {
            ToolResult::ok(format!(
                "Status: {}\nTool calls: {}\nDuration: {:.2}s\n\n{}",
                status.to_str(),
                tool_calls_made,
                duration.as_secs_f64(),
                output,
            ))
        }
        _ => {
            ToolResult::err(format!(
                "Status: {}\nDuration: {:.2}s\n\n{}",
                status.to_str(),
                duration.as_secs_f64(),
                output,
            ))
        }
    }
}

/// The execute_code tool implementation.
pub struct CodeExecutionTool {
    config: CodeExecutionConfig,
    tool_callback: Option<Arc<dyn Fn(&str, Value) -> Result<String> + Send + Sync>>,
}

impl CodeExecutionTool {
    pub fn new() -> Self {
        Self {
            config: CodeExecutionConfig::default(),
            tool_callback: None,
        }
    }

    /// Set the tool callback for RPC dispatch.
    pub fn with_tool_callback(
        mut self,
        callback: impl Fn(&str, Value) -> Result<String> + Send + Sync + 'static,
    ) -> Self {
        self.tool_callback = Some(Arc::new(callback));
        self
    }

    /// Check if sandbox is available (not Windows).
    pub fn is_available() -> bool {
        !cfg!(target_os = "windows")
    }
}

#[async_trait]
impl Tool for CodeExecutionTool {
    fn name(&self) -> &str {
        "execute_code"
    }

    fn toolset(&self) -> &str {
        "code_execution"
    }

    fn description(&self) -> &str {
        "Run a Python script that can call Hermes tools (web_search, web_extract, read_file, write_file, search_files, patch, terminal) via RPC. Tool results never enter the context window; only the script's stdout is returned."
    }

    fn schema(&self) -> Value {
        let tool_list = SANDBOX_ALLOWED_TOOLS
            .iter()
            .map(|t| format!("`{t}`"))
            .collect::<Vec<_>>()
            .join(", ");

        json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": format!("Python code to execute. The script can call these Hermes tools: {tool_list}. All tools are pre-imported in the sandbox environment.")
                }
            },
            "required": ["code"],
        })
    }

    fn max_result_size_chars(&self) -> Option<usize> {
        Some(100_000)
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let code = args
            .get("code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: code"))?
            .to_string();

        if !Self::is_available() {
            return Ok(ToolResult::err(
                "Code execution sandbox is not available on Windows. \
                 Install WSL2 or use a Linux/macOS environment.",
            ));
        }

        // Check Python availability
        let python = get_python_path();
        let check = Command::new(&python)
            .arg("--version")
            .output();
        if check.is_err() {
            return Ok(ToolResult::err(format!(
                "Python 3 is not available. Please install Python 3 (tried: {python})."
            )));
        }

        let enabled_tools = HashSet::new();
        let passthrough = HashSet::new();

        let result = execute_code_sync(
            &code,
            &self.config,
            &enabled_tools,
            &passthrough,
            &ctx.working_dir,
            self.tool_callback.clone(),
        );

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_basic() {
        assert_eq!(strip_ansi("\x1b[31mHello\x1b[0m"), "Hello");
    }

    #[test]
    fn test_strip_ansi_complex() {
        assert_eq!(strip_ansi("\x1b[1;32mBold Green\x1b[0m"), "Bold Green");
    }

    #[test]
    fn test_strip_ansi_no_escapes() {
        assert_eq!(strip_ansi("plain text"), "plain text");
    }

    #[test]
    fn test_filter_child_env_blocked_secrets() {
        let original = std::env::var("TEST_API_KEY").ok();
        unsafe { std::env::set_var("TEST_API_KEY", "secret123") }

        let env = filter_child_env(&HashSet::new());
        assert!(!env.contains_key("TEST_API_KEY"));

        if let Some(val) = original {
            unsafe { std::env::set_var("TEST_API_KEY", val) }
        } else {
            unsafe { std::env::remove_var("TEST_API_KEY") }
        }
    }

    #[test]
    fn test_filter_child_env_passes_safe() {
        let env = filter_child_env(&HashSet::new());
        assert!(env.contains_key("PATH") || env.contains_key("HOME"));
    }

    #[test]
    fn test_filter_child_env_python_dont_write_bytecode() {
        let env = filter_child_env(&HashSet::new());
        assert_eq!(env.get("PYTHONDONTWRITEBYTECODE"), Some(&"1".to_string()));
    }

    #[test]
    fn test_strip_blocked_terminal_params() {
        let mut args = json!({
            "command": "ls -la",
            "background": true,
            "pty": true,
            "timeout": 30,
        });
        strip_blocked_terminal_params(&mut args);
        assert!(args.get("background").is_none());
        assert!(args.get("pty").is_none());
        assert!(args.get("timeout").is_some());
    }

    #[test]
    fn test_generate_hermes_tools_module_uds() {
        let enabled = HashSet::new();
        let content = generate_hermes_tools_module(&enabled, "uds", Some("/tmp/test.sock"), None);
        assert!(content.contains("import json"));
        assert!(content.contains("import socket"));
        assert!(content.contains("def _call("));
        assert!(content.contains("def web_search(") || content.contains("def read_file("));
    }

    #[test]
    fn test_generate_hermes_tools_module_file() {
        let enabled = HashSet::new();
        let content = generate_hermes_tools_module(
            &enabled,
            "file",
            None,
            Some("/tmp/hermes_rpc"),
        );
        assert!(content.contains("import json"));
        assert!(content.contains("_RPC_DIR"));
        assert!(content.contains("def _call("));
    }

    #[test]
    fn test_generate_hermes_tools_helpers() {
        let enabled = HashSet::new();
        let content = generate_hermes_tools_module(&enabled, "uds", Some("/tmp/test.sock"), None);
        assert!(content.contains("def json_parse("));
        assert!(content.contains("def retry("));
    }

    #[test]
    fn test_is_available() {
        if !cfg!(target_os = "windows") {
            assert!(CodeExecutionTool::is_available());
        }
    }

    #[tokio::test]
    async fn test_execute_missing_code() {
        let tool = CodeExecutionTool::new();
        let ctx = ToolContext::default();
        let err = tool.execute(json!({}), &ctx).await.unwrap_err();
        assert!(err.to_string().contains("code"));
    }
}
