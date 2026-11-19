use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::foundry::db::agent::{AgentStore, ContextFact};
use crate::foundry::git_protection::{self, PreparedKind};
use crate::foundry::resources::{RelativePath, RootHandle};

const MAX_CHANGE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryError {
    NotFound,
    NoProtection,
    RootUnavailable,
    NeedsConfirmation,
    Conflict,
    Unavailable,
}

impl RecoveryError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::NoProtection => "no_protection",
            Self::RootUnavailable => "root_unavailable",
            Self::NeedsConfirmation => "needs_confirmation",
            Self::Conflict => "conflict",
            Self::Unavailable => "unavailable",
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::NotFound => "the change set was not found",
            Self::NoProtection => "no recovery protection is available for this change",
            Self::RootUnavailable => "the authorized root is no longer available",
            Self::NeedsConfirmation => "destructive recovery requires confirmation",
            Self::Conflict => "the file changed after the change was applied",
            Self::Unavailable => "recovery could not be completed",
        }
    }
}

impl std::fmt::Display for RecoveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for RecoveryError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverableChange {
    pub run_id: Option<i64>,
    pub action_id: i64,
    pub operation: String,
    pub path: String,
    pub protected: bool,
    pub destructive: bool,
    pub additions: u32,
    pub deletions: u32,
    pub diff: String,
    pub diff_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryOutcome {
    pub action_id: i64,
    pub operation: String,
    pub path: String,
    pub restored: bool,
}

pub fn list_recoverable(
    store: &AgentStore<'_>,
    conversation_id: i64,
    run_id: Option<i64>,
) -> Result<Vec<RecoverableChange>, RecoveryError> {
    let facts = store
        .facts(conversation_id)
        .map_err(|_| RecoveryError::Unavailable)?;
    let changes = facts
        .iter()
        .filter_map(|fact| {
            let ContextFact::ChangeSet {
                operation,
                path,
                additions,
                deletions,
                diff,
                diff_truncated,
                ..
            } = &fact.fact
            else {
                return None;
            };
            if run_id.is_some() && fact.run_id != run_id {
                return None;
            }
            let action_id = fact.key.strip_prefix("change-")?.parse::<i64>().ok()?;
            let protected = facts.iter().any(|other| {
                matches!(
                    &other.fact,
                    ContextFact::RecoveryRecord { change_set_id, .. } if *change_set_id == fact.id
                )
            });
            Some(RecoverableChange {
                run_id: fact.run_id,
                action_id,
                operation: operation.clone(),
                path: path.clone(),
                protected,
                destructive: operation == "create",
                additions: *additions,
                deletions: *deletions,
                diff: diff.clone(),
                diff_truncated: *diff_truncated,
            })
        })
        .collect();
    Ok(changes)
}

pub fn recover(
    store: &AgentStore<'_>,
    conversation_id: i64,
    action_id: i64,
    confirmed_destructive: bool,
) -> Result<RecoveryOutcome, RecoveryError> {
    let change = store
        .fact_by_key(conversation_id, &format!("change-{action_id}"))
        .map_err(|_| RecoveryError::NotFound)?;
    let ContextFact::ChangeSet {
        operation,
        path,
        proposed_fingerprint,
        resource_keys,
        ..
    } = &change.fact
    else {
        return Err(RecoveryError::NotFound);
    };
    let recovery = store
        .fact_by_key(conversation_id, &format!("recovery-{action_id}"))
        .map_err(|_| RecoveryError::NoProtection)?;
    let ContextFact::RecoveryRecord {
        change_set_id,
        git_reference,
    } = &recovery.fact
    else {
        return Err(RecoveryError::NoProtection);
    };
    if *change_set_id != change.id {
        return Err(RecoveryError::NoProtection);
    }
    if operation == "create" && !confirmed_destructive {
        return Err(RecoveryError::NeedsConfirmation);
    }
    let root_id = resource_keys.first().ok_or(RecoveryError::Unavailable)?;
    let root = store
        .open_resource_root(conversation_id, root_id, action_id)
        .map_err(|_| RecoveryError::RootUnavailable)?;
    let relative = RelativePath::parse(path).map_err(|_| RecoveryError::Unavailable)?;
    let restored = git_protection::read_recovery(&root, &relative, git_reference)
        .map_err(|_| RecoveryError::Unavailable)?;

    match operation.as_str() {
        "create" => {
            let current = current_bytes(&root, &relative)?;
            match current {
                Some(bytes)
                    if proposed_fingerprint.as_deref() == Some(&content_fingerprint(&bytes)) => {}
                _ => return Err(RecoveryError::Conflict),
            }
            root.remove(&relative)
                .map_err(|_| RecoveryError::Unavailable)?;
        }
        "edit" => {
            let current = current_bytes(&root, &relative)?;
            match current {
                Some(bytes)
                    if proposed_fingerprint.as_deref() == Some(&content_fingerprint(&bytes)) => {}
                _ => return Err(RecoveryError::Conflict),
            }
            if restored.kind != PreparedKind::Content {
                return Err(RecoveryError::Unavailable);
            }
            root.rewrite(&relative, &restored.bytes)
                .map_err(|_| RecoveryError::Unavailable)?;
        }
        "delete" => {
            match current_bytes(&root, &relative)? {
                None => {}
                Some(_) => return Err(RecoveryError::Conflict),
            }
            if restored.kind != PreparedKind::Content {
                return Err(RecoveryError::Unavailable);
            }
            root.create_new(&relative, &restored.bytes)
                .map_err(|_| RecoveryError::Unavailable)?;
        }
        _ => return Err(RecoveryError::Unavailable),
    }
    Ok(RecoveryOutcome {
        action_id,
        operation: operation.clone(),
        path: path.clone(),
        restored: true,
    })
}

fn current_bytes(
    root: &RootHandle,
    relative: &RelativePath,
) -> Result<Option<Vec<u8>>, RecoveryError> {
    match root.open_read(relative) {
        Ok(opened) => opened
            .read(MAX_CHANGE_BYTES)
            .map(Some)
            .map_err(|_| RecoveryError::Unavailable),
        Err(error) if error.is_missing() => Ok(None),
        Err(_) => Err(RecoveryError::Unavailable),
    }
}

fn content_fingerprint(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::foundry::db::Database;
    use crate::foundry::resources::RootHandle;

    struct Fixture {
        database: Database,
        _directory: tempfile::TempDir,
        conversation: i64,
        run_id: i64,
    }

    impl Fixture {
        fn project() -> Self {
            let database = Database::open_in_memory().expect("database");
            let directory = tempfile::tempdir().expect("directory");
            let root = RootHandle::enroll(directory.path()).expect("root");
            let store = database.agent_store();
            let project = store
                .create_project(&root, Some("Recovery"))
                .expect("project");
            let conversation = store
                .create_project_conversation(Some(project.id), "fixture-model")
                .expect("conversation")
                .conversation
                .id;
            let run_id = store.start_run(conversation, "recovery").expect("run").id;
            Self {
                database,
                _directory: directory,
                conversation,
                run_id,
            }
        }

        fn store(&self) -> AgentStore<'_> {
            self.database.agent_store()
        }

        fn revision(&self) -> i64 {
            self.store()
                .authorization_revision(self.conversation)
                .expect("revision")
                .key()
                .context_revision
        }
    }

