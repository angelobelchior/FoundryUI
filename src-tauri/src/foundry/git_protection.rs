use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use super::resources::{RelativePath, ResourceErrorKind, RootHandle};

const MAX_GIT_OUTPUT: usize = 65_536;
const MAX_PROTECTED_BYTES: usize = 8 * 1024 * 1024;
const CANDIDATES: [&str; 3] = [
    "/opt/homebrew/bin/git",
    "/usr/local/bin/git",
    "/usr/bin/git",
];
const PRIVATE_REF_PREFIX: &str = "refs/foundryui/recovery/";
static UNIQUE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtectionOperation {
    Create,
    Edit,
    Delete,
}

impl ProtectionOperation {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "create" => Some(Self::Create),
            "edit" => Some(Self::Edit),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryState {
    WorkTree,
    Absent,
    Bare,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeState {
    Clean,
    Unstaged,
    Staged,
    StagedAndUnstaged,
    Untracked,
    Ignored,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtectionReason {
    Eligible,
    GitNotInstalled,
    NotRepository,
    BareRepository,
    RepositoryInvalid,
    Untracked,
    Ignored,
    TargetExists,
    Missing,
    UnsupportedType,
    Unreadable,
    LimitExceeded,
    GitError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileProtection {
    pub path: String,
    pub operation: ProtectionOperation,
    pub tracked: bool,
    pub ignored: bool,
    pub worktree: WorktreeState,
    pub eligible: bool,
    pub reason: ProtectionReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtectionWarningCode {
    NoGit,
    NoRepository,
    BareRepository,
    RepositoryInvalid,
    UnprotectedFiles,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectionWarning {
    pub code: ProtectionWarningCode,
    pub message_key: String,
    pub message: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectionInspection {
    pub git_installed: bool,
    pub git_version: Option<String>,
    pub repository: RepositoryState,
    pub files: Vec<FileProtection>,
    pub warning: Option<ProtectionWarning>,
    pub undo_available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparedKind {
    Content,
    Absence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedProtection {
    pub reference: String,
    pub commit: String,
    pub tree: String,
    pub kind: PreparedKind,
    pub blob: Option<String>,
    pub size: u64,
    pub mode: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoredContent {
    pub kind: PreparedKind,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[error("git protection failed: {kind:?}")]
pub struct ProtectionError {
    pub kind: ProtectionErrorKind,
    pub reason: ProtectionReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtectionErrorKind {
    NotEligible,
    CommandFailed,
    InvalidPath,
    InvalidReference,
    Unavailable,
}

#[derive(Clone)]
struct GitCli {
    executable: PathBuf,
    version: String,
}

struct Repository {
    work_tree: PathBuf,
    prefix: PathBuf,
}

struct CommandResult {
    status: Option<i32>,
    stdout: Vec<u8>,
}

impl CommandResult {
    fn ok(&self) -> bool {
        self.status == Some(0)
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim().to_string()
    }
}

pub fn git_executable() -> Option<PathBuf> {
    resolve().map(|git| git.executable)
}

pub fn inspect(
    root: &RootHandle,
    operation: ProtectionOperation,
    relative: &RelativePath,
) -> ProtectionInspection {
    let git = resolve();
    let (repository, repository_state) = match &git {
        Some(git) => match Repository::open(git, root.display_path()) {
            Ok(repository) => (Some(repository), RepositoryState::WorkTree),
            Err(state) => (None, state),
        },
        None => (None, RepositoryState::Absent),
    };
    let file = inspect_file(
        git.as_ref(),
        repository.as_ref(),
        repository_state,
        root,
        operation,
        relative,
    );
    let warning = warning_for(git.as_ref(), repository_state, std::slice::from_ref(&file));
    ProtectionInspection {
        git_installed: git.is_some(),
        git_version: git.as_ref().map(|git| git.version.clone()),
        repository: repository_state,
        files: vec![file],
        warning,
        undo_available: false,
    }
}

pub fn prepare(
    root: &RootHandle,
    operation: ProtectionOperation,
    relative: &RelativePath,
    suffix: &str,
) -> Result<PreparedProtection, ProtectionError> {
    let git = resolve().ok_or_else(|| failure(ProtectionReason::GitNotInstalled))?;
    let repository = Repository::open(&git, root.display_path())
        .map_err(|state| failure(state_reason(state)))?;
    let inspection = inspect_file(
        Some(&git),
        Some(&repository),
        RepositoryState::WorkTree,
        root,
        operation,
        relative,
    );
    if !inspection.eligible {
        return Err(ProtectionError {
            kind: ProtectionErrorKind::NotEligible,
            reason: inspection.reason,
        });
    }
    let reference = private_reference(suffix)?;
    let git_path = repository.git_path(relative)?;
    let (bytes, mode) = match operation {
        ProtectionOperation::Create => (None, 0o100644u32),
        ProtectionOperation::Edit | ProtectionOperation::Delete => {
            let bytes = read_source(root, relative)?;
            let mode = source_mode(root, relative);
            (Some(bytes), mode)
        }
    };
    let temporary = tempfile::tempdir().map_err(|_| command_failure())?;
    let index = temporary.path().join("index");
    let mut blob = None;
    let mut size = 0u64;
    if let Some(bytes) = &bytes {
        let object = hash_object(&git, &repository, bytes)?;
        size = bytes.len() as u64;
        update_index(&git, &repository, &index, mode, &object, &git_path)?;
        blob = Some(object);
    }
    let tree = run_ok(
        &git,
        &repository.work_tree,
        &["write-tree"],
        &[("GIT_INDEX_FILE", index.as_os_str())],
    )?
    .text();
    validate_object_id(&tree)?;
    let message = format!("FoundryUI recovery {reference}");
    let mut command = git.command(&repository.work_tree);
    command.args(["commit-tree", tree.as_str(), "-m", message.as_str()]);
    apply_identity(&mut command);
    let output = finish(command).map_err(|_| command_failure())?;
    if !output.ok() {
        return Err(command_failure());
    }
    let commit = output.text();
    validate_object_id(&commit)?;
    let update = run_ok(
        &git,
        &repository.work_tree,
        &["update-ref", reference.as_str(), commit.as_str()],
        &[],
    );
    if update.is_err() {
        return Err(command_failure());
    }
    let verified = run_ok(
        &git,
        &repository.work_tree,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
        &[],
    )
    .map(|result| result.text())
    .unwrap_or_default();
    if verified != commit {
        let _ = git
            .command(&repository.work_tree)
            .args(["update-ref", "-d", reference.as_str()])
            .output();
        return Err(command_failure());
    }
    Ok(PreparedProtection {
        reference,
        commit,
        tree,
        kind: if bytes.is_some() {
            PreparedKind::Content
        } else {
            PreparedKind::Absence
        },
        blob,
        size,
        mode,
    })
}

pub fn read_recovery(
    root: &RootHandle,
    relative: &RelativePath,
    reference: &str,
) -> Result<RestoredContent, ProtectionError> {
    validate_reference(reference)?;
    let git = resolve().ok_or_else(|| failure(ProtectionReason::GitNotInstalled))?;
    let repository = Repository::open(&git, root.display_path())
        .map_err(|state| failure(state_reason(state)))?;
    let commit = run_ok(
        &git,
        &repository.work_tree,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
        &[],
    )?
    .text();
    validate_object_id(&commit)?;
    let git_path = repository.git_path(relative)?;
    let literal = format!(":(literal){git_path}");
    let listing = run_ok(
        &git,
        &repository.work_tree,
        &["ls-tree", commit.as_str(), "--", literal.as_str()],
        &[],
    )?
    .text();
    if listing.is_empty() {
        return Ok(RestoredContent {
            kind: PreparedKind::Absence,
            bytes: Vec::new(),
        });
    }
    let object = listing
        .split_whitespace()
        .nth(2)
        .filter(|object| object.len() >= 40 && object.len() <= 64)
        .filter(|object| object.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(command_failure)?;
    let bytes = run_bytes(
        &git,
        &repository.work_tree,
        &["cat-file", "blob", object],
        &[],
        MAX_PROTECTED_BYTES,
    )?;
    Ok(RestoredContent {
        kind: PreparedKind::Content,
        bytes,
    })
}

fn validate_reference(reference: &str) -> Result<(), ProtectionError> {
    if !reference.starts_with(PRIVATE_REF_PREFIX)
        || reference.contains("..")
        || reference.ends_with(".lock")
        || reference.len() > 256
        || !reference
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_./-".contains(&byte))
    {
        return Err(ProtectionError {
            kind: ProtectionErrorKind::InvalidReference,
            reason: ProtectionReason::GitError,
        });
    }
    Ok(())
}

fn run_bytes(
    git: &GitCli,
    directory: &Path,
    args: &[&str],
    environment: &[(&str, &OsStr)],
    max_bytes: usize,
) -> Result<Vec<u8>, ProtectionError> {
    let mut command = git.command(directory);
    command.args(args);
    for (key, value) in environment {
        command.env(key, value);
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|_| command_failure())?;
    let stdout = child.stdout.take().ok_or_else(command_failure)?;
    let mut bytes = Vec::new();
    stdout
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| command_failure())?;
    let status = child.wait().map_err(|_| command_failure())?;
    if !status.success() {
        return Err(command_failure());
    }
    if bytes.len() > max_bytes {
        return Err(ProtectionError {
            kind: ProtectionErrorKind::Unavailable,
            reason: ProtectionReason::LimitExceeded,
        });
    }
    Ok(bytes)
}

fn resolve() -> Option<GitCli> {
    CANDIDATES.iter().map(PathBuf::from).find_map(|executable| {
        if !executable.is_file() {
            return None;
        }
        let mut command = Command::new(&executable);
        command.arg("--version").stdin(Stdio::null());
        apply_base_environment(&mut command);
        let output = finish(command).ok()?;
        if !output.ok() {
            return None;
        }
        let version = sanitize_line(&output.text());
        (!version.is_empty()).then_some(GitCli {
            executable,
            version,
        })
    })
}

impl GitCli {
    fn command(&self, directory: &Path) -> Command {
        let mut command = Command::new(&self.executable);
        command.arg("-C").arg(directory);
        apply_base_environment(&mut command);
        command.stdin(Stdio::null());
        command
    }
}

impl Repository {
    fn open(git: &GitCli, root: &Path) -> Result<Self, RepositoryState> {
        let bare = run(git, root, &["rev-parse", "--is-bare-repository"], &[]);
        if bare.as_ref().is_some_and(CommandResult::ok)
            && bare.as_ref().is_some_and(|result| result.text() == "true")
        {
            return Err(RepositoryState::Bare);
        }
        let top = run(git, root, &["rev-parse", "--show-toplevel"], &[])
            .ok_or(RepositoryState::Absent)?;
        if !top.ok() {
            return Err(RepositoryState::Absent);
        }
        let work_tree = std::fs::canonicalize(top.text()).map_err(|_| RepositoryState::Invalid)?;
        if !work_tree.is_dir() {
            return Err(RepositoryState::Invalid);
        }
        let root = std::fs::canonicalize(root).map_err(|_| RepositoryState::Invalid)?;
        let prefix = root
            .strip_prefix(&work_tree)
            .map_err(|_| RepositoryState::Invalid)?
            .to_path_buf();
        Ok(Self { work_tree, prefix })
    }

    fn git_path(&self, relative: &RelativePath) -> Result<String, ProtectionError> {
        let path = self.prefix.join(relative.as_path());
        let text = path.to_str().ok_or(ProtectionError {
            kind: ProtectionErrorKind::InvalidPath,
            reason: ProtectionReason::GitError,
        })?;
        if text.is_empty() || text.starts_with('/') || text.contains('\0') || text.contains('\\') {
            return Err(ProtectionError {
                kind: ProtectionErrorKind::InvalidPath,
                reason: ProtectionReason::GitError,
            });
        }
        Ok(text.to_string())
    }
}

fn inspect_file(
    git: Option<&GitCli>,
    repository: Option<&Repository>,
    state: RepositoryState,
    root: &RootHandle,
    operation: ProtectionOperation,
    relative: &RelativePath,
) -> FileProtection {
    let path = relative.as_path().to_string_lossy().into_owned();
    let mut file = FileProtection {
        path,
        operation,
        tracked: false,
        ignored: false,
        worktree: WorktreeState::Unknown,
        eligible: false,
        reason: match state {
            RepositoryState::WorkTree => ProtectionReason::Unreadable,
            RepositoryState::Bare => ProtectionReason::BareRepository,
            RepositoryState::Invalid => ProtectionReason::RepositoryInvalid,
            RepositoryState::Absent => {
                if git.is_some() {
                    ProtectionReason::NotRepository
                } else {
                    ProtectionReason::GitNotInstalled
                }
            }
        },
    };
    let (Some(git), Some(repository)) = (git, repository) else {
        return file;
    };
    let git_path = match repository.git_path(relative) {
        Ok(path) => path,
        Err(_) => return file,
    };
    let literal = format!(":(literal){git_path}");
    let tracked = run_ok(
        git,
        &repository.work_tree,
        &["ls-files", "--error-unmatch", "--", literal.as_str()],
        &[],
    )
    .is_ok();
    file.tracked = tracked;
    let ignored = !tracked
        && run_ok(
            git,
            &repository.work_tree,
            &["check-ignore", "-q", "--", git_path.as_str()],
            &[],
        )
        .is_ok();
    file.ignored = ignored;
    if tracked
        && let Some(status) = run(
            git,
            &repository.work_tree,
            &["status", "--porcelain=v1", "-z", "--", literal.as_str()],
            &[],
        )
        && status.ok()
    {
        file.worktree = parse_status(&status.stdout);
    } else if !tracked {
        file.worktree = if ignored {
            WorktreeState::Ignored
        } else {
            WorktreeState::Untracked
        };
    }
    match operation {
        ProtectionOperation::Create => match root.open_read(relative) {
            Ok(_) => {
                file.reason = ProtectionReason::TargetExists;
                return file;
            }
            Err(error) if error.is_missing() => {}
            Err(error) => {
                file.reason = resource_reason(&error.kind);
                return file;
            }
        },
        ProtectionOperation::Edit | ProtectionOperation::Delete => match root.open_read(relative) {
            Ok(_) => {}
            Err(error) if error.is_missing() => {
                file.reason = ProtectionReason::Missing;
                return file;
            }
            Err(error) => {
                file.reason = resource_reason(&error.kind);
                return file;
            }
        },
    }
    if !tracked && !matches!(operation, ProtectionOperation::Create) {
        file.reason = if ignored {
            ProtectionReason::Ignored
        } else {
            ProtectionReason::Untracked
        };
        return file;
    }
    if ignored {
        file.reason = ProtectionReason::Ignored;
        return file;
    }
    file.eligible = true;
    file.reason = ProtectionReason::Eligible;
    file
}

fn warning_for(
    git: Option<&GitCli>,
    state: RepositoryState,
    files: &[FileProtection],
) -> Option<ProtectionWarning> {
    if git.is_none() {
        return Some(ProtectionWarning {
            code: ProtectionWarningCode::NoGit,
            message_key: "recovery.no_git".into(),
            message: "This folder does not use Git. You can continue, but the application cannot undo these changes.".into(),
            paths: files.iter().map(|file| file.path.clone()).collect(),
        });
    }
    let (code, key, message) = match state {
        RepositoryState::WorkTree => {
            let paths: Vec<String> = files
                .iter()
                .filter(|file| !file.eligible)
                .map(|file| file.path.clone())
                .collect();
            if paths.is_empty() {
                return None;
            }
            return Some(ProtectionWarning {
                code: ProtectionWarningCode::UnprotectedFiles,
                message_key: "recovery.unprotected_files".into(),
                message: "No recovery is available for these files. The application cannot undo changes to them.".into(),
                paths,
            });
        }
        RepositoryState::Bare => (
            ProtectionWarningCode::BareRepository,
            "recovery.bare_repository",
            "This Git repository has no working tree, so these files cannot be protected.",
        ),
        RepositoryState::Invalid => (
            ProtectionWarningCode::RepositoryInvalid,
            "recovery.repository_invalid",
            "Git could not protect this folder because its repository state is invalid.",
        ),
        RepositoryState::Absent => (
            ProtectionWarningCode::NoRepository,
            "recovery.no_repository",
            "This folder is not a Git repository. You can continue, but the application cannot undo these changes.",
        ),
    };
    Some(ProtectionWarning {
        code,
        message_key: key.into(),
        message: message.into(),
        paths: files.iter().map(|file| file.path.clone()).collect(),
    })
}

fn resource_reason(kind: &ResourceErrorKind) -> ProtectionReason {
    match kind {
        ResourceErrorKind::LimitExceeded => ProtectionReason::LimitExceeded,
        ResourceErrorKind::UnsupportedResourceType => ProtectionReason::UnsupportedType,
        _ => ProtectionReason::Unreadable,
    }
}

fn parse_status(bytes: &[u8]) -> WorktreeState {
    if bytes.is_empty() {
        return WorktreeState::Clean;
    }
    let Some(index) = bytes.iter().position(|byte| *byte == 0) else {
        return WorktreeState::Unknown;
    };
    let entry = &bytes[..index];
    if entry.len() < 2 {
        return WorktreeState::Unknown;
    }
    match (entry[0], entry[1]) {
        (b'?', b'?') => WorktreeState::Untracked,
        (b'!', b'!') => WorktreeState::Ignored,
        (b' ', b' ') => WorktreeState::Clean,
        (b' ', _) => WorktreeState::Unstaged,
        (_, b' ') => WorktreeState::Staged,
        _ => WorktreeState::StagedAndUnstaged,
    }
}

fn read_source(root: &RootHandle, relative: &RelativePath) -> Result<Vec<u8>, ProtectionError> {
    root.read(relative, MAX_PROTECTED_BYTES)
        .map_err(|error| ProtectionError {
            kind: ProtectionErrorKind::Unavailable,
            reason: resource_reason(&error.kind),
        })
}

#[cfg(unix)]
fn source_mode(root: &RootHandle, relative: &RelativePath) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(root.display_path().join(relative.as_path()))
        .map(|metadata| {
            if metadata.permissions().mode() & 0o111 != 0 {
                0o100755
            } else {
                0o100644
            }
        })
        .unwrap_or(0o100644)
}

#[cfg(not(unix))]
fn source_mode(_root: &RootHandle, _relative: &RelativePath) -> u32 {
    0o100644
}

fn private_reference(suffix: &str) -> Result<String, ProtectionError> {
    let suffix = suffix.trim_matches('/');
    if suffix.is_empty()
        || suffix.len() > 200
        || suffix.contains("..")
        || suffix.ends_with(".lock")
        || !suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_./-".contains(&byte))
    {
        return Err(ProtectionError {
            kind: ProtectionErrorKind::InvalidReference,
            reason: ProtectionReason::GitError,
        });
    }
    let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let stamp = super::jobs::now_millis().max(0);
    Ok(format!(
        "{PRIVATE_REF_PREFIX}{suffix}/{stamp}-{}-{unique}",
        std::process::id()
    ))
}

fn hash_object(
    git: &GitCli,
    repository: &Repository,
    bytes: &[u8],
) -> Result<String, ProtectionError> {
    let mut command = git.command(&repository.work_tree);
    command.args(["hash-object", "-w", "--no-filters", "--stdin"]);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|_| command_failure())?;
    child
        .stdin
        .as_mut()
        .ok_or_else(command_failure)?
        .write_all(bytes)
        .map_err(|_| command_failure())?;
    drop(child.stdin.take());
    let output = child.wait_with_output().map_err(|_| command_failure())?;
    let output = bounded(output).map_err(|_| command_failure())?;
    if !output.ok() {
        return Err(command_failure());
    }
    let object = output.text();
    validate_object_id(&object)?;
    Ok(object)
}

fn update_index(
    git: &GitCli,
    repository: &Repository,
    index: &Path,
    mode: u32,
    object: &str,
    path: &str,
) -> Result<(), ProtectionError> {
    let mut command = git.command(&repository.work_tree);
    command.args(["update-index", "-z", "--index-info"]);
    command
        .env("GIT_INDEX_FILE", index)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|_| command_failure())?;
    {
        let stdin = child.stdin.as_mut().ok_or_else(command_failure)?;
        write!(stdin, "{mode:o} {object}\t{path}\0").map_err(|_| command_failure())?;
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().map_err(|_| command_failure())?;
    let output = bounded(output).map_err(|_| command_failure())?;
    if !output.ok() {
        return Err(command_failure());
    }
    Ok(())
}

fn validate_object_id(value: &str) -> Result<(), ProtectionError> {
    if value.len() < 40 || value.len() > 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ProtectionError {
            kind: ProtectionErrorKind::CommandFailed,
            reason: ProtectionReason::GitError,
        });
    }
    Ok(())
}

fn run_ok(
    git: &GitCli,
    directory: &Path,
    args: &[&str],
    environment: &[(&str, &OsStr)],
) -> Result<CommandResult, ProtectionError> {
    run(git, directory, args, environment)
        .ok_or_else(command_failure)
        .and_then(|result| {
            if result.ok() {
                Ok(result)
            } else {
                Err(command_failure())
            }
        })
}

fn run(
    git: &GitCli,
    directory: &Path,
    args: &[&str],
    environment: &[(&str, &OsStr)],
) -> Option<CommandResult> {
    let mut command = git.command(directory);
    command.args(args);
    for (key, value) in environment {
        command.env(key, value);
    }
    finish(command).ok()
}

fn finish(mut command: Command) -> std::io::Result<CommandResult> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = command.output()?;
    bounded(output).map_err(|_| std::io::Error::other("bounded git output"))
}

fn bounded(output: Output) -> std::io::Result<CommandResult> {
    if output.stdout.len() > MAX_GIT_OUTPUT || output.stderr.len() > MAX_GIT_OUTPUT {
        return Err(std::io::Error::other("git output limit"));
    }
    Ok(CommandResult {
        status: output.status.code(),
        stdout: output.stdout,
    })
}

fn apply_base_environment(command: &mut Command) {
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "/usr/bin/false")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_PAGER", "cat");
}

fn apply_identity(command: &mut Command) {
    command
        .env("GIT_AUTHOR_NAME", "FoundryUI")
        .env("GIT_AUTHOR_EMAIL", "recovery@foundryui.invalid")
        .env("GIT_COMMITTER_NAME", "FoundryUI")
        .env("GIT_COMMITTER_EMAIL", "recovery@foundryui.invalid")
        .env("GIT_AUTHOR_DATE", "@1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "@1700000000 +0000");
}

fn sanitize_line(value: &str) -> String {
    value
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|character| !character.is_control())
        .take(120)
        .collect()
}

fn state_reason(state: RepositoryState) -> ProtectionReason {
    match state {
        RepositoryState::Absent => ProtectionReason::NotRepository,
        RepositoryState::Bare => ProtectionReason::BareRepository,
        RepositoryState::Invalid => ProtectionReason::RepositoryInvalid,
        RepositoryState::WorkTree => ProtectionReason::GitError,
    }
}

fn failure(reason: ProtectionReason) -> ProtectionError {
    ProtectionError {
        kind: ProtectionErrorKind::NotEligible,
        reason,
    }
}

fn command_failure() -> ProtectionError {
    ProtectionError {
        kind: ProtectionErrorKind::CommandFailed,
        reason: ProtectionReason::GitError,
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    struct Fixture {
        _directory: tempfile::TempDir,
        root: RootHandle,
        git: GitCli,
    }

    impl Fixture {
        fn repository() -> Option<Self> {
            let git = resolve()?;
            let directory = tempfile::tempdir().ok()?;
            let fixture = Self {
                root: RootHandle::enroll(directory.path()).ok()?,
                _directory: directory,
                git,
            };
            fixture.run(&["init", "-q"]);
            fixture.run(&["config", "user.name", "Fixture"]);
            fixture.run(&["config", "user.email", "fixture@example.invalid"]);
            Some(fixture)
        }

        fn directory(&self) -> &Path {
            self.root.display_path()
        }

        fn run(&self, args: &[&str]) -> String {
            let mut command = self.git.command(self.directory());
            command.args(args);
            apply_identity(&mut command);
            let output = finish(command).expect("git command");
            assert!(output.ok(), "git {:?} failed", args);
            output.text()
        }

        fn run_raw(&self, args: &[&str]) -> CommandResult {
            let mut command = self.git.command(self.directory());
            command.args(args);
            apply_identity(&mut command);
            finish(command).expect("git command")
        }

        fn write(&self, path: &str, content: &str) {
            std::fs::write(self.directory().join(path), content).expect("write");
        }

        fn commit(&self) {
            self.run(&["add", "-A"]);
            self.run(&["commit", "-qm", "fixture"]);
        }

        fn status(&self) -> String {
            self.run(&["status", "--porcelain=v1"])
        }

        fn head(&self) -> String {
            self.run(&["rev-parse", "HEAD"])
        }

        fn index_bytes(&self) -> Vec<u8> {
            std::fs::read(self.directory().join(".git/index")).expect("index")
        }

        fn refs(&self) -> String {
            self.run(&["for-each-ref", "--format=%(refname)", PRIVATE_REF_PREFIX])
        }
    }

    fn relative(path: &str) -> RelativePath {
        RelativePath::parse(path).expect("path")
    }

    fn file(inspection: &ProtectionInspection) -> &FileProtection {
        &inspection.files[0]
    }

    #[test]
    fn detection_distinguishes_installation_repository_and_concrete_file_state() {
        let Some(fixture) = Fixture::repository() else {
            return;
        };
        fixture.write("tracked.txt", "committed");
        fixture.commit();
        let tracked = relative("tracked.txt");

        let inspection = inspect(&fixture.root, ProtectionOperation::Edit, &tracked);
        assert!(inspection.git_installed);
        assert_eq!(inspection.repository, RepositoryState::WorkTree);
        assert_eq!(file(&inspection).worktree, WorktreeState::Clean);
        assert!(file(&inspection).tracked);
        assert!(file(&inspection).eligible);
        assert_eq!(file(&inspection).reason, ProtectionReason::Eligible);
        assert!(inspection.warning.is_none());
        assert!(!inspection.undo_available);

        fixture.write("tracked.txt", "user worktree");
        let inspection = inspect(&fixture.root, ProtectionOperation::Edit, &tracked);
        assert_eq!(file(&inspection).worktree, WorktreeState::Unstaged);
        assert!(file(&inspection).eligible);

        fixture.run(&["add", "tracked.txt"]);
        let inspection = inspect(&fixture.root, ProtectionOperation::Edit, &tracked);
        assert_eq!(file(&inspection).worktree, WorktreeState::Staged);
        assert!(file(&inspection).eligible);

        fixture.write("notes.txt", "untracked");
        let inspection = inspect(
            &fixture.root,
            ProtectionOperation::Edit,
            &relative("notes.txt"),
        );
        assert!(!file(&inspection).tracked);
        assert!(!file(&inspection).eligible);
        assert_eq!(file(&inspection).reason, ProtectionReason::Untracked);
        assert_eq!(file(&inspection).worktree, WorktreeState::Untracked);
        assert_eq!(
            inspection.warning.as_ref().map(|warning| warning.code),
            Some(ProtectionWarningCode::UnprotectedFiles)
        );

        fixture.write(".gitignore", "*.log\n");
        fixture.commit();
        fixture.write("debug.log", "ignored");
        let inspection = inspect(
            &fixture.root,
            ProtectionOperation::Edit,
            &relative("debug.log"),
        );
        assert!(file(&inspection).ignored);
        assert!(!file(&inspection).eligible);
        assert_eq!(file(&inspection).reason, ProtectionReason::Ignored);
        assert_eq!(file(&inspection).worktree, WorktreeState::Ignored);
    }

    #[test]
    fn a_folder_without_a_repository_never_claims_recovery() {
        let directory = tempfile::tempdir().expect("directory");
        std::fs::write(directory.path().join("file.txt"), "content").expect("write");
        let root = RootHandle::enroll(directory.path()).expect("root");

        let inspection = inspect(&root, ProtectionOperation::Edit, &relative("file.txt"));

        assert_eq!(inspection.repository, RepositoryState::Absent);
        assert!(!file(&inspection).eligible);
        assert_eq!(file(&inspection).reason, ProtectionReason::NotRepository);
        assert!(!inspection.undo_available);
        let warning = inspection.warning.expect("warning");
        assert_eq!(warning.code, ProtectionWarningCode::NoRepository);
        assert_eq!(warning.message_key, "recovery.no_repository");
        assert!(!warning.message.is_empty());
    }

    #[test]
    fn a_missing_git_installation_has_an_explicit_warning() {
        let file = FileProtection {
            path: "file.txt".into(),
            operation: ProtectionOperation::Edit,
            tracked: false,
            ignored: false,
            worktree: WorktreeState::Unknown,
            eligible: false,
            reason: ProtectionReason::GitNotInstalled,
        };
        let warning = warning_for(None, RepositoryState::Absent, &[file]).expect("warning");
        assert_eq!(warning.code, ProtectionWarningCode::NoGit);
        assert_eq!(warning.message_key, "recovery.no_git");
    }

    #[test]
    fn preparation_preserves_dirty_work_staged_content_and_untracked_work() {
        let Some(fixture) = Fixture::repository() else {
            return;
        };
        fixture.write("tracked.txt", "committed");
        fixture.write("staged.txt", "old staged");
        fixture.commit();
        fixture.write("tracked.txt", "user worktree");
        fixture.write("staged.txt", "prepared by user");
        fixture.run(&["add", "staged.txt"]);
        fixture.write("notes.txt", "untracked work");

        let head = fixture.head();
        let branch = fixture.run(&["symbolic-ref", "--short", "HEAD"]);
        let index = fixture.index_bytes();
        let status = fixture.status();
        let tracked = std::fs::read(fixture.directory().join("tracked.txt")).expect("tracked");
        let staged = std::fs::read(fixture.directory().join("staged.txt")).expect("staged");
        let notes = std::fs::read(fixture.directory().join("notes.txt")).expect("notes");

        let prepared = prepare(
            &fixture.root,
            ProtectionOperation::Edit,
            &relative("tracked.txt"),
            "1/1/edit-1",
        )
        .expect("prepare");

        assert_eq!(prepared.kind, PreparedKind::Content);
        assert_eq!(prepared.size, "user worktree".len() as u64);
        assert!(prepared.reference.starts_with(PRIVATE_REF_PREFIX));
        assert_eq!(fixture.head(), head);
        assert_eq!(fixture.run(&["symbolic-ref", "--short", "HEAD"]), branch);
        assert_eq!(fixture.index_bytes(), index);
        assert_eq!(fixture.status(), status);
        assert_eq!(
            std::fs::read(fixture.directory().join("tracked.txt")).expect("tracked"),
            tracked
        );
        assert_eq!(
            std::fs::read(fixture.directory().join("staged.txt")).expect("staged"),
            staged
        );
        assert_eq!(
            std::fs::read(fixture.directory().join("notes.txt")).expect("notes"),
            notes
        );
        assert_eq!(
            fixture.run(&["show", &format!("{}:tracked.txt", prepared.reference)]),
            "user worktree"
        );
        assert_eq!(fixture.refs(), prepared.reference);
        assert!(
            fixture
                .run_raw(&[
                    "rev-parse",
                    "--verify",
                    &format!("{}^{{commit}}", prepared.reference)
                ])
                .ok()
        );
    }

    #[test]
    fn untracked_and_ignored_files_are_refused_before_preparation() {
        let Some(fixture) = Fixture::repository() else {
            return;
        };
        fixture.write(".gitignore", "*.log\n");
        fixture.commit();
        fixture.write("notes.txt", "untracked");
        fixture.write("debug.log", "ignored");

        for (path, reason) in [
            ("notes.txt", ProtectionReason::Untracked),
            ("debug.log", ProtectionReason::Ignored),
        ] {
            let result = prepare(
                &fixture.root,
                ProtectionOperation::Edit,
                &relative(path),
                "1/1/edit",
            );
            assert!(matches!(
                result,
                Err(ProtectionError {
                    kind: ProtectionErrorKind::NotEligible,
                    reason: actual
                }) if actual == reason
            ));
        }
        assert!(fixture.refs().is_empty());
    }

    #[test]
    fn a_new_file_records_absence_and_existing_targets_are_refused() {
        let Some(fixture) = Fixture::repository() else {
            return;
        };
        let prepared = prepare(
            &fixture.root,
            ProtectionOperation::Create,
            &relative("new.txt"),
            "1/1/create-1",
        )
        .expect("prepare");
        assert_eq!(prepared.kind, PreparedKind::Absence);
        assert!(prepared.blob.is_none());
        assert!(
            fixture
                .run(&["ls-tree", "--name-only", &prepared.reference])
                .is_empty()
        );
        assert!(!fixture.directory().join("new.txt").exists());

        fixture.write("existing.txt", "existing");
        let inspection = inspect(
            &fixture.root,
            ProtectionOperation::Create,
            &relative("existing.txt"),
        );
        assert!(!file(&inspection).eligible);
        assert_eq!(file(&inspection).reason, ProtectionReason::TargetExists);
        assert!(matches!(
            prepare(
                &fixture.root,
                ProtectionOperation::Create,
                &relative("existing.txt"),
                "1/1/create-2"
            ),
            Err(ProtectionError {
                kind: ProtectionErrorKind::NotEligible,
                reason: ProtectionReason::TargetExists
            })
        ));
    }

    #[test]
    fn ignored_new_files_are_not_assumed_protected() {
        let Some(fixture) = Fixture::repository() else {
            return;
        };
        fixture.write(".gitignore", "*.log\n");
        fixture.commit();
        let inspection = inspect(
            &fixture.root,
            ProtectionOperation::Create,
            &relative("new.log"),
        );
        assert!(file(&inspection).ignored);
        assert!(!file(&inspection).eligible);
        assert_eq!(file(&inspection).reason, ProtectionReason::Ignored);
        assert!(matches!(
            prepare(
                &fixture.root,
                ProtectionOperation::Create,
                &relative("new.log"),
                "1/1/create-1"
            ),
            Err(ProtectionError {
                kind: ProtectionErrorKind::NotEligible,
                reason: ProtectionReason::Ignored
            })
        ));
        assert!(fixture.refs().is_empty());
    }

    #[test]
    fn a_storage_failure_never_produces_a_recovery_reference() {
        let Some(fixture) = Fixture::repository() else {
            return;
        };
        fixture.write("tracked.txt", "content");
        fixture.commit();
        let objects = fixture.directory().join(".git/objects");
        let moved = fixture.directory().join(".git/objects-moved");
        std::fs::rename(&objects, &moved).expect("move objects");

        let result = prepare(
            &fixture.root,
            ProtectionOperation::Edit,
            &relative("tracked.txt"),
            "1/1/edit-1",
        );

        std::fs::rename(&moved, &objects).expect("restore objects");
        assert!(result.is_err());
        assert!(fixture.refs().is_empty());
    }

    #[test]
    fn a_project_subfolder_is_protected_with_repository_relative_paths() {
        let Some(git) = resolve() else {
            return;
        };
        let directory = tempfile::tempdir().expect("directory");
        let project = directory.path().join("project");
        std::fs::create_dir(&project).expect("project");
        let run = |args: &[&str]| {
            let mut command = git.command(directory.path());
            command.args(args);
            apply_identity(&mut command);
            let output = finish(command).expect("git");
            assert!(output.ok(), "git {:?} failed", args);
            output.text()
        };
        run(&["init", "-q"]);
        run(&["config", "user.name", "Fixture"]);
        run(&["config", "user.email", "fixture@example.invalid"]);
        std::fs::write(project.join("file name.txt"), "committed").expect("write");
        run(&["add", "-A"]);
        run(&["commit", "-qm", "fixture"]);
        std::fs::write(project.join("file name.txt"), "dirty").expect("write");
        let root = RootHandle::enroll(&project).expect("root");
        let relative = relative("file name.txt");

        let inspection = inspect(&root, ProtectionOperation::Edit, &relative);
        assert!(file(&inspection).eligible);
        assert_eq!(file(&inspection).reason, ProtectionReason::Eligible);

        let prepared =
            prepare(&root, ProtectionOperation::Edit, &relative, "2/2/edit-1").expect("prepare");
        assert_eq!(
            run(&[
                "show",
                &format!("{}:project/file name.txt", prepared.reference)
            ]),
            "dirty"
        );
    }

    #[test]
    fn a_bare_repository_is_not_reported_as_a_work_tree() {
        let Some(git) = resolve() else {
            return;
        };
        let directory = tempfile::tempdir().expect("directory");
        let bare = directory.path().join("bare.git");
        std::fs::create_dir(&bare).expect("bare");
        let mut command = Command::new(&git.executable);
        command.args(["init", "--bare", "-q"]).arg(&bare);
        apply_base_environment(&mut command);
        assert!(finish(command).expect("git").ok());
        let root = RootHandle::enroll(&bare).expect("root");
        let inspection = inspect(&root, ProtectionOperation::Edit, &relative("file.txt"));
        assert_eq!(inspection.repository, RepositoryState::Bare);
        assert!(!file(&inspection).eligible);
        assert_eq!(file(&inspection).reason, ProtectionReason::BareRepository);
        assert_eq!(
            inspection.warning.as_ref().map(|warning| warning.code),
            Some(ProtectionWarningCode::BareRepository)
        );
    }
}
