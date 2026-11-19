use async_trait::async_trait;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::cycle::{ToolExecution, ToolExecutor, ToolHost};
use super::parser::ToolCall;
use super::schema::{RegisteredTool, ToolAvailability, ToolRegistry, ToolSpec, ToolSpecError};
use crate::foundry::db::agent::authorization::{AuthorizationRevision, RequirementKind};
use crate::foundry::db::agent::{ActionRecord, AgentStore, ContextFact};
use crate::foundry::git_protection::{self, ProtectionOperation};
use crate::foundry::policy as p;
use crate::foundry::resources::RelativePath;

const MAX_CHANGE_BYTES: usize = 1024 * 1024;

pub fn register(registry: &mut ToolRegistry) -> Result<(), ToolSpecError> {
    registry.register(RegisteredTool {
        spec: ToolSpec {
            name: "propose_file_change".into(),
            description: "Propose creating, editing or deleting an authorized file.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "operation": {"type": "string", "enum": ["create", "edit", "delete"]},
                    "path": {"type": "string", "minLength": 1, "maxLength": 4096},
                    "content": {"type": "string", "maxLength": 1048576},
                    "base_version": {"type": "string", "maxLength": 256}
                },
                "required": ["operation", "path"],
                "additionalProperties": false
            }),
        },
        availability: ToolAvailability::Available,
    })
}

pub struct ChangeExecutor;

#[async_trait]
impl ToolExecutor for ChangeExecutor {
    async fn execute(
        &mut self,
        call: &ToolCall,
        _spec: &ToolSpec,
        host: &ToolHost<'_, '_>,
    ) -> ToolExecution {
        let Some(operation) = call
            .arguments
            .get("operation")
            .and_then(Value::as_str)
            .and_then(ProtectionOperation::parse)
        else {
            return change_failure("change");
        };
        let Some(path) = call.arguments.get("path").and_then(Value::as_str) else {
            return change_failure("change");
        };
        let proposed_content = call
            .arguments
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(resource) = host.facts.action.resources.first() else {
            return change_failure("change");
        };
        let conversation_id = host.facts.action.key.context.conversation_id;
        let context_revision = host.facts.action.key.context.context_revision;
        let run_id = host.facts.action.key.run_id;
        let action_id = host.facts.action.key.action_id;
        let root =
            match host
                .store
                .open_resource_root(conversation_id, &resource.root_id, action_id)
            {
                Ok(root) => root,
                Err(_) => return change_failure("change"),
            };
        let relative = match RelativePath::parse(path) {
            Ok(relative) => relative,
            Err(_) => return change_failure("change"),
        };
        let current = match operation {
            ProtectionOperation::Create => match root.open_read(&relative) {
                Ok(_) => {
                    return ToolExecution::Failed {
                        summary: json!({"status": "failed", "code": "conflict"}),
                    };
                }
                Err(error) if error.is_missing() => Vec::new(),
                Err(_) => return change_failure("change"),
            },
            ProtectionOperation::Edit | ProtectionOperation::Delete => {
                match root.read(&relative, MAX_CHANGE_BYTES) {
                    Ok(bytes) => bytes,
                    Err(error) if error.is_missing() => {
                        return ToolExecution::Failed {
                            summary: json!({"status": "failed", "code": "conflict"}),
                        };
                    }
                    Err(_) => return change_failure("change"),
                }
            }
        };
        if operation != ProtectionOperation::Create {
            let expected = call
                .arguments
                .get("base_version")
                .and_then(Value::as_str)
                .unwrap_or("absent");
            if content_fingerprint(&current) != expected {
                return ToolExecution::Failed {
                    summary: json!({"status": "failed", "code": "conflict"}),
                };
            }
        }
        let base = String::from_utf8_lossy(&current).into_owned();
        let proposed = if operation == ProtectionOperation::Delete {
            String::new()
        } else {
            proposed_content.to_string()
        };
        let diff = diff(&base, &proposed);
        let protection = git_protection::inspect(&root, operation, &relative);
        let fact = ContextFact::ChangeSet {
            proposal_fingerprint: content_fingerprint(diff.as_bytes()),
            resource_keys: vec![resource.root_id.clone()],
        };
        if host
            .store
            .record_fact(
                conversation_id,
                context_revision,
                Some(run_id),
                &format!("change-{action_id}"),
                &fact,
            )
            .is_err()
        {
            return change_failure("change");
        }
        ToolExecution::Failed {
            summary: json!({
                "status": "failed",
                "code": "write_not_enabled",
                "diff": diff,
                "recovery": protection,
                "undoAvailable": false,
            }),
        }
    }
}

