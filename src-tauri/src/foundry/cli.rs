use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use semver::Version;
use serde::de::DeserializeOwned;
use tokio::process::Command as AsyncCommand;

use super::error::{FoundryError, FoundryResult};
use super::json::{CliErrorPayload, CliInfo, parse_json};

pub const MINIMUM_CLI_VERSION: &str = "0.13.0";

const STOP_TIMEOUT: Duration = Duration::from_secs(15);

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone)]
pub struct CliBackend {
    binary: PathBuf,
    version: Version,
}

impl CliBackend {
    pub fn detect() -> FoundryResult<Self> {
        Self::detect_from(&candidate_paths())
    }

    pub(crate) fn detect_from(candidates: &[PathBuf]) -> FoundryResult<Self> {
        let minimum = Version::parse(MINIMUM_CLI_VERSION).expect("minimum version is valid");
        let mut best: Option<(Version, PathBuf)> = None;
        let mut last_failure: Option<String> = None;

        for candidate in candidates {
            if !is_executable(candidate) {
                continue;
            }

            match probe_version(candidate) {
                Ok(version) => {
                    let is_better = best.as_ref().is_none_or(|(current, _)| version > *current);

                    if is_better {
                        best = Some((version, candidate.clone()));
                    }
                }
                Err(error) => last_failure = Some(error),
            }
        }

        if let Some((version, binary)) = best {
            if version < minimum {
                return Err(FoundryError::CliOutdated {
                    found: version.to_string(),
                    minimum: MINIMUM_CLI_VERSION.to_string(),
                });
            }

            return Ok(Self { binary, version });
        }

        if let Some(detail) = last_failure {
            return Err(FoundryError::CommandFailed {
                command: format!("{} --version", binary_name()),
                message: detail,
                hint: None,
            });
        }

        Err(FoundryError::CliMissing)
    }

    pub fn info(&self) -> CliInfo {
        CliInfo {
            path: self.binary.to_string_lossy().into_owned(),
            version: self.version.to_string(),
            minimum_version: MINIMUM_CLI_VERSION.to_string(),
        }
    }

    pub async fn run_json<T: DeserializeOwned>(&self, args: &[&str]) -> FoundryResult<T> {
        let output = self.run(args, true, &[]).await?;

        parse_json(&output.stdout).map_err(|detail| FoundryError::BadResponse { detail })
    }

    pub async fn run_json_env<T: DeserializeOwned>(
        &self,
        args: &[&str],
        envs: &[(&str, &str)],
    ) -> FoundryResult<T> {
        let output = self.run(args, true, envs).await?;

        parse_json(&output.stdout).map_err(|detail| FoundryError::BadResponse { detail })
    }

    pub async fn run_text(&self, args: &[&str]) -> FoundryResult<String> {
        let output = self.run(args, false, &[]).await?;

        Ok(output.stdout.trim().to_string())
    }

    pub async fn run_text_env(
        &self,
        args: &[&str],
        envs: &[(&str, &str)],
    ) -> FoundryResult<String> {
        let output = self.run(args, false, envs).await?;

        Ok(output.stdout.trim().to_string())
    }

