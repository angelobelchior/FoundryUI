use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::command_execution::{self, CommandRun, CommandStatus, FileChange};

const MAX_TERMINAL_OUTPUT: usize = 64 * 1024;
const MARKER_WINDOW: usize = 512;

pub struct TerminalSession {
    _staging: tempfile::TempDir,
    staging_root: PathBuf,
    real: PathBuf,
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    stderr: ChildStderr,
    cwd: String,
    counter: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSessionStatus {
    pub root_id: String,
    pub cwd: String,
    pub alive: bool,
}

pub fn terminal_sandbox_available() -> bool {
    command_execution::command_sandbox_available()
}

pub fn run_session_command(
    root_id: &str,
    real: &Path,
    command: &str,
    timeout: Duration,
) -> Result<(CommandRun, Vec<FileChange>), String> {
    if !terminal_sandbox_available() {
        return Err("terminal isolation is unavailable on this platform".to_string());
    }
    let mut sessions = sessions()
        .lock()
        .map_err(|_| "the terminal session registry is unavailable".to_string())?;
    let session = match sessions.get_mut(root_id) {
        Some(session) => session,
        None => {
            let opened = TerminalSession::open(real)?;
            sessions.insert(root_id.to_string(), opened);
            sessions
                .get_mut(root_id)
                .expect("the terminal session was just inserted")
        }
    };
    session.run(command, timeout)
}

pub fn session_statuses() -> Vec<TerminalSessionStatus> {
    let Ok(mut sessions) = sessions().lock() else {
        return Vec::new();
    };
    sessions
        .iter_mut()
        .map(|(root_id, session)| TerminalSessionStatus {
            root_id: root_id.clone(),
            cwd: session.cwd.clone(),
            alive: session.is_alive(),
        })
        .collect()
}

fn sessions() -> &'static Mutex<HashMap<String, TerminalSession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, TerminalSession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

impl TerminalSession {
    pub fn open(real: &Path) -> Result<Self, String> {
        let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
        let staging_root = fs::canonicalize(staging.path()).map_err(|error| error.to_string())?;
        command_execution::copy_tree(real, &staging_root)?;
        let (child, stdin, stdout, stderr) = spawn_shell(&staging_root)?;
        Ok(Self {
            _staging: staging,
            staging_root,
            real: real.to_path_buf(),
            child,
            stdin,
            stdout,
            stderr,
            cwd: String::new(),
            counter: 0,
        })
    }

    pub fn run(
        &mut self,
        command: &str,
        timeout: Duration,
    ) -> Result<(CommandRun, Vec<FileChange>), String> {
        if !self.is_alive() {
            self.respawn()?;
        }
        let run = self.run_command(command, timeout);
        let changes = command_execution::diff_tree(&self.real, &self.staging_root);
        if run.status == CommandStatus::Completed {
            self.update_cwd(command);
        }
        Ok((run, changes))
    }

    fn run_command(&mut self, command: &str, timeout: Duration) -> CommandRun {
        self.counter = self.counter.wrapping_add(1);
        let nonce = self.counter;
        let script = format!("{command}\nprintf '\\n__FOUNDRY_EOM_{nonce}_%s__\\n' \"$?\"\n");
        if self
            .stdin
            .write_all(script.as_bytes())
            .and_then(|_| self.stdin.flush())
            .is_err()
        {
            return CommandRun {
                status: CommandStatus::Failed,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
            };
        }
        let prefix = format!("__FOUNDRY_EOM_{nonce}_");
        let mut stdout_capture = OutputCapture::new(MAX_TERMINAL_OUTPUT, MARKER_WINDOW);
        let mut stderr_bytes = Vec::new();
        let started = Instant::now();
        let mut status = CommandStatus::Completed;
        let mut marker_found = false;
        loop {
            let _ = capture_available(&mut self.stdout, &mut stdout_capture);
            let _ = capture_stderr(&mut self.stderr, &mut stderr_bytes, MAX_TERMINAL_OUTPUT);
            if stdout_capture.contains(prefix.as_bytes()) {
                marker_found = true;
                break;
            }
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    status = CommandStatus::Failed;
                    break;
                }
                Ok(None) => {}
                Err(_) => {
                    status = CommandStatus::Failed;
                    break;
                }
            }
            if started.elapsed() >= timeout {
                status = CommandStatus::TimedOut;
                self.terminate();
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let (raw_stdout, exit_code) = if marker_found {
            extract_output(stdout_capture.head(), stdout_capture.tail(), prefix.as_bytes())
        } else {
            (stdout_capture.head().to_vec(), None)
        };
        if status == CommandStatus::Completed && exit_code.is_some_and(|code| code != 0) {
            status = CommandStatus::Failed;
        }
        CommandRun {
            status,
            exit_code,
            stdout: sanitize_output(&raw_stdout),
            stderr: sanitize_output(&stderr_bytes),
        }
    }

    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn respawn(&mut self) -> Result<(), String> {
        self.terminate();
        let (child, stdin, stdout, stderr) = spawn_shell(&self.staging_root)?;
        self.child = child;
        self.stdin = stdin;
        self.stdout = stdout;
        self.stderr = stderr;
        self.cwd.clear();
        Ok(())
    }

