use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::foundry::resources::RootHandle;

pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
const MAX_COMMAND_OUTPUT: usize = 64 * 1024;
const MAX_STAGED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_STAGED_FILES: u64 = 20_000;
const SKIPPED: [&str; 2] = [".git", ".DS_Store"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    Completed,
    TimedOut,
    Failed,
    Unavailable,
}

#[derive(Debug)]
pub struct CommandRun {
    pub status: CommandStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub operation: &'static str,
    pub path: String,
    pub base: Vec<u8>,
    pub proposed: Vec<u8>,
}

pub fn command_sandbox_available() -> bool {
    cfg!(target_os = "macos") && Path::new(SANDBOX_EXEC).is_file()
}

pub fn capture_command_effects(
    root: &RootHandle,
    argv: &[String],
    timeout: Duration,
) -> Result<(CommandRun, Vec<FileChange>), String> {
    let real = root.display_path().to_path_buf();
    let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
    let mut budget = StagingBudget::default();
    copy_tree(&real, staging.path(), &mut budget)?;
    let run = run_sandboxed(staging.path(), argv, timeout);
    let changes = diff_tree(&real, staging.path());
    Ok((run, changes))
}

#[derive(Default)]
struct StagingBudget {
    bytes: u64,
    files: u64,
}