    pub fn spawn_text(&self, args: &[&str]) -> FoundryResult<CliProcess> {
        let stdout_file = capture_file(self, args)?;
        let stderr_file = capture_file(self, args)?;
        let stdout_target = stdout_file
            .try_clone()
            .map_err(|error| self.capture_error(args, error.to_string()))?;
        let stderr_target = stderr_file
            .try_clone()
            .map_err(|error| self.capture_error(args, error.to_string()))?;

        let mut command = AsyncCommand::new(&self.binary);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout_target))
            .stderr(Stdio::from(stderr_target))
            .kill_on_drop(true);

        suppress_window_async(&mut command);

        let child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                FoundryError::CliMissing
            } else {
                FoundryError::CommandFailed {
                    command: self.display_command(args),
                    message: error.to_string(),
                    hint: None,
                }
            }
        })?;

        Ok(CliProcess {
            child,
            stdout_file,
            stderr_file,
            command: self.display_command(args),
        })
    }

    async fn run(
        &self,
        args: &[&str],
        as_json: bool,
        envs: &[(&str, &str)],
    ) -> FoundryResult<CapturedOutput> {
        let stdout_file = capture_file(self, args)?;
        let stderr_file = capture_file(self, args)?;
        let stdout_target = stdout_file
            .try_clone()
            .map_err(|error| self.capture_error(args, error.to_string()))?;
        let stderr_target = stderr_file
            .try_clone()
            .map_err(|error| self.capture_error(args, error.to_string()))?;

        let mut command = AsyncCommand::new(&self.binary);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout_target))
            .stderr(Stdio::from(stderr_target))
            .kill_on_drop(true);

        suppress_window_async(&mut command);

        if !envs.is_empty() {
            command.envs(envs.iter().copied());
        }

        if as_json {
            command.args(["-o", "json"]);
        }

        let status = command.status().await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                FoundryError::CliMissing
            } else {
                FoundryError::CommandFailed {
                    command: self.display_command(args),
                    message: error.to_string(),
                    hint: None,
                }
            }
        })?;

        let output = CapturedOutput {
            status,
            stdout: read_capture(stdout_file).unwrap_or_default(),
            stderr: read_capture(stderr_file).unwrap_or_default(),
        };

        if output.status.success() {
            return Ok(output);
        }

        Err(self.failure(args, &output))
    }

    fn capture_error(&self, args: &[&str], detail: String) -> FoundryError {
        FoundryError::CommandFailed {
            command: self.display_command(args),
            message: format!("could not capture command output: {detail}"),
            hint: None,
        }
    }

    fn failure(&self, args: &[&str], output: &CapturedOutput) -> FoundryError {
        failure_from(
            self.display_command(args),
            &output.stdout,
            &output.stderr,
            output.status,
        )
    }

    fn display_command(&self, args: &[&str]) -> String {
        format!("{} {}", binary_name(), args.join(" "))
    }

    pub fn stop_blocking(&self) {
        let mut command = Command::new(&self.binary);
        command
            .args(["server", "stop"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        suppress_window(&mut command);

        let child = command.spawn();

        let Ok(mut child) = child else {
            return;
        };

        let deadline = Instant::now() + STOP_TIMEOUT;

        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(_) => return,
            }
        }
    }
}

pub struct CliProcess {
    child: tokio::process::Child,
    stdout_file: File,
    stderr_file: File,
    command: String,
}

impl CliProcess {
    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    pub fn captured_stdout(&self) -> String {
        self.stdout_file
            .try_clone()
            .ok()
            .and_then(|file| read_capture(file).ok())
            .unwrap_or_default()
    }

    pub async fn cancel(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }

    pub fn exit_error(&mut self, status: ExitStatus) -> FoundryError {
        let stdout = self
            .stdout_file
            .try_clone()
            .ok()
            .and_then(|file| read_capture(file).ok())
            .unwrap_or_default();
        let stderr = self
            .stderr_file
            .try_clone()
            .ok()
            .and_then(|file| read_capture(file).ok())
            .unwrap_or_default();

        failure_from(self.command.clone(), &stdout, &stderr, status)
    }
}

struct CapturedOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn failure_from(command: String, stdout: &str, stderr: &str, status: ExitStatus) -> FoundryError {
    if let Some(problem) = error_problem(stdout).or_else(|| error_problem(stderr)) {
        return FoundryError::CommandFailed {
            command,
            message: problem.message,
            hint: problem.hint,
        };
    }

    let message = last_non_empty(stderr)
        .or_else(|| last_non_empty(stdout))
        .unwrap_or_else(|| format!("the command exited with {status}"));

    FoundryError::CommandFailed {
        command,
        message,
        hint: None,
    }
}

fn capture_file(backend: &CliBackend, args: &[&str]) -> FoundryResult<File> {
    tempfile::tempfile().map_err(|error| backend.capture_error(args, error.to_string()))
}