    fn terminate(&mut self) {
        #[cfg(unix)]
        {
            let pid = self.child.id() as libc::pid_t;
            unsafe {
                libc::killpg(pid, libc::SIGKILL);
            }
        }
        let _ = self.child.kill();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) if Instant::now() >= deadline => break,
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    }

    fn update_cwd(&mut self, command: &str) {
        let Some(target) = detect_cd(command) else {
            return;
        };
        self.cwd = join_cwd(&self.cwd, target);
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn spawn_shell(
    directory: &Path,
) -> Result<(Child, ChildStdin, ChildStdout, ChildStderr), String> {
    let profile = command_execution::sandbox_profile(directory);
    let mut command = Command::new(command_execution::SANDBOX_EXEC);
    command.arg("-p").arg(profile).arg("/bin/sh");
    command.current_dir(directory);
    command.env_clear();
    command.env("PATH", "/usr/bin:/bin");
    command.env("HOME", "/nonexistent");
    command.env("TMPDIR", directory);
    command.env("LANG", "C");
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "the terminal shell has no stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "the terminal shell has no stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "the terminal shell has no stderr".to_string())?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        set_nonblocking(stdout.as_raw_fd());
        set_nonblocking(stderr.as_raw_fd());
    }
    Ok((child, stdin, stdout, stderr))
}

#[cfg(unix)]
fn set_nonblocking(fd: std::os::fd::RawFd) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
}

struct OutputCapture {
    head: Vec<u8>,
    tail: Vec<u8>,
    head_cap: usize,
    tail_cap: usize,
}

impl OutputCapture {
    fn new(head_cap: usize, tail_cap: usize) -> Self {
        Self {
            head: Vec::new(),
            tail: Vec::new(),
            head_cap,
            tail_cap,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        let remaining = self.head_cap.saturating_sub(self.head.len());
        self.head.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
        self.tail.extend_from_slice(bytes);
        if self.tail.len() > self.tail_cap {
            let excess = self.tail.len() - self.tail_cap;
            self.tail.drain(..excess);
        }
    }

    fn contains(&self, needle: &[u8]) -> bool {
        find_subsequence(&self.tail, needle).is_some()
    }

    fn head(&self) -> &[u8] {
        &self.head
    }

    fn tail(&self) -> &[u8] {
        &self.tail
    }
}

fn capture_available(reader: &mut ChildStdout, capture: &mut OutputCapture) -> std::io::Result<()> {
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(n) => capture.push(&chunk[..n]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(_) => return Ok(()),
        }
    }
}