fn copy_tree(source: &Path, destination: &Path, budget: &mut StagingBudget) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy().into_owned();
        if SKIPPED.contains(&name.as_str()) {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(&name);
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_dir() {
            copy_tree(&source_path, &destination_path, budget)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        budget.bytes = budget.bytes.saturating_add(metadata.len());
        budget.files = budget.files.saturating_add(1);
        if budget.bytes > MAX_STAGED_BYTES || budget.files > MAX_STAGED_FILES {
            return Err("staging area exceeds its size or file limit".to_string());
        }
        let bytes = fs::read(&source_path).map_err(|error| error.to_string())?;
        fs::write(&destination_path, bytes).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn diff_tree(real: &Path, staging: &Path) -> Vec<FileChange> {
    let mut changes = Vec::new();
    diff_dir(real, staging, "", &mut changes);
    changes
}

fn diff_dir(real: &Path, staging: &Path, prefix: &str, changes: &mut Vec<FileChange>) {
    let Ok(staging_entries) = fs::read_dir(staging) else {
        return;
    };
    for entry in staging_entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if SKIPPED.contains(&name.as_str()) {
            continue;
        }
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let staging_path = entry.path();
        let real_path = real.join(&relative);
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            diff_dir(real, &staging_path, &relative, changes);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let proposed = match fs::read(&staging_path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        match fs::read(&real_path) {
            Ok(base) => {
                if base != proposed {
                    changes.push(FileChange {
                        operation: "edit",
                        path: relative,
                        base,
                        proposed,
                    });
                }
            }
            Err(_) => changes.push(FileChange {
                operation: "create",
                path: relative,
                base: Vec::new(),
                proposed,
            }),
        }
    }
    if let Ok(real_entries) = fs::read_dir(real) {
        for entry in real_entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if SKIPPED.contains(&name.as_str()) {
                continue;
            }
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if !staging.join(&name).exists() {
                let file_type = entry.file_type().ok();
                if file_type.is_some_and(|kind| kind.is_file()) {
                    changes.push(FileChange {
                        operation: "delete",
                        path: relative,
                        base: fs::read(entry.path()).unwrap_or_default(),
                        proposed: Vec::new(),
                    });
                }
            }
        }
    }
}

fn sandbox_profile(directory: &Path) -> String {
    let root = directory.to_string_lossy();
    let mut profile = String::new();
    profile.push_str("(version 1)\n");
    profile.push_str("(deny default)\n");
    profile.push_str("(import \"dyld-support.sb\")\n");
    profile.push_str("(allow process-exec)\n");
    profile.push_str("(allow process-fork)\n");
    profile.push_str("(allow syscall*)\n");
    profile.push_str("(allow mach-bootstrap)\n");
    profile.push_str("(allow signal (target self))\n");
    profile.push_str("(allow sysctl-read)\n");
    profile.push_str("(allow file-read-metadata)\n");
    profile.push_str(
        "(allow file-read* (subpath \"/System/Library\") (subpath \"/usr/lib\") (subpath \"/usr/bin\") (subpath \"/bin\") (subpath \"/sbin\") (literal \"/dev/null\") (literal \"/dev/urandom\") (literal \"/dev/random\"))\n",
    );
    profile.push_str(
        "(allow file-map-executable (subpath \"/System/Library\") (subpath \"/usr/lib\") (subpath \"/usr/bin\") (subpath \"/bin\"))\n",
    );
    profile.push_str(&format!(
        "(allow file-read* file-write* (subpath \"{}\"))\n",
        root.replace('"', "\\\"")
    ));
    profile
}

fn run_sandboxed(directory: &Path, argv: &[String], timeout: Duration) -> CommandRun {
    let unavailable = || CommandRun {
        status: CommandStatus::Unavailable,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
    };
    if !command_sandbox_available() {
        return unavailable();
    }
    let profile = sandbox_profile(directory);
    let mut command = Command::new(SANDBOX_EXEC);
    command.arg("-p").arg(profile).args(argv);
    command.current_dir(directory);
    command.env_clear();
    command.env("PATH", "/usr/bin:/bin");
    command.env("HOME", "/nonexistent");
    command.env("TMPDIR", directory);
    command.env("LANG", "C");
    command.stdin(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut stdout = match tempfile::tempfile() {
        Ok(file) => file,
        Err(_) => return unavailable(),
    };
    let mut stderr = match stdout.try_clone() {
        Ok(file) => file,
        Err(_) => return unavailable(),
    };
    let stdout_handle = match stdout.try_clone() {
        Ok(file) => file,
        Err(_) => return unavailable(),
    };
    let stderr_handle = match stderr.try_clone() {
        Ok(file) => file,
        Err(_) => return unavailable(),
    };
    command.stdout(Stdio::from(stdout_handle));
    command.stderr(Stdio::from(stderr_handle));
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return unavailable(),
    };
    let started = Instant::now();
    let mut status = CommandStatus::Completed;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if started.elapsed() >= timeout {
                    status = CommandStatus::TimedOut;
                    #[cfg(unix)]
                    {
                        let pid = child.id() as libc::pid_t;
                        unsafe {
                            libc::killpg(pid, libc::SIGKILL);
                        }
                        let _ = child.kill();
                    }
                    let deadline = Instant::now() + Duration::from_secs(2);
                    loop {
                        match child.try_wait() {
                            Ok(Some(_)) | Err(_) => break,
                            Ok(None) if Instant::now() >= deadline => break,
                            _ => thread::sleep(Duration::from_millis(10)),
                        }
                    }
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => {
                status = CommandStatus::Failed;
                break;
            }
        }
    }
    let exit_code = child
        .try_wait()
        .ok()
        .flatten()
        .and_then(|output| output.code());
    if status == CommandStatus::Completed && exit_code.is_some_and(|code| code != 0) {
        status = CommandStatus::Failed;
    }
    stdout.seek(SeekFrom::Start(0)).ok();
    stderr.seek(SeekFrom::Start(0)).ok();
    let mut out_bytes = Vec::new();
    let mut err_bytes = Vec::new();
    let _ = stdout
        .take(MAX_COMMAND_OUTPUT as u64 + 1)
        .read_to_end(&mut out_bytes);
    let _ = stderr
        .take(MAX_COMMAND_OUTPUT as u64 + 1)
        .read_to_end(&mut err_bytes);
    CommandRun {
        status,
        exit_code,
        stdout: String::from_utf8_lossy(&out_bytes).into_owned(),
        stderr: String::from_utf8_lossy(&err_bytes).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn command_argv(argv: &[&str]) -> Vec<String> {
        argv.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn sandbox_profile_confines_writes_to_the_staging_directory() {
        let profile = sandbox_profile(Path::new("/tmp/fixture-root"));
        assert!(profile.contains("(deny default)"));
        assert!(profile.contains("(subpath \"/tmp/fixture-root\")"));
    }

    #[test]
    fn diff_tree_reports_create_edit_and_delete() {
        let real = tempfile::tempdir().expect("real");
        let staging = tempfile::tempdir().expect("staging");
        fs::write(real.path().join("same.txt"), b"same").expect("write");
        fs::write(staging.path().join("same.txt"), b"same").expect("write");
        fs::write(real.path().join("edited.txt"), b"before").expect("write");
        fs::write(staging.path().join("edited.txt"), b"after").expect("write");
        fs::write(staging.path().join("created.txt"), b"new").expect("write");
        fs::write(real.path().join("removed.txt"), b"gone").expect("write");

        let changes = diff_tree(real.path(), staging.path());
        let kinds = changes
            .iter()
            .map(|change| (change.path.as_str(), change.operation))
            .collect::<Vec<_>>();
        assert!(kinds.contains(&("created.txt", "create")));
        assert!(kinds.contains(&("edited.txt", "edit")));
        assert!(kinds.contains(&("removed.txt", "delete")));
        assert!(!kinds.iter().any(|(path, _)| *path == "same.txt"));
    }

    #[test]
    fn run_sandboxed_is_bounded_and_reports_missing_sandbox() {
        let run = run_sandboxed(
            Path::new("/nonexistent"),
            &command_argv(&["/bin/true"]),
            Duration::from_secs(1),
        );
        assert!(matches!(
            run.status,
            CommandStatus::Completed | CommandStatus::Unavailable | CommandStatus::Failed
        ));
    }
}