    fn run_git(directory: &std::path::Path, args: &[&str]) -> bool {
        let Some(git) = crate::foundry::git_protection::git_executable() else {
            return false;
        };
        std::process::Command::new(git)
            .current_dir(directory)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .status()
            .is_ok_and(|status| status.success())
    }

    fn initialize_repository(directory: &std::path::Path) -> bool {
        run_git(directory, &["init", "-q"])
            && run_git(directory, &["commit", "--allow-empty", "-qm", "fixture"])
    }

    fn root_id(store: &AgentStore<'_>, conversation: i64) -> String {
        store
            .authorization_revision(conversation)
            .expect("revision")
            .project_root_id()
            .expect("project root")
            .to_string()
    }

    #[test]
    fn listing_exposes_protected_changes_per_run() {
        let fixture = Fixture::project();
        assert!(initialize_repository(fixture._directory.path()));
        std::fs::write(fixture._directory.path().join("a.txt"), b"before").expect("write");
        assert!(run_git(fixture._directory.path(), &["add", "a.txt"]));
        assert!(run_git(
            fixture._directory.path(),
            &["commit", "-qm", "tracked"]
        ));
        let store = fixture.store();
        let revision = fixture.revision();
        let root = store
            .open_resource_root(
                fixture.conversation,
                &root_id(&store, fixture.conversation),
                fixture.run_id,
            )
            .expect("root");
        let relative = RelativePath::parse("a.txt").expect("path");
        let protection = crate::foundry::git_protection::prepare(
            &root,
            crate::foundry::git_protection::ProtectionOperation::Edit,
            &relative,
            &format!("recovery-list-{}", fixture.run_id),
        )
        .expect("protection");
        let change = store
            .record_fact(
                fixture.conversation,
                revision,
                Some(fixture.run_id),
                &format!("change-{}", fixture.run_id),
                &ContextFact::ChangeSet {
                    operation: "edit".into(),
                    path: "a.txt".into(),
                    base_fingerprint: Some(content_fingerprint(b"before")),
                    proposed_fingerprint: Some(content_fingerprint(b"after")),
                    proposal_fingerprint: content_fingerprint(b"x"),
                    additions: 1,
                    deletions: 1,
                    diff: "-before\n+after\n".into(),
                    diff_truncated: false,
                    resource_keys: vec![root_id(&store, fixture.conversation)],
                },
            )
            .expect("change set");
        store
            .record_fact(
                fixture.conversation,
                revision,
                Some(fixture.run_id),
                &format!("recovery-{}", fixture.run_id),
                &ContextFact::RecoveryRecord {
                    change_set_id: change.id,
                    git_reference: protection.reference,
                },
            )
            .expect("recovery record");

        let changes =
            list_recoverable(&store, fixture.conversation, Some(fixture.run_id)).expect("list");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].operation, "edit");
        assert_eq!(changes[0].path, "a.txt");
        assert!(changes[0].protected);
        assert!(!changes[0].destructive);
    }

    #[test]
    fn editing_back_is_rejected_when_the_user_changed_the_file_after_apply() {
        let fixture = Fixture::project();
        assert!(initialize_repository(fixture._directory.path()));
        std::fs::write(fixture._directory.path().join("a.txt"), b"before").expect("write");
        assert!(run_git(fixture._directory.path(), &["add", "a.txt"]));
        assert!(run_git(
            fixture._directory.path(),
            &["commit", "-qm", "tracked"]
        ));
        std::fs::write(fixture._directory.path().join("a.txt"), b"after").expect("write");
        let store = fixture.store();
        let revision = fixture.revision();
        let root = store
            .open_resource_root(
                fixture.conversation,
                &root_id(&store, fixture.conversation),
                fixture.run_id,
            )
            .expect("root");
        let relative = RelativePath::parse("a.txt").expect("path");
        let protection = crate::foundry::git_protection::prepare(
            &root,
            crate::foundry::git_protection::ProtectionOperation::Edit,
            &relative,
            &format!("recovery-edit-{}", fixture.run_id),
        )
        .expect("protection");
        let change = store
            .record_fact(
                fixture.conversation,
                revision,
                Some(fixture.run_id),
                &format!("change-{}", fixture.run_id),
                &ContextFact::ChangeSet {
                    operation: "edit".into(),
                    path: "a.txt".into(),
                    base_fingerprint: Some(content_fingerprint(b"before")),
                    proposed_fingerprint: Some(content_fingerprint(b"after")),
                    proposal_fingerprint: content_fingerprint(b"x"),
                    additions: 1,
                    deletions: 1,
                    diff: "-before\n+after\n".into(),
                    diff_truncated: false,
                    resource_keys: vec![root_id(&store, fixture.conversation)],
                },
            )
            .expect("change set");
        store
            .record_fact(
                fixture.conversation,
                revision,
                Some(fixture.run_id),
                &format!("recovery-{}", fixture.run_id),
                &ContextFact::RecoveryRecord {
                    change_set_id: change.id,
                    git_reference: protection.reference,
                },
            )
            .expect("recovery record");

        std::fs::write(fixture._directory.path().join("a.txt"), b"user work").expect("write");

        let result = recover(&store, fixture.conversation, fixture.run_id, false);
        assert_eq!(result, Err(RecoveryError::Conflict));
        assert_eq!(
            std::fs::read(fixture._directory.path().join("a.txt")).expect("read"),
            b"user work"
        );
    }

    #[test]
    fn deleting_back_restores_the_previous_content() {
        let fixture = Fixture::project();
        assert!(initialize_repository(fixture._directory.path()));
        std::fs::write(fixture._directory.path().join("a.txt"), b"before").expect("write");
        assert!(run_git(fixture._directory.path(), &["add", "a.txt"]));
        assert!(run_git(
            fixture._directory.path(),
            &["commit", "-qm", "tracked"]
        ));
        let store = fixture.store();
        let revision = fixture.revision();
        let root = store
            .open_resource_root(
                fixture.conversation,
                &root_id(&store, fixture.conversation),
                fixture.run_id,
            )
            .expect("root");
        let relative = RelativePath::parse("a.txt").expect("path");
        let protection = crate::foundry::git_protection::prepare(
            &root,
            crate::foundry::git_protection::ProtectionOperation::Delete,
            &relative,
            &format!("recovery-delete-{}", fixture.run_id),
        )
        .expect("protection");
        let change = store
            .record_fact(
                fixture.conversation,
                revision,
                Some(fixture.run_id),
                &format!("change-{}", fixture.run_id),
                &ContextFact::ChangeSet {
                    operation: "delete".into(),
                    path: "a.txt".into(),
                    base_fingerprint: Some(content_fingerprint(b"before")),
                    proposed_fingerprint: Some(content_fingerprint(b"")),
                    proposal_fingerprint: content_fingerprint(b"x"),
                    additions: 0,
                    deletions: 1,
                    diff: "-before\n".into(),
                    diff_truncated: false,
                    resource_keys: vec![root_id(&store, fixture.conversation)],
                },
            )
            .expect("change set");
        store
            .record_fact(
                fixture.conversation,
                revision,
                Some(fixture.run_id),
                &format!("recovery-{}", fixture.run_id),
                &ContextFact::RecoveryRecord {
                    change_set_id: change.id,
                    git_reference: protection.reference,
                },
            )
            .expect("recovery record");

        std::fs::remove_file(fixture._directory.path().join("a.txt")).expect("remove");

        let outcome =
            recover(&store, fixture.conversation, fixture.run_id, false).expect("recover");
        assert!(outcome.restored);
        assert_eq!(
            std::fs::read(fixture._directory.path().join("a.txt")).expect("read"),
            b"before"
        );
    }

    #[test]
    fn undoing_a_create_requires_confirmation_and_then_removes_the_file() {
        let fixture = Fixture::project();
        assert!(initialize_repository(fixture._directory.path()));
        let store = fixture.store();
        let revision = fixture.revision();
        let root = store
            .open_resource_root(
                fixture.conversation,
                &root_id(&store, fixture.conversation),
                fixture.run_id,
            )
            .expect("root");
        let relative = RelativePath::parse("new.txt").expect("path");
        let protection = crate::foundry::git_protection::prepare(
            &root,
            crate::foundry::git_protection::ProtectionOperation::Create,
            &relative,
            &format!("recovery-create-{}", fixture.run_id),
        )
        .expect("protection");
        let change = store
            .record_fact(
                fixture.conversation,
                revision,
                Some(fixture.run_id),
                &format!("change-{}", fixture.run_id),
                &ContextFact::ChangeSet {
                    operation: "create".into(),
                    path: "new.txt".into(),
                    base_fingerprint: None,
                    proposed_fingerprint: Some(content_fingerprint(b"created")),
                    proposal_fingerprint: content_fingerprint(b"x"),
                    additions: 1,
                    deletions: 0,
                    diff: "+created\n".into(),
                    diff_truncated: false,
                    resource_keys: vec![root_id(&store, fixture.conversation)],
                },
            )
            .expect("change set");
        store
            .record_fact(
                fixture.conversation,
                revision,
                Some(fixture.run_id),
                &format!("recovery-{}", fixture.run_id),
                &ContextFact::RecoveryRecord {
                    change_set_id: change.id,
                    git_reference: protection.reference,
                },
            )
            .expect("recovery record");
        std::fs::write(fixture._directory.path().join("new.txt"), b"created").expect("write");

        assert_eq!(
            recover(&store, fixture.conversation, fixture.run_id, false),
            Err(RecoveryError::NeedsConfirmation)
        );
        let outcome = recover(&store, fixture.conversation, fixture.run_id, true).expect("recover");
        assert!(outcome.restored);
        assert!(!fixture._directory.path().join("new.txt").exists());
    }

    #[test]
    fn a_missing_recovery_record_reports_no_protection() {
        let fixture = Fixture::project();
        let store = fixture.store();
        let revision = fixture.revision();
        store
            .record_fact(
                fixture.conversation,
                revision,
                Some(fixture.run_id),
                &format!("change-{}", fixture.run_id),
                &ContextFact::ChangeSet {
                    operation: "edit".into(),
                    path: "a.txt".into(),
                    base_fingerprint: Some(content_fingerprint(b"before")),
                    proposed_fingerprint: Some(content_fingerprint(b"after")),
                    proposal_fingerprint: content_fingerprint(b"x"),
                    additions: 1,
                    deletions: 1,
                    diff: "-before\n+after\n".into(),
                    diff_truncated: false,
                    resource_keys: vec![root_id(&store, fixture.conversation)],
                },
            )
            .expect("change set");

        assert_eq!(
            recover(&store, fixture.conversation, fixture.run_id, false),
            Err(RecoveryError::NoProtection)
        );
    }
}