fn capture_stderr(
    reader: &mut ChildStderr,
    buffer: &mut Vec<u8>,
    cap: usize,
) -> std::io::Result<()> {
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(n) => {
                let space = cap.saturating_sub(buffer.len());
                buffer.extend_from_slice(&chunk[..n.min(space)]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(_) => return Ok(()),
        }
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn extract_output(head: &[u8], tail: &[u8], prefix: &[u8]) -> (Vec<u8>, Option<i32>) {
    let reported = match find_subsequence(head, prefix) {
        Some(index) => {
            let mut output = head[..index].to_vec();
            while output.last().is_some_and(|byte| *byte == b'\n' || *byte == b'\r') {
                output.pop();
            }
            output
        }
        None => head.to_vec(),
    };
    let exit_code = find_subsequence(tail, prefix).and_then(|index| {
        let after = &tail[index + prefix.len()..];
        let end = find_subsequence(after, b"__").unwrap_or(after.len());
        String::from_utf8_lossy(&after[..end]).trim().parse::<i32>().ok()
    });
    (reported, exit_code)
}

fn sanitize_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|character| {
            if character.is_control() && !matches!(character, '\n' | '\r' | '\t') {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn detect_cd(command: &str) -> Option<&str> {
    let trimmed = command.trim();
    let rest = trimmed.strip_prefix("cd")?;
    if rest.is_empty() {
        return Some("");
    }
    rest.starts_with(char::is_whitespace).then(|| rest.trim_start())
}

fn join_cwd(current: &str, target: &str) -> String {
    if target.is_empty() {
        return String::new();
    }
    if target.starts_with('/') {
        return target.trim_start_matches('/').to_string();
    }
    let mut parts = current
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_cd_parses_cd_commands() {
        assert_eq!(detect_cd("cd"), Some(""));
        assert_eq!(detect_cd("cd src"), Some("src"));
        assert_eq!(detect_cd("cd src/nested"), Some("src/nested"));
        assert_eq!(detect_cd("echo cd"), None);
        assert_eq!(detect_cd("cdx"), None);
    }

    #[test]
    fn join_cwd_normalizes_relative_paths() {
        assert_eq!(join_cwd("", "src"), "src");
        assert_eq!(join_cwd("src", ".."), "");
        assert_eq!(join_cwd("src", "nested"), "src/nested");
        assert_eq!(join_cwd("src/nested", ".."), "src");
        assert_eq!(join_cwd("src", "/abs"), "abs");
    }

    #[test]
    fn extract_output_splits_the_sentinel_and_exit_code() {
        let head = b"hello\n__FOUNDRY_EOM_7_0__\n";
        let tail = head;
        let (output, exit_code) = extract_output(head, tail, b"__FOUNDRY_EOM_7_");
        assert_eq!(output, b"hello");
        assert_eq!(exit_code, Some(0));
    }

    #[test]
    fn sanitize_output_strips_escape_sequences_but_keeps_newlines() {
        let input = b"line1\n\x1b[31mline2\x1b[0m\n";
        let output = sanitize_output(input);
        assert!(output.contains("line1"));
        assert!(output.contains("line2"));
        assert!(output.contains('\n'));
        assert!(!output.contains('\u{1b}'));
    }

    #[test]
    fn the_terminal_session_returns_output() {
        if !terminal_sandbox_available() {
            return;
        }
        let directory = tempfile::tempdir().expect("directory");
        let root_id = format!("terminal-output-{}", std::process::id());
        let (run, changes) =
            run_session_command(&root_id, directory.path(), "printf 'hello'", Duration::from_secs(5))
                .expect("run");
        assert_eq!(run.stdout, "hello");
        assert!(changes.is_empty());
    }

    #[test]
    fn the_terminal_session_detects_file_changes() {
        if !terminal_sandbox_available() {
            return;
        }
        let directory = tempfile::tempdir().expect("directory");
        let root_id = format!("terminal-change-{}", std::process::id());
        let (_, changes) = run_session_command(
            &root_id,
            directory.path(),
            "printf 'hello' > created.txt",
            Duration::from_secs(5),
        )
        .expect("run");
        assert!(changes
            .iter()
            .any(|change| change.path == "created.txt" && change.operation == "create"));
        assert!(!directory.path().join("created.txt").exists());
    }

    #[test]
    fn the_terminal_session_confines_writes_to_the_root() {
        if !terminal_sandbox_available() {
            return;
        }
        let directory = tempfile::tempdir().expect("directory");
        let outside = std::env::temp_dir().join(format!(
            "foundry_terminal_escape_{}_{}",
            std::process::id(),
            now_nanos()
        ));
        let root_id = format!("terminal-escape-{}", std::process::id());
        let (_, changes) = run_session_command(
            &root_id,
            directory.path(),
            &format!("printf 'escape' > {}", outside.display()),
            Duration::from_secs(5),
        )
        .expect("run");
        assert!(!outside.exists());
        assert!(changes.is_empty());
        let _ = std::fs::remove_file(&outside);
    }

    fn now_nanos() -> u128 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    }
}