fn read_capture(mut file: File) -> std::io::Result<String> {
    let mut buffer = Vec::new();
    file.seek(SeekFrom::Start(0))?;
    file.read_to_end(&mut buffer)?;

    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

fn error_problem(text: &str) -> Option<super::json::CliProblem> {
    parse_json::<CliErrorPayload>(text)
        .ok()
        .and_then(|payload| payload.error)
}

fn last_non_empty(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_string)
}

fn probe_version(binary: &Path) -> Result<Version, String> {
    let mut command = Command::new(binary);
    command.arg("--version").stdin(Stdio::null());
    suppress_window(&mut command);

    let output = command.output().map_err(|error| error.to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        return Err(last_non_empty(&stderr).unwrap_or_else(|| "version check failed".to_string()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    extract_version(&stdout).ok_or_else(|| format!("unexpected version output: {}", stdout.trim()))
}

fn extract_version(text: &str) -> Option<Version> {
    text.split_whitespace()
        .find_map(|token| Version::parse(token.trim_start_matches('v')).ok())
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            push_unique(&mut candidates, directory.join(binary_name()));
        }
    }

    for directory in ["/opt/homebrew/bin", "/usr/local/bin"] {
        push_unique(&mut candidates, Path::new(directory).join(binary_name()));
    }

    if let Some(home) = dirs::home_dir() {
        push_unique(
            &mut candidates,
            home.join(".foundry").join("bin").join(binary_name()),
        );
    }

    candidates
}

fn push_unique(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.contains(&candidate) {
        paths.push(candidate);
    }
}

#[cfg(windows)]
fn binary_name() -> &'static str {
    "foundry.exe"
}

#[cfg(not(windows))]
fn binary_name() -> &'static str {
    "foundry"
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };

    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
}

#[cfg(windows)]
fn suppress_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn suppress_window(_command: &mut Command) {}

#[cfg(windows)]
fn suppress_window_async(command: &mut AsyncCommand) {
    use std::os::windows::process::CommandExt;

    command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn suppress_window_async(_command: &mut AsyncCommand) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_version_from_common_outputs() {
        assert_eq!(
            extract_version("0.13.0"),
            Some(Version::parse("0.13.0").expect("valid"))
        );
        assert_eq!(
            extract_version("foundry 0.13.0"),
            Some(Version::parse("0.13.0").expect("valid"))
        );
        assert_eq!(
            extract_version("v0.14.2\n"),
            Some(Version::parse("0.14.2").expect("valid"))
        );
        assert_eq!(extract_version("no version here"), None);
    }

    #[test]
    fn deduplicates_candidate_paths() {
        let mut paths = Vec::new();

        push_unique(&mut paths, PathBuf::from("/usr/bin/foundry"));
        push_unique(&mut paths, PathBuf::from("/usr/bin/foundry"));
        push_unique(&mut paths, PathBuf::from("/opt/homebrew/bin/foundry"));

        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn reports_missing_cli() {
        let error = CliBackend::detect_from(&[]).expect_err("cli is missing");

        assert_eq!(error.code(), "CliMissing");
    }

    #[cfg(unix)]
    #[test]
    fn prefers_the_newest_installed_cli() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temp directory");
        let old = directory.path().join("old-foundry");
        let new = directory.path().join("new-foundry");

        for (path, version) in [(&old, "0.12.0"), (&new, "0.13.0")] {
            let mut script = File::create(path).expect("script file");
            script
                .write_all(format!("#!/bin/sh\necho {version}\n").as_bytes())
                .expect("script writes");
            drop(script);
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
                .expect("script is executable");
        }

        let backend = CliBackend::detect_from(&[old, new.clone()]).expect("newest cli is selected");

        assert_eq!(backend.info().version, "0.13.0");
        assert_eq!(backend.info().path, new.to_string_lossy());
    }

    #[cfg(unix)]
    #[test]
    fn reports_outdated_cli() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temp directory");
        let binary = directory.path().join("foundry");
        let mut script = File::create(&binary).expect("script file");
        script
            .write_all(b"#!/bin/sh\necho 0.12.0\n")
            .expect("script writes");
        drop(script);
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
            .expect("script is executable");

        let error = CliBackend::detect_from(&[binary]).expect_err("cli is outdated");

        assert_eq!(error.code(), "CliOutdated");
    }
}