fn change_failure(code: &str) -> ToolExecution {
    ToolExecution::Failed {
        summary: json!({"status": "failed", "code": code}),
    }
}

fn content_fingerprint(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn diff(base: &str, proposed: &str) -> String {
    let mut output = String::new();
    for line in base.lines() {
        output.push('-');
        output.push_str(line);
        output.push('\n');
    }
    for line in proposed.lines() {
        output.push('+');
        output.push_str(line);
        output.push('\n');
    }
    output
}

pub struct ChangeAuthorizer;

impl super::cycle::ToolAuthorizer for ChangeAuthorizer {
    fn facts(
        &self,
        store: &AgentStore<'_>,
        revision: &AuthorizationRevision,
        action: &ActionRecord,
        call: &ToolCall,
    ) -> Result<p::TrustedFacts, String> {
        let operation = call
            .arguments
            .get("operation")
            .and_then(Value::as_str)
            .ok_or_else(|| "propose_file_change requires an operation argument".to_string())?;
        let object_id = call
            .arguments
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| "propose_file_change requires a path argument".to_string())?;
        let base_version = call
            .arguments
            .get("base_version")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let file_operation = match operation {
            "create" => p::FileOperation::Create,
            "edit" => p::FileOperation::Edit,
            "delete" => p::FileOperation::Delete,
            _ => return Err("unsupported file operation".to_string()),
        };
        let (root_id, scope) = match revision.project_root_id() {
            Some(project_root_id) => (project_root_id.to_string(), p::ResourceScope::Project),
            None => {
                let root_id = store
                    .list_grants(revision.key().conversation_id)
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .find(|grant| grant.requirement.kind == RequirementKind::StandaloneRoot)
                    .and_then(|grant| grant.requirement.root_identity_key)
                    .ok_or_else(|| "no authorized standalone root".to_string())?;
                (root_id, p::ResourceScope::Standalone)
            }
        };
        let resource = p::Resource {
            root_id,
            object_id: object_id.to_string(),
            version: base_version.unwrap_or("absent").to_string(),
        };
        let effects = match file_operation {
            p::FileOperation::Create | p::FileOperation::Edit => p::Effects::Alteration,
            p::FileOperation::Delete => p::Effects::Destructive,
            p::FileOperation::Read => p::Effects::ReadOnly,
        };
        let read_evidence = match file_operation {
            p::FileOperation::Create => p::ReadEvidence::ProvenNoReads,
            _ => p::ReadEvidence::ProvenReads {
                resources: vec![resource.clone()],
            },
        };
        let policy_action = p::Action {
            key: p::ActionKey {
                context: revision.key().clone(),
                run_id: action.run_id,
                action_id: action.id,
            },
            operation: p::Operation::File(file_operation),
            arguments_fingerprint: action.arguments_fingerprint.clone(),
            resources: vec![resource.clone()],
            origin: p::InstructionOrigin::Model,
        };
        Ok(p::TrustedFacts {
            current_key: policy_action.key.clone(),
            action: policy_action,
            mode: revision.mode(),
            mode_at_proposal: revision.mode(),
            project_root_id: revision.project_root_id().map(str::to_string),
            global_read_allowed: revision.global_read_allowed(),
            read_evidence,
            capability: p::Capability::Proven {
                effects,
                opened_resources_validated: true,
                process_isolation: false,
                network: p::NetworkControl::Disabled,
            },
            resources: vec![p::ResolvedResource { resource, scope }],
            concrete_change_id: Some(format!("change-{}", action.id)),
            recovery: p::Recovery::NotGuaranteed,
            approvals: Vec::new(),
            denied_actions: Vec::new(),
            grants: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::foundry::agent_runtime::cycle::ToolAuthorizer;
    use crate::foundry::db::Database;
    use crate::foundry::resources::RootHandle;
    use serde_json::json;
    use std::future::Future;

    struct Fixture {
        database: Database,
        _directory: tempfile::TempDir,
        conversation: i64,
    }

    impl Fixture {
        fn project() -> Self {
            let database = Database::open_in_memory().expect("database");
            let directory = tempfile::tempdir().expect("directory");
            let root = RootHandle::enroll(directory.path()).expect("root");
            let store = database.agent_store();
            let project = store
                .create_project(&root, Some("ChangeTools"))
                .expect("project");
            let conversation = store
                .create_project_conversation(Some(project.id), "fixture-model")
                .expect("conversation")
                .conversation
                .id;
            Self {
                database,
                _directory: directory,
                conversation,
            }
        }

        fn store(&self) -> AgentStore<'_> {
            self.database.agent_store()
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(future)
    }

    fn dummy_spec() -> ToolSpec {
        ToolSpec {
            name: "propose_file_change".into(),
            description: String::new(),
            parameters: json!({}),
        }
    }

    fn call(arguments: Value) -> ToolCall {
        ToolCall {
            id: "call-a".into(),
            name: "propose_file_change".into(),
            arguments,
        }
    }

    fn propose(
        store: &AgentStore<'_>,
        conversation: i64,
        arguments: &Value,
    ) -> (ActionRecord, AuthorizationRevision) {
        let run = store.start_run(conversation, "change-tools").expect("run");
        let record = store
            .propose_call(run.id, "call-a", "propose_file_change", arguments)
            .expect("proposal");
        let revision = store
            .authorization_revision(conversation)
            .expect("revision");
        (record, revision)
    }

    fn facts(
        store: &AgentStore<'_>,
        revision: &AuthorizationRevision,
        action: &ActionRecord,
        call: &ToolCall,
    ) -> p::TrustedFacts {
        ChangeAuthorizer
            .facts(store, revision, action, call)
            .expect("facts")
    }

    fn run_executor(
        store: &AgentStore<'_>,
        trusted: &p::TrustedFacts,
        call: &ToolCall,
    ) -> ToolExecution {
        let mut executor = ChangeExecutor;
        let host = ToolHost {
            store,
            facts: trusted,
        };
        block_on(executor.execute(call, &dummy_spec(), &host))
    }

    #[test]
    fn create_proposal_requires_alteration_review() {
        let fixture = Fixture::project();
        let store = fixture.store();
        let arguments = json!({"operation": "create", "path": "a.txt", "content": "hello"});
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        match p::evaluate(&trusted.action, &trusted) {
            p::Decision::Ask(prompts) => assert!(
                prompts.iter().any(|prompt| matches!(
                    prompt.requirement,
                    p::Requirement::AlterationReview { .. }
                )),
                "expected an alteration review prompt, got {prompts:?}"
            ),
            p::Decision::Allow => panic!("expected Ask, got Allow"),
            p::Decision::Deny(denial) => panic!("expected Ask, got Deny {denial:?}"),
        }
    }

    #[test]
    fn delete_proposal_requires_destructive_confirmation() {
        let fixture = Fixture::project();
        let store = fixture.store();
        let arguments = json!({"operation": "delete", "path": "a.txt"});
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        match p::evaluate(&trusted.action, &trusted) {
            p::Decision::Ask(prompts) => assert!(
                prompts.iter().any(|prompt| matches!(
                    prompt.requirement,
                    p::Requirement::DestructiveConfirmation { .. }
                )),
                "expected a destructive confirmation prompt, got {prompts:?}"
            ),
            p::Decision::Allow => panic!("expected Ask, got Allow"),
            p::Decision::Deny(denial) => panic!("expected Ask, got Deny {denial:?}"),
        }
    }

    #[test]
    fn the_executor_surfaces_the_diff_without_writing() {
        let fixture = Fixture::project();
        let store = fixture.store();
        let arguments = json!({
            "operation": "create",
            "path": "new.txt",
            "content": "hello from the proposal"
        });
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        let execution = run_executor(&store, &trusted, &call);

        match execution {
            ToolExecution::Failed { summary } => {
                assert_eq!(summary["status"], "failed");
                assert_eq!(summary["code"], "write_not_enabled");
                let diff = summary["diff"].as_str().expect("diff");
                assert!(!diff.is_empty());
            }
            other => panic!("expected failure, got {other:?}"),
        }
        assert!(!fixture._directory.path().join("new.txt").exists());
    }

    #[test]
    fn the_executor_conflicts_when_the_base_version_mismatches() {
        let fixture = Fixture::project();
        std::fs::write(fixture._directory.path().join("a.txt"), b"original").expect("write");
        let store = fixture.store();
        let wrong_base = content_fingerprint(b"different");
        let arguments = json!({
            "operation": "edit",
            "path": "a.txt",
            "content": "edited",
            "base_version": wrong_base
        });
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        let execution = run_executor(&store, &trusted, &call);

        match execution {
            ToolExecution::Failed { summary } => assert_eq!(summary["code"], "conflict"),
            other => panic!("expected failure, got {other:?}"),
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

    #[test]
    fn an_untracked_target_is_reported_unprotected_without_claiming_undo() {
        let fixture = Fixture::project();
        assert!(initialize_repository(fixture._directory.path()));
        std::fs::write(fixture._directory.path().join("notes.txt"), b"user work").expect("write");
        let store = fixture.store();
        let arguments = json!({
            "operation": "edit",
            "path": "notes.txt",
            "content": "agent change",
            "base_version": content_fingerprint(b"user work")
        });
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        let execution = run_executor(&store, &trusted, &call);

        match execution {
            ToolExecution::Failed { summary } => {
                assert_eq!(summary["code"], "write_not_enabled");
                assert_eq!(summary["undoAvailable"], false);
                assert_eq!(summary["recovery"]["warning"]["code"], "unprotected_files");
                assert_eq!(summary["recovery"]["files"][0]["reason"], "untracked");
                assert_eq!(summary["recovery"]["files"][0]["eligible"], false);
            }
            other => panic!("expected failure, got {other:?}"),
        }
        assert_eq!(
            std::fs::read(fixture._directory.path().join("notes.txt")).expect("read"),
            b"user work"
        );
    }

    #[test]
    fn a_tracked_dirty_target_reports_git_ready_but_not_undo_available() {
        let fixture = Fixture::project();
        assert!(initialize_repository(fixture._directory.path()));
        std::fs::write(fixture._directory.path().join("tracked.txt"), b"committed").expect("write");
        assert!(run_git(fixture._directory.path(), &["add", "tracked.txt"]));
        assert!(run_git(
            fixture._directory.path(),
            &["commit", "-qm", "tracked"]
        ));
        std::fs::write(
            fixture._directory.path().join("tracked.txt"),
            b"user worktree",
        )
        .expect("write");
        let store = fixture.store();
        let arguments = json!({
            "operation": "edit",
            "path": "tracked.txt",
            "content": "agent change",
            "base_version": content_fingerprint(b"user worktree")
        });
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        let execution = run_executor(&store, &trusted, &call);

        match execution {
            ToolExecution::Failed { summary } => {
                assert_eq!(summary["code"], "write_not_enabled");
                assert_eq!(summary["undoAvailable"], false);
                assert_eq!(summary["recovery"]["repository"], "work_tree");
                assert!(summary["recovery"]["warning"].is_null());
                assert_eq!(summary["recovery"]["files"][0]["tracked"], true);
                assert_eq!(summary["recovery"]["files"][0]["worktree"], "unstaged");
                assert_eq!(summary["recovery"]["files"][0]["eligible"], true);
            }
            other => panic!("expected failure, got {other:?}"),
        }
        assert_eq!(
            std::fs::read(fixture._directory.path().join("tracked.txt")).expect("read"),
            b"user worktree"
        );
    }

    #[test]
    fn creating_a_file_that_already_exists_conflicts() {
        let fixture = Fixture::project();
        std::fs::write(fixture._directory.path().join("existing.txt"), b"existing").expect("write");
        let store = fixture.store();
        let arguments = json!({"operation": "create", "path": "existing.txt", "content": "new"});
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        let execution = run_executor(&store, &trusted, &call);

        match execution {
            ToolExecution::Failed { summary } => assert_eq!(summary["code"], "conflict"),
            other => panic!("expected failure, got {other:?}"),
        }
        assert_eq!(
            std::fs::read(fixture._directory.path().join("existing.txt")).expect("read"),
            b"existing"
        );
    }

    #[test]
    fn the_executor_records_a_change_set_fact() {
        let fixture = Fixture::project();
        let store = fixture.store();
        let arguments = json!({"operation": "create", "path": "new.txt", "content": "hello"});
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        let execution = run_executor(&store, &trusted, &call);
        assert!(matches!(execution, ToolExecution::Failed { .. }));

        let facts = store.facts(fixture.conversation).expect("facts");
        assert!(
            facts
                .iter()
                .any(|fact| matches!(fact.fact, ContextFact::ChangeSet { .. })),
            "expected a change set fact to be recorded"
        );
    }
}
